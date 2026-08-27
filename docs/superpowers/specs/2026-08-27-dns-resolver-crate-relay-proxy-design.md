# DNS Resolver Crate And Relay Proxy Design

**Date:** 2026-08-27

## Goal

Publish the existing `dns_relay` resolver as a reusable Rust crate, consume it
from `relay-proxy` without running a DNS daemon, and make direct HTTPS routing
faster by tunnelling TLS unchanged. HTTPS requests sent through the existing
Cloudflare or Google Apps Script HTTP relays continue to use the local CA
because those providers require decoded HTTP requests.

## Scope

- Add a small programmatic IPv4 resolver API to `dns_relay`.
- Reuse the DNS relay's existing cache, concurrent-miss coalescing, resolver
  health selection, UDP multiplexing, DoH pooling, DoQ pooling, and encrypted
  DNS-relay transports.
- Make the workspace packages publishable on crates.io.
- Publish from GitHub Actions on version tags.
- Replace `relay-proxy` system DNS lookups with the library resolver.
- Tunnel direct HTTPS without terminating TLS.
- Preserve CA-based HTTPS termination for HTTP relay rules.
- Add a catch-all domain rule for relaying every domain received on configured
  listener ports.

The cancelled OS-transparent "all" mode is not in scope. The proxy listens only
on ports explicitly configured by its rules and does not install PF, nftables,
iptables, routing, or Network Extension configuration.

## Repository Boundaries

The work spans two repositories:

- `/Users/vangabond/projects/dns-relay`: resolver API, crate packaging,
  crates.io workflow, documentation, and resolver tests.
- `/Users/vangabond/Documents/Projects/relay-proxy`: crate consumption, DNS
  configuration, direct TLS tunnelling, catch-all rules, relay hot-path cleanup,
  and proxy tests.

No third repository or new service is introduced.

## Crate Packaging

The existing `dns_relay` package remains both a library and a binary. Adding it
as a dependency builds and links the library; it does not run the DNS server.
The user-facing dependency is:

```toml
dns_relay = "1"
```

The current local `shared` package is published as `dns-relay-shared`. Source
code may continue importing it through the local alias `shared`:

```toml
shared = { package = "dns-relay-shared", path = "../shared", version = "1" }
```

This is the smallest publishable layout because crates.io does not accept a
normal dependency that has only a local path. Consumers do not add the internal
package directly; Cargo obtains it as a transitive dependency.

Both packages use the workspace version and MIT license. Their manifests gain
descriptions, repository links, README paths, keywords, categories, and
`rust-version = "1.85"`, the first stable Rust release for edition 2024. A root
`LICENSE` file is included in both crate packages. The public README contains
one minimal resolver example and
clearly separates the library from the DNS-server binary.

No feature matrix or additional resolver-only subcrate is added in this change.
The first useful public API is kept small; dependency/compile-time splitting can
be justified later with measurements.

## Public Resolver API

The public API is intentionally limited to one configuration value and one
long-lived resolver:

```rust
use dns_relay::{DnsResolver, ResolverConfig};

let resolver = DnsResolver::new(ResolverConfig {
    resolvers: vec![
        "https://cloudflare-dns.com/dns-query".into(),
        "1.1.1.1:53".into(),
    ],
    relay: None,
})
.await?;

let addresses = resolver.resolve_ipv4("example.com").await?;
```

`ResolverConfig` is constructible directly in Rust and deserializable for
applications that already use a configuration file. It contains:

- A non-empty list of direct DoH, UDP, or DoQ resolvers.
- An optional existing DNS `RelayConf` for direct Cloudflare Worker or
  Google-chained encrypted DNS resolution.

`DnsResolver` owns and reuses one HTTP client, one UDP dispatcher, one DoQ pool,
one resolver picker, an optional relay picker, the TTL response cache, and
in-flight query state. Its public `resolve_ipv4` name makes the current A-record
limit explicit. IPv6 is not silently approximated and can be added as a later,
separately tested API.

The reusable core also has an internal raw-packet method used by the DNS-server
handler. The handler keeps drop/redirect policy, metrics, and optional history
around that core. Direct library calls do not construct daemon configuration,
bind port 53, alter system DNS, run Netguard, read rule files, or start background
tasks unrelated to resolution.

