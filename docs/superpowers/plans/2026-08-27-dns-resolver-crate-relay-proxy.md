# DNS Resolver Crate And Relay Proxy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish `dns_relay` as a reusable resolver crate and use it in `relay-proxy` for fail-closed upstream resolution and faster direct HTTPS tunnelling.

**Architecture:** Add one `DnsResolver` facade over the existing resolver transports, cache, and miss coalescing. Keep direct HTTPS encrypted end-to-end with a raw Tokio TCP tunnel; retain local CA termination only for Cloudflare/Google HTTP relay rules.

**Tech Stack:** Rust 2024, Tokio, reqwest/rustls, existing DNS relay modules, Cargo workspaces, GitHub Actions, crates.io.

**Spec:** `docs/superpowers/specs/2026-08-27-dns-resolver-crate-relay-proxy-design.md`

## Global Constraints

- Work only in `/Users/vangabond/projects/dns-relay` and `/Users/vangabond/Documents/Projects/relay-proxy`.
- Do not implement the cancelled OS-transparent interception mode.
- Use Rust `1.85` as the minimum supported version.
- Add no new runtime dependency beyond the local/published `dns_relay` dependency in `relay-proxy`.
- Direct HTTPS must never terminate TLS; relay-enabled HTTPS must keep the local CA and Google Apps Script compatibility.
- Resolver failures must not fall back to the operating-system resolver.
- Keep buffers, caches, queues, request bodies, and retries bounded.
- Never print or commit crates.io tokens, DNS relay keys, or Google API keys.
- Run each task's focused check before committing it.
- After every task, update `docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md` with results and the exact next task.
- Use `rtk` as the prefix for every shell command.

## File Map

### dns-relay repository

- `LICENSE`: repository MIT license.
- `Cargo.toml`: shared publish metadata and version.
- `shared/Cargo.toml`: internal `dns-relay-shared` package metadata.
- `dns_relay/Cargo.toml`: public package metadata and versioned local dependency.
- `dns_relay/src/client.rs`: public programmatic resolver and shared transport selection.
- `dns_relay/src/lib.rs`: public resolver exports.
- `dns_relay/src/handler.rs`: reuse shared transport selection while preserving daemon policy/metrics.
- `dns_relay/src/resolver.rs`: remove the per-lookup public-IP HTTP dependency.
- `dns_relay/src/tests.rs`: daemon regression coverage if handler signatures change.
- `dns_relay/README.md`: crate installation and programmatic configuration.
- `.github/workflows/publish-crates.yml`: tag-gated crates.io publication.
- `.github/workflows/test.yml`: workspace library and binary tests.
- `scripts/bump.sh`: commit the lock-file version with each release tag.
- `docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`: durable progress state.

### relay-proxy repository

- `Cargo.toml`: local-plus-registry `dns_relay` dependency and reqwest version alignment.
- `config.toml`: explicit DNS resolver configuration.
- `src/helper/config.rs`: deserialize `ResolverConfig`.
- `src/net/proxy.rs`: resolver-backed connects and unchanged TCP/TLS tunnelling.
- `src/rules/rule_parser.rs`: catch-all domain semantics.
- `src/rules/rule_engine.rs`: shared resolver, optional CA acceptor, rule validation, and pre-sorted providers.
- `src/net/relay.rs`: consume providers in their pre-sorted order and reduce hot-path logging.
- `src/main.rs`: await asynchronous engine construction and remove unused system resolver code.
- `README.md`: direct versus relayed HTTPS, `[dns]`, wildcard rule, and CA behavior.

---

### Task 1: Make Both DNS Workspace Packages Publishable

