# Secure DNS Responses Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in fail-closed DNS mode that accepts only authenticated upstream transports, rejects zero-address sinkhole answers, and supplies the user's public IPv4 `/24` to relay queries when available.

**Architecture:** Keep the existing resolver and relay pipeline. Shared DNS helpers parse public `/24` values, select one effective subnet, insert ECS, and classify zero-only answers; `ResolverPicker` filters unauthenticated candidates; `RelayPicker` owns best-effort subnet discovery; cache and in-flight keys use the same effective subnet as the outgoing query.

**Tech Stack:** Rust 2024, Tokio, reqwest/rustls, quinn/rustls, AES-256-GCM relay transport, Cloudflare Workers JavaScript, Node.js standard library tests.

**Spec:** `docs/superpowers/specs/2026-08-28-secure-dns-responses-design.md`

## Global Constraints

- Preserve the current resolver, relay, cache, hedged-failover, drop, redirect, obfuscation, and stale-if-error architecture.
- Keep current behavior when secure mode is disabled, except that the relay Worker universally rejects zero-only upstream answers so its own provider fallback can continue.
- Add no dependency and do not change the AES-256-GCM relay packet format.
- Never fall back to unauthenticated UDP while secure mode is enabled.
- DNS must continue securely when public-subnet discovery fails.
- Support IPv4 ECS `/24` only. IPv6 ECS is out of scope.
- Preserve the existing two-candidate hedge and one total `RESOLVE_TIMEOUT`.
- Never log relay keys, full discovered public IPs, or domain-derived secrets.

---

### Task 1: Shared subnet and zero-answer primitives

**Files:**
- Modify: `shared/src/dns.rs`
- Modify: `shared/src/cache.rs`
- Modify: `shared/src/tests.rs`

**Interfaces:**
- Produces: `pub type Ipv4Subnet = [u8; 3]`
- Produces: `pub fn parse_public_ipv4_subnet(value: &str) -> Option<Ipv4Subnet>`
- Produces: `pub fn public_ipv4_subnet(addr: SocketAddr) -> Option<Ipv4Subnet>`
- Produces: `pub fn effective_ipv4_subnet(override_subnet: Option<Ipv4Subnet>, client_addr: SocketAddr, discovered: Option<Ipv4Subnet>) -> Option<Ipv4Subnet>`
- Produces: `pub fn set_ecs_ipv4_subnet(payload: &[u8], subnet: Option<Ipv4Subnet>) -> Option<Vec<u8>>`
- Produces: `pub fn response_has_only_unspecified_addresses(packet: &[u8]) -> bool`
- Produces: `pub fn cache_key_from_query_for_subnet(payload: &[u8], subnet: Option<Ipv4Subnet>) -> Option<CacheKey>`
- Preserves: `set_ecs_option` and `cache_key_from_query_for_client` as compatibility wrappers.

- [x] **Step 1: Add failing public-subnet tests**

Add imports for the new helpers in `shared/src/tests.rs`, then add:

```rust
#[test]
fn public_ipv4_subnets_are_canonical_and_global() {
    assert_eq!(parse_public_ipv4_subnet("8.8.8.0/24"), Some([8, 8, 8]));
    assert_eq!(parse_public_ipv4_subnet("8.8.8.8/24"), None);
    assert_eq!(parse_public_ipv4_subnet("10.0.0.0/24"), None);
    assert_eq!(parse_public_ipv4_subnet("192.0.2.0/24"), None);
    assert_eq!(parse_public_ipv4_subnet("2001:db8::/56"), None);
}

#[test]
fn effective_subnet_uses_override_global_client_then_discovery() {
    let global: SocketAddr = "8.8.4.4:53000".parse().unwrap();
    let private: SocketAddr = "192.168.1.20:53000".parse().unwrap();
    assert_eq!(effective_ipv4_subnet(Some([9, 9, 9]), global, Some([1, 1, 1])), Some([9, 9, 9]));
    assert_eq!(effective_ipv4_subnet(None, global, Some([1, 1, 1])), Some([8, 8, 4]));
    assert_eq!(effective_ipv4_subnet(None, private, Some([1, 1, 1])), Some([1, 1, 1]));
    assert_eq!(effective_ipv4_subnet(None, private, None), None);
}
```

