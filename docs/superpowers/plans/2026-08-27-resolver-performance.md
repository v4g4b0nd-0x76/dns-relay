# Resolver Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve resolver throughput, cache reuse, tail latency, and I/O isolation while preserving DNS policy and platform compatibility.

**Architecture:** A single bounded UDP dispatcher multiplexes IPv4 upstream traffic; the resolver adaptively hedges to one secondary inside the existing deadline. The cache gains TTL aging, RFC 2308 negative entries, stale-if-error, and coalesced misses; optional file work moves to Tokio's blocking pool.

**Tech Stack:** Rust 2024, Tokio, socket2, lru, crossbeam-queue, existing workspace test modules.

**Spec:** `docs/superpowers/specs/2026-08-27-resolver-performance-design.md`

## Global Constraints

- Preserve filtering, redirect, ECS, relay, obfuscation, metrics, and configuration behavior unless the spec explicitly changes it.
- Preserve cache isolation by qname, qtype, qclass, relevant query flags, DNSSEC OK, and client IPv4 `/24`.
- Keep every queue, map, cache, and concurrency limit bounded.
- Add no dependencies.
- Preserve Windows compilation while optimizing macOS and Linux.
- Never block DNS resolution on history persistence.
- Do not stage or modify the user's existing `Cargo.lock` change.

---

### Task 1: Cache Packet Semantics

**Files:**
- Modify: `shared/src/constants.rs`
- Modify: `shared/src/dns.rs:251`
- Modify: `shared/src/cache.rs:29`
- Test: `shared/src/tests.rs`

**Interfaces:**
- Produces: `response_cache_ttl(&[u8]) -> Option<u32>` in `shared::dns`.
- Produces: `age_response_ttls(&mut [u8], Duration, bool) -> bool` in `shared::dns`.
- Produces: `cache_lookup_stale(&ResponseCache, &CacheKey) -> Option<Vec<u8>>` in `shared::cache`.
- Preserves: `cache_lookup` and `cache_store` call signatures.

- [ ] **Step 1: Add failing positive, negative, aging, and stale-cache tests**

Add tests that build a normal answer and an NXDOMAIN response containing one SOA authority RR. Assert that `response_cache_ttl` returns the smallest answer TTL or `min(SOA TTL, SOA.MINIMUM)`, fresh lookup subtracts elapsed seconds, stale lookup zeroes TTLs, and lookup removes an entry after `stale_until`.

```rust
#[test]
fn cache_ages_fresh_and_zeroes_stale_ttls() {
    let cache = empty_cache();
    let query = mock_query_google();
    let key = cache_key_from_query(query).unwrap();
    let (_, qname_end) = parse_domain(query, 12).unwrap();
    let answer = craft_redirect_response(query, qname_end, vec!["9.9.9.9"]).unwrap();
    cache_store(&cache, key.clone(), &answer);

    {
        let mut guard = cache.lock().unwrap();
        let entry = guard.get_mut(&key).unwrap();
        entry.inserted_at -= Duration::from_secs(10);
        entry.fresh_until = Instant::now() - Duration::from_secs(1);
    }

    assert!(cache_lookup(&cache, &key).is_none());
    let stale = cache_lookup_stale(&cache, &key).unwrap();
    assert_eq!(min_answer_ttl(&stale), None);
}
```

- [ ] **Step 2: Run the focused tests and observe RED**

Run: `cargo test -p shared cache_ -- --nocapture`

Expected: compilation fails because stale lookup and the new entry timestamps do not exist.

- [ ] **Step 3: Implement DNS TTL traversal and cache lifecycle**

Set `CACHE_CAPACITY = 8192`, `CACHE_TTL_MIN = 1s`, `CACHE_TTL_MAX = 3600s`, and `CACHE_STALE_TTL = 300s`. Traverse questions with `skip_name`, then answer and authority RRs; mutate each non-OPT TTL with saturating subtraction or zeroing. Parse SOA RDATA by skipping MNAME and RNAME and reading the final 20-byte integer block's MINIMUM field.