**Files:**
- Create: `/Users/vangabond/projects/dns-relay/LICENSE`
- Modify: `/Users/vangabond/projects/dns-relay/Cargo.toml`
- Modify: `/Users/vangabond/projects/dns-relay/shared/Cargo.toml`
- Modify: `/Users/vangabond/projects/dns-relay/dns_relay/Cargo.toml`
- Modify: `/Users/vangabond/projects/dns-relay/Cargo.lock`
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`

**Interfaces:**
- Consumes: existing workspace version `1.6.8`.
- Produces: packages `dns-relay-shared` and `dns_relay`; source code continues importing the internal crate as `shared`.

- [ ] **Step 1: Reproduce the current packaging failure**

Run:

```bash
rtk cargo package -p dns_relay --allow-dirty --no-verify
```

Expected: FAIL because `shared` has a path but no registry version.

- [ ] **Step 2: Add the MIT license**

Create `LICENSE` with the standard MIT text and this copyright line:

```text
Copyright (c) 2026 Mohammadreza Jafari
```

- [ ] **Step 3: Add workspace publication metadata**

Extend `[workspace.package]` in the root manifest:

```toml
version = "1.6.8"
rust-version = "1.85"
license = "MIT"
repository = "https://github.com/v4g4b0nd-0x76/dns-hijacker"
```

Change `shared/Cargo.toml` package metadata to:

```toml
[package]
name = "dns-relay-shared"
version.workspace = true
edition = "2024"
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "Internal DNS packet, cache, policy, and logging support for dns_relay"
```

Extend `dns_relay/Cargo.toml` package metadata with:

```toml
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "Reusable encrypted DNS resolver and configurable DNS relay server"
readme = "README.md"
keywords = ["dns", "resolver", "doh", "doq", "relay"]
categories = ["network-programming"]
```

Change its local dependency to:

```toml
shared = { package = "dns-relay-shared", path = "../shared", version = "1" }
```

- [ ] **Step 4: Refresh the lock file and format manifests**

Run:

```bash
rtk cargo check --workspace
```

Expected: PASS and `Cargo.lock` names `dns-relay-shared` at workspace version `1.6.8`.

- [ ] **Step 5: Verify both package archives**

Run:

```bash
rtk cargo package -p dns-relay-shared --allow-dirty
rtk cargo package -p dns_relay --allow-dirty --no-verify
```

Expected: both `.crate` archives are created. The public package uses
`--no-verify` only because its internal dependency has not had its first registry
publication yet.

- [ ] **Step 6: Record and commit checkpoint 2**

Update the checkpoint with the commands above, then run:

```bash
rtk git add LICENSE Cargo.toml Cargo.lock shared/Cargo.toml dns_relay/Cargo.toml docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md
rtk git commit -m "chore: make dns resolver crates publishable"
```

---

### Task 2: Add the Programmatic Resolver API

**Files:**
- Create: `/Users/vangabond/projects/dns-relay/dns_relay/src/client.rs`
- Modify: `/Users/vangabond/projects/dns-relay/dns_relay/src/lib.rs`
- Modify: `/Users/vangabond/projects/dns-relay/dns_relay/src/resolver.rs`
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`

**Interfaces:**
- Consumes: `ResolverPicker`, `RelayPicker`, `RelayConf`, `DoqPool`, `UdpDispatcher`, and `build_http_client`.
- Produces: `ResolverConfig`, `DnsResolver::new(ResolverConfig)`, and `DnsResolver::resolve_ipv4(&self, &str) -> Result<Vec<Ipv4Addr>, Error>`.

- [ ] **Step 1: Write failing API tests in `client.rs`**

Add tests that start a loopback UDP resolver, answer the health probe and one A
query with `127.0.0.42`, then exercise the public API:

```rust
#[tokio::test]
async fn resolves_ipv4_with_programmatic_config() {
    let (upstream, server) = mock_udp_resolver(Ipv4Addr::new(127, 0, 0, 42), 2).await;
    let resolver = DnsResolver::new(ResolverConfig {
        resolvers: vec![upstream.to_string()],
        relay: None,
    })
    .await
    .unwrap();

    assert_eq!(
        resolver.resolve_ipv4("example.test").await.unwrap(),
        vec![Ipv4Addr::new(127, 0, 0, 42)]
    );
    server.await.unwrap();
}

#[tokio::test]
async fn rejects_empty_resolver_configuration() {
let error = DnsResolver::new(ResolverConfig {
        resolvers: Vec::new(),
        relay: None,
    })
    .await
    .err()
    .expect("empty resolver configuration must fail");
    assert!(error.to_string().contains("at least one resolver"));
}
```

The mock uses existing `parse_domain` and `craft_redirect_response`, so it adds
no network-test dependency.

- [ ] **Step 2: Run the tests and observe the missing API**

Run:

```bash
rtk cargo test -p dns_relay client::tests -- --nocapture
```

Expected: FAIL because `DnsResolver` and `ResolverConfig` do not exist.

- [ ] **Step 3: Implement the minimum public facade**

Create these types in `client.rs`:

```rust
#[derive(Clone, serde::Deserialize)]
pub struct ResolverConfig {
    pub resolvers: Vec<String>,
    #[serde(default)]
    pub relay: Option<RelayConf>,
}

pub struct DnsResolver {
    http: reqwest::Client,
    picker: ResolverPicker,
    relay_picker: Option<RelayPicker>,
    doq_pool: Arc<DoqPool>,
    udp_dispatcher: Arc<UdpDispatcher>,
}
```

`DnsResolver::new` must reject an empty resolver list, build each long-lived
transport once, and construct `RelayPicker` only for `Some(conf)` where
`conf.enable` is true. `resolve_ipv4` builds one A query, resolves it through the
configured relay picker or direct picker, parses A records, and returns an error
when the response contains no A record.

Use this source address for library lookups:

```rust
SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
```

- [ ] **Step 4: Remove the external public-IP lookup from resolver bootstrap**

In `ResolverPicker::resolve`, replace `get_public_ip(http).await` and its Iranian
fallback IP with the same unspecified source address. Remove the now-unused
`get_public_ip` import from `resolver.rs`; keep the helper only if another caller
still uses it.

- [ ] **Step 5: Export and test the API**

Add to `lib.rs`:

```rust
mod client;
pub use client::{DnsResolver, ResolverConfig};
```

Run:

```bash
rtk cargo fmt --all
rtk cargo test -p dns_relay client::tests
rtk cargo test --workspace
```

Expected: all checks PASS.

- [ ] **Step 6: Record and commit checkpoint 3**

```bash
rtk git add dns_relay/src/client.rs dns_relay/src/lib.rs dns_relay/src/resolver.rs dns_relay/src/tests.rs docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md
rtk git commit -m "feat: expose reusable DNS resolver client"
```

---

### Task 3: Reuse Cache, Miss Coalescing, and Transport Selection

**Files:**
- Modify: `/Users/vangabond/projects/dns-relay/dns_relay/src/client.rs`
- Modify: `/Users/vangabond/projects/dns-relay/dns_relay/src/handler.rs`
- Modify: `/Users/vangabond/projects/dns-relay/dns_relay/src/tests.rs`
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`

**Interfaces:**
- Consumes: Task 2's `DnsResolver`, existing `ResponseCache`, and existing `InFlightQueries`.
- Produces: one crate-private `resolve_transport` function used by both the public client and DNS-server handler; cached/coalesced `resolve_ipv4` calls.

- [ ] **Step 1: Write failing cache and coalescing tests**

Extend the loopback resolver with an `Arc<AtomicUsize>` query counter. Reset it
after `DnsResolver::new` completes its health probe, then assert:

```rust
let first = resolver.resolve_ipv4("cached.test").await.unwrap();
let second = resolver.resolve_ipv4("cached.test").await.unwrap();
assert_eq!(first, second);
assert_eq!(queries.load(Ordering::Relaxed), 1);
```

For miss coalescing, place `Arc<DnsResolver>` behind 32 spawned tasks, release
them with a `Barrier`, and assert every answer is `127.0.0.42` while the upstream
counter is exactly one.

- [ ] **Step 2: Run the focused tests and confirm repeated upstream work**

```bash
rtk cargo test -p dns_relay client::tests -- --nocapture
```

Expected: the new count assertions FAIL because Task 2 has no client-side cache
or in-flight sharing.

- [ ] **Step 3: Extract shared transport selection**

Add a crate-private function with this exact boundary:

```rust
pub(crate) async fn resolve_transport(
    domain: &str,
    payload: &[u8],
    src_addr: SocketAddr,
    prefer_doh: bool,
    picker: &ResolverPicker,
    relay_picker: Option<&RelayPicker>,
    http: &reqwest::Client,
    doq_pool: &DoqPool,
    udp_dispatcher: &UdpDispatcher,
) -> Result<Vec<u8>, Error>
```

It applies the existing relay timeout/instance selection when a relay picker is
present; otherwise it calls `ResolverPicker::resolve_packet`. Replace the same
branch in `handler::resolve_query` with this function without changing policy,
metrics, history, stale-cache behavior, or transaction-ID restoration.

- [ ] **Step 4: Add bounded cache and in-flight state to `DnsResolver`**

Add:

```rust
cache: Arc<ResponseCache>,
in_flight: Arc<InFlightQueries>,
```

`resolve_ipv4` must use `cache_key_from_query`, `cache_lookup`, `cache_store`,
`cache_lookup_stale`, and the existing leader/follower API. Normalize published
transaction IDs to zero and restore the caller's ID before parsing. On complete
transport failure, return a stale A answer when available; otherwise return the
original resolver error. Never craft a successful answer or call system DNS.

- [ ] **Step 5: Run focused and workspace checks**

```bash
rtk cargo fmt --all
rtk cargo test -p dns_relay client::tests
rtk cargo test -p dns_relay
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all checks PASS and existing daemon cache/metrics tests remain green.

