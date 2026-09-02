.PHONY: build build-gnu build-musl build-mac gui-linux gui-linux-install gui-linux-test gui-mac gui-mac-install gui-mac-install-test gui-install test run caps patch minor major

# Which workspace binary to operate on. Override per-invocation, e.g.:
#   make build bin=resolver_proxy
#   make run bin=resolver_proxy
bin ?= dns_relay

build:
	@./scripts/build.sh auto $(bin)
build-gnu:
	@./scripts/build.sh gnu $(bin)
build-musl:
	@./scripts/build.sh musl $(bin)
build-mac:
	@./scripts/build.sh mac $(bin)
gui-linux:
	@cargo build --release --target x86_64-unknown-linux-gnu --bin dns_relay --bin dns_relay_admin
	@./scripts/stage_gui_sidecars.sh x86_64-unknown-linux-gnu
	@cd gui && npm run tauri build

gui-linux-install: gui-linux
	@deb=$$(find target/release/bundle/deb -maxdepth 1 -name 'DNS Relay_*.deb' -print -quit); \
	test -n "$$deb"; \
	sudo apt-get install -y "./$$deb"

gui-linux-test: gui-linux
	@cd gui && npm test -- --workers=1

gui-mac:
	@cargo build --release --target aarch64-apple-darwin --bin dns_relay --bin dns_relay_admin
	@./scripts/stage_gui_sidecars.sh aarch64-apple-darwin
	@cd gui && npm run tauri build

gui-mac-install: gui-mac
	@sudo rm -rf "/Applications/DNS Relay.app"
	@sudo /usr/bin/ditto "target/release/bundle/macos/DNS Relay.app" "/Applications/DNS Relay.app"

gui-mac-install-test: gui-mac
	@sudo /usr/bin/ditto "target/release/bundle/macos/DNS Relay.app" "/Applications/DNS Relay.app"
	@cd gui && npm test -- --workers=1

gui-install:
	@case "$$(uname -s)" in \
	  Linux) $(MAKE) gui-linux-install ;; \
	  Darwin) $(MAKE) gui-mac-install ;; \
	  *) echo "unsupported OS for GUI install: $$(uname -s)" >&2; exit 1 ;; \
	esac
test:
	@cargo test --bin $(bin)
run: build
	@./target/release/$(bin) 2>/dev/null || \
	  ./target/*/release/$(bin)
# Linux: allow binding :53 without root (alternative to systemd AmbientCapabilities)
caps:
	@sudo setcap cap_net_bind_service=+ep ./target/release/$(bin)

# Semver bump: updates Cargo.toml, commits "chore: release vX.Y.Z" (triggers release.yml), tags vX.Y.Z
# Optional: PUSH=1 make patch  (also pushes commit + tag)
patch:
	@PUSH="$(PUSH)" ./scripts/bump.sh patch
minor:
	@PUSH="$(PUSH)" ./scripts/bump.sh minor
major:
	@PUSH="$(PUSH)" ./scripts/bump.sh major
