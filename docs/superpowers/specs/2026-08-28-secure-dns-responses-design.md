# Secure DNS Responses And Relay Geography Design

**Date:** 2026-08-28

## Goal

Add an opt-in mode that prevents ISP DNS interception from supplying accepted answers, rejects unusable zero-address sinkhole replies, and preserves CDN geography when DNS is resolved through a relay whose public address differs from the user.

## Constraints

- Preserve the current resolver, relay, cache, hedged-failover, drop, redirect, obfuscation, and stale-if-error architecture.
- Keep current behavior when secure mode is disabled, except that the relay Worker universally rejects zero-only upstream answers so its own provider fallback can continue.
- Add no dependency and do not change the AES-256-GCM relay packet format.
- Never fall back to unauthenticated UDP while secure mode is enabled.
- DNS must continue securely when public-subnet discovery fails.
- Support IPv4 ECS `/24` only. IPv6 ECS is out of scope.

## Chosen Approach

Security comes from authenticated transport, not from guessing whether an answer looks censored. In secure mode, direct upstreams are limited to DoH and DoQ, while relay replies retain their existing end-to-end AES-GCM authentication. Plain UDP is excluded before candidate selection and cannot become a fallback.

A narrow semantic guard complements transport authentication: a response whose complete A/AAAA answer set contains only unspecified addresses (`0.0.0.0` or `::`) is unusable. It is not cached and the existing secondary attempt may win. Private, loopback, mixed, NODATA, and NXDOMAIN responses are not rejected by this rule.

Multi-resolver answer quorum is excluded because legitimate CDN answers vary by resolver and client location, and waiting for agreement would hurt latency. Full local DNSSEC validation is also excluded: it would add a validator and still would not cover unsigned domains.

## Configuration

Two backward-compatible top-level fields are added:

```toml
secure_only = true

# Optional manual override. When absent, relay-based discovery is attempted.
client_subnet = "8.8.8.0/24"
```

`secure_only` defaults to `false`. `client_subnet` is optional and must be a canonical public IPv4 `/24`; invalid, private, loopback, link-local, documentation, multicast, reserved, or non-`/24` values fail configuration loading.

With `secure_only = true`, configuration must provide at least one usable secure path: an enabled relay instance or a resolver beginning with `https://` or `quic://`. Otherwise startup fails with a configuration error instead of starting a server that can only return `SERVFAIL`.

The reusable `DnsResolver::new` API retains its current behavior and struct shape. A new secure constructor accepts the secure-mode and optional-subnet settings without breaking existing external struct literals.

## Effective Client Subnet

Each query receives one effective IPv4 `/24`, chosen in this order:

1. The configured `client_subnet` override.
2. The source address of the DNS client when it is globally routable IPv4.
3. The last successfully discovered public subnet.
4. No ECS.

Private and loopback client source addresses are never forwarded as ECS. The existing ECS encoder inserts or replaces the option in the DNS packet. The exact effective subnet is also used in the response-cache and in-flight keys, so a location-specific answer cannot survive a public-subnet change or leak across client networks.

## Automatic Discovery

When no override is configured, the resolver uses the first configured direct Cloudflare Worker relay as its discovery endpoint. A `GET` to the existing relay URL with the `subnet=1` query parameter returns the caller's canonical public IPv4 `/24`, derived from Cloudflare's connection metadata. The response is protected by HTTPS server authentication and marked `Cache-Control: no-store`.

The existing relay picker owns the discovered-subnet state so both the server binary and reusable resolver client use one implementation. Discovery runs at startup and then every five minutes in one background task. A valid result replaces the current discovered subnet and clears the response cache because its ECS scope changed. A failed or invalid result clears the discovered subnet; DNS resolution continues without ECS and retries discovery at the next interval. Logging is state-change-only: available, changed, or unavailable, not one warning per failed refresh.

Google Apps Script cannot observe the original public address after proxying to the Worker. Google-chained relay requests therefore use the manual, globally routable client-source, or previously discovered subnet. If none exists, they carry no ECS rather than incorrectly advertising a Google or Cloudflare edge subnet.

## Query Flow

The existing drop and redirect policy remains first. For an unresolved query:

```text
query
  -> select effective /24
  -> fresh cache / in-flight lookup scoped by that /24
  -> relay, DoH, or DoQ candidate
  -> authenticated and structurally valid reply
  -> reject zero-only A/AAAA reply
  -> first usable reply -> cache -> client
  -> all secure attempts fail -> eligible stale cache -> SERVFAIL
```