- [ ] **Step 6: Record and commit checkpoint 4**

```bash
rtk git add dns_relay/src/client.rs dns_relay/src/handler.rs dns_relay/src/tests.rs docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md
rtk git commit -m "refactor: share DNS resolution core"
```

---

### Task 4: Add crates.io Publication Automation And Library Documentation

**Files:**
- Create: `/Users/vangabond/projects/dns-relay/.github/workflows/publish-crates.yml`
- Modify: `/Users/vangabond/projects/dns-relay/.github/workflows/test.yml`
- Modify: `/Users/vangabond/projects/dns-relay/scripts/bump.sh`
- Modify: `/Users/vangabond/projects/dns-relay/dns_relay/README.md`
- Modify: `/Users/vangabond/projects/dns-relay/README.md`
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`

**Interfaces:**
- Consumes: publishable Task 1 packages and Task 3 public API.
- Produces: tag-triggered publication using `CARGO_REGISTRY_TOKEN`; copy-ready crate usage documentation.

- [ ] **Step 1: Add a failing workflow-contract check**

Before creating the workflow, run:

```bash
rtk ls -l .github/workflows/publish-crates.yml
```

Expected: FAIL because the workflow does not exist.

- [ ] **Step 2: Create the tag-gated workflow**

Use this job boundary in `publish-crates.yml`:

```yaml
name: Publish crates
on:
  push:
    tags: ["v*"]
permissions:
  contents: read
jobs:
  publish:
    runs-on: ubuntu-latest
    env:
      CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

Use these shell steps after checkout, stable-toolchain, and Cargo-cache steps:

```yaml
- name: Verify tag version
  shell: bash
  run: |
    version="${GITHUB_REF_NAME#v}"
    manifest_version="$(cargo metadata --no-deps --format-version 1 |
      jq -r '.packages[] | select(.name == "dns_relay") | .version')"
    test "$version" = "$manifest_version"
- name: Verify workspace
  run: |
    cargo fmt --all -- --check
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
- name: Package crates
  run: |
    cargo package -p dns-relay-shared --locked
    cargo package -p dns_relay --locked --no-verify
- name: Publish internal crate
  run: cargo publish -p dns-relay-shared --locked
- name: Wait for internal crate
  shell: bash
  run: |
    version="${GITHUB_REF_NAME#v}"
    for attempt in {1..12}; do
      cargo info --registry crates-io "dns-relay-shared@${version}" && exit 0
      sleep 10
    done
    exit 1
- name: Publish public crate
  run: cargo publish -p dns_relay --locked
```

Do not add `continue-on-error`, token output, or duplicate-version suppression.

- [ ] **Step 3: Make release bumps update `Cargo.lock`**

After `scripts/bump.sh` rewrites the workspace version, run
`cargo check --workspace` and stage both files:

```bash
cargo check --workspace
git add Cargo.toml Cargo.lock
```

This keeps tag builds compatible with the publication workflow's `--locked`
checks.

- [ ] **Step 4: Test libraries and binaries in CI**

Replace the two binary-only test jobs in `test.yml` with one workspace job that
runs:

```yaml
- run: cargo fmt --all -- --check
- run: cargo test --workspace
- run: cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: Document the public API and release prerequisite**

Add the exact dependency and Rust example from the specification to
`dns_relay/README.md`. In the root README document that maintainers must create
a crates.io API token with publish permission and save it as the GitHub Actions
secret `CARGO_REGISTRY_TOKEN`; the next `PUSH=1 make patch` tag triggers both the
binary release and crate publication workflows.

- [ ] **Step 6: Validate workflow syntax and packages locally**

```bash
rtk ruby -e 'require "yaml"; Dir[".github/workflows/*.yml"].each { |f| YAML.load_file(f, aliases: true) }'
rtk cargo fmt --all -- --check
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo package -p dns-relay-shared --allow-dirty
rtk cargo package -p dns_relay --allow-dirty --no-verify
```

Expected: every local check PASS. Do not push a tag or publish from the local
machine.

- [ ] **Step 7: Record and commit checkpoint 5**

```bash
rtk git add .github/workflows/publish-crates.yml .github/workflows/test.yml scripts/bump.sh README.md dns_relay/README.md docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md
rtk git commit -m "ci: publish dns resolver crates on tags"
```

---

### Task 5: Integrate `DnsResolver` Into relay-proxy

**Files:**
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/Cargo.toml`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/config.toml`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/helper/config.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/net/proxy.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/rules/rule_engine.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/main.rs`
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`

**Interfaces:**
- Consumes: `dns_relay::{DnsResolver, ResolverConfig}` from Tasks 2-3.
- Produces: asynchronous `RuleEngine::new`, resolver-backed `resolve_upstream`, and no system DNS fallback.

- [ ] **Step 1: Add failing configuration and upstream tests**

Add a config parser test that expects:

```toml
[dns]
resolvers = ["127.0.0.1:5353"]
```

to deserialize into `Config::dns.resolvers`. Add a loopback DNS test for:

```rust
let addr = resolve_upstream(&resolver, "upstream.test:8443").await.unwrap();
assert_eq!(addr, "127.0.0.42:8443".parse().unwrap());
```

Also assert `resolve_upstream` returns an error after the mock resolver fails;
the test must not succeed through system DNS.

- [ ] **Step 2: Run tests and observe missing dependency/configuration**

```bash
rtk cargo test --all-targets
```

Expected: FAIL because `Config::dns` and resolver-backed upstream resolution do
not exist.

- [ ] **Step 3: Add the local-plus-registry dependency**

Use the portable relative path from `relay-proxy` to the local checkout:

```toml
dns_relay = { version = "1", path = "../../../projects/dns-relay/dns_relay" }
```

Align relay-proxy's existing reqwest dependency to `0.12` so Cargo does not
compile two reqwest major versions for the same process.

- [ ] **Step 4: Deserialize and construct the resolver**

Add:

```rust
pub struct Config {
    pub dns: dns_relay::ResolverConfig,
    // existing fields remain unchanged
}
```

Update `config.toml` with explicit DoH and UDP resolvers. Change
`RuleEngine::new` to `pub async fn new(...) -> Result<Self, Error>`, construct
`Arc::new(DnsResolver::new(conf.dns.clone()).await?)`, and await it in `main`.
Map resolver errors into `relay_proxy::error::Error::Other` without exposing
relay keys.

- [ ] **Step 5: Replace system upstream lookup**

Use this signature:

```rust
pub async fn resolve_upstream(
    resolver: &DnsResolver,
    upstream: &str,
) -> Result<SocketAddr, Error>
```

Return parsed `SocketAddr` values immediately. Otherwise split `host:port`,
validate the port, call `resolve_ipv4(host)`, select the first address, and
construct `SocketAddr`. Pass the shared resolver through `connect_upstream`,
`proxy_connection`, socket handlers, and connection handlers. Delete every
`lookup_host` import and call.

- [ ] **Step 6: Run focused and complete checks**

```bash
rtk cargo fmt --all
rtk cargo test --all-targets
rtk cargo clippy --all-targets -- -D warnings
rtk cargo tree -d
```

Expected: tests and Clippy PASS; `cargo tree -d` contains no duplicate reqwest
`0.12`/`0.13` pair.

- [ ] **Step 7: Commit relay-proxy and record checkpoint 6**

In `relay-proxy`:

```bash
rtk git add Cargo.toml Cargo.lock config.toml src/helper/config.rs src/net/proxy.rs src/rules/rule_engine.rs src/main.rs
rtk git commit -m "feat: resolve proxy upstreams with dns_relay"
```

Then update the central checkpoint with the relay-proxy commit ID and commit the
checkpoint in `dns-relay`:

```bash
rtk git add docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md
rtk git commit -m "docs: checkpoint proxy resolver integration"
```

---

### Task 6: Tunnel Direct HTTPS And Initialize CA Only For Relay Rules

**Files:**
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/net/proxy.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/rules/rule_engine.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/main.rs`
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`

**Interfaces:**
- Consumes: Task 5's resolver-backed `proxy_connection`.
- Produces: direct raw TLS tunnelling; `RuleEngine::tls_acceptor: Option<TlsAcceptor>` used only by relay-enabled HTTPS.

- [ ] **Step 1: Write failing direct-tunnel and CA-need tests**

Add a loopback proxy test that sends bytes beginning with a TLS record marker,
has the upstream echo them, and asserts exact equality in both directions:

```rust
let payload = b"\x16\x03\x03direct-tls-payload";
client.write_all(payload).await.unwrap();
client.shutdown().await.unwrap();
let mut echoed = Vec::new();
client.read_to_end(&mut echoed).await.unwrap();
assert_eq!(echoed, payload);
```

Add unit assertions for `rules_need_ca`: false for direct-only rules and true
when any rule has `relay_config: Some(_)`.

Add `relay_https_keeps_tls_acceptor`, which creates a temporary CA with
`load_or_generate_ca`, builds the acceptor, completes a loopback rustls
client/server handshake that trusts that CA, and asserts both handshake tasks
succeed. This protects the relay-only TLS termination path while the direct path
is deleted.

- [ ] **Step 2: Run focused tests and observe current MITM behavior boundary**

```bash
rtk cargo test direct_https_tunnels_bytes_unchanged -- --nocapture
rtk cargo test rules_need_ca -- --nocapture
```

Expected: FAIL because the CA is unconditional and direct HTTPS calls
`handle_https`.

- [ ] **Step 3: Delete direct HTTPS interception**

For a direct HTTPS match, call `proxy_connection` with the untouched client
stream, selected destination, protocol `"https"`, and SNI target. Delete
`handle_https`, `TlsConnector`, upstream root-store construction, and their
unused imports. Keep `copy_bidirectional` as the tunnel primitive.

- [ ] **Step 4: Make relay CA initialization conditional**

After loading and validating rules, compute:

```rust
fn rules_need_ca(rules: &[Rule]) -> bool {
    rules.iter().any(|rule| rule.relay_config.is_some())
}
```

Store `Option<TlsAcceptor>`. Generate/install/load the CA only when the helper is
true. In the relay HTTPS branch, require the acceptor and return a configuration
error if the invariant is broken. Keep `gen-cert` and `install-cert` unchanged.

- [ ] **Step 5: Run complete relay-proxy checks**

```bash
rtk cargo fmt --all
rtk cargo test --all-targets
rtk cargo clippy --all-targets -- -D warnings
rtk cargo build --profile release-perf
```

Expected: all checks PASS and direct-only startup does not touch CA files.

- [ ] **Step 6: Commit relay-proxy and record checkpoint 7**

```bash
rtk git add src/net/proxy.rs src/rules/rule_engine.rs src/main.rs
rtk git commit -m "perf: tunnel direct HTTPS without interception"
```

Update and commit the central checkpoint in `dns-relay` with the new relay-proxy
commit and verification results.

---

### Task 7: Add Catch-All Relay Rules And Remove Remaining Hot-Path Work

**Files:**
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/net/proxy.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/net/relay.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/rules/rule_parser.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/src/rules/rule_engine.rs`
- Modify: `/Users/vangabond/Documents/Projects/relay-proxy/README.md`
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`

**Interfaces:**
- Consumes: existing `Rule`, `RuleWrapper`, and provider fallback.
- Produces: `domains = ["*"]` fallback semantics, deterministic rule priority, startup validation, and providers sorted once.

- [ ] **Step 1: Write failing rule and validation tests**

Add these behaviors:

```rust
assert!(catch_all.matches_domain("anything.example"));
assert!(specific.matches_domain("api.example.com"));
assert!(Arc::ptr_eq(
    &find_rule(&[catch_all_wrapper, specific_wrapper.clone()], "api.example.com").unwrap(),
    &specific_wrapper,
));
assert!(validate_rules(&[catch_all_80_a, catch_all_80_b]).is_err());
assert!(validate_rules(&[catch_all_80, catch_all_443]).is_ok());
```

Add a provider-order test that constructs priorities `10, 100, 50`, creates a
`RuleWrapper`, and asserts stored order `100, 50, 10`.

Add `relay_falls_back_to_second_provider`: bind a loopback HTTP server that
returns this body, configure an unreachable first Cloudflare URL and the local
server as the second provider, write through a Tokio duplex stream, and assert
the client side begins with HTTP status 200:

```json
{"status":200,"headers":{"content-type":["text/plain"]},"body_base64":"b2s=","relay_error":null}
```

```rust
assert!(relay_request_result.is_ok());
assert!(response.starts_with(b"HTTP/1.1 200"));
assert!(response.ends_with(b"ok"));
```

- [ ] **Step 2: Run tests and observe missing wildcard/validation behavior**

```bash
rtk cargo test rule -- --nocapture
```

Expected: new assertions FAIL.

- [ ] **Step 3: Implement deterministic catch-all behavior**

Make `Rule::matches_domain` return true for a literal `"*"`. Change `find_rule`
to search matching non-catch-all rules first, then a catch-all. Add
`validate_rules(&[Rule]) -> Result<(), Error>` that counts catch-all rules per
port and reports the duplicate port. Call it before constructing wrappers or
opening sockets.

- [ ] **Step 4: Sort providers once**

In `RuleWrapper::new(mut rule)`, sort `rule.relay_config.providers` descending by
priority. In `relay_request`, iterate `config.providers.iter()` directly and
delete the temporary vector and per-request sort.

- [ ] **Step 5: Remove small per-connection overhead**

Change protocol detection from:

```rust
let mut buf = vec![0u8; PEEK_BUF_SIZE];
```

to:

```rust
let mut buf = [0u8; PEEK_BUF_SIZE];
```

Move successful byte-count and provider-response tracing from INFO to DEBUG.
Keep warnings, failures, and configured access/error logs unchanged.

- [ ] **Step 6: Document the wildcard relay rule**

Add a README example with `domains = ["*"]`, ports `80, 443`, no direct
upstreams, and both Cloudflare and Google providers. State that it covers only
traffic sent to configured listeners and that HTTPS relay rules require the
local CA.

- [ ] **Step 7: Verify and commit checkpoint 8**

```bash
rtk cargo fmt --all
rtk cargo test --all-targets
rtk cargo clippy --all-targets -- -D warnings
rtk cargo build --profile release-perf
rtk git add src/net/proxy.rs src/net/relay.rs src/rules/rule_parser.rs src/rules/rule_engine.rs README.md
rtk git commit -m "perf: add catch-all relay routing"
```

Update and commit the central checkpoint in `dns-relay`.

---

### Task 8: Perform Cross-Repository Release Verification

**Files:**
- Modify: `/Users/vangabond/projects/dns-relay/docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`
- Modify only if verification exposes a defect: files already listed in Tasks 1-7.

**Interfaces:**
- Consumes: every preceding checkpoint.
- Produces: two clean repositories, package artifacts, release-ready workflow, and exact handoff instructions for the first crates.io publication.

- [ ] **Step 1: Verify dns-relay from a clean command sequence**

```bash
rtk cargo fmt --all -- --check
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
rtk cargo package -p dns-relay-shared --allow-dirty
rtk cargo package -p dns_relay --allow-dirty --no-verify
rtk git diff --check
rtk git status --short
```

Expected: all checks PASS; status is clean before checkpoint updates.

- [ ] **Step 2: Verify relay-proxy from a clean command sequence**

```bash
rtk cargo fmt --all -- --check
rtk cargo test --all-targets
rtk cargo clippy --all-targets -- -D warnings
rtk cargo build --profile release-perf
rtk cargo tree -d
rtk git diff --check
rtk git status --short
```

Expected: all checks PASS, no reqwest major-version duplication, and status is
clean.

- [ ] **Step 3: Inspect packaged public files**

```bash
rtk cargo package -p dns_relay --allow-dirty --no-verify --list
rtk cargo package -p dns-relay-shared --allow-dirty --list
```

Expected: source, manifest, README, and license metadata are present; no config
containing keys, logs, runtime state, or build output is packaged.

- [ ] **Step 4: Write the final resume/publication checkpoint**

Record both repository HEADs, every verification result, and these external
steps that remain user-controlled:

1. Create a scoped crates.io publish token.
2. Add it to the GitHub repository as `CARGO_REGISTRY_TOKEN`.
3. Run `make patch PUSH=1` to create and push `v1.6.9` or the next unused
   version.
4. Confirm `Publish crates`, `Test`, and `Release` GitHub Actions succeed.

- [ ] **Step 5: Commit the final checkpoint**

```bash
rtk git add docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md
rtk git commit -m "docs: record resolver crate release checkpoint"
```

Do not create a crates.io token, alter GitHub secrets, push a tag, or publish a
crate during implementation; those are explicit external-state actions for the
user after code review.
