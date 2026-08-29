# DNS Relay Desktop GUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a bundled cross-platform Tauri application that installs and manages `dns_relay` as a privileged system service, with a reviewed interactive HTML prototype completed first.

**Architecture:** A framework-free Tauri webview calls a non-privileged Rust backend. The backend owns validation, status, secrets, and a fixed request protocol to an elevated `dns_relay_admin` helper; the helper alone installs binaries, writes system config, and controls systemd, launchd, or the Windows Service Control Manager. Config apply is staged, validated by the bundled resolver, atomically swapped, health-checked, and rolled back on failure.

**Tech Stack:** Rust 2024, Tauri 2, TypeScript, Vite, semantic HTML/CSS, Lucide icons, Playwright, serde/toml, keyring, Argon2id, ChaCha20-Poly1305, systemd, launchd, Windows Service Control Manager.

**Spec:** `docs/superpowers/specs/2026-08-29-dns-relay-gui-design.md`

## Global Constraints

- Version 1 manages `dns_relay` only; do not add `resolver_proxy` management.
- Build for Windows 10+, macOS 10.13+ where dependencies permit, and the oldest viable WebKitGTK 4.1/glibc Linux base.
- The Tauri webview and GUI process must never run elevated.
- JavaScript must not receive shell execution or arbitrary filesystem capabilities.
- Admin operations and destinations are closed allowlists.
- Config writes are validated, atomic, system-only readable, and retain one rollback copy.
- Secrets use the OS vault with an explicit encrypted passphrase fallback; exports omit secrets by default.
- Use framework-free TypeScript, semantic HTML, CSS, and Lucide icons.
- The HTML prototype must be reviewed before Tauri scaffolding or backend implementation begins.
- Preserve all current TOML field names, including `resolver_searching.resfresh_interval`.
- Do not automate Cloudflare Worker or Google Apps Script deployment.

## File Structure

- `gui/prototype/index.html`: directly openable interactive prototype markup.
- `gui/prototype/styles.css`: shared visual tokens and responsive prototype layout.
- `gui/prototype/app.js`: fixture state, interactions, validation, and view rendering.
- `gui/prototype/vendor/lucide.min.js`: pinned local Lucide browser bundle.
- `gui/tests/prototype.spec.mjs`: Playwright interactions, accessibility assertions, overlap checks, and screenshots.
- `gui/package.json`: Vite, TypeScript, Lucide, and Playwright scripts.
- `gui/index.html`: production HTML promoted from the approved prototype.
- `gui/src/`: framework-free TypeScript state, backend adapter, renderers, and screen modules.
- `gui/src-tauri/`: Tauri application, commands, capabilities, icons, and platform bundle config.
- `dns_relay_admin/`: elevated request protocol, atomic config application, platform service managers, and Windows service host.
- `shared/src/lib.rs`: TOML-compatible redirect serialization helper.
- `shared/src/dns.rs`: serializable IPv4 subnet value.
- `dns_relay/src/conf.rs`: serializable public config model and validation entry point.
- `shared/src/metric_wrapper.rs`: public serializable metrics response used by the GUI.
- `assets/gui/`: system service templates and platform packaging resources.
- `.github/workflows/gui.yml`: GUI tests and platform bundle builds.
- `README.md` and `gui/README.md`: build, support, install, and security behavior.

---

### Task 1: Interactive HTML Prototype

**Files:**
- Create: `gui/package.json`
- Create: `gui/package-lock.json`
- Create: `gui/scripts/vendor-lucide.mjs`
- Create: `gui/prototype/index.html`
- Create: `gui/prototype/styles.css`
- Create: `gui/prototype/app.js`
- Create: `gui/prototype/assets/app-mark.png`
- Create: `gui/prototype/vendor/lucide.min.js`
- Create: `gui/tests/prototype.spec.mjs`
- Create: `gui/tests/screenshots/.gitkeep`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: Approved screen and visual requirements from the design spec.
- Produces: A directly openable prototype and stable DOM hooks (`data-view`, `data-action`, `data-state`) reused by production UI tests.

- [ ] **Step 1: Add the prototype tool manifest**

Create `gui/package.json` with pinned major versions and no application framework:

```json
{
  "name": "dns-relay-gui",
  "private": true,
  "type": "module",
  "scripts": {
    "vendor": "node scripts/vendor-lucide.mjs",
    "prototype:test": "playwright test tests/prototype.spec.mjs",
    "prototype:show": "playwright show-report"
  },
  "dependencies": {
    "lucide": "^0.468.0"
  },
  "devDependencies": {
    "@playwright/test": "^1.50.0"
  }
}
```

Add `gui/scripts/vendor-lucide.mjs` using `node:fs/promises.copyFile` to copy
`node_modules/lucide/dist/umd/lucide.js` into
`prototype/vendor/lucide.min.js`. Add `gui/node_modules/`,
`gui/playwright-report/`, and `gui/test-results/` to `.gitignore`.

- [ ] **Step 2: Write the failing prototype smoke test**

