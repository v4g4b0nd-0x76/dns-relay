# DNS Relay Desktop GUI Design

**Date:** 2026-08-29

**Status:** Approved for implementation planning

## Goal

Build a bundled Tauri desktop application for Windows, macOS, and Linux that
installs and manages `dns_relay` as a system service. The first release manages
only `dns_relay`; `resolver_proxy` support is deferred.

The interface should feel like a compact network appliance, taking visual cues
from Windscribe's desktop application without copying its brand, assets, or
layout verbatim. A standalone interactive HTML prototype must be completed and
reviewed before production Tauri work begins.

## Scope

Version 1 includes:

- Bundled, version-matched `dns_relay` binary.
- System service install, update, repair, start, stop, restart, and uninstall.
- Live service status, health, metrics, logs, history, and update state.
- Structured editing for all current `dns_relay` configuration fields.
- Resolver add/edit/delete, latency display, connectivity testing, discovery
  sources, secure-only mode, DoH, DoQ, UDP, and ECS subnet configuration.
- Drop and redirect rule add/edit/delete, search, enable/disable, blocklist
  import, validation, and live-reload status.
- Direct and Google-chained relay configuration, key generation and storage,
  URL testing, timeout, manual bootstrap, and client subnet settings.
- VPN DNS reassertion, hot reload, metrics, history retention, config
  import/export, and validated raw TOML editing.
- System tray status and start/stop controls.
- Signed platform installers and an application update path.

Version 1 excludes:

- `resolver_proxy` management.
- Automated Cloudflare Worker or Google Apps Script deployment.
- Remote administration.
- Mobile applications.

## Platform Support

- Windows 10 and newer. The installer embeds the WebView2 bootstrapper so it
  does not assume the runtime is already installed.
- macOS 10.13 and newer where the chosen Tauri/Rust dependency set permits it.
  Distribution uses a signed, notarized universal application bundle.
- Linux systems with glibc and WebKitGTK 4.1 versions compatible with the build.
  AppImage, `.deb`, and `.rpm` packages are built on the oldest viable base
  image. Approximately ten-year-old distributions are a goal, but compatibility
  is best-effort because system WebKitGTK and glibc set the effective floor.