- [x] **Step 2: Run the subnet tests and confirm they fail**

Run:

```bash
cargo test -p dns-relay-shared public_ipv4_subnets_are_canonical_and_global
cargo test -p dns-relay-shared effective_subnet_uses_override_global_client_then_discovery
```

Expected: compilation fails because the new functions do not exist.

- [x] **Step 3: Implement public `/24` parsing and selection**

In `shared/src/dns.rs`, add `Ipv4Subnet`, reject any prefix other than `/24`, require the last octet to be zero, and accept only globally routable IPv4. The private helper must reject `is_unspecified`, `is_private`, `is_loopback`, `is_link_local`, `is_broadcast`, `is_documentation`, and `is_multicast`, plus `0.0.0.0/8`, shared `100.64.0.0/10`, protocol-assignment `192.0.0.0/24`, benchmarking `198.18.0.0/15`, and reserved `240.0.0.0/4`.

Keep selection as one expression with the approved priority:

```rust
pub fn effective_ipv4_subnet(
    override_subnet: Option<Ipv4Subnet>,
    client_addr: SocketAddr,
    discovered: Option<Ipv4Subnet>,
) -> Option<Ipv4Subnet> {
    override_subnet
        .or_else(|| public_ipv4_subnet(client_addr))
        .or(discovered)
}
```

- [x] **Step 4: Add failing ECS and cache-key tests using an explicit subnet**

Add:

```rust
#[test]
fn explicit_subnet_drives_ecs_and_cache_scope() {
    let query = mock_query_google();
    let with_ecs = set_ecs_ipv4_subnet(query, Some([8, 8, 8])).unwrap();
    assert!(with_ecs.ends_with(&[8, 8, 8]));
    assert_eq!(
        cache_key_from_query_for_subnet(query, Some([8, 8, 8])).unwrap().client_subnet,
        Some([8, 8, 8])
    );
    assert_ne!(
        cache_key_from_query_for_subnet(query, Some([8, 8, 8])).unwrap(),
        cache_key_from_query_for_subnet(query, Some([1, 1, 1])).unwrap()
    );
}
```

- [x] **Step 5: Run the ECS/cache test and confirm it fails**

Run: `cargo test -p dns-relay-shared explicit_subnet_drives_ecs_and_cache_scope`

Expected: compilation fails because the explicit-subnet functions do not exist.

- [x] **Step 6: Refactor existing ECS and cache code around the explicit subnet**

Move the OPT-record mutation body from `set_ecs_option` into `set_ecs_ipv4_subnet`. `None` returns the original query unchanged. Keep `set_ecs_option(payload, client_addr, fabricated)` as a wrapper that converts its existing inputs to an `Ipv4Subnet`, preserving current callers and tests.

In `shared/src/cache.rs`, make `cache_key_from_query_for_subnet` construct `CacheKey`. Keep `cache_key_from_query_for_client` as a wrapper around `public_ipv4_subnet`, and keep `cache_key_from_query` passing `None`.

- [x] **Step 7: Add failing zero-only answer tests**

Use `craft_redirect_response` for A records and a small local helper that appends an AAAA answer. Add:

```rust
#[test]
fn only_unspecified_addresses_are_unusable() {
    let query = mock_query_google();
    let (_, qname_end) = parse_domain(query, 12).unwrap();
    let zero = craft_redirect_response(query, qname_end, vec!["0.0.0.0"]).unwrap();
    let mixed = craft_redirect_response(query, qname_end, vec!["0.0.0.0", "8.8.8.8"]).unwrap();
    let private = craft_redirect_response(query, qname_end, vec!["192.168.1.1"]).unwrap();

    assert!(response_has_only_unspecified_addresses(&zero));
    assert!(!response_has_only_unspecified_addresses(&mixed));
    assert!(!response_has_only_unspecified_addresses(&private));
    assert!(!response_has_only_unspecified_addresses(&negative_response_with_soa(3)));
    assert!(response_has_only_unspecified_addresses(&aaaa_response([0; 16])));
}
```

- [x] **Step 8: Run the zero-answer test and confirm it fails**