Relay queries include ECS inside the DNS message before the existing AES-GCM encryption step. No relay envelope, nonce layout, key handling, Apps Script JSON shape, or Worker cache-key algorithm changes.

## Secure Candidate Selection

When secure mode is enabled:

- Static UDP resolver addresses are excluded.
- Dynamically fetched or discovered UDP resolver addresses are excluded.
- UDP health probes do not make an excluded resolver eligible.
- DoH continues to use reqwest certificate validation and connection pooling.
- DoQ continues to use rustls certificate validation and the existing connection pool.
- Direct and Google-chained relay responses must pass the existing AES-GCM authentication before DNS validation.

The existing two-candidate hedge and one total resolution deadline remain unchanged. An unusable response is handled like the other rejected responses: it cannot win, and the other eligible attempt remains available. No third direct attempt or quorum is added.

## Zero-Address Guard

Shared DNS response inspection determines whether the Answer section contains A or AAAA records and whether any address is not unspecified.

- No A/AAAA records: accepted according to existing response-code and cache rules.
- At least one non-unspecified A/AAAA address: accepted.
- One or more A/AAAA records, all `0.0.0.0` or `::`: rejected.

Rust applies this rule before accepting or caching responses from direct or relay resolution. The Worker applies the same rule before accepting or caching a DoH provider's reply, allowing its existing provider hedge/fallback to continue instead of encrypting a known sinkhole result. The whole DNS response is rejected; records are never rewritten or selectively removed.

## Error Handling

- Secure mode with no configured secure path is a startup configuration error.
- A TLS, QUIC, relay authentication, malformed-packet, or zero-only failure remains eligible for existing secondary failover.
- Exhausted secure attempts use stale-if-error under the existing five-minute policy, then return `SERVFAIL`.
- Public-subnet discovery failure affects only ECS accuracy and never enables UDP fallback.
- Invalid manual subnet configuration fails early instead of silently disabling ECS.
- Discovery and rejected-answer logs contain no relay key, full public IP, or domain-derived secret.

## Files

Expected production changes are limited to:

- `dns_relay/src/conf.rs`: secure-mode and subnet configuration validation.
- `dns_relay/src/main.rs`: secure-mode and manual-subnet wiring.
- `dns_relay/src/handler.rs`: effective-subnet cache/in-flight scoping.
- `dns_relay/src/client.rs`: pass the effective subnet through relay and direct resolution.
- `dns_relay/src/resolver.rs`: secure candidate filtering and ECS insertion from the effective subnet.
- `dns_relay/src/relay.rs`: shared discovered-subnet state and refresh task using a direct relay instance.
- `shared/src/cache.rs`: accept the effective subnet directly in cache keys.
- `shared/src/dns.rs`: public IPv4 `/24` parsing and zero-only response inspection.
- `assets/relay_worker.js`: subnet response and zero-only upstream rejection.
- Existing Rust test modules plus one dependency-free Node self-check for Worker logic.

No crate, protocol abstraction, persistent store, or separate discovery service is introduced.

## Verification

Tests are written before each behavior change and observed failing for the intended reason. Focused coverage includes:

- Secure mode excludes configured and dynamically discovered UDP resolvers.
- Secure mode fails configuration loading when no authenticated path exists.
- Secure mode never falls back to UDP after DoH, DoQ, or relay failure.
- Zero-only IPv4 and IPv6 answers are rejected and uncached.
- A zero-only primary permits the secondary authenticated response to win.
- NXDOMAIN, NODATA, private, loopback, and mixed answers remain usable.
- Manual subnet parsing accepts only canonical public IPv4 `/24` values.
- Effective-subnet priority follows override, global client, discovery, then none.
- A subnet change isolates cache and in-flight work and clears old cached answers.
- Discovery success, invalid data, HTTP failure, and recovery produce the intended state.
- Relay encryption round trips remain byte-compatible with ECS inside the DNS query.
- Worker discovery truncates the address and Worker upstream validation retries zero-only replies.

Final checks are:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
node assets/relay_worker_test.js
```

## Non-Goals

- IPv6 ECS discovery or encoding.
- DNSSEC validation.
- Comparing or voting on answers from multiple resolvers.
- Detecting every possible malicious but syntactically plausible DNS answer.
- Rejecting private, loopback, documentation, or other special-purpose addresses in ordinary DNS answers.
- Replacing the resolver picker, relay picker, cache, or Worker provider hedge.
- Changing deployment, background-process, obfuscation, or relay-key management.
