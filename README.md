# DNS Relay

Rust workspace for a configurable DNS resolver and its local anti-DPI proxy.
The current workspace version is **1.6.10** and requires Rust 1.85 or newer.

## Components

| Component | Role | Documentation |
| --- | --- | --- |
| `dns_relay` | Main DNS server and reusable resolver library | [dns_relay/README.md](dns_relay/README.md) |
| `resolver_proxy` | Local UDP forwarder with optional authenticated obfuscation | [resolver_proxy/README.md](resolver_proxy/README.md) |
| `dns-relay-shared` | Shared DNS, cache, metrics, rules, network guard, and obfuscation code | Internal workspace crate |
| `gui` / `dns_relay_gui` | Tauri desktop control plane for installing, configuring, and monitoring `dns_relay` | [DEVELOPER.md](DEVELOPER.md#desktop-gui) |

The normal two-machine deployment is:

```text
OS or LAN -> resolver_proxy -> plain UDP or encrypted udp_obfs -> dns_relay -> UDP / DoH / DoQ / HTTPS relay
```

`resolver_proxy` is optional. Clients can query `dns_relay` directly when the
network path does not inspect or forge DNS traffic.

## Current Features

- Domain drop and redirect rules, including suffix wildcards and label globs, plus external drop-list files.
- In-memory LRU response caching and sharing of concurrent identical lookups.
- Plain UDP, DNS-over-HTTPS (DoH), and DNS-over-QUIC (DoQ) upstreams.
- Fail-closed `secure_only` mode that excludes UDP upstreams and rejects zero-address sinkholes.
- AES-256-GCM HTTPS relays through Cloudflare Workers, directly or through Google Apps Script.
- ChaCha20-Poly1305 padded UDP transport between `resolver_proxy` and `dns_relay`.
- Ordered failover or round-robin proxy target selection.
- Config and rule-list hot reload, metrics, optional query history, and VPN DNS reassertion.
- Linux, macOS, and Windows release builds; background mode is available on Linux and macOS.
- Reusable `dns_relay::DnsResolver` Rust API.

The proxy does **not** currently implement TLS transport. Its supported target
modes are exactly `plain` and `udp_obfs`.

## Build And Test

```bash
cargo build --workspace
cargo test --workspace
node assets/relay_worker_test.mjs
```

For contributor workflow and file ownership, see [DEVELOPER.md](DEVELOPER.md).

Build one release binary with Make:

```bash
make build                         # dns_relay for the host
make build bin=resolver_proxy      # resolver_proxy for the host
make build-musl                    # static Linux dns_relay
make build-musl bin=resolver_proxy
make build-gnu bin=resolver_proxy
make build-mac bin=resolver_proxy
./scripts/build.sh windows dns_relay
./scripts/build.sh windows resolver_proxy
```

Build and test the desktop GUI:

```bash
cd gui && npm test
cd gui && npm run build
make gui-linux-test
```

Build and reinstall the desktop GUI for the current platform:

```bash
make gui-install
```

On Linux CI or a fresh Linux workstation, install the GUI native headers first:

```bash
sudo apt-get update
sudo apt-get install -y build-essential libayatana-appindicator3-dev libglib2.0-dev librsvg2-dev libssl-dev libwebkit2gtk-4.1-dev libxdo-dev pkg-config
```

Run either binary with a config file:

```bash
dns_relay --conf /path/to/dns-relay.toml check-conf
dns_relay --conf /path/to/dns-relay.toml run

resolver_proxy --conf /path/to/resolver-proxy.toml check-conf
resolver_proxy --conf /path/to/resolver-proxy.toml run
```

Binding port 53 normally requires root/Administrator privileges. On Linux,
grant only the bind capability when full root access is unnecessary:

```bash
sudo setcap cap_net_bind_service=+ep /path/to/dns_relay
sudo setcap cap_net_bind_service=+ep /path/to/resolver_proxy
```

See [assets/SERVICES.md](assets/SERVICES.md) for systemd and launchd setup.

## Releases

Workspace version bumps, commits, and tags are handled by Make:

```bash
make patch
make minor
make major
make patch PUSH=1
```

`PUSH=1` pushes the release commit and tag. Tagged releases build both binaries;
the crates.io workflow publishes `dns-relay-shared` before `dns_relay` and needs
the `CARGO_REGISTRY_TOKEN` GitHub Actions secret.

Windows release ZIPs contain `dns_relay.exe` or `resolver_proxy.exe`. Run them
from an elevated PowerShell or Command Prompt when listening on port 53.

## Platform Status

Linux and macOS are manually tested. Windows binaries are produced by the
release workflow, but privileged port binding and background service behavior
differ by platform. Bug reports and feature suggestions are welcome.
