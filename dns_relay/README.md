# dns_relay

`dns_relay` is the main DNS server and reusable resolver library in this
workspace. It applies local policy, caches replies, and resolves through UDP,
DoH, DoQ, or an encrypted HTTPS relay.

See [resolver_proxy](../resolver_proxy/README.md) for the optional local client
that sends padded, authenticated UDP packets across a filtered network.

## Resolution Pipeline

For each DNS query, `dns_relay`:

1. Returns NXDOMAIN for a matching `drop_list` rule.
2. Returns configured A records for a matching `redirect_list` rule.
3. Serves a fresh response from the in-memory LRU cache when available.
4. Resolves through an enabled relay or the healthiest configured upstream.
5. Falls back to a stale cached reply when live resolution fails.

Concurrent identical cache misses share one upstream lookup. Upstream selection
uses measured latency and a delayed second request when the preferred resolver
is slow. VPN detection can prefer DoH and reassert the configured system DNS.

Supported upstream formats:

```text
1.1.1.1:53                              # plain UDP
https://cloudflare-dns.com/dns-query    # DoH
quic://dns.adguard-dns.com:853          # DoQ
```

DoQ needs `init_tls = true`. Public DoQ interoperability is less tested than
UDP and DoH.

## Rules

Rules are case-insensitive. `*.example.com` matches the base domain and all of
its subdomains. Label globs such as `ad-*.example.com` are also supported.

`drop_list` can contain inline domains or paths beginning with `/`, `./`, or
`../`. External drop files may use plain domains, hosts-file lines, Adblock
`||domain^`, dnsmasq `address=/domain/...`, or supported Unbound `local-zone`
lines. Comments and duplicate domains are ignored.

Every `redirect_list` item uses inline `domain:ip1,ip2` syntax. External file
loading currently applies only to `drop_list`. Relative drop-list paths are
resolved from the process working directory.

When the config file or a referenced rule file changes, the rule trie reloads
and the response cache is cleared. Invalid replacement config is logged and
the last valid config remains active.

## Security Modes

`secure_only = true` provides fail-closed upstream selection:

- Plain UDP resolvers are excluded.
- At least one `https://` DoH, `quic://` DoQ, or enabled HTTPS relay is required.
- Relay URLs must use HTTPS.
- Zero-only A/AAAA sinkhole replies are rejected before caching.
- `relay_conf.resolve_manual = true` requires a secure resolver for relay-host bootstrap.

Secure direct Worker relays discover the caller's public IPv4 `/24` for ECS.
Set `client_subnet` to a canonical public `/24` to override discovery. Discovery
failure keeps resolution secure but continues without ECS and retries later.

The optional `udp_obfs` listener uses ChaCha20-Poly1305, a fresh nonce, and
random authenticated padding. Invalid packets are silently dropped. Multiple
keys allow independent proxy clients to share one `dns_relay` instance.

## Build And Run

```bash
cargo build --release -p dns_relay
cargo test -p dns_relay
./target/release/dns_relay --conf /path/to/conf.toml check-conf
./target/release/dns_relay --conf /path/to/conf.toml run
```

The default config path is `./conf.toml`; the default listen address is
`127.0.0.1:53`. Binding port 53 normally requires root or this Linux capability:

```bash
sudo setcap cap_net_bind_service=+ep /path/to/dns_relay
```

Background mode is supported on Linux and macOS:

```bash
dns_relay --conf /absolute/path/conf.toml run --background
dns_relay logs --follow
dns_relay stop
```

Linux stores its PID and log under `$XDG_STATE_HOME/dns_relay` or
`~/.local/state/dns_relay`. macOS uses
`~/Library/Application Support/dns_relay`. See
[service installation](../assets/SERVICES.md) for systemd and launchd units.

## CLI

```text
dns_relay [--conf PATH] [run [--background]]
dns_relay [--conf PATH] stop
dns_relay [--conf PATH] logs [--follow]
dns_relay [--conf PATH] check-conf
dns_relay [--conf PATH] list-rules
dns_relay [--conf PATH] resolvers
dns_relay [--conf PATH] resolve [--relay] DOMAIN [RESOLVER]
dns_relay [--conf PATH] gen-relay-key
```

With no subcommand, the server runs in the foreground. `resolvers` prints up to
ten healthy resolvers by latency. `resolve` performs one A-record lookup;
`--relay` uses `relay_conf`, while the optional positional `RESOLVER` selects a
specific direct upstream.

## Complete Config

This example contains every current config section and passes `check-conf` as
written. Disable or remove optional sections you do not need.