```rust
pub struct CacheEntry {
    pub packet: Vec<u8>,
    pub inserted_at: Instant,
    pub fresh_until: Instant,
    pub stale_until: Instant,
}

pub fn cache_lookup_stale(cache: &ResponseCache, key: &CacheKey) -> Option<Vec<u8>> {
    let mut guard = cache.lock().ok()?;
    let entry = guard.get(key)?;
    let now = Instant::now();
    if now < entry.fresh_until || now >= entry.stale_until {
        if now >= entry.stale_until {
            guard.pop(key);
        }
        return None;
    }
    let mut packet = entry.packet.clone();
    age_response_ttls(&mut packet, entry.inserted_at.elapsed(), true).then_some(packet)
}
```

- [ ] **Step 4: Run shared tests and verify GREEN**

Run: `cargo test -p shared`

Expected: all shared tests pass.

- [ ] **Step 5: Commit the cache behavior**

```bash
git add shared/src/constants.rs shared/src/dns.rs shared/src/cache.rs shared/src/tests.rs
git commit -m "perf: extend safe DNS caching"
```

### Task 2: Shared UDP Dispatcher

**Files:**
- Modify: `shared/src/constants.rs`
- Modify: `shared/src/lib.rs:63`
- Modify: `dns_relay/src/resolver.rs:1`
- Test: `dns_relay/src/resolver.rs`

**Interfaces:**
- Consumes: `bind_udp_socket("0.0.0.0:0")` and `RESOLVE_SEMAPHORE`.
- Produces: `UdpDispatcher::new() -> Result<Self, Error>`.
- Produces: `UdpDispatcher::resolve(&self, &[u8], SocketAddr) -> Result<(Vec<u8>, usize), Error>`.
- Changes: `resolve_from_upstream` accepts `&UdpDispatcher` as its final parameter.

- [ ] **Step 1: Add failing concurrent demultiplexing tests**

Start two local UDP upstreams, issue concurrent dispatcher requests with identical client transaction IDs, make the upstreams reply in reverse order, and assert each future receives the answer from its configured source. Repeat with 512 concurrent loopback requests and assert every reply arrives and the pending map drains to zero. Add a cancellation test that times out one request and asserts `pending_len() == 0` through a `#[cfg(test)]` accessor.

```rust
#[tokio::test]
async fn dispatcher_demultiplexes_same_client_txid() {
    let dispatcher = UdpDispatcher::new().unwrap();
    let first = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let second = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let query = DNS_PROBE_PACKET.to_vec();

    let a = dispatcher.resolve(&query, first.local_addr().unwrap());
    let b = dispatcher.resolve(&query, second.local_addr().unwrap());
    let ((a, _), (b, _)) = tokio::join!(a, b);
    assert_ne!(a.unwrap(), b.unwrap());
}
```

- [ ] **Step 2: Run the dispatcher test and observe RED**

Run: `cargo test -p dns_relay dispatcher_ -- --nocapture`

Expected: compilation fails because `UdpDispatcher` does not exist.

- [ ] **Step 3: Implement the dispatcher and socket buffers**

Add `SOCKET_SNDBUF_BYTES = 4 * 1024 * 1024` and request it in `bind_udp_socket`. `UdpDispatcher` owns one `Arc<UdpSocket>`, one `Arc<Mutex<HashMap<u16, PendingUdp>>>`, and one `Arc<AtomicU16>`. Register before `send_to`, use a drop guard to remove cancelled requests, validate source address in the receive loop, and drain at most `RECV_BATCH_MAX` ready datagrams after each awaited receive.

```rust
#[derive(Clone)]
pub struct UdpDispatcher {
    socket: Arc<UdpSocket>,
    pending: Arc<Mutex<HashMap<u16, PendingUdp>>>,
    next_id: Arc<AtomicU16>,
}

struct PendingUdp {
    upstream: SocketAddr,
    reply: oneshot::Sender<Vec<u8>>,
}
```

For IPv6 `SocketAddr`, keep the existing temporary connected socket branch. For IPv4, `resolve_from_upstream` calls the dispatcher without an inner timeout; callers provide the total timeout.