Create `gui/tests/prototype.spec.mjs` and assert the required shell before it
exists:

```javascript
import { test, expect } from "@playwright/test";
import { pathToFileURL } from "node:url";
import path from "node:path";

const prototype = pathToFileURL(path.resolve("prototype/index.html")).href;

test("prototype exposes the six operational views", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(prototype);
  await expect(page.locator("[data-app-shell]")).toBeVisible();
  await expect(page.locator("[data-view-nav]")).toHaveCount(6);
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
});
```

- [ ] **Step 3: Run the smoke test and confirm failure**

Run:

```bash
cd gui
npm install
npm run vendor
npx playwright install chromium
npm run prototype:test
```

Expected: FAIL because `prototype/index.html` and `[data-app-shell]` do not yet
exist.

- [ ] **Step 4: Add the Kuro Nezumi app mark**

Convert the Kuro Nezumi macOS Ghostty icon from
`~/projects/kuro-nezumi/assets/icons/macos/ghostty.icns` to the committed
1024px transparent `gui/prototype/assets/app-mark.png`. Use the mark in the app
header and setup view without placing it inside a card.

- [ ] **Step 5: Build the prototype shell and visual system**

Implement semantic HTML with a native-titlebar-safe app header, dashboard main
area, six navigation buttons, bottom Apply/Revert bar, dialogs, toast region,
and screen-reader live region. Define fixed CSS tokens:

```css
:root {
  --bg: #080808;
  --bg-alt: #101010;
  --surface: #171717;
  --surface-raised: #202020;
  --text: #d7d2c8;
  --muted: #9a948a;
  --signal: #b73535;
  --healthy: #8a8f73;
  --danger: #d94a4a;
  --warning: #b8a781;
  --border: #343434;
  --radius: 8px;
  --control: 44px;
}
```

Use a 420 by 720 compact layout, stable control dimensions, bottom navigation
below 760 px, and a 72 px left rail above 760 px. Use no gradients, decorative
blobs, nested cards, or viewport-scaled font sizes.

- [ ] **Step 6: Implement fixture state and all view interactions**

In `app.js`, define one serializable state object:

```javascript
const state = {
  setupComplete: true,
  service: "running",
  activeView: "dashboard",
  dirty: false,
  secureOnly: true,
  metrics: { requests: 18429, cacheHit: 71, failed: 12, latency: 34 },
  resolvers: [],
  dropRules: [],
  redirectRules: [],
  relays: [],
  logs: []
};
```

Populate realistic fixture rows and implement event delegation for:

- Setup completion and installed-state reset.
- Start/stop transitions through `applying` to `running` or `stopped`.
- All six views and responsive navigation.
- Resolver test success/failure and row latency updates.
- Rule add, edit, enable/disable, delete, validation, and search.
- Relay secret reveal/mask and connectivity test.
- Dirty state, Apply, and Revert.
- Activity pause/filter/clear.
- Loading, empty, warning, and error fixture switches in Settings.

Call `lucide.createIcons()` after each render. Every icon-only button must have
an `aria-label` and tooltip.

- [ ] **Step 7: Expand Playwright coverage**

Add tests for power state, navigation, rule creation, resolver testing,
Apply/Revert, secret masking, keyboard focus, reduced motion, and setup. Add a
DOM overlap assertion:

```javascript
const overlaps = await page.locator("[data-no-overlap]").evaluateAll((nodes) =>
  nodes.flatMap((node, index) => {
    const a = node.getBoundingClientRect();
    return nodes.slice(index + 1).filter((other) => {
      const b = other.getBoundingClientRect();
      return a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    }).map((other) => [node.dataset.noOverlap, other.dataset.noOverlap]);
  })
);
expect(overlaps).toEqual([]);
```

Capture full-page screenshots for dashboard, rules, relay, activity, settings,
and setup at 420 by 720 and 1024 by 720.

- [ ] **Step 8: Run prototype verification**

Run:

```bash
cd gui
npm run prototype:test
```

Expected: all interaction and screenshot tests PASS with no overlap failures.
Open `gui/prototype/index.html` directly and verify navigation and dialogs work
without a server.

- [ ] **Step 9: Commit and stop for visual review**

```bash
git add .gitignore gui
git commit -m "feat: prototype dns relay desktop gui"
```

Present the direct HTML file and desktop/wide screenshots. Do not begin Task 2
until the user approves the prototype.

---

### Task 2: Serializable DNS Relay Configuration

**Files:**
- Modify: `shared/src/lib.rs`
- Modify: `shared/src/dns.rs`
- Modify: `shared/src/metric_wrapper.rs`
- Modify: `dns_relay/src/conf.rs`
- Modify: `dns_relay/src/tests.rs`

**Interfaces:**
- Consumes: Existing `Conf`, `Ipv4Subnet`, redirect-list syntax, and metrics JSON.
- Produces: `Conf: Serialize + Deserialize`, `Conf::validate()`, `Conf::to_toml()`, and public `MetricReport` for GUI use.

- [ ] **Step 1: Add failing config round-trip tests**