```toml
# Main plain-DNS listener.
dns_target = "127.0.0.1:53"

# Exclude UDP upstreams and reject zero-address sinkholes.
secure_only = true

# Optional canonical public IPv4 /24 for ECS. Omit to auto-discover for a
# direct secure relay, or when ECS is unnecessary.
# client_subnet = "8.8.8.0/24"

# Initialize the rustls provider. Required for quic:// upstreams.
init_tls = true

# Reassert dns_target as system DNS while a VPN is active. Requires OS-level
# permission for networksetup, scutil, resolvectl, or /etc/resolv.conf.
vpn_reassertion = false

# Write resolved domain/address pairs to ./history.txt.
record_history = false

drop_list = [
    "ads.example.com",
    "*.tracking.example",
    "ad-*.example.net",
    "./blocklist.txt",
]

redirect_list = [
    "internal.example:192.0.2.10",
    "multi.example:192.0.2.11,192.0.2.12",
]

resolvers = [
    "https://cloudflare-dns.com/dns-query",
    "quic://dns.adguard-dns.com:853",
]

[record_history_conf]
matched_list = ["*.example.com", "api.example.net"]
lines = 100000

[hotreload_conf]
enable = true
poll_interval_ms = 1000

[resolver_searching]
enable = false
resolver_source = [
    "https://public-dns.info/nameservers-all.txt",
    "https://raw.githubusercontent.com/trickest/resolvers/main/resolvers.txt",
]
# The field name is intentionally documented as implemented.
resfresh_interval = 30
ipv4 = true
doh = true

[relay_conf]
enable = false
resolve_manual = false
relay_timeout_sec = 5
relay_instances = []

[metric_conf]
enable = false
report_type = "log" # "log" or "http"
report_interval = 30

[obfs_conf]
enable = false
bind_addr = "0.0.0.0:8853"
keys = []
```

Required top-level fields are `drop_list`, `redirect_list`, and `resolvers`.
Other top-level settings have defaults. `hotreload_conf` defaults to enabled
with a 1000 ms poll interval when omitted. The config watcher currently runs
whenever the server runs; keep `enable = true` when the section is present.

`resolver_searching.resfresh_interval` is the current serialized field name
(including that spelling) and is measured in seconds. If omitted, it defaults
to 15 seconds. `ipv4` keeps plain IPv4 resolver entries and `doh` keeps HTTPS
entries from downloaded source lists. Secure mode still discards discovered
plain UDP resolvers.

### Relay Config

Enable an encrypted HTTPS relay by replacing the empty relay block above:

```toml
[relay_conf]
enable = true
resolve_manual = false
relay_timeout_sec = 5

[[relay_conf.relay_instances]]
relay_key = "<base64 AES-256-GCM key>"
relay_url = "https://your-worker.workers.dev/"
transport = "direct"

[[relay_conf.relay_instances]]
relay_key = "<same base64 key used by the Worker>"
relay_url = "https://script.google.com/macros/s/DEPLOYMENT_ID/exec"
transport = "google_chained"
```

`transport` is required and supports `direct` and `google_chained`. Instances
are selected round-robin. `resolve_manual = true` resolves the relay hostname
through the configured resolver pool rather than the operating system.

Generate `relay_key` locally with `dns_relay gen-relay-key`. Store the same key
as the Cloudflare Worker secret `RELAY_KEY`. The Worker implementation is
[assets/relay_worker.js](../assets/relay_worker.js). For `google_chained`, deploy
[assets/relay_google_script.js](../assets/relay_google_script.js) as an anonymous
Apps Script web app and store the Worker URL in its Script Properties. The
Google hop sees only encrypted payloads and an HMAC cache tag.

### Proxy Obfuscation Listener

Generate a key with `resolver_proxy gen-obfs-key`, then enable the matching
listener in `dns_relay`:

```toml
[obfs_conf]
enable = true
bind_addr = "0.0.0.0:8853"
keys = [
    "<base64 ChaCha20-Poly1305 key>",
]
```

Use that key in a `resolver_proxy` target with `mode = "udp_obfs"`.

### Metrics

`report_type = "log"` emits counters at `report_interval` seconds when traffic
has changed. `report_type = "http"` serves JSON metrics at
`http://127.0.0.1:5053/metrics` and health at
`http://127.0.0.1:5053/health`. Only one process can own that fixed HTTP port.

## Rust Library

Using the crate does not bind a DNS port or alter system DNS:

```toml
[dependencies]
dns_relay = "1"
```

```rust
use dns_relay::{DnsResolver, ResolverConfig};

let resolver = DnsResolver::new(ResolverConfig {
    resolvers: vec!["https://cloudflare-dns.com/dns-query".into()],
    relay: None,
})
.await?;

let addresses = resolver.resolve_ipv4("example.com").await?;
```

Keep one `DnsResolver` for the application lifetime to reuse health state,
connections, caching, and concurrent lookup sharing. Use
`DnsResolver::new_secure(config, client_subnet)` for fail-closed resolution.

## Platform Status

Linux and macOS are manually tested. DoQ has less real-world coverage than UDP
and DoH. Google Apps Script adds latency and is subject to Google account
quotas, so it is a fallback when direct Worker access is blocked.
