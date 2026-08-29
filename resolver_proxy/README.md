# resolver_proxy

`resolver_proxy` is a small local DNS forwarder for reaching a remote
[`dns_relay`](../dns_relay/README.md) instance across a network that inspects,
rewrites, or drops plaintext DNS.

Clients send ordinary UDP DNS queries to the proxy. It applies local drop and
redirect rules, checks its cache, and tries configured upstream targets until
one replies. The response is decoded when necessary and returned to the client
with the original DNS transaction ID.

## Transport Modes

The current binary supports exactly two UDP target modes:

- `plain`: sends the original DNS packet over UDP. This is useful for testing
  or unfiltered networks, but does not prevent DNS inspection or injection.
- `udp_obfs`: sends a ChaCha20-Poly1305 authenticated packet with a fresh nonce
  and random encrypted padding. A matching `dns_relay` `obfs_conf` listener
  decrypts the query and encrypts the response.

The authenticated wire format is:

```text
[12-byte nonce][ciphertext: 2-byte DNS length | DNS packet | random padding][16-byte tag]
```

Invalid or unauthenticated packets receive no response from `dns_relay`.
`resolver_proxy` does not currently implement TCP or TLS transport.

Targets may use `ip:port` or `host:port`; hostnames are resolved once at proxy
startup with the operating system resolver.

## Target Selection

- `ordered` tries targets in config order on every request.
- `round_robin` rotates the first target for each request, then uses the other
  targets as a fallback chain.

Each attempt must time out before failover continues. The config field
`upstream_timeout_ms` is accepted, but the current forwarding path uses the
built-in resolver timeout rather than this value. There is no active target
health-check setting in the current config format.

## Rules And Cache

Rules use the same format as `dns_relay`: suffix wildcards such as
`*.example.com`, label globs such as `ad-*.example.com`, external blocklist
files, and inline `domain:ip1,ip2` redirects. External file loading currently
applies only to `drop_list`.

The proxy caches successful forwarded replies. Config and referenced rule files
can be hot-reloaded; invalid replacement config is logged and the last valid
config remains active.

## Build And Run

```bash
cargo build --release -p resolver_proxy
cargo test -p resolver_proxy
./target/release/resolver_proxy --conf /path/to/conf.toml check-conf
./target/release/resolver_proxy --conf /path/to/conf.toml run
```

The default config path is `./conf.toml`. Binding port 53 normally requires
root or this Linux capability:

```bash
sudo setcap cap_net_bind_service=+ep /path/to/resolver_proxy
```

## CLI

```text
resolver_proxy [--conf PATH] [run]
resolver_proxy [--conf PATH] check-conf
resolver_proxy [--conf PATH] list-rules
resolver_proxy [--conf PATH] gen-obfs-key
resolver_proxy [--conf PATH] gen-relay-key
```

With no subcommand, the proxy runs in the foreground. `gen-obfs-key` prints a
base64 key for a paired `udp_obfs` target and listener. `gen-relay-key` prints
the AES-256-GCM key format used by the separate HTTPS relay feature.

## Complete Config

This example contains every current config section and passes `check-conf` as
written.

```toml
# Used only by VPN DNS reassertion. The proxy listener is targets.listen_addr.
dns_target = "127.0.0.1:53"
vpn_reassertion = false

drop_list = [
    "ads.example.com",
    "*.tracking.example",
    "./blocklist.txt",
]

redirect_list = [
    "internal.example:192.0.2.10",
    "multi.example:192.0.2.11,192.0.2.12",
]

[hotreload_conf]
enable = true
poll_interval_ms = 1000

[metric_conf]
enable = false
report_type = "log" # "log" or "http"
report_interval = 30

[targets]
listen_addr = "127.0.0.1:53"
strategy = "ordered" # "ordered" or "round_robin"
upstream_timeout_ms = 2000

[[targets.targets]]
name = "remote_obfs"
mode = "udp_obfs"
address = "resolver.example.com:8853"
shared_key = "<base64 key from resolver_proxy gen-obfs-key>"

[[targets.targets]]
name = "plain_fallback"
mode = "plain"
address = "192.0.2.53:53"
```

`drop_list`, `redirect_list`, `hotreload_conf`, `metric_conf`,
`vpn_reassertion`, and `dns_target` may be omitted. `dns_target` defaults to
`127.0.0.1:53`; it does not change the proxy listener. `targets` and at least
one `targets.targets` item are required at runtime. `strategy` defaults to
`ordered`, and `upstream_timeout_ms` defaults to 2000 when omitted.

`shared_key` is required for `udp_obfs` and ignored for `plain`. Replace the
placeholder before running the proxy. The matching remote `dns_relay` config is:

```toml
[obfs_conf]
enable = true
bind_addr = "0.0.0.0:8853"
keys = ["<the same base64 key>"]
```

`report_type = "log"` emits counters when traffic changes.
`report_type = "http"` serves JSON metrics at
`http://127.0.0.1:5053/metrics` and health at
`http://127.0.0.1:5053/health`. Only one local process can own that fixed port.

`vpn_reassertion = true` starts the shared network guard and points system DNS
at `dns_target`. This requires permission for macOS `networksetup`/`scutil` or
Linux `resolvectl`/`/etc/resolv.conf` updates.

## Deployment

Run `resolver_proxy` on the filtered network and `dns_relay` on a reachable
remote machine. Point the operating system or LAN DNS setting at
`targets.listen_addr`. For `udp_obfs`, expose the remote `obfs_conf.bind_addr`
UDP port and keep the shared key private.

Linux and macOS are manually tested. Windows builds are released, but port 53
requires an elevated terminal.