Run: `cargo test -p dns-relay-shared only_unspecified_addresses_are_unusable`

Expected: compilation fails because `response_has_only_unspecified_addresses` does not exist.

- [x] **Step 9: Implement one bounded Answer-section scan**

Reuse `skip_name`. Walk only `ANCOUNT` records and track `(address_count, has_non_unspecified)`. Recognize A only when `RDLENGTH == 4` and AAAA only when `RDLENGTH == 16`. Return `true` only for `address_count > 0 && !has_non_unspecified`; malformed packets return `false` so existing structural validation remains authoritative.

- [x] **Step 10: Run shared tests and commit**

Run:

```bash
cargo test -p dns-relay-shared
cargo fmt --all -- --check
```

Expected: all shared tests pass and formatting is clean.

Commit:

```bash
git add shared/src/dns.rs shared/src/cache.rs shared/src/tests.rs
git commit -m "feat: classify secure DNS answers and subnets"
```

---

### Task 2: Secure configuration and resolver selection

**Files:**
- Modify: `dns_relay/src/conf.rs`
- Modify: `dns_relay/src/resolver.rs`
- Modify: `dns_relay/src/client.rs`
- Modify: `dns_relay/src/main.rs`
- Modify: `dns_relay/src/tests.rs`
- Modify: `dns_relay/src/lib.rs`

**Interfaces:**
- Consumes: `Ipv4Subnet`, `parse_public_ipv4_subnet` from Task 1.
- Produces: `Conf::secure_only: bool` and `Conf::client_subnet: Option<Ipv4Subnet>`.
- Produces: `pub fn is_secure_resolver(resolver: &str) -> bool`.
- Produces: `ResolverPicker::new_secure(resolvers: Vec<String>, http: reqwest::Client, doq_pool: &Arc<DoqPool>, udp_dispatcher: &Arc<UdpDispatcher>) -> Result<ResolverPicker, Error>` while retaining the existing `ResolverPicker::new` signature.
- Produces: `DnsResolver::new_secure(config: ResolverConfig, client_subnet: Option<Ipv4Subnet>) -> Result<DnsResolver, Error>` while retaining `DnsResolver::new(config)`.
- Produces: `run_secure_resolver_finder(resolver_searching: ResolverSearchingConf, healthy_resolvers: Arc<RwLock<Vec<Resolver>>>, is_searching: Arc<AtomicBool>, udp_dispatcher: Arc<UdpDispatcher>) -> Result<(), Error>` while retaining the existing `run_resolver_finder` signature.

- [x] **Step 1: Add failing configuration tests**

In `dns_relay/src/tests.rs`, add this helper, then write temporary TOML files and call `load_conf`:

```rust
fn load_toml(content: &str) -> Result<Conf, crate::Error> {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    load_conf(&file.path().to_path_buf())
}
```

```rust
#[test]
fn secure_config_requires_an_authenticated_path() {
    let error = match load_toml("secure_only = true\nresolvers = ['1.1.1.1:53']\ndrop_list = []\nredirect_list = []") {
        Err(error) => error,
        Ok(_) => panic!("insecure-only config must fail"),
    };
    assert!(error.to_string().contains("authenticated resolver or relay"));
}

#[test]
fn secure_config_parses_manual_public_subnet() {
    let conf = load_toml("secure_only = true\nclient_subnet = '8.8.8.0/24'\nresolvers = ['https://dns.google/dns-query']\ndrop_list = []\nredirect_list = []").unwrap();
    assert_eq!(conf.client_subnet, Some([8, 8, 8]));
}
```

Also cover an invalid manual subnet and an `http://` relay URL under secure mode.

- [x] **Step 2: Run configuration tests and confirm they fail**

Run:

```bash
cargo test -p dns_relay secure_config_requires_an_authenticated_path
cargo test -p dns_relay secure_config_parses_manual_public_subnet
```

Expected: compilation fails because `Conf` has no security fields and `load_conf` performs no validation.

- [x] **Step 3: Implement config deserialization and validation**

Add serde-defaulted fields without adding a nested config object:

```rust
#[serde(default)]
pub secure_only: bool,
#[serde(default, deserialize_with = "deserialize_client_subnet")]
pub client_subnet: Option<Ipv4Subnet>,
```

After TOML parsing, validate:

- secure mode has an enabled relay with at least one instance or at least one `https://`/`quic://` resolver;
- every enabled secure-mode relay URL uses `https`;
- invalid subnet strings return `Error::Config`.

- [x] **Step 4: Add failing resolver-filter tests**

In the resolver-local test module, add:

```rust
#[test]
fn secure_candidates_exclude_udp_even_if_it_is_fastest() {
    let picker = ResolverPicker::from_healthy_secure(vec![
        ("1.1.1.1:53".into(), Duration::from_millis(1)),
        ("https://dns.google/dns-query".into(), Duration::from_millis(20)),
        ("quic://dns.example:853".into(), Duration::from_millis(30)),
    ]);
    assert_eq!(
        resolvers_to_addrs(&picker.candidates(false)),
        ["https://dns.google/dns-query", "quic://dns.example:853"]
    );
}
```

- [x] **Step 5: Run the resolver-filter test and confirm it fails**

Run: `cargo test -p dns_relay secure_candidates_exclude_udp_even_if_it_is_fastest`

Expected: compilation fails because secure picker construction does not exist.

- [x] **Step 6: Add secure filtering at every resolver entrance**

Add `secure_only: bool` to `ResolverPicker`. Keep `new` and `from_healthy` as compatibility wrappers with `false`; add `new_secure` and test-only `from_healthy_secure` with `true`. Filter candidates defensively in `candidates`, and filter before static health checks so UDP is never probed for eligibility.

Keep `run_resolver_finder` unchanged as a compatibility wrapper. Add `run_secure_resolver_finder` that filters fetched candidates with `is_secure_resolver` before health checks and merging. Update `main.rs` to select secure constructors/functions when `conf.secure_only` is true.

- [x] **Step 7: Preserve the reusable resolver API while adding secure construction**

Keep `ResolverConfig` unchanged. Implement:

```rust
pub async fn new(config: ResolverConfig) -> Result<Self, Error> {
    Self::build(config, false, None).await
}

pub async fn new_secure(
    config: ResolverConfig,
    client_subnet: Option<Ipv4Subnet>,
) -> Result<Self, Error> {
    Self::build(config, true, client_subnet).await
}
```

The private `build` selects `ResolverPicker::new_secure` and rejects configurations with no authenticated path. Re-export `Ipv4Subnet` from `dns_relay::lib` so callers do not need to depend on `shared` directly.

- [x] **Step 8: Run focused and crate tests, then commit**

Run:

```bash
cargo test -p dns_relay secure_config
cargo test -p dns_relay secure_candidates
cargo test -p dns_relay client::tests
```

Expected: all focused tests pass and existing `DnsResolver::new` tests still compile unchanged.

Commit:

```bash
git add dns_relay/src/conf.rs dns_relay/src/resolver.rs dns_relay/src/client.rs dns_relay/src/main.rs dns_relay/src/tests.rs dns_relay/src/lib.rs
git commit -m "feat: add fail-closed secure resolver mode"
```

---

### Task 3: Relay subnet discovery and effective ECS propagation

**Files:**
- Modify: `dns_relay/src/relay.rs`
- Modify: `dns_relay/src/client.rs`
- Modify: `dns_relay/src/handler.rs`
- Modify: `dns_relay/src/resolver.rs`
- Modify: `dns_relay/src/main.rs`
- Modify: `dns_relay/src/tests.rs`
- Modify: `assets/relay_worker.js`
- Create: `assets/relay_worker_test.mjs`

**Interfaces:**
- Consumes: `Ipv4Subnet`, `effective_ipv4_subnet`, `set_ecs_ipv4_subnet`, and `cache_key_from_query_for_subnet` from Task 1.
- Produces: `RelayPicker::effective_subnet(client_addr: SocketAddr) -> Option<Ipv4Subnet>`.
- Produces: `RelayPicker::new_secure(conf: &RelayConf, resolver_picker: &ResolverPicker, http: &reqwest::Client, doq_pool: &DoqPool, udp_dispatcher: &UdpDispatcher, configured_subnet: Option<Ipv4Subnet>, cache: Arc<ResponseCache>) -> Result<RelayPicker, Error>`.
- Produces: `async fn discover_client_subnet(client: &reqwest::Client, relay_url: &str) -> Result<Ipv4Subnet, Error>`.
- Changes internal resolution calls to accept `effective_subnet: Option<Ipv4Subnet>` instead of deriving ECS independently from `src_addr`.