## Resolution Flow

For an ordinary library lookup:

```text
domain
  -> build A query
  -> fresh TTL cache hit
  -> join matching in-flight miss, or become leader
  -> optional encrypted DNS relay, otherwise healthy direct resolver hedge
  -> validate DNS response
  -> cache response and publish to waiters
  -> parse A records
```

IP-literal upstreams in `relay-proxy` bypass DNS. A domain upstream is resolved
only through `DnsResolver`. If resolution, stale-cache recovery, and every
configured direct or relay path fail, the connection fails closed. There is no
fallback to `tokio::net::lookup_host` or the operating-system resolver.

The current per-domain helper's public-IP HTTP lookup is removed from the
connection path. Raw packet resolution already receives the client address when
ECS scoping is required; library hostname resolution uses a stable unspecified
client address and does not perform an external IP lookup before DNS.

## Relay Proxy Configuration

`relay-proxy/config.toml` gains a required `[dns]` section that deserializes to
the library configuration:

```toml
[dns]
resolvers = [
  "https://cloudflare-dns.com/dns-query",
  "1.1.1.1:53",
]
```

An optional nested DNS relay uses the `dns_relay` relay configuration. This is
distinct from a rule's existing `relay_config`:

- `[dns]` decides how proxy upstream hostnames are resolved.
- Per-rule `relay_config` decides whether decoded HTTP requests are sent through
  Cloudflare or Google Apps Script.

The distinction is documented with separate names and examples so relay keys
are not accidentally placed in the wrong protocol configuration.

`RuleEngine::new` becomes asynchronous because resolver construction performs
initial resolver health checks. The engine stores one shared `Arc<DnsResolver>`
for every listener and connection.

## HTTPS Routing

### Direct HTTPS

```text
client ClientHello
  -> inspect SNI without consuming bytes
  -> match specific rule
  -> choose configured upstream
  -> resolve domain upstream through DnsResolver when needed
  -> connect
  -> copy both byte streams unchanged
```

The client negotiates TLS end-to-end with the selected upstream. The proxy does
not generate a leaf certificate, decrypt application data, build a second TLS
client configuration, or perform a second TLS handshake. Existing byte-count
access logging remains available after the tunnel closes.

### Relayed HTTPS

```text
client ClientHello
  -> inspect SNI
  -> match relay rule
  -> accept TLS with local CA certificate
  -> read one bounded HTTP request
  -> try pre-sorted Cloudflare/Google providers in priority order
  -> write provider response to client
```

This path deliberately keeps TLS termination and the self-signed local CA
because the existing providers receive HTTP method, URL, headers, and body. CA
loading/generation and automatic trust installation occur at engine startup only
when at least one loaded rule has an HTTP relay configuration. The explicit
`gen-cert` and `install-cert` commands remain available.

Google Apps Script compatibility keeps bounded request buffering and its JSON
plus base64 protocol. Streaming or a new byte-tunnel protocol is not part of
this change.

## Catch-All Relay Rule

`domains = ["*"]` means fallback for every HTTP Host or TLS SNI received on the
rule's configured listener ports. Domain-specific rules always have precedence
regardless of filesystem enumeration order. Engine startup rejects more than
one catch-all rule for the same port because the result would otherwise be
ambiguous.

A catch-all rule is sufficient to use Cloudflare or Google Apps Script for all
domains that arrive at configured proxy ports. It does not capture arbitrary
operating-system traffic.

## Performance Changes

The direct HTTPS improvement comes primarily from deleting local and upstream
TLS work from the normal redirect path. Supporting changes are deliberately
small:

- Use a fixed stack buffer for initial protocol detection instead of allocating
  a vector per connection.
- Reuse the shared DNS resolver and its TTL cache instead of resolving a domain
  through the system on each connection.
- Sort each rule's relay providers once during rule construction instead of on
  every request.
- Keep one reused reqwest client and connection pool.
- Move successful per-request diagnostics from INFO to DEBUG while preserving
  error and configured access logging.