Add tests that load a complete TOML fixture, serialize it, reload it, and assert
the preserved fields, especially redirects and `resfresh_interval`:

```rust
#[test]
fn config_round_trip_preserves_gui_editable_fields() {
    let original = load_conf(&write_conf(complete_conf())).unwrap();
    let encoded = original.to_toml().unwrap();
    let decoded: Conf = toml::from_str(&encoded).unwrap();
    assert_eq!(decoded.redirect_list, original.redirect_list);
    assert_eq!(decoded.resolver_searching.resfresh_interval, Some(30));
    assert_eq!(decoded.client_subnet, original.client_subnet);
}
```

- [ ] **Step 2: Run the focused test and confirm failure**

Run:

```bash
cargo test -p dns_relay config_round_trip_preserves_gui_editable_fields -- --exact
```

Expected: compile failure because config types do not implement `Serialize` and
`Conf::to_toml` does not exist.

- [ ] **Step 3: Implement TOML-compatible serialization**

Add `Serialize` to config enums/structs and `Ipv4Subnet`. Implement a paired
`serialize_redirect_list` in `shared/src/lib.rs` that emits the current string
format:

```rust
pub fn serialize_redirect_list<S>(
    entries: &[(String, String)],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    entries
        .iter()
        .map(|(domain, target)| format!("{domain}:{target}"))
        .collect::<Vec<_>>()
        .serialize(serializer)
}
```

Make `Conf::validate` public and add:

```rust
pub fn to_toml(&self) -> Result<String, Error> {
    toml::to_string_pretty(self).map_err(|error| Error::Config(error.to_string()))
}
```

Expose `MetricReport` and `MetricWrapper::prepare_report` for typed GUI polling.

- [ ] **Step 4: Run config and workspace tests**

Run:

```bash
cargo test -p dns_relay config_round_trip_preserves_gui_editable_fields -- --exact
cargo test --workspace
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add shared/src/lib.rs shared/src/dns.rs shared/src/metric_wrapper.rs dns_relay/src/conf.rs dns_relay/src/tests.rs
git commit -m "feat: serialize dns relay configuration"
```

---

### Task 3: Admin Request Protocol And Fixed Paths

**Files:**
- Modify: `Cargo.toml`
- Create: `dns_relay_admin/Cargo.toml`
- Create: `dns_relay_admin/src/lib.rs`
- Create: `dns_relay_admin/src/main.rs`
- Create: `dns_relay_admin/src/paths.rs`
- Create: `dns_relay_admin/src/tests.rs`

**Interfaces:**
- Consumes: JSON request files created in the fixed per-user request directory.
- Produces: `AdminAction`, `AdminRequest`, `AdminResponse`, `PlatformPaths`, and a CLI accepting only `--request-id UUID` or `service-run`.

- [ ] **Step 1: Add failing protocol tests**

Test that malformed IDs, symlinks, wrong-parent request paths, and unknown JSON
actions are rejected while a valid `Status` request parses:

```rust
#[test]
fn action_rejects_unknown_variants() {
    let error = serde_json::from_str::<AdminRequest>(r#"{"action":"shell"}"#).unwrap_err();
    assert!(error.to_string().contains("unknown variant"));
}
```

- [ ] **Step 2: Run and confirm failure**

Run `cargo test -p dns_relay_admin`.

Expected: FAIL because the crate and protocol do not exist.

- [ ] **Step 3: Implement the closed protocol**

Define:

```rust
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AdminAction {
    Install { config_toml: String, expected_binary_sha256: String },
    Update { expected_binary_sha256: String },
    Repair { expected_binary_sha256: String },
    Uninstall,
    Start,
    Stop,
    Restart,
    ApplyConfig { config_toml: String, restart: bool },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminRequest { pub id: Uuid, pub action: AdminAction }

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminResponse { pub id: Uuid, pub ok: bool, pub message: String }
```

`PlatformPaths` returns constants for installed binary, config, backup, logs,
service definition, request directory, and response directory. Validate that a
request ID is a UUID; construct its path internally; reject symlinks and files
not owned by the invoking user; require mode `0600` on Unix.

- [ ] **Step 4: Implement the minimal CLI dispatcher**

Use Clap subcommands:

```text
dns_relay_admin request --request-id <UUID>
dns_relay_admin service-run
```

The dispatcher reads one validated request, calls a temporary
`UnsupportedServiceManager`, writes a structured response, and deletes the
request. It must have no command or path passthrough arguments.

- [ ] **Step 5: Run checks and commit**

Run:

```bash
cargo test -p dns_relay_admin
cargo clippy -p dns_relay_admin --all-targets -- -D warnings
```

Then:

```bash
git add Cargo.toml Cargo.lock dns_relay_admin
git commit -m "feat: define privileged admin protocol"
```

---

### Task 4: Atomic Config Apply And Rollback

**Files:**
- Create: `dns_relay_admin/src/apply.rs`
- Create: `dns_relay_admin/src/process.rs`
- Modify: `dns_relay_admin/src/lib.rs`
- Modify: `dns_relay_admin/src/tests.rs`

