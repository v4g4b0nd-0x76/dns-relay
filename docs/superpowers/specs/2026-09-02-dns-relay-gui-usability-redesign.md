# DNS Relay GUI Usability Redesign

**Date:** 2026-09-02

**Status:** Approved direction; awaiting specification review

## Goal

Make every GUI page practical, visually coherent, and comfortable at both the
420 by 720 default window and wider desktop sizes. Preserve all configuration,
service-management, validation, and observability behavior.

## Approach

Restructure the existing semantic HTML only where hierarchy is currently weak,
then replace the accumulated page-specific spacing patches with a small shared
CSS system. Keep framework-free TypeScript, existing Lucide icons, current
state management, and current backend commands. Do not add dependencies or
rewrite the frontend into components.

## Shared Shell

- Use a quiet neutral-dark palette. Reserve red for selection, primary apply
  actions, stopped/error state, and destructive controls. Keep healthy and
  warning colors distinct.
- Use one spacing rhythm for page headings, toolbars, sections, fields, and
  rows. No arbitrary margins on individual cards.
- Keep the desktop rail narrow and the header compact. Let content use the
  available desktop width while retaining a readable maximum width.
- Keep the six-item bottom navigation fixed on compact windows. Scrollable
  content and the pending-changes bar must always end above it.
- Use clear page titles and section headings. Remove repetitive uppercase
  labels where they do not add hierarchy.
- Use cards only for real groups, repeated records, and empty states. Do not
  place cards inside cards.
- Keep keyboard focus, semantic labels, reduced motion, and text-based status
  indicators.

## Page Designs

### Setup

Keep setup as a focused task flow. Use one compact intro, a readable three-step
list, one primary install or repair action, and inline warnings. Match the main
shell typography and control styling.

### Dashboard

Present service state as the primary control without an oversized empty hero.
Keep listener, mode, transport, and saved state readable beside it. Place the
four metrics in a stable responsive grid and show the latest activity as a
compact list with one route to the full Activity page.

### Resolvers

Use one top toolbar for secure-only mode, transport selection, and Add. Render
each resolver as a readable row with transport, address, probe result, and
aligned icon actions. Group discovery switches in one column and discovery
inputs in another on desktop; stack them in label-control order on compact
windows.

### Rules

Keep Add rule as the primary action and blocklist import as secondary. Empty
state messaging and its action belong in one compact region. Existing rules
use scannable rows with rule type, domain, target, enabled state, and edit or
delete actions. Keep the explanatory import note visually secondary.

### Relay

Align enable and manual-bootstrap switches as settings rows. Align timeout and
client-subnet inputs below them. Keep the endpoint list or empty state directly
under the configuration group, with Add relay as the single primary empty-state
action.

### Activity

Use one responsive toolbar: filter first, then Pause, Copy, Export, and Clear.
Display service logs and query history as distinct, compact data regions with
monospace rows. Stack them on compact windows and use available width on
desktop without decorative dead space.

### Settings

Group settings by System, Metrics and history, Obfuscated listener, and Service.
Within each group, align switches together and fields together on desktop;
stack each group predictably on compact windows. Service status and actions
form one compact row.

Remove the Shared states fixture panel from the production UI. It is developer
test scaffolding, not a user setting. Keep tests able to create backend warning,
loading, empty, and error states through mocked state instead.

Place Advanced Raw TOML in a native `details` disclosure that is collapsed by
default. Inside it, keep the validation badge, editor, and actions separated by
the shared spacing rhythm. Import and export remain secondary; plaintext export
remains destructive.

## Responsive Behavior

- Desktop starts at 760 px and uses the left rail with two-column forms where
  labels and controls remain aligned.
- Compact mode uses the bottom navigation, 16 px content padding, one-column
  forms, full-width text controls, and wrapped action groups.
- Essential labels, addresses, status text, and action icons must not clip or
  overlap at 390, 420, 760, 1024, and 1440 px widths.
- Dynamic content must not resize navigation, metric tiles, switches, or icon
  buttons.

## Implementation Boundaries

- Primary files: `gui/src/render.ts`, `gui/src/styles.css`, and focused layout
  tests in `gui/tests/app.spec.mjs`.
- Reuse current render helpers and DOM actions. Add no UI framework, design
  dependency, animation package, or new state abstraction.
- Preserve the user's current uncommitted CSS edit until implementation; then
  replace the conflicting settings margin rule as part of the approved layout.
- Do not change Rust commands, configuration paths, persistence, or service
  behavior for this redesign.

## Verification

1. Add regression checks for production fixture removal, Raw TOML disclosure,
   consistent section gaps, action containment, and horizontal overflow.
2. Run the frontend test suite and production frontend build.
3. Capture and inspect every page at 420 by 720 and 1024 by 768, plus Settings
   at 1440 by 900.
4. Verify no content overlaps navigation or the pending-changes bar.
5. Run `git diff --check` and the Tauri production build before delivery.

## Acceptance Criteria

- Every production page has a clear primary task and consistent hierarchy.
- Settings no longer exposes Shared states and Raw TOML is collapsed initially.
- Forms and action groups align without crowding at desktop and compact sizes.
- Dashboard and empty operational pages use space intentionally.
- No page has horizontal overflow, clipped essential text, overlapping controls,
  or content hidden behind fixed navigation.
- Existing functional tests continue to pass and the packaged GUI builds.