- [x] **Step 1: Add failing relay discovery tests**

Use a loopback TCP HTTP mock as existing relay tests do. Add tests that return `8.8.8.0/24`, malformed text, and HTTP 503:

```rust
#[tokio::test]
async fn relay_discovery_accepts_only_public_canonical_subnet() {
    let (url, server) = mock_http_response(200, "8.8.8.0/24").await;
    assert_eq!(discover_client_subnet(&reqwest::Client::new(), &url).await.unwrap(), [8, 8, 8]);
    server.await.unwrap();
}
```

Assert that the request method is `GET` and its query contains `subnet=1`.

- [x] **Step 2: Run discovery tests and confirm they fail**

Run: `cargo test -p dns_relay relay_discovery`

Expected: compilation fails because discovery does not exist.

- [x] **Step 3: Implement the discovery request and RelayPicker state**

Add to `RelayPicker`:

```rust
configured_subnet: Option<Ipv4Subnet>,
discovered_subnet: Arc<RwLock<Option<Ipv4Subnet>>>,
```

Keep `RelayPicker::new` unchanged as the compatibility constructor. Add `RelayPicker::new_secure` with the same existing arguments plus `configured_subnet: Option<Ipv4Subnet>` and `cache: Arc<ResponseCache>`. If no override exists and a direct relay instance exists, spawn one interval task with `MissedTickBehavior::Delay`. Each tick requests the same relay URL with `subnet=1`, parses the bounded text body, stores success or `None`, and clears the cache only when the stored subnet changes. Compare old/new state before logging so messages occur only on available, changed, or unavailable transitions.

`effective_subnet` calls `effective_ipv4_subnet(configured, client_addr, discovered)`.

- [x] **Step 4: Add the Worker discovery response**

Before the POST-only branch in `assets/relay_worker.js`, handle only `GET` with `subnet=1`. Read `cf-connecting-ip`, require dotted IPv4 with four decimal octets, return `a.b.c.0/24`, and set:

```javascript
{
  "content-type": "text/plain; charset=utf-8",
  "cache-control": "no-store",
}
```

Return 404 for other GET requests and 503 when the connecting address is absent or IPv6.

Export a pure `subnetForIp(value)` helper. Create `assets/relay_worker_test.mjs` using `node:fs`, `node:assert/strict`, and a `data:` module import of `relay_worker.js`; assert:

```javascript
assert.equal(subnetForIp("8.8.8.42"), "8.8.8.0/24");
assert.equal(subnetForIp("192.168.1.2"), null);
assert.equal(subnetForIp("2001:db8::1"), null);
assert.equal(subnetForIp("not-an-ip"), null);
```

Run: `node assets/relay_worker_test.mjs`

Expected: all discovery helper assertions pass.

- [x] **Step 5: Add failing effective-subnet integration tests**

Add handler/client tests proving:

- a configured subnet wins over a global client address;
- a private/loopback client uses discovery;
- discovery absence leaves the query without ECS;
- two different effective subnets produce different cache/in-flight keys.

The cache assertion must use the exact helper that production calls:

```rust
let first = cache_key_from_query_for_subnet(&query, Some([8, 8, 8])).unwrap();
let second = cache_key_from_query_for_subnet(&query, Some([1, 1, 1])).unwrap();
assert_ne!(first, second);
```

- [x] **Step 6: Run integration tests and confirm they fail**

Run:

```bash
cargo test -p dns_relay effective_subnet
cargo test -p dns_relay discovered_subnet
```

Expected: tests fail because handlers still derive cache scope and ECS directly from `src_addr`.

- [x] **Step 7: Thread one effective subnet through cache and transport**