**Interfaces:**
- Consumes: `AdminAction::ApplyConfig`, fixed paths, bundled resolver path, and a `ServiceManager`.
- Produces: `apply_config(request, paths, service, runner)` with validation, atomic replacement, health confirmation, and rollback.

- [ ] **Step 1: Add failure-first apply tests**

Use temporary files and fake process/service implementations to cover invalid
TOML, failed `check-conf`, elevation-independent write failure, restart failure,
health failure, successful rule-only apply, and successful restarting apply.

```rust
#[test]
fn failed_health_check_restores_config_and_running_state() {
    let fixture = ApplyFixture::running_with_config("old");
    fixture.health.fail_next();
    assert!(fixture.apply("new", true).is_err());
    assert_eq!(fixture.live_config(), "old");
    assert!(fixture.service.is_running());
}
```

- [ ] **Step 2: Run and confirm failure**

Run `cargo test -p dns_relay_admin apply::tests`.

Expected: FAIL because apply logic does not exist.

- [ ] **Step 3: Implement staged validation and atomic swap**

Implement this exact order: create sibling staging file with `0600`, write and
sync, run bundled `dns_relay --conf STAGED check-conf`, preserve one `.bak`,
rename staged to live, sync parent directory, restart only when requested, poll
health for 10 seconds, then either remove stale backup or restore it and the
prior running state.

Use small injected traits because process execution and service state need real
and fake implementations:

```rust
pub trait CommandRunner {
    fn check_conf(&self, binary: &Path, config: &Path) -> Result<(), AdminError>;
}

pub trait ServiceManager {
    fn status(&self) -> Result<ServiceStatus, AdminError>;
    fn start(&self) -> Result<(), AdminError>;
    fn stop(&self) -> Result<(), AdminError>;
    fn restart(&self) -> Result<(), AdminError>;
}
```

- [ ] **Step 4: Verify and commit**

Run `cargo test -p dns_relay_admin` and `cargo fmt --all -- --check`.

```bash
git add dns_relay_admin/src
git commit -m "feat: apply service config atomically"
```

---

### Task 5: Linux Service Management And Elevation

**Files:**
- Create: `dns_relay_admin/src/platform/linux.rs`
- Create: `assets/gui/dns-relay-gui.service`
- Create: `assets/gui/com.dns-relay.gui.policy`
- Modify: `dns_relay_admin/src/platform/mod.rs`
- Modify: `dns_relay_admin/src/tests.rs`

**Interfaces:**
- Consumes: fixed `/opt/dns-relay-gui` and `/etc/systemd/system/dns-relay-gui.service` paths.
- Produces: Linux `ServiceManager`, install/update/repair/uninstall behavior, and documented `pkexec` launch command.

- [ ] **Step 1: Add command-construction tests**

Assert exact `systemctl` argument vectors and that no request content becomes a
command argument:

```rust
assert_eq!(linux.restart_command(), CommandSpec::new("/usr/bin/systemctl", ["restart", "dns-relay-gui.service"]));
```

- [ ] **Step 2: Run and confirm failure**

Run `cargo test -p dns_relay_admin linux`.

Expected: FAIL because the Linux manager does not exist.

- [ ] **Step 3: Implement hardened systemd installation**

Install a service running the bundled resolver in the foreground with
`--conf /opt/dns-relay-gui/conf.toml`, `CAP_NET_BIND_SERVICE`,
`NoNewPrivileges=true`, `ProtectSystem=strict`, a read-only install directory,
and a writable log/history directory. Use `systemctl daemon-reload`, enable,
start, stop, restart, and disable through fixed `CommandSpec` values.

Install a polkit policy allowing the signed admin helper to request native
authentication. If `pkexec` is absent, return an error containing the exact
terminal fallback `sudo <helper> request --request-id <UUID>`.

- [ ] **Step 4: Run Linux tests and commit**

Run:

```bash
cargo test -p dns_relay_admin linux
cargo clippy -p dns_relay_admin --all-targets -- -D warnings
```

```bash
git add dns_relay_admin/src/platform assets/gui/dns-relay-gui.service assets/gui/com.dns-relay.gui.policy
git commit -m "feat: manage dns relay with systemd"
```

---

### Task 6: macOS Service Management And Elevation

**Files:**
- Create: `dns_relay_admin/src/platform/macos.rs`
- Create: `assets/gui/com.dns-relay.gui.plist`
- Modify: `dns_relay_admin/src/platform/mod.rs`
- Modify: `dns_relay_admin/src/tests.rs`

**Interfaces:**
- Consumes: fixed `/Library/Application Support/DNS Relay/` paths and request UUID.
- Produces: launchd `ServiceManager`, root-owned LaunchDaemon install, and native administrator prompt invocation.

- [ ] **Step 1: Add launchd and quoting tests**

Test exact `launchctl` vectors and AppleScript quoting for a fixed helper path
plus UUID. Include spaces, apostrophes, and non-ASCII application paths; reject
any request ID that is not a UUID before command construction.