- [ ] **Step 4: Run resolver tests and verify GREEN**

Run: `cargo test -p dns_relay resolver::tests::dispatcher_ -- --nocapture`

Expected: dispatcher ordering, source validation, and cancellation tests pass.

- [ ] **Step 5: Commit dispatcher behavior**

```bash
git add shared/src/constants.rs shared/src/lib.rs dns_relay/src/resolver.rs
git commit -m "perf: multiplex upstream UDP queries"
```

### Task 3: Adaptive Secondary Resolver Hedging

**Files:**
- Modify: `dns_relay/src/resolver.rs:114`
- Test: `dns_relay/src/resolver.rs`

**Interfaces:**
- Produces: `ResolverPicker::candidates(bool) -> Vec<Resolver>` returning at most two entries.
- Produces: `ResolverPicker::resolve_packet(&self, &[u8], SocketAddr, bool, &reqwest::Client, &DoqPool, &UdpDispatcher) -> Result<Vec<u8>, Error>`.
- Preserves: two-second total `RESOLVE_TIMEOUT`.

- [ ] **Step 1: Add failing hedge timing and ordering tests**

Use local UDP upstreams to cover: slow primary/fast secondary, fast primary with no secondary packet before the hedge delay, immediate primary error, and both attempts bounded by one deadline. Add a merge test proving old and discovered resolvers are globally sorted and deduplicated.

```rust
#[test]
fn candidates_use_adaptive_delay_order() {
    let picker = ResolverPicker::from_healthy(vec![
        ("127.0.0.1:5301".into(), Duration::from_millis(40)),
        ("127.0.0.1:5302".into(), Duration::from_millis(70)),
    ]);
    let candidates = picker.candidates(false);
    assert_eq!(candidates.len(), 2);
    assert_eq!(hedge_delay(candidates[0].1), Duration::from_millis(80));
}
```

- [ ] **Step 2: Run hedge tests and observe RED**

Run: `cargo test -p dns_relay hedge_ -- --nocapture`

Expected: compilation fails because candidate and hedge APIs do not exist.

- [ ] **Step 3: Implement candidate selection and hedged resolution**

Sort the complete resolver list by measured duration after every discovery merge. Compute `primary_rtt.saturating_mul(2).clamp(25ms, 250ms)`. Use local pinned futures and `tokio::select!`: start secondary on delay or primary error, keep the other attempt alive after a rejected response, cancel the loser after a valid response, and wrap the complete operation once with `timeout(RESOLVE_TIMEOUT, ...)`.

```rust
fn hedge_delay(rtt: Duration) -> Duration {
    rtt.saturating_mul(2).clamp(
        Duration::from_millis(25),
        Duration::from_millis(250),
    )
}
```

- [ ] **Step 4: Run resolver tests and verify GREEN**

Run: `cargo test -p dns_relay resolver::tests -- --nocapture`

Expected: all resolver tests pass without timing beyond the two-second total bound.

- [ ] **Step 5: Commit hedging**

```bash
git add dns_relay/src/resolver.rs
git commit -m "perf: hedge slow DNS resolvers"
```

### Task 4: Coalesced Misses And Stale Fallback

**Files:**
- Modify: `dns_relay/src/handler.rs:34`
- Modify: `dns_relay/src/tests.rs:34`
- Modify: `dns_relay/src/main.rs:310`

**Interfaces:**
- Consumes: `cache_lookup_stale` and `ResolverPicker::resolve_packet`.
- Produces: `InFlightQueries::new()`, `InFlightQueries::join(&Arc<Self>, CacheKey) -> Flight`.
- Changes: `HandleQueryParams` receives `in_flight: &Arc<InFlightQueries>` and `udp_dispatcher: &UdpDispatcher`.

- [ ] **Step 1: Add failing coalescing and stale integration tests**

Send 20 concurrent identical misses through `resolve_query`, count upstream datagrams, and assert exactly one primary/secondary operation occurs. Seed an expired-but-stale cache entry, blackhole every resolver, and assert the reply has the client transaction ID and zero TTL instead of `SERVFAIL`.