In `handler::resolve_query`, compute the subnet once from `relay_picker.effective_subnet(src_addr)` when a relay exists, otherwise from `public_ipv4_subnet(src_addr)`. Use it for `cache_key_from_query_for_subnet` and pass it to `resolve_transport`.

In `DnsResolver::resolve_ipv4`, compute the subnet from the relay picker with the existing unspecified synthetic source. Use it for both cache key and `resolve_transport`.

Change internal resolver signatures from `src_addr: SocketAddr` to `effective_subnet: Option<Ipv4Subnet>` through `resolve_packet`, `resolve_candidates`, `resolve_candidate`, and `resolve_from_upstream_inner`. Call `set_ecs_ipv4_subnet` exactly once before selecting DoH, DoQ, or UDP transport. Relay resolution receives the same already-ECS-adjusted payload before AES-GCM encryption.

Update every `HandleQueryParams` constructor in `main.rs` and tests only if the final signature requires a new field; prefer computing from the existing `src_addr` and `relay_picker` fields to avoid widening the struct.

- [x] **Step 8: Run relay, handler, client, and cache tests, then commit**

Run:

```bash
cargo test -p dns_relay relay
cargo test -p dns_relay effective_subnet
cargo test -p dns_relay client::tests
cargo test -p dns_relay upstream_failure_returns_stale_cached_answer
```

Expected: all focused tests pass; stale-if-error and existing relay encryption tests remain green.

Commit:

```bash
git add dns_relay/src/relay.rs dns_relay/src/client.rs dns_relay/src/handler.rs dns_relay/src/resolver.rs dns_relay/src/main.rs dns_relay/src/tests.rs assets/relay_worker.js assets/relay_worker_test.mjs
git commit -m "feat: preserve client geography through relays"
```

---

### Task 4: Reject and retry zero-address responses

**Files:**
- Modify: `dns_relay/src/resolver.rs`
- Modify: `dns_relay/src/client.rs`
- Modify: `dns_relay/src/tests.rs`
- Modify: `assets/relay_worker.js`
- Modify: `assets/relay_worker_test.mjs`

**Interfaces:**
- Consumes: `response_has_only_unspecified_addresses` from Task 1.
- Produces: Rust direct and relay paths treat zero-only replies as ordinary candidate failures when secure mode is active.
- Produces: Worker `isCacheableReply` rejects zero-only provider responses before Cache API writes.

- [x] **Step 1: Add a failing Rust hedge test**

Create two loopback UDP upstream mocks using the existing `mock_udp_resolver` pattern. The primary replies with `0.0.0.0`; the secondary replies with `8.8.8.8`. Call the internal candidate routine with `reject_unspecified = true`; this unit test exercises hedge acceptance directly while candidate filtering is covered in Task 2:

```rust
let primary = (zero_addr.to_string(), Duration::from_millis(1));
let secondary = (valid_addr.to_string(), Duration::from_millis(1));
let response = resolve_candidates(
    &query,
    None,
    &primary,
    Some(&secondary),
    &reqwest::Client::new(),
    &DoqPool::new(),
    &UdpDispatcher::new().unwrap(),
    true,
)
.await
.unwrap();
assert_eq!(parse_a_records(&response), [Ipv4Addr::new(8, 8, 8, 8)]);
```

Also assert the zero response was never inserted into `ResponseCache` through `resolve_query`.

- [x] **Step 2: Run the Rust test and confirm it fails**

Run: `cargo test -p dns_relay zero_only_primary_uses_authenticated_secondary`

Expected: the primary `0.0.0.0` response currently wins because `is_usable_response` checks only DNS flags and response code.

- [x] **Step 3: Apply the guard at the shared Rust acceptance boundary**

Change `is_usable_response` to take `reject_unspecified: bool`. Keep existing structural checks, then add:

```rust
&& (!reject_unspecified || !response_has_only_unspecified_addresses(packet))
```

Pass `ResolverPicker::secure_only` into candidate resolution. For relay replies, apply the same guard in `resolve_transport` before returning success when secure mode is active; convert rejection to `Error::Other` so stale-if-error and `SERVFAIL` behavior remain unchanged.

- [x] **Step 4: Add a failing dependency-free Worker self-check**