- [ ] **Step 2: Run and confirm failure**

Run `cargo test -p dns_relay_admin macos`.

Expected: FAIL because the macOS manager does not exist.

- [ ] **Step 3: Implement launchd management**

Create a root LaunchDaemon plist with fixed `ProgramArguments`, working
directory, stdout/stderr log files, `RunAtLoad`, and `KeepAlive`. Implement
install/update with `launchctl bootout`, atomic binary/config replacement,
`launchctl bootstrap`, `enable`, and `kickstart -k`. Status uses
`launchctl print system/com.dns-relay.gui`.

The GUI backend requests elevation through `/usr/bin/osascript` with a single
fixed admin-helper command. Only the internally resolved signed helper path and
validated UUID enter the quoted AppleScript string.

- [ ] **Step 4: Run macOS tests and commit**

Run `cargo test -p dns_relay_admin macos` and `cargo fmt --all -- --check`.

```bash
git add dns_relay_admin/src/platform assets/gui/com.dns-relay.gui.plist
git commit -m "feat: manage dns relay with launchd"
```

---

### Task 7: Windows Service Host And Elevation

**Files:**
- Create: `dns_relay_admin/src/platform/windows.rs`
- Create: `dns_relay_admin/src/windows_service.rs`
- Modify: `dns_relay_admin/Cargo.toml`
- Modify: `dns_relay_admin/src/main.rs`
- Modify: `dns_relay_admin/src/tests.rs`

