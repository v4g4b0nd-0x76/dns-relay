#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd -P)
target=${1:?usage: stage_gui_sidecars.sh TARGET [DESTINATION]}
destination=${2:-"$root/gui/src-tauri/binaries"}
extension=
[[ $target == *-windows-* ]] && extension=.exe
source_dir=${CARGO_TARGET_DIR:-"$root/target"}/$target/release

mkdir -p "$destination"
for name in dns_relay dns_relay_admin; do
  source="$source_dir/$name$extension"
  output="$destination/$name-$target$extension"
  test -f "$source" || { echo "missing release sidecar: $source" >&2; exit 1; }
  install -m 755 "$source" "$output"
  if command -v sha256sum >/dev/null; then
    sha256sum "$output" > "$output.sha256"
  else
    shasum -a 256 "$output" > "$output.sha256"
  fi
done