Export `hasOnlyUnspecifiedAddresses` as a named export from the Worker module. Extend the `data:` module import in `assets/relay_worker_test.mjs`, then add local `aResponse(addresses)` and `noDataResponse()` fixture builders that emit a one-question DNS packet with compressed A answers. Assert zero-only, mixed, private, and NODATA fixtures:

```javascript
assert.equal(hasOnlyUnspecifiedAddresses(aResponse("0.0.0.0")), true);
assert.equal(hasOnlyUnspecifiedAddresses(aResponse("8.8.8.8")), false);
assert.equal(hasOnlyUnspecifiedAddresses(aResponse("192.168.1.1")), false);
assert.equal(hasOnlyUnspecifiedAddresses(noDataResponse()), false);
```

- [x] **Step 5: Run the Worker test and confirm it fails**

Run: `node assets/relay_worker_test.mjs`

Expected: import fails because the named helper does not exist.

- [x] **Step 6: Implement the Worker Answer-section scan**

Add a bounded DNS-name skipper and walk only `ANCOUNT`. Recognize A length 4 and AAAA length 16. Return `true` only when at least one address exists and all address bytes are zero. Make `isCacheableReply` return false for this condition, causing `queryDoh` to reject and the existing `Promise.any`/third-provider fallback to continue. Do not rewrite responses.

- [x] **Step 7: Run focused Rust and Worker tests, then commit**

Run:

```bash
cargo test -p dns_relay zero_only
cargo test -p dns_relay upstream_failure_returns_stale_cached_answer
node assets/relay_worker_test.mjs
```

Expected: zero-only answers trigger secondary/fallback behavior, ordinary answers pass, and stale-if-error remains intact.

Commit:

```bash
git add dns_relay/src/resolver.rs dns_relay/src/client.rs dns_relay/src/tests.rs assets/relay_worker.js assets/relay_worker_test.mjs
git commit -m "feat: reject zero-address DNS sinkholes"
```

---

### Task 5: User-facing configuration and complete verification

**Files:**
- Modify: `conf.toml`
- Modify: `dns_relay/README.md`
- Modify: `docs/superpowers/plans/2026-08-28-secure-dns-responses.md`

**Interfaces:**
- Documents: `secure_only`, `client_subnet`, automatic discovery, IPv4-only ECS, fail-closed behavior, and no-ECS continuation after discovery failure.
- Verifies: the whole workspace and Worker asset without adding tooling.

- [x] **Step 1: Document the minimal configuration**

Add commented safe examples without changing the default deployment behavior:

```toml
# Reject unauthenticated UDP upstreams. At least one DoH, DoQ, or HTTPS relay is required.
secure_only = false

# Optional IPv4 /24 override; omit to discover through a direct relay Worker.
# client_subnet = "8.8.8.0/24"
```

In `dns_relay/README.md`, state that discovery failure continues with authenticated DNS but without ECS, and that Google-chained-only setups need the override when exact geography is required.

- [x] **Step 2: Run formatting and the complete test suite**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
node assets/relay_worker_test.mjs
```

Expected: every Rust and Worker test passes.

- [x] **Step 3: Run strict lint and build checks**

Run:

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets
```

Expected: no errors or warnings promoted by Clippy. Existing workspace profile-location notices may still appear from Cargo.

- [x] **Step 4: Inspect the final diff for scope and secrets**

Run:

```bash
git diff --check
git status --short
git diff --stat master...HEAD
git diff master...HEAD -- . ':!Cargo.lock'
```

Confirm there are no relay keys, full discovered IPs, new dependencies, protocol-envelope changes, unrelated refactors, or accidental `Cargo.lock` edits.

- [x] **Step 5: Mark the plan complete and commit documentation**

Change completed checkboxes in this plan to `[x]`, then commit only the intended documentation and configuration changes:

```bash
git add conf.toml dns_relay/README.md docs/superpowers/plans/2026-08-28-secure-dns-responses.md
git commit -m "docs: explain secure DNS response mode"
```

- [x] **Step 6: Record final branch evidence**

Run:

```bash
git status --short --branch
git log --oneline --decorate master..HEAD
```

Expected: clean `feat/secure-dns-responses` branch with the design commit plus five focused implementation/documentation commits.