**Interfaces:**
- Consumes: fixed `%ProgramFiles%\DNS Relay\` and `%ProgramData%\DNS Relay\` paths.
- Produces: SCM install/control implementation, `service-run` dispatcher, child resolver supervision, and UAC `runas` launcher.

- [ ] **Step 1: Add Windows service-state tests**

Extract a platform-independent supervisor state machine and test start, stop,
unexpected child exit, and shutdown timeout:

```rust
assert_eq!(SupervisorState::Running.on(ServiceEvent::Stop), SupervisorAction::TerminateChild);
assert_eq!(SupervisorState::Stopping.on(ServiceEvent::ChildExited), SupervisorAction::ReportStopped);
```

- [ ] **Step 2: Run and confirm failure**

Run `cargo test -p dns_relay_admin windows_service`.

Expected: FAIL because service hosting does not exist.

- [ ] **Step 3: Implement SCM integration**

Use target-specific `windows-service` and `windows` dependencies. Install the
admin binary as `DNSRelayGui` with argument `service-run`. The service host
spawns the fixed installed `dns_relay.exe --conf <ProgramData config>`, reports
SCM state transitions, forwards stop, terminates after a bounded grace period,
and reports unexpected child exit as service failure.

Implement UAC launch with `ShellExecuteW` verb `runas`, the fixed helper path,
and `request --request-id <validated UUID>`. No config content or path supplied
by JavaScript becomes a process argument.

- [ ] **Step 4: Run checks and commit**

Run:

```bash
cargo test -p dns_relay_admin windows_service
cargo check -p dns_relay_admin --target x86_64-pc-windows-msvc
```

```bash
git add dns_relay_admin
git commit -m "feat: host dns relay as a Windows service"
```

---

### Task 8: Credential Vault And Encrypted Fallback

**Files:**
- Modify: `Cargo.toml`
- Create: `gui/src-tauri/Cargo.toml`
- Create: `gui/src-tauri/src/lib.rs`
- Create: `gui/src-tauri/src/secrets.rs`
- Create: `gui/src-tauri/src/secrets_tests.rs`
- Create: `gui/src-tauri/src/vault.rs`

**Interfaces:**
- Consumes: logical secret IDs and optional fallback passphrase.
- Produces: `SecretStore` with OS-keyring primary storage, encrypted local fallback, masked listings, and explicit materialization for Apply.

- [ ] **Step 1: Add failing secret-store tests**

Test store/get/delete, keyring unavailable, wrong fallback passphrase, ciphertext
tamper, and secret-free export. Use an in-memory `CredentialBackend` fake.

```rust
#[test]
fn tampered_fallback_vault_fails_closed() {
    let mut vault = encrypted_fixture("correct horse");
    vault.ciphertext[0] ^= 1;
    assert!(vault.open("correct horse").is_err());
}
```

- [ ] **Step 2: Run and confirm failure**

Run `cargo test -p dns_relay_gui secrets`.

Expected: FAIL because the secret store does not exist. Create the minimal
`dns_relay_gui` library manifest and add it to the workspace before running the
test; do not add Tauri dependencies or UI scaffolding in this task.

- [ ] **Step 3: Implement vault behavior**

Use `keyring` for OS storage. Store fallback vault JSON under the per-user app
data directory with a random 16-byte salt, Argon2id-derived 256-bit key, random
12-byte nonce, and ChaCha20-Poly1305 ciphertext. Never persist the passphrase or
derived key. Zeroize derived key material after use.

Expose:

```rust
pub trait SecretStore {
    fn put(&self, id: &SecretId, value: &[u8]) -> Result<(), SecretError>;
    fn get(&self, id: &SecretId) -> Result<SecretValue, SecretError>;
    fn delete(&self, id: &SecretId) -> Result<(), SecretError>;
}
```

- [ ] **Step 4: Verify and commit**

Run `cargo test -p dns_relay_gui secrets` and `cargo clippy -p dns_relay_gui --all-targets -- -D warnings`.

```bash
git add gui/src-tauri Cargo.toml Cargo.lock
git commit -m "feat: store gui secrets securely"
```

---

### Task 9: Tauri Shell And Backend Contract

**Files:**
- Modify: `gui/package.json`
- Modify: `gui/package-lock.json`
- Create: `gui/tsconfig.json`
- Create: `gui/vite.config.ts`
- Create: `gui/index.html`
- Create: `gui/src/main.ts`
- Create: `gui/src/backend.ts`
- Create: `gui/src/types.ts`
- Create: `gui/src-tauri/build.rs`
- Create: `gui/src-tauri/src/main.rs`
- Create: `gui/src-tauri/src/commands.rs`
- Create: `gui/src-tauri/src/state.rs`
- Create: `gui/src-tauri/tauri.conf.json`
- Create: `gui/src-tauri/capabilities/default.json`
- Create: `gui/src-tauri/icons/`

**Interfaces:**
- Consumes: Approved prototype DOM/CSS, serializable `Conf`, admin protocol, and `SecretStore`.
- Produces: Tauri commands `get_app_state`, `load_draft`, `validate_draft`, `apply_draft`, `service_action`, `test_resolver`, `test_relay`, `read_logs`, and `read_history`.

- [ ] **Step 1: Add failing command serialization tests**

Test that each command response serializes to the TypeScript shape and errors
contain stable `code`, `message`, and optional `field` keys.

```rust
#[test]
fn command_error_has_stable_shape() {
    let value = serde_json::to_value(CommandError::field("invalid_subnet", "clientSubnet")).unwrap();
    assert_eq!(value["code"], "invalid_subnet");
    assert_eq!(value["field"], "clientSubnet");
}
```

- [ ] **Step 2: Scaffold Tauri without shell capability**

Add Tauri 2, Vite, TypeScript, `@tauri-apps/api`, dialog, updater, and
single-instance dependencies. Do not add `tauri-plugin-shell`. Add package
scripts `dev: vite`, `build: tsc --noEmit && vite build`, `test: playwright
test`, and `tauri: tauri`. Defer `externalBin` configuration until Task 13,
after the staging script exists. The default capability includes only core
window/event/tray, scoped dialog, and updater permissions.

Set CSP to local assets only:

```json
"security": {
  "csp": "default-src 'self'; img-src 'self' asset:; style-src 'self'; script-src 'self'; connect-src ipc: http://127.0.0.1:5053"
}
```

- [ ] **Step 3: Define shared frontend contract**

Create discriminated unions for `ServiceState`, `ViewId`, `CommandError`,
`ResolverRow`, `RuleRow`, `RelayRow`, `Metrics`, and `AppState`. Define:

```typescript
export interface Backend {
  getAppState(): Promise<AppState>;
  validateDraft(draft: DnsRelayConfig): Promise<ValidationResult>;
  applyDraft(draft: DnsRelayConfig): Promise<ApplyResult>;
  serviceAction(action: "start" | "stop" | "restart" | "repair" | "uninstall"): Promise<ServiceState>;
}
```

Provide `FixtureBackend` for browser/prototype mode and `TauriBackend` using
`invoke` for packaged mode.

- [ ] **Step 4: Implement the Rust command boundary**

Commands deserialize typed payloads, validate in Rust, and call internal
backend services. They never accept executable paths, destination paths, or
shell strings. `apply_draft` converts vault references to secrets only inside
Rust, creates the fixed request file, invokes platform elevation, and returns
the structured response.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cd gui
npm install
npm run build
cargo test -p dns_relay_gui
npm run tauri -- build --debug --no-bundle
```

```bash
git add gui Cargo.toml Cargo.lock
git commit -m "feat: scaffold secure tauri gui"
```

---

### Task 10: Production UI State And Approved Visual Shell

**Files:**
- Modify: `gui/index.html`
- Create: `gui/src/styles.css`
- Create: `gui/src/store.ts`
- Create: `gui/src/render.ts`
- Create: `gui/src/events.ts`
- Modify: `gui/src/main.ts`
- Create: `gui/tests/app.spec.mjs`

**Interfaces:**
- Consumes: Approved prototype markup/tokens and `Backend`.
- Produces: Production navigation, responsive shell, state transitions, dialogs, toasts, Apply/Revert, keyboard support, and shared screen mounting API.

- [ ] **Step 1: Port prototype tests to production URL**

Copy the approved interaction assertions to `app.spec.mjs` and run them against
Vite. Expected initial result: FAIL because production renderers do not yet
provide the prototype DOM hooks.

