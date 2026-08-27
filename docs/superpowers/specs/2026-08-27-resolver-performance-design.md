# Resolver Performance Design

**Date:** 2026-08-27

## Goal

Improve DNS throughput and tail latency on macOS and Linux, raise safe cache reuse, prevent optional history/configuration I/O from delaying resolution, and fail over to a configured secondary resolver within the existing two-second deadline.

## Constraints

- Preserve the current filtering, redirect, ECS, relay, obfuscation, metrics, and configuration behavior unless this document explicitly changes it.
- Preserve cache isolation by qname, qtype, qclass, relevant query flags, DNSSEC OK, and client IPv4 `/24`.
- Keep all queues, maps, caches, and concurrency bounded.
- Add no dependencies; use Tokio, socket2, lru, and the standard library already in the workspace.
- Keep the implementation portable to Windows even though the performance target is macOS and Linux.
- Never make DNS resolution wait for history persistence.

## Architecture

The resolver keeps its existing request pipeline and adds three focused mechanisms:

1. A shared IPv4 UDP dispatcher replaces one socket per upstream request. It rewrites outbound transaction IDs, multiplexes replies through bounded pending requests, and drains ready datagrams in batches.
2. Direct resolution starts the fastest healthy resolver first and starts the second after an adaptive hedge delay. The first valid reply wins within one total deadline.
3. The response cache distinguishes fresh, stale, and absent entries. Concurrent misses for one cache key share one upstream operation.

The resulting flow is:

```text
query -> policy -> fresh cache -> reply
                -> existing in-flight query -> shared reply
                -> primary resolver
                   + delayed secondary resolver
                -> valid reply -> cache -> reply
                -> all attempts fail -> stale cache -> SERVFAIL
```

DoH continues to use reqwest connection pooling. DoQ continues to use the existing QUIC connection pool. Relay requests use the improved cache, stale serving, and miss coalescing, but direct-resolver hedging and the UDP dispatcher do not alter relay instance selection.

## Resolver Selection And Hedging

Healthy resolvers remain stored with measured round-trip times. Initial health checks and later discovery merges sort the complete deduplicated list by latency before truncating it to the existing bound.

For a direct query:

- The primary is the first eligible resolver. When VPN preference is active, eligible DoH resolvers remain ahead of non-DoH resolvers.
- The secondary is the next distinct eligible resolver, if one exists.
- The hedge delay is `clamp(primary_rtt * 2, 25 ms, 250 ms)`.
- If the primary fails before the delay, the secondary starts immediately.
- If the primary is still pending at the delay, the secondary starts concurrently.
- The first valid response wins and the losing future is cancelled.
- Both attempts share the existing two-second `RESOLVE_TIMEOUT`; hedging never doubles the caller-visible timeout.
- With only one configured healthy resolver, behavior remains a single attempt with the same deadline.

A valid direct response has the DNS response bit set, matches the dispatcher's internal transaction ID and expected source address, is not truncated, and does not have `SERVFAIL` or `REFUSED` as its response code. NXDOMAIN and NODATA are valid responses.

## UDP Dispatcher

One unconnected wildcard IPv4 UDP socket is created during startup with the existing nonblocking socket2 path. Both receive and send buffer sizes are requested at 4 MiB; inability to raise an OS-limited buffer is logged and is not fatal.

The dispatcher owns:

- The shared socket.
- A standard-library mutex protecting a pending map keyed by a unique internal `u16` transaction ID.
- For each pending entry, the expected upstream `SocketAddr` and a Tokio oneshot sender.
- A transaction-ID allocator that skips IDs already present in the map.

The existing global 512-query resolver semaphore bounds the pending map well below the 65,536-ID space. Registering a request rewrites only the two DNS transaction-ID bytes. Completion, timeout, cancellation, or send failure removes the pending entry. The handler restores the original client transaction ID before replying.

The receive loop awaits one datagram, then drains all immediately ready datagrams up to the existing batch limit. A reply is delivered only when both its internal transaction ID and source address match the pending entry. Unknown, late, malformed, or spoofed datagrams are discarded.

Using an unconnected wildcard socket avoids binding the process to a particular interface. Linux and macOS select the current route for each `send_to`, allowing ordinary NIC and VPN route changes without application-specific interface code. Linux-only calls such as `SO_BINDTODEVICE`, `recvmmsg`, and `sendmmsg` are intentionally excluded.

UDP health probes use the same dispatcher so concurrent probes cannot consume one another's replies. IPv6 upstreams retain the current per-query socket path; automatic resolver discovery already excludes IPv6.

## Response Cache

The existing cache key remains unchanged. Cache capacity increases from 4,096 to 8,192 entries. Fresh lifetime is clamped between 1 second and 1 hour, while never exceeding the TTL supplied by the upstream response.

Each entry records insertion time, fresh expiration, and stale expiration. Stale expiration is exactly five minutes after fresh expiration.

Fresh cache lookup:

- Returns only entries before fresh expiration.
- Decrements TTL fields in answer and authority records by elapsed whole seconds, saturating at zero.
- Restores the requesting client's transaction ID in the handler.

Stale cache lookup:

- Runs only after every eligible upstream attempt fails.
- Returns entries between fresh and stale expiration.
- Sets answer and authority TTL fields to zero so downstream clients retry rather than extending stale state.
- Removes entries beyond stale expiration.

The cache accepts:

- Successful responses with cacheable answers, using the smallest answer TTL.
- NXDOMAIN and NODATA responses only when a valid SOA exists in the authority section, using the RFC 2308 value `min(SOA record TTL, SOA.MINIMUM)`.

The cache continues rejecting malformed packets, non-responses, truncation, `SERVFAIL`, `REFUSED`, and negative responses without a parseable SOA.

## Concurrent Miss Coalescing

An in-flight map is keyed by the exact response-cache key and bounded by the existing resolver semaphore. The first miss becomes the leader and performs relay or direct resolution. Later requests for that key wait on a Tokio watch channel rather than starting more upstream work.

The leader publishes one normalized response with transaction ID zero, including valid non-cacheable responses. Each waiter restores its own transaction ID. If the leader and all upstreams fail, it publishes the stale response when available; otherwise every waiter receives a locally crafted `SERVFAIL`. Removing the map entry and notifying waiters must happen on success, error, timeout, and cancellation.

## History I/O

History remains optional and best-effort.

- `push_many` uses one nonblocking queue attempt and never spins.
- When the queue is full, the entry is dropped and an atomic dropped counter increments.
- A flush reports the accumulated dropped count once, rather than logging once per entry.
- Queue draining remains bounded by its current capacity.
- Reading the existing history, deduplicating entries, enforcing the line limit, rendering output, creating the file, writing, and flushing all execute inside `tokio::task::spawn_blocking`.
- Resolver tasks only enqueue; they never await a file operation.
- Explicit close still waits for an active flush and flushes remaining queued entries.

## Configuration And Rule Reload I/O

The default hot-reload polling interval changes from 100 ms to 1,000 ms. An explicitly configured interval remains authoritative.

Metadata checks use `tokio::fs::metadata`. Configuration reading, TOML parsing, referenced-list reading, trie construction, and list mtime collection run in `spawn_blocking`. The existing configuration and rule trie remain active until the replacement has been built successfully. A successful policy reload still clears the response cache.

## Error Handling And Logging

- A fast primary error starts the secondary immediately.
- A rejected primary response does not cancel a still-eligible secondary.
- All-attempt failure serves stale data when available, then returns `SERVFAIL`.
- Dispatcher send failure and timeout remove pending state immediately.
- The UDP receive loop discards unrelated traffic without per-packet warning logs.
- History saturation produces one aggregate message per flush.
- Existing timeout, resolved, failed, and cache metrics remain intact; no unbounded per-domain labels are added.

## Files

Expected production changes are limited to:

- `shared/src/constants.rs`: cache limits and the socket send-buffer constant.
- `shared/src/dns.rs`: response validation, negative TTL parsing, and TTL aging.
- `shared/src/cache.rs`: fresh/stale entry lifecycle.
- `dns_relay/src/resolver.rs`: dispatcher, sorted candidates, and hedged resolution.
- `dns_relay/src/handler.rs`: fresh/coalesced/upstream/stale flow and nonblocking history.
- `dns_relay/src/conf.rs`: asynchronous metadata and blocking-pool reload construction.
- `dns_relay/src/main.rs`: construct and pass shared dispatcher and in-flight state.

Tests stay in the existing `shared/src/tests.rs`, `dns_relay/src/tests.rs`, and resolver-local test module. No new crate or subsystem is introduced.

## Verification

Tests will be written before each behavior change and observed failing for the intended reason. Focused coverage includes:

- A slow primary causes the faster secondary to win before the primary timeout.
- A primary response before the hedge delay sends no secondary query.
- A fast primary error starts the secondary without waiting for the hedge delay.
- One total deadline bounds both attempts.
- Concurrent UDP requests receive only their matching transaction IDs and sources.
- Concurrent health probes cannot consume one another's replies.
- Concurrent identical misses produce one hedged upstream operation.
- Fresh hits age TTLs.
- NXDOMAIN/NODATA cache only with a valid SOA TTL.
- All-upstream failure serves stale data with zero TTL for at most five minutes.
- History saturation returns immediately and reports aggregate drops.
- Rule reload work does not execute file parsing on a Tokio worker.

Final checks are:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Non-Goals

- Replacing the resolver with Hickory DNS.
- Linux-specific packet syscalls or interface binding.
- A TCP DNS listener or TCP fallback for truncated UDP replies.
- Hedging multiple relay instances.
- Persistent cache storage.
- Unbounded cache growth, history backpressure, or lossless history under overload.
- Reworking the self-managed background process, launchd compatibility path, or configuration schema.