- Avoid unrelated parser, cache, or transport abstractions.

The code will not claim kernel zero-copy: Tokio's bidirectional copy remains the
transport primitive. The improvement is removal of redundant cryptography,
certificate work, handshakes, allocations, DNS lookups, sorting, and hot-path
logging.

## Error Handling

- Empty resolver configuration is rejected during startup.
- Invalid resolver and DNS relay settings fail startup with configuration
  context and never expose relay keys in logs.
- A DNS failure closes only the affected connection and does not fall back to
  system DNS.
- A direct upstream connection failure uses the existing per-rule error log.
- A relay provider failure tries the next configured provider before returning
  an error.
- HTTPS without a usable SNI cannot use domain routing and follows the existing
  unknown-protocol fallback rule.
- Oversized or malformed relayed HTTP requests retain their current bounded
  rejection behavior.
- Missing CA material is relevant only when relay-enabled HTTPS rules exist.

## crates.io GitHub Action

A dedicated workflow runs on pushed `v*` tags. It:

1. Verifies that the tag exactly matches the workspace package version.
2. Runs formatting, workspace tests, and strict Clippy.
3. Runs `cargo package` for `dns-relay-shared` and `dns_relay`.
4. Publishes `dns-relay-shared`.
5. Waits with a bounded retry for that exact internal version to appear in the
   registry index.
6. Publishes `dns_relay`.

Publishing authenticates with the scoped GitHub repository secret
`CARGO_REGISTRY_TOKEN`. The token is never printed, stored in configuration, or
committed. A failed or duplicate publication stops the workflow; it does not
silently ignore registry errors. The existing binary release workflow remains
separate and unchanged except where package-name updates require command fixes.

The first crate release from the new workflow is the next version after the
already-tagged `v1.6.8` release.

## Verification

Tests are written before each non-trivial behavior change. Coverage includes:

- Programmatic direct DNS lookup through a local mock UDP resolver.
- Programmatic encrypted-relay selection without system-resolver fallback.
- Fresh cache hits and coalesced simultaneous library lookups.
- Resolver construction rejects an empty direct-resolver list.
- IP literals bypass resolver calls.
- Domain upstream resolution fails closed.
- A direct HTTPS ClientHello reaches the upstream unchanged and no generated
  certificate is presented.
- A relay-enabled HTTPS rule still terminates TLS with the local CA.
- A specific domain rule wins over the catch-all rule.
- Duplicate catch-all rules on one port fail configuration.
- Relay providers retain priority fallback without sorting per request.

Final repository checks are:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo package -p dns-relay-shared
cargo package -p dns_relay
```

and in `relay-proxy`:

```bash
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --profile release-perf
```

The publish workflow is syntax-checked locally and its actual publication is
validated by GitHub Actions because a crates.io release is permanent external
state.

## Checkpoints And Resume Safety

Implementation is split into verified commits rather than one long uncommitted
change:

1. Design specification.
2. Publishable manifests, license, and package dry-runs.
3. `DnsResolver` API and DNS-server integration.
4. crates.io GitHub Action and library documentation.
5. `relay-proxy` dependency, DNS configuration, and fail-closed upstream lookup.
6. Direct TLS tunnel and conditional CA initialization.
7. Catch-all rules and relay hot-path cleanup.
8. Final cross-repository verification and documentation.

`docs/superpowers/checkpoints/2026-08-27-dns-resolver-crate-relay-proxy.md`
records, after each phase, the completed commit in each repository, commands and
results, remaining work, and the exact next step. A resumed session reads the
specification, implementation plan, checkpoint file, and current `git status`
before editing. No phase is marked complete until its focused tests and the
repository-wide checks appropriate to that checkpoint pass.

## Non-Goals

- OS-transparent traffic interception.
- Removing Google Apps Script support.
- Removing the local CA from HTTP relay rules.
- Turning Cloudflare or Google into a raw TCP/TLS byte tunnel.
- Falling back to system DNS.
- Adding IPv6 resolution to the first public API.
- Creating another resolver crate, plugin system, or dependency-injection
  framework.
- Publishing from the local machine or storing a crates.io token in the repo.