- [ ] **Step 2: Promote the approved shell**

Move the approved semantic markup and CSS tokens into production files. Replace
fixture-global mutation with a small observable store:

```typescript
export function createStore(initial: AppState) {
  let state = structuredClone(initial);
  const listeners = new Set<(state: AppState) => void>();
  return {
    get: () => state,
    update: (change: (draft: AppState) => void) => {
      const next = structuredClone(state);
      change(next);
      state = next;
      listeners.forEach((listener) => listener(state));
    },
    subscribe: (listener: (state: AppState) => void) => (listeners.add(listener), () => listeners.delete(listener))
  };
}
```

Use event delegation and explicit render functions; do not introduce a UI
framework.

- [ ] **Step 3: Implement shared states and accessibility**

Implement loading, empty, warning, error, dirty, applying, running, and stopped
states. Restore focus after dialogs, trap focus only while modal, support Escape,
and update an `aria-live` region for service/apply results. Respect
`prefers-reduced-motion`.

- [ ] **Step 4: Run UI tests and commit**

Run `npm run build` and Playwright at both viewports.

```bash
git add gui/index.html gui/src gui/tests/app.spec.mjs
git commit -m "feat: add production gui shell"
```

---

### Task 11: Setup, Service Control, Dashboard, And Tray

**Files:**
- Create: `gui/src/screens/setup.ts`
- Create: `gui/src/screens/dashboard.ts`
- Create: `gui/src-tauri/src/observability.rs`
- Create: `gui/src-tauri/src/tray.rs`
- Modify: `gui/src-tauri/src/main.rs`
- Modify: `gui/tests/app.spec.mjs`

**Interfaces:**
- Consumes: service manager status, `/health`, `/metrics`, bundled versions, and elevation flow.
- Produces: setup/adoption flow, power control, dashboard state, recovery actions, update state, and tray commands.

- [ ] **Step 1: Add failing setup and power tests**

Cover first launch, existing install adoption, elevation cancellation, install
success, start/stop, restart failure, independent metrics failure, and tray quit
without service stop.

- [ ] **Step 2: Implement observability aggregation**

Poll service status every two seconds and health/metrics every five seconds.
Return each source as independent `DataState<T>` so one failure does not erase
other data. Compare bundled and installed `--version` output before Apply.

- [ ] **Step 3: Implement setup, dashboard, and tray**

Connect the three-step setup to `Install`; connect the power control to Start or
Stop; display listener, transport, secure state, uptime, metrics, recent events,
and repair/update actions. Tray menu items are Open, Start/Stop, Restart, and
Quit; Quit closes only the GUI.

- [ ] **Step 4: Verify and commit**

Run Rust tests, `npm run build`, and the setup/dashboard Playwright cases.

```bash
git add gui/src/screens gui/src-tauri/src gui/tests/app.spec.mjs
git commit -m "feat: manage resolver service from gui"
```

---

### Task 12: Resolver, Rule, Relay, Activity, And Settings Views

**Files:**
- Create: `gui/src/screens/resolvers.ts`
- Create: `gui/src/screens/rules.ts`
- Create: `gui/src/screens/relay.ts`
- Create: `gui/src/screens/activity.ts`
- Create: `gui/src/screens/settings.ts`
- Create: `gui/src/components/fields.ts`
- Create: `gui/src/components/table.ts`
- Create: `gui/src-tauri/src/probes.rs`
- Create: `gui/src-tauri/src/logs.rs`
- Modify: `gui/tests/app.spec.mjs`

**Interfaces:**
- Consumes: complete typed draft, resolver/relay probes, logs, metrics, history, and secret store.
- Produces: Structured editing for every current config field, diagnostics, import/export, and raw TOML validation.

- [ ] **Step 1: Add failing per-view workflow tests**

Cover resolver CRUD/reorder/test, secure-only invalid state, drop/redirect rule
validation, blocklist import, relay key generation/store/test, log filters,
history retention, config import, secret-free export, plaintext export warning,
raw TOML validation, and Apply/Revert.

- [ ] **Step 2: Implement resolver and rule views**

Map UDP, DoH, and DoQ formats to badges and field validation. Preserve list
order. Render current `resfresh_interval` behind the label Refresh interval.
Rules support suffix/label wildcards, external drop lists, and inline redirects;
do not offer external redirect files.

- [ ] **Step 3: Implement relay and secret workflows**

Support Direct and Google chained transport, timeout, manual bootstrap, client
subnet, generate key, choose vault, reveal with confirmation, replace, delete,
and HTTPS connectivity test. Manage `obfs_conf.keys` through the same secret
workflow. Do not add cloud deployment buttons.

- [ ] **Step 4: Implement activity and settings**

Read platform logs with bounded line count; support pause, filter, copy, export,
and clear. Read history independently. Implement all remaining config fields,
service repair/uninstall, update controls, TOML import/export, and raw editor.
Exports replace secrets with `vault://<secret-id>` unless plaintext export is
explicitly confirmed.

- [ ] **Step 5: Verify complete field coverage**