```rust
#[tokio::test]
async fn concurrent_identical_misses_share_upstream_work() {
    let in_flight = Arc::new(InFlightQueries::new());
    let (replies, upstream_count) = run_identical_queries(20, in_flight).await;
    assert_eq!(replies.len(), 20);
    assert!(replies.iter().all(Option::is_some));
    assert_eq!(upstream_count, 1);
}
```

Implement `run_identical_queries` in the test module using `JoinSet`; each task owns cloned `Arc` state, changes only the two client transaction-ID bytes, constructs `HandleQueryParams`, and calls `resolve_query`.

- [ ] **Step 2: Run handler integration tests and observe RED**

Run: `cargo test -p dns_relay concurrent_identical_misses -- --nocapture`

Run: `cargo test -p dns_relay all_upstreams_fail -- --nocapture`

Expected: compilation fails because `InFlightQueries` and stale fallback wiring do not exist.

- [ ] **Step 3: Implement leader/follower response sharing**

Use a standard mutex around `HashMap<CacheKey, watch::Sender<Option<Vec<u8>>>>`. A leader guard removes its key on every drop. Followers subscribe, wait for `Some(packet)`, and restore their request transaction ID. The leader normalizes the winning reply to transaction ID zero, stores cacheable responses, falls back to stale after upstream failure, otherwise publishes a normalized local `SERVFAIL`.

```rust
pub enum Flight {
    Leader(FlightLeader),
    Follower(watch::Receiver<Option<Vec<u8>>>),
}

pub struct InFlightQueries {
    entries: Mutex<HashMap<CacheKey, watch::Sender<Option<Vec<u8>>>>>,
}
```

- [ ] **Step 4: Run DNS relay tests and verify GREEN**

Run: `cargo test -p dns_relay`

Expected: all handler, relay, resolver, and history tests pass.

- [ ] **Step 5: Commit coalescing and stale fallback**

```bash
git add dns_relay/src/handler.rs dns_relay/src/tests.rs dns_relay/src/main.rs
git commit -m "perf: coalesce concurrent DNS misses"
```

### Task 5: Nonblocking History Persistence

**Files:**
- Modify: `dns_relay/src/handler.rs:166`
- Test: `dns_relay/src/tests.rs:590`

**Interfaces:**
- Preserves: `HistoryBuffer::new`, `push`, `push_many`, and `close` signatures.
- Produces for tests: `HistoryBuffer::dropped_count() -> u64` under `#[cfg(test)]`.

- [ ] **Step 1: Add a failing saturation test**

On a current-thread Tokio test, call `push` 101 times without yielding. Assert the call completes, exactly one entry is counted as dropped, then close and verify the first 100 entries were persisted.

```rust
#[tokio::test(flavor = "current_thread")]
async fn history_saturation_drops_without_spinning() {
    let file = NamedTempFile::new().unwrap();
    let history = Arc::new(HistoryBuffer::new(file.path(), None));
    for i in 0..101 {
        history.push(format!("domain{i}.test"), "192.0.2.1".into());
    }
    assert_eq!(history.dropped_count(), 1);
    history.close().await.unwrap();
}
```

- [ ] **Step 2: Run the history test and observe RED**

Run: `cargo test -p dns_relay history_saturation_drops_without_spinning -- --nocapture`

Expected: compilation fails because `dropped_count` does not exist; the old push loop would also spin until the runtime schedules its flush.

- [ ] **Step 3: Remove hot-path spinning and offload the whole flush**

Replace the retry loop with one `queue.push`. Increment `AtomicU64` on rejection and trigger a flush. Drain the bounded batch before `spawn_blocking`; inside the closure use `std::fs::read_to_string`, existing deduplication/order logic, `std::fs::File::create`, `write_all`, and `flush`. Map join failure to `Error::Other`. Swap the dropped counter to zero once per flush and emit one aggregate warning.

- [ ] **Step 4: Run all history tests and verify GREEN**