The frontend must avoid newer-only web APIs and provide reduced-motion and
reduced-effects behavior on older webviews. Tauri's current platform details
are documented in its [prerequisites](https://v2.tauri.app/start/prerequisites/),
[webview reference](https://v2.tauri.app/reference/webview-versions/), and
[macOS bundle guide](https://v2.tauri.app/distribute/macos-application-bundle/).

## Technical Approach

Use Tauri 2 with framework-free TypeScript, semantic HTML, and CSS. The HTML
prototype becomes the production UI instead of being rewritten in a frontend
framework. Use Lucide icons for familiar actions and keep dependencies limited
to needs that the platform or standard library does not cover.

The desktop application has three layers:

1. **Tauri webview:** Renders the UI, holds unsaved form state, uses scoped file
   pickers, and sends typed commands. It never runs elevated.
2. **GUI Rust backend:** Reuses `dns_relay` config and validation code, manages
   typed drafts, tests resolvers and relays, reads status/metrics/logs, accesses
   credentials, and invokes the admin helper through fixed operations.
3. **Elevated admin helper:** A bundled Rust executable with allowlisted
   `install`, `update`, `repair`, `uninstall`, `start`, `stop`, `restart`, and
   `apply-config` operations. It does not accept arbitrary commands or output
   paths.

The helper integrates with systemd on Linux and launchd on macOS. On Windows it
uses the Service Control Manager and provides the service-host protocol needed
to supervise the foreground `dns_relay` executable.

## Privilege Model

Read-only status, metrics, logs, and config drafts do not require elevation.
Only system service changes and writes to the installed config/binary request
administrator authorization.

Platform elevation uses a native prompt:

- Linux: polkit/`pkexec`, with a documented terminal `sudo` fallback where
  polkit is unavailable.
- macOS: a fixed signed helper operation authorized by the operating system.
- Windows: UAC and the Service Control Manager.

The helper validates operation names, destinations, file ownership, and bundled
binary hashes. It accepts config content through structured IPC and writes only
to fixed application paths. It must never expose a general shell bridge.

## Configuration Flow

The UI maintains a typed draft separate from the installed TOML. Apply performs
these steps:

1. Validate all fields in the unprivileged Rust backend.
2. Resolve secret references from the credential vault.
3. Send the rendered TOML to the elevated helper.
4. Stage it beside the live system config with system-only permissions.
5. Run the bundled `dns_relay check-conf` against the staged file.
6. Atomically replace the live config while retaining one rollback copy.
7. Allow rule-only changes to use existing live reload; restart for other
   changes.
8. Confirm service state and `/health` after restart.
9. Restore the previous config and service state if validation or health fails.

An elevation cancellation or failed apply leaves the draft and installed
service untouched. The UI shows Apply and Revert whenever the draft differs
from the installed configuration.

Import accepts current `dns_relay` TOML and validates before changing the
draft. Export omits secrets by default; plaintext secret export requires an
explicit warning and confirmation.

## Secret Storage

Relay and obfuscation keys use the operating system credential vault when it is
available:

- macOS Keychain.
- Windows Credential Manager.
- Linux Secret Service.

If a usable vault is unavailable, the app offers an encrypted local vault
protected by a user passphrase. Secrets remain masked by default in the UI.

The running service ultimately requires relay keys in TOML. Applied config and
its rollback copy therefore contain materialized keys with strict system-only
permissions. Setup must disclose this once rather than imply that service
configuration can remain secret-reference-only.

## Service Observability

The Rust backend combines independent sources into one UI state:

- Native OS service status and installed version.
- Existing `dns_relay` `/health` response.
- Existing `/metrics` counters.
- Resolver and relay connectivity probes.
- Platform service logs and `history.txt`.

Failure of one source does not collapse the others. For example, missing
metrics shows Metrics unavailable while service-manager state can still show
Running. Read operations must remain useful when elevation is unavailable.

## Visual Design

The default window is 420 by 720 pixels with the native OS title bar. It uses:

- Kuro Nezumi ink and ash-gray surfaces with warm paper text.
- Moss for running/healthy state and signal red for actions and selection.
- Alert red for stopped/error state and brass for warnings and pending changes.
- The app mark is the Kuro Nezumi macOS Ghostty icon supplied from
  `assets/icons/macos/ghostty.icns` in the Kuro Nezumi project.
- No gradients in the application chrome, decorative blobs, oversized
  marketing typography, or nested cards.
- Cards and framed controls with at most an 8 px radius.
- Stable control dimensions that do not shift with dynamic labels or metrics.

At wider sizes, bottom navigation becomes a narrow left rail. Font sizes do not
scale with viewport width. Status is always communicated through text or icons
as well as color.

The UI uses semantic controls, Lucide icons, tooltips for unfamiliar actions,
visible keyboard focus, screen-reader labels, and reduced motion. Text must fit
at both prototype viewports without overlap or truncating essential state.

## Screens

### Setup

First launch opens the working setup flow rather than a marketing page:

1. Confirm installation and config locations.
2. Select or enter the initial secure resolver profile.
3. Request one elevation prompt to install and start the service.

Existing installations can be imported, validated, and adopted.

### Dashboard

- Large start/stop power control.
- Running, Stopped, Applying, and Error states.
- Listener address, selected upstream or relay, secure-mode state, and uptime.
- Request, cache-hit, failure, and latency counters.
- Quick resolver test, recent events, update state, and recovery action.

### Resolvers

- Latency-ranked upstream list with health and transport type.
- Add, edit, delete, reorder, and test actions.
- UDP, DoH, and DoQ validation.
- Secure-only mode, discovery sources, refresh interval, and ECS subnet.

### Rules

- Drop and Redirect tabs.
- Search, add, edit, delete, enable/disable, and conflict validation.
- External drop-list import and reload state.
- Clear display of inline-only redirect rules.

### Relay

- Direct and Google-chained instances.
- Enable state, URL, transport, timeout, and manual bootstrap.
- Key generation, vault selection, replacement, and connectivity test.
- Client subnet configuration.

### Activity

- Live counters and compact time-series plots.
- Filterable logs with pause, copy, export, and clear controls.
- Query history with configured retention and matching filters.

### Settings

- Listener, VPN reassertion, hot reload, metrics, and history settings.
- Start at login and tray behavior.
- Service install, update, repair, restart, and uninstall.
- Application updates.
- Config import/export.
- Advanced raw-TOML editor with validation and Apply/Revert.

## System Tray

The tray icon communicates running, stopped, and error states without relying
on color alone. Its menu provides Open, Start/Stop, Restart, and Quit. Quitting
the GUI does not stop the system service.

## Error Handling

- Invalid drafts identify the field and preserve user input.
- Failed elevation changes nothing.
- Failed service restart restores the previous config and prior running state.
- Version mismatch blocks unsafe apply and offers service update/repair.
- Unavailable vault access offers retry or encrypted local fallback.
- Unavailable metrics, logs, or history affect only that view.
- Destructive uninstall and plaintext secret export require confirmation.
- Service errors include a direct route to relevant logs and recovery actions.

## Security Requirements

- Strict Content Security Policy and no remote content in production.
- Tauri capabilities scoped to the minimum commands and file locations.
- No shell plugin or arbitrary process execution from JavaScript.
- Admin helper actions and destinations are closed enums/constants.
- Bundled binary integrity and version checks before installation or update.
- Atomic config writes with system-only permissions and one rollback copy.
- Secret masking and secret-free export by default.
- URL, resolver, domain, subnet, and rule validation at the Rust boundary.

Security and validation cannot be weakened for older OS compatibility. An
unsupported secure facility must fail closed or use the explicit encrypted
fallback.

## HTML Prototype

Before Tauri scaffolding, build a standalone interactive prototype using the
same semantic HTML, CSS tokens, and TypeScript-compatible JavaScript structure
planned for production. It uses fixture data and no backend.

The prototype must demonstrate:

- Setup and installed states.
- All six primary views.
- Start/stop state transitions.
- Navigation and responsive left-rail/bottom-bar behavior.
- Rule add/edit/delete and resolver testing.
- Relay secret masked/revealed states.
- Dirty config Apply/Revert behavior.
- Loading, empty, warning, and error states.
- Keyboard navigation and reduced motion.

It must open locally without a development server. Visual QA uses Playwright
screenshots and DOM overlap checks at 420 by 720 and 1024 by 720. Any blank,
clipped, overlapping, or inaccessible primary interaction blocks acceptance.

## Verification Strategy

- Unit tests for config round trips, validation, secret-reference resolution,
  command allowlists, service state parsing, and atomic rollback decisions.
- Integration tests using a fake service manager and temporary filesystem.
- Failure tests for elevation cancellation, invalid staged config, failed
  restart, failed health check, vault unavailability, and version mismatch.
- Existing `dns_relay` workspace tests remain green.
- Platform CI builds the Windows, macOS, and Linux installers.
- Installer/service smoke tests run where CI privileges allow; platform gaps are
  documented and manually verified before release.
- Playwright interaction, accessibility, screenshot, and overlap checks cover
  both prototype viewports and every primary state.

## Acceptance Criteria

The design is implemented when:

1. A user can install the bundled resolver, create a valid config, and start it
   with one native elevation flow.
2. The GUI can manage every current `dns_relay` config field without silently
   discarding imported values.
3. Apply is validated, atomic, permission-restricted, and automatically rolls
   back after a failed restart or health check.
4. Service state, logs, metrics, history, rules, resolvers, and relays remain
   independently usable when another data source is unavailable.
5. The GUI never runs as administrator and exposes no arbitrary shell or file
   operation.
6. Installers are produced for Windows 10+, macOS, and the supported Linux base.
7. The reviewed HTML prototype and production UI pass the specified visual,
   interaction, and accessibility checks.