Add a Rust test that enumerates serialized top-level and nested config keys and
compares them to the GUI field registry. The test must fail when a future config
field lacks a GUI mapping or explicit hidden/internal classification.

- [ ] **Step 6: Run checks and commit**

Run:

```bash
cargo test --workspace
cd gui
npm run build
npm test
```

```bash
git add gui/src gui/src-tauri/src gui/tests
git commit -m "feat: configure dns relay from gui"
```

---

### Task 13: Packaging, Updates, And Platform CI

**Files:**
- Modify: `gui/src-tauri/tauri.conf.json`
- Create: `gui/src-tauri/tauri.windows.conf.json`
- Create: `gui/src-tauri/tauri.macos.conf.json`
- Create: `gui/src-tauri/tauri.linux.conf.json`
- Create: `scripts/stage_gui_sidecars.sh`
- Create: `.github/workflows/gui.yml`
- Modify: `.github/workflows/release.yml`
- Create: `gui/README.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: target-specific `dns_relay` and `dns_relay_admin` binaries.
- Produces: NSIS, universal macOS app/DMG, AppImage, `.deb`, `.rpm`, signed updater artifacts, and documented build/install flow.

- [ ] **Step 1: Add sidecar staging verification**

Create `stage_gui_sidecars.sh` that takes one target triple, verifies both
release binaries exist, computes SHA-256 files, and copies them to Tauri's
required `name-$TARGET_TRIPLE` filenames. Add a shell self-test using temporary
fixture binaries and expected names.

Configure Tauri `externalBin` only after this script has staged target-suffixed
`dns_relay` and `dns_relay_admin` binaries in `gui/src-tauri/binaries/`.

- [ ] **Step 2: Configure platform bundles**

Set Windows `webviewInstallMode` to `embedBootstrapper`; configure macOS
universal targets, minimum system version, entitlements, signing, and
notarization; configure Linux AppImage, deb, and rpm metadata. Generate app
icons from `gui/prototype/assets/app-mark.png` using `npm run tauri -- icon`.

- [ ] **Step 3: Configure signed updates**

Run `npm run tauri -- signer generate` once, keep the private key only in CI secrets,
commit the public key in Tauri config, enable `createUpdaterArtifacts`, and use
the GitHub release endpoint:

```text
https://github.com/v4g4b0nd-0x76/dns-hijacker/releases/latest/download/latest.json
```

The GUI update workflow must update the app bundle and then offer the admin
helper's version-checked service Update action.

- [ ] **Step 4: Add platform build CI**

Create Linux, macOS, and Windows matrix jobs that run Rust tests, frontend build,
Playwright Chromium tests, target-specific sidecar builds, and Tauri bundle
creation. Upload unsigned artifacts on pull requests and signed/notarized
artifacts only on release tags with repository secrets present.

- [ ] **Step 5: Document support and security behavior**

Document supported platforms, bundled components, privilege prompts, installed
paths, vault fallback, plaintext system config reality, service commands,
build prerequisites, and uninstall behavior. Link the design spec and keep
`resolver_proxy` marked as future scope.

- [ ] **Step 6: Verify and commit**

Run the sidecar staging self-test, Tauri config validation, docs link check, and
one local debug bundle.

```bash
git add gui scripts/stage_gui_sidecars.sh .github/workflows README.md
git commit -m "build: package dns relay desktop gui"
```

---

### Task 14: Final Security, Visual, And Regression Verification

**Files:**
- Modify only files required by failures found during this task.

**Interfaces:**
- Consumes: Complete implementation.
- Produces: Evidence that the accepted spec and security boundaries are met.

- [ ] **Step 1: Run Rust verification**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --all-targets
```

Expected: PASS; the existing real-network DoQ test may remain explicitly
ignored.

- [ ] **Step 2: Run frontend verification**

```bash
cd gui
npm ci
npm run build
npm test
```

Expected: PASS at 420 by 720 and 1024 by 720 with no blank screenshots, DOM
overlaps, clipped primary controls, keyboard traps, or missing accessible names.

- [ ] **Step 3: Audit the privilege boundary**

Confirm Tauri capabilities contain no shell permission, frontend commands accept
no executable/destination path, admin actions are a closed enum, request IDs are
UUID-only, request files reject symlinks/wrong owners, and all installed config
and backup files are system-only readable.

- [ ] **Step 4: Exercise rollback paths**

Run integration cases for invalid config, elevation cancellation, service stop
failure, restart failure, failed health check, unavailable metrics/logs, vault
failure, wrong fallback password, tampered vault, and binary version mismatch.

- [ ] **Step 5: Verify platform bundles**

Confirm Windows, macOS, and Linux CI jobs produce their expected installer and
updater artifacts. Record any CI environment that cannot run an elevated
service smoke test in `gui/README.md`, with the exact manual release check.

- [ ] **Step 6: Commit verification fixes**

If verification required changes, stage only those files and commit:

```bash
git commit -m "fix: complete desktop gui verification"
```

If no files changed, do not create an empty commit.
