#!/usr/bin/env bash
set -euo pipefail

# Installs the already-built Apple Silicon performance binary as the local DNS
# hijacker. It deliberately preserves /opt/dns-hijacker/conf.toml and lists.

if [[ "$(id -u)" -ne 0 ]]; then
    echo "Run with: sudo $0" >&2
    exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LABEL="com.dns-hijacker"
BIN_SOURCE="$ROOT/target/release-perf/dns_relay"
BIN_DEST="/opt/dns-hijacker/dns-hijacker"
PLIST_SOURCE="$ROOT/assets/com.dns-hijacker.plist"
PLIST_DEST="/Library/LaunchDaemons/$LABEL.plist"

if [[ ! -x "$BIN_SOURCE" ]]; then
    echo "Missing $BIN_SOURCE. Build it first with: cargo build --profile release-perf -p dns_relay" >&2
    exit 1
fi

plutil -lint "$PLIST_SOURCE" >/dev/null
install -d -o root -g wheel -m 755 /opt/dns-hijacker

# Stop an older job before replacing its executable. `bootout` is expected to
# fail on a first install, so do not let that prevent deployment.
launchctl bootout "system/$LABEL" 2>/dev/null || true
install -o root -g wheel -m 755 "$BIN_SOURCE" "$BIN_DEST"
install -o root -g wheel -m 644 "$PLIST_SOURCE" "$PLIST_DEST"

# A disabled LaunchDaemon rejects bootstrap with a generic I/O error. Enable
# the label before loading it, then verify launchd accepted the job.
launchctl enable "system/$LABEL"
launchctl bootstrap system "$PLIST_DEST"
launchctl kickstart -k "system/$LABEL"
launchctl print "system/$LABEL"

echo "Installed $($BIN_DEST --version) as $LABEL"
