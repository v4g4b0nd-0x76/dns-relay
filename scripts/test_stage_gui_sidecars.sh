#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd -P)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
target=x86_64-unknown-linux-gnu
mkdir -p "$tmp/target/$target/release"
printf resolver > "$tmp/target/$target/release/dns_relay"
printf admin > "$tmp/target/$target/release/dns_relay_admin"

CARGO_TARGET_DIR="$tmp/target" "$root/scripts/stage_gui_sidecars.sh" "$target" "$tmp/binaries"

cmp "$tmp/target/$target/release/dns_relay" "$tmp/binaries/dns_relay-$target"
cmp "$tmp/target/$target/release/dns_relay_admin" "$tmp/binaries/dns_relay_admin-$target"
test -s "$tmp/binaries/dns_relay-$target.sha256"
test -s "$tmp/binaries/dns_relay_admin-$target.sha256"

commands=$(make -n -C "$root" gui-mac)
[[ $commands == *'cargo build --release --target aarch64-apple-darwin --bin dns_relay --bin dns_relay_admin'* ]]
[[ $commands == *'stage_gui_sidecars.sh aarch64-apple-darwin'* ]]
[[ $commands == *'npm run tauri build'* ]]

commands=$(make -n -C "$root" gui-mac-install-test)
[[ $commands == *'sudo /usr/bin/ditto "target/release/bundle/macos/DNS Relay.app" "/Applications/DNS Relay.app"'* ]]
[[ $commands == *'npm test -- --workers=1'* ]]
