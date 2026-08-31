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