Run: `cargo test -p dns_relay history -- --nocapture`

Expected: saturation and existing persistence tests pass.

- [ ] **Step 5: Commit history isolation**

```bash
git add dns_relay/src/handler.rs dns_relay/src/tests.rs
git commit -m "perf: isolate history file IO"
```

### Task 6: Nonblocking Configuration Reload

**Files:**
- Modify: `dns_relay/src/conf.rs:98`
- Test: `dns_relay/src/tests.rs`

**Interfaces:**
- Preserves: `load_conf` and `watch_conf_and_reload` public signatures.
- Changes: `HotreloadConf::default().poll_interval_ms` returns `1000`.
- Produces: private async `rule_file_mtimes(&Conf)` using `tokio::fs::metadata`.

- [ ] **Step 1: Add failing default and reload tests**

Assert the default interval is 1,000 ms. In a paused-time Tokio test, edit a temporary referenced rule list, advance the watcher interval, and assert the ArcSwap trie changes only after the blocking build completes while the previous trie remains usable before completion.

```rust
#[test]
fn hotreload_default_polls_once_per_second() {
    assert_eq!(HotreloadConf::default().poll_interval_ms, 1_000);
}
```

- [ ] **Step 2: Run configuration tests and observe RED**

Run: `cargo test -p dns_relay hotreload_ -- --nocapture`

Expected: the default interval assertion reports `100` instead of `1000`.

- [ ] **Step 3: Offload parsing and trie construction**

Use `tokio::fs::metadata` for configuration and referenced-file mtimes. Clone only the path and current configuration before calling `spawn_blocking(move || { load_conf; DomainTrie::build })`. Store the new trie/config and clear the cache only after the blocking task returns `Ok`. Set interval missed-tick behavior to `Delay` to avoid burst reload checks.

- [ ] **Step 4: Run configuration tests and verify GREEN**

Run: `cargo test -p dns_relay hotreload_ -- --nocapture`

Expected: default and reload tests pass.

- [ ] **Step 5: Commit reload isolation**

```bash
git add dns_relay/src/conf.rs dns_relay/src/tests.rs
git commit -m "perf: isolate rule reload IO"
```

### Task 7: Runtime Wiring And Full Verification

**Files:**
- Modify: `dns_relay/src/main.rs`
- Modify: `dns_relay/src/relay.rs`
- Modify: `dns_relay/src/tests.rs`
- Modify: `dns_relay/src/lib.rs`

**Interfaces:**
- Consumes: `UdpDispatcher`, `InFlightQueries`, changed resolver APIs.
- Produces: one dispatcher and one in-flight map shared by plain DNS, obfuscated DNS, health checks, relay bootstrap DNS, CLI resolution, and resolver discovery.

- [ ] **Step 1: Update every constructor and caller**

Construct `Arc<UdpDispatcher>` and `Arc<InFlightQueries>` once in `run_server`. Pass them through both listener paths and `HandleQueryParams`. Update CLI `list_resolvers` and `resolve` to construct their own dispatcher. Update `RelayPicker::new` to accept the dispatcher for relay hostname bootstrap resolution. Update test helpers once rather than duplicating state at every call.

- [ ] **Step 2: Run formatting and compile checks**

Run: `cargo fmt --all`

Run: `cargo check --workspace --all-targets`

Expected: formatting completes and every target compiles.

- [ ] **Step 3: Run the full test suite**

Run: `cargo test --workspace`

Expected: all workspace tests pass with zero failures.

- [ ] **Step 4: Run strict linting**

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: zero warnings and exit code 0.

- [ ] **Step 5: Inspect scope and commit final wiring**

Run: `git diff --check`

Run: `git status --short`

Confirm only the planned source/tests/docs are changed and the pre-existing `Cargo.lock` modification remains unstaged.

```bash
git add dns_relay/src/main.rs dns_relay/src/relay.rs dns_relay/src/lib.rs dns_relay/src/tests.rs docs/superpowers/plans/2026-08-27-resolver-performance.md
git commit -m "perf: wire shared resolver runtime state"
```
