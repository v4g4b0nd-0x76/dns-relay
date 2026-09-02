# DNS Relay GUI Usability Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a coherent, responsive, and visually verified GUI across Setup, Dashboard, Resolvers, Rules, Relay, Activity, and Settings without changing backend behavior.

**Architecture:** Keep the existing framework-free TypeScript renderer and event flow. Fix hierarchy in `render.ts`, establish one shared grid-and-gap layout in `styles.css`, and protect each page with focused Playwright layout checks before visual QA.

**Tech Stack:** TypeScript, semantic HTML, CSS, Lucide, Playwright, Vite, Tauri 2

**Spec:** `docs/superpowers/specs/2026-09-02-dns-relay-gui-usability-redesign.md`

## Global Constraints

- Preserve every configuration path, backend command, service action, validation flow, and observability source.
- Add no dependency, UI framework, design package, or new state abstraction.
- Support compact bottom navigation below 760 px and desktop left navigation from 760 px.
- Essential content must fit at 390, 420, 760, 1024, and 1440 px without overlap or horizontal overflow.
- Use red only for selection, primary apply actions, stopped/error state, and destructive controls.
- Keep cards at 8 px radius or less and do not nest cards.
- Keep semantic labels, keyboard focus, reduced motion, and text-based status indicators.
- Replace the existing uncommitted `.settings-view` margin override as part of the shared layout; do not discard unrelated user work.

## File Map

- `gui/src/render.ts`: production page hierarchy and semantic grouping.
- `gui/src/styles.css`: production overrides, shared spacing, and responsive page layouts.
- `gui/tests/app.spec.mjs`: behavior, containment, alignment, and overflow regressions.
- `gui/prototype/styles.css`: unchanged; the prototype remains historical reference coverage.

---

### Task 1: Shared Shell and Page Rhythm

**Files:**
- Modify: `gui/src/styles.css:1-413`
- Test: `gui/tests/app.spec.mjs:120-190`

**Interfaces:**
- Consumes: existing `.app-shell`, `.view-host`, `.view`, `.page-heading`, `.card`, and `.toolbar` markup.
- Produces: every production `.view` as a grid with a stable shared row gap and no inherited sibling margins.

- [ ] **Step 1: Write the failing shared-layout tests**

```js
test("production pages use one spacing rhythm", async ({ page }) => {
  await openApp(page, 1024, 768);
  for (const view of ["dashboard", "resolvers", "rules", "relay", "activity", "settings"]) {
    await page.locator(`[data-target='${view}']`).click();
    const layout = await page.locator(`[data-view='${view}']`).evaluate((node) => {
      const style = getComputedStyle(node);
      return { display: style.display, gap: Number.parseFloat(style.rowGap) };
    });
    expect(layout.display).toBe("grid");
    expect(layout.gap).toBeGreaterThanOrEqual(14);
    expect(layout.gap).toBeLessThanOrEqual(18);
  }
});

for (const width of [390, 420, 760, 1024, 1440]) {
  test(`production pages have no horizontal overflow at ${width}px`, async ({ page }) => {
    await openApp(page, width, 768);
    for (const view of ["dashboard", "resolvers", "rules", "relay", "activity", "settings"]) {
      await page.locator(`[data-target='${view}']`).click();
      const overflow = await page.locator("[data-view-host]").evaluate((host) => host.scrollWidth - host.clientWidth);
      expect(overflow).toBeLessThanOrEqual(1);
    }
  });
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `cd gui && npm test -- --workers=1 -g "spacing rhythm|horizontal overflow"`

Expected: spacing test fails because `.view` is not a grid; the uncommitted Settings margin rule also prevents one consistent rhythm.

- [ ] **Step 3: Implement the shared shell rules**

Add production overrides in `gui/src/styles.css` and remove the conflicting `.settings-view` sibling margin block:

```css
.app-shell { max-width: 1200px; }

.view-host { padding: 22px 24px 84px; }

.view {
  display: grid;
  gap: 16px;
  width: min(100%, 1040px);
}

.page-heading,
.toolbar,
.card + .card,
.section-heading + .card,
.card + .section-heading { margin: 0; }

@media (max-width: 759px) {
  .view-host { padding: 16px 16px 88px; }
  .view { gap: 14px; }
}
```

- [ ] **Step 4: Run the focused tests and existing navigation checks**

Run: `cd gui && npm test -- --workers=1 -g "spacing rhythm|horizontal overflow|navigation does not overlap"`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: unify gui shell spacing"
```

### Task 2: Setup and Dashboard

**Files:**
- Modify: `gui/src/render.ts:118-157`
- Modify: `gui/src/styles.css`
- Test: `gui/tests/app.spec.mjs:1-80,133-315`

**Interfaces:**
- Consumes: `heading()`, `metric()`, `renderActivityPreview()`, and existing setup fixtures.
- Produces: `.dashboard-view`, compact service summary, stable metric tiles, and bounded setup content.

- [ ] **Step 1: Write failing compactness tests**

```js
test("dashboard service summary stays compact at both layouts", async ({ page }) => {
  for (const width of [420, 1024]) {
    await openApp(page, width, 720);
    const hero = await page.locator("[data-view='dashboard'] .hero").boundingBox();
    expect(hero.height).toBeLessThan(width < 760 ? 230 : 200);
  }
});

test("setup content fits above compact navigation", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=first-launch`);
  const setup = await page.locator("[data-view='setup']").boundingBox();
  const nav = await page.getByRole("navigation", { name: "Primary" }).boundingBox();
  expect(setup.y + setup.height).toBeLessThanOrEqual(nav.y - 8);
});
```

- [ ] **Step 2: Run and verify RED**

Run: `cd gui && npm test -- --workers=1 -g "service summary stays compact|setup content fits"`

Expected: dashboard compact height or setup containment fails under the current oversized controls.

- [ ] **Step 3: Add the dashboard hook and compact styles**

Change the dashboard section opening to:

```ts
return `<section class="view dashboard-view" data-view="dashboard">${heading("Control", "Dashboard", "Service and resolver state at a glance")}
```

Add CSS:

```css
.dashboard-view .hero { grid-template-columns: 104px minmax(0, 1fr); padding: 18px; }
.dashboard-view .power-button { width: 92px; height: 92px; }
.dashboard-view .power-button svg { width: 34px; height: 34px; }
.dashboard-view .service-title { font-size: 26px; }
.dashboard-view .metrics,
.dashboard-view .activity-preview { margin-top: 0; }
.setup { margin-top: 0; }

@media (max-width: 759px) {
  .dashboard-view .hero { grid-template-columns: 92px minmax(0, 1fr); padding: 14px; }
  .dashboard-view .power-button { width: 82px; height: 82px; }
  .setup > img { width: 76px; height: 76px; }
  .setup-steps { margin: 14px 0; }
}
```

- [ ] **Step 4: Run dashboard and setup tests**

Run: `cd gui && npm test -- --workers=1 -g "dashboard|first launch|setup content|existing install|partial install"`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gui/src/render.ts gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: tighten setup and dashboard layouts"
```

### Task 3: Resolvers

**Files:**
- Modify: `gui/src/render.ts:159-173`
- Modify: `gui/src/styles.css`
- Test: `gui/tests/app.spec.mjs:225-240,328-345`

**Interfaces:**
- Consumes: existing resolver commands and `checkField()`.
- Produces: `.resolver-toolbar` and a labeled `.resolver-options` section with aligned desktop columns and compact stacking.

- [ ] **Step 1: Write the failing hierarchy test**

```js
test("resolver discovery is a labeled aligned section", async ({ page }) => {
  await openApp(page, 1024, 768);
  await page.locator("[data-target='resolvers']").click();
  await expect(page.locator("[data-resolver-options]")).toContainText("Discovery");
  const columns = await page.locator("[data-resolver-options]").evaluate((card) =>
    getComputedStyle(card).gridTemplateColumns.split(" ").length,
  );
  expect(columns).toBe(2);
});
```

- [ ] **Step 2: Run and verify RED**

Run: `cd gui && npm test -- --workers=1 -g "resolver discovery is a labeled"`

Expected: FAIL because the discovery card has no section heading.

- [ ] **Step 3: Add semantic hooks and layout rules**

Use these render hooks:

```ts
<div class="toolbar resolver-toolbar">
<div class="card split-form resolver-options" data-resolver-options>
  <div class="section-label span-2"><strong>Discovery</strong><span>Find and refresh compatible upstreams</span></div>
```

Add CSS:

```css
.resolver-toolbar label { flex: 1; }
.resolver-toolbar select { min-width: 180px; }
.resolver-options { align-items: start; }
.section-label { display: grid; gap: 2px; }
.section-label span { color: var(--muted); font-size: 12px; }

@media (max-width: 759px) {
  .resolver-toolbar { display: grid; grid-template-columns: 1fr auto; }
  .resolver-toolbar .security-toggle { grid-column: 1 / -1; }
  .resolver-toolbar .button { align-self: stretch; }
}
```

- [ ] **Step 4: Run resolver tests**

Run: `cd gui && npm test -- --workers=1 -g "resolver"`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gui/src/render.ts gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: clarify resolver controls"
```

### Task 4: Rules

**Files:**
- Modify: `gui/src/render.ts:175-191`
- Modify: `gui/src/styles.css`
- Test: `gui/tests/app.spec.mjs:195-225,465-532`

**Interfaces:**
- Consumes: existing rule dialog and blocklist import actions.
- Produces: exactly one Add rule action in both populated and empty states, plus `.rules-note` secondary copy.

- [ ] **Step 1: Write the failing empty-state action test**

```js
test("empty rules keeps one primary add action inside the empty state", async ({ page }) => {
  await openApp(page, 420, 720);
  await page.locator("[data-target='rules']").click();
  await expect(page.getByRole("button", { name: "Add rule" })).toHaveCount(1);
  await expect(page.locator(".empty-state").getByRole("button", { name: "Add rule" })).toBeVisible();
});
```

- [ ] **Step 2: Run and verify RED**

Run: `cd gui && npm test -- --workers=1 -g "empty rules keeps one primary"`

Expected: FAIL because Add rule currently sits outside the empty state.

- [ ] **Step 3: Render one context-aware action and style the note**

Define the shared action once, then use it in exactly one place:

```ts
const addRule = '<button class="button primary" data-action="open-dialog"><i data-lucide="plus"></i> Add rule</button>';
return `<section class="view" data-view="rules">${heading("Policy", "Rules", `${drops.length} drop · ${redirects.length} redirect`)}
  <div class="toolbar rules-toolbar">${rows.length ? addRule : ""}<label class="button">Import blocklist<input class="sr-only" type="file" accept="text/plain" data-blocklist-import></label></div>
  <div class="card${rows.length ? "" : " empty-card"}">${rows.length ? rows.map((rule) => `<div class="row"><span class="badge transport">${rule.kind === "drop" ? "DROP" : "DNS"}</span><div class="row-main"><strong>${escapeHtml(rule.domain)}</strong><span>${escapeHtml(rule.target)}</span></div><div class="row-actions">${iconAction("settings", `Edit ${rule.domain}`, "edit-rule", `data-kind="${rule.kind}" data-index="${rule.index}"`)}${iconAction("trash-2", `Delete ${rule.domain}`, "delete-rule", `data-kind="${rule.kind}" data-index="${rule.index}"`)}</div></div>`).join("") : emptyState("shield-ban", "No rules yet", "Add a drop or redirect rule, or import a plain-text blocklist.", addRule)}</div>
  <p class="muted rules-note">Drop lists may be imported. Redirect rules remain inline-only.</p>
</section>`;
```

Add:

```css
.rules-note { margin: -4px 0 0; font-size: 12px; }
.rules-toolbar { justify-content: flex-start; }
```

- [ ] **Step 4: Run rule tests**

Run: `cd gui && npm test -- --workers=1 -g "rule|blocklist|empty operational"`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gui/src/render.ts gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: make rules actions contextual"
```

### Task 5: Relay

**Files:**
- Modify: `gui/src/render.ts:193-206`
- Modify: `gui/src/styles.css`
- Test: `gui/tests/app.spec.mjs:163-184,246-269,533-620`

**Interfaces:**
- Consumes: existing relay instance, vault, probe, and secret actions.
- Produces: `.relay-settings` for aligned switches and fields and `.relay-card` for endpoint records.

- [ ] **Step 1: Write the failing alignment test**

```js
test("relay settings separate switches from text fields on desktop", async ({ page }) => {
  await openApp(page, 1024, 768);
  await page.locator("[data-target='relay']").click();
  const layout = await page.locator(".relay-settings").evaluate((card) => ({
    columns: getComputedStyle(card).gridTemplateColumns.split(" ").length,
    toggles: card.querySelectorAll(".option-column .check-row").length,
    fields: card.querySelectorAll(".field-column input").length,
  }));
  expect(layout).toEqual({ columns: 2, toggles: 2, fields: 2 });
});
```

- [ ] **Step 2: Run and verify RED**

Run: `cd gui && npm test -- --workers=1 -g "relay settings separate"`

Expected: FAIL because relay controls currently share a flat form grid.

- [ ] **Step 3: Reuse the existing split-form pattern**

Render:

```ts
<div class="card split-form relay-settings"><div class="option-column">${checkField("Enable relay", "relay_conf.enable", relay?.enable)}${checkField("Manual bootstrap", "relay_conf.resolve_manual", relay?.resolve_manual)}</div><div class="field-column"><label>Timeout (seconds)<input type="number" min="1" data-config-path="relay_conf.relay_timeout_sec" data-value-type="number" value="${relay?.relay_timeout_sec ?? 5}"></label><label>Client subnet<input data-config-path="client_subnet" data-value-type="optional-string" value="${escapeHtml(draft?.client_subnet ?? "")}" placeholder="Auto"></label></div></div>
```

Add `relay-card` to populated endpoint cards and CSS:

```css
.relay-card .section-heading { align-items: flex-start; }
.relay-card .secret-line { flex-wrap: wrap; }
.relay-card .secret-line .row-actions { flex-wrap: wrap; justify-content: flex-end; }

@media (max-width: 759px) {
  .relay-card .secret-line { align-items: flex-start; }
  .relay-card .secret-line .row-actions { width: 100%; justify-content: flex-start; }
}
```

- [ ] **Step 4: Run relay tests**

Run: `cd gui && npm test -- --workers=1 -g "relay"`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gui/src/render.ts gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: align relay settings and actions"
```

### Task 6: Activity

**Files:**
- Modify: `gui/src/render.ts:208-217`
- Modify: `gui/src/styles.css`
- Test: `gui/tests/app.spec.mjs:622-651`

**Interfaces:**
- Consumes: existing filter, pause, copy, export, clear, logs, and history behavior.
- Produces: unframed `.activity-toolbar` and responsive `.activity-grid` containing two sibling data cards.

- [ ] **Step 1: Write the failing responsive-grid test**

```js
test("activity data regions use desktop width and stack compactly", async ({ page }) => {
  for (const [width, expectedColumns] of [[420, 1], [1024, 2]]) {
    await openApp(page, width, 720);
    await page.locator("[data-target='activity']").click();
    const columns = await page.locator(".activity-grid").evaluate((grid) =>
      getComputedStyle(grid).gridTemplateColumns.split(" ").length,
    );
    expect(columns).toBe(expectedColumns);
  }
});
```

- [ ] **Step 2: Run and verify RED**

Run: `cd gui && npm test -- --workers=1 -g "activity data regions use"`

Expected: FAIL because `.activity-grid` does not exist.

- [ ] **Step 3: Separate toolbar and data regions**

Replace `renderActivity()` with:

```ts
function renderActivity(state: ShellState) {
  const query = state.activityFilter.toLowerCase();
  const logs = (state.logs.value ?? []).filter((line) => line.toLowerCase().includes(query));
  const history = (state.history.value ?? []).filter((line) => line.toLowerCase().includes(query));
  return `<section class="view" data-view="activity">${heading("Observability", "Activity", "Bounded service logs and query history")}
    ${state.logs.error ? `<div class="notice warning">Logs unavailable: ${escapeHtml(state.logs.error)}</div>` : ""}${state.history.error ? `<div class="notice warning">History unavailable: ${escapeHtml(state.history.error)}</div>` : ""}
    <div class="toolbar activity-toolbar"><label><span class="sr-only">Filter activity</span><input type="search" aria-label="Filter activity" data-activity-filter data-focus="activity-filter" value="${escapeHtml(state.activityFilter)}" placeholder="Filter activity"></label><button class="button" data-action="pause-activity">${state.activityPaused ? "Resume" : "Pause"}</button><button class="button" data-action="copy-activity"><i data-lucide="copy"></i> Copy</button><button class="button" data-action="export-activity"><i data-lucide="download"></i> Export</button><button class="button danger-button" data-action="clear-activity">Clear view</button></div>
    <div class="activity-grid"><section class="card"><p class="eyebrow">Service logs</p>${logs.length ? `<ul class="log-list">${logs.map((line) => `<li>${escapeHtml(line)}</li>`).join("")}</ul>` : '<div class="empty-state">No matching logs</div>'}</section><section class="card"><p class="eyebrow">Query history</p>${history.length ? `<ul class="log-list">${history.map((line) => `<li>${escapeHtml(line)}</li>`).join("")}</ul>` : '<div class="empty-state">No matching history</div>'}</section></div>
  </section>`;
}
```

Add:

```css
.activity-toolbar { display: grid; grid-template-columns: minmax(220px, 1fr) repeat(4, auto); }
.activity-toolbar label { min-width: 0; }
.activity-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
.activity-grid .card { min-width: 0; }
.activity-grid .log-list { overflow-wrap: anywhere; }

@media (max-width: 759px) {
  .activity-toolbar { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .activity-toolbar label { grid-column: 1 / -1; }
  .activity-grid { grid-template-columns: 1fr; gap: 14px; }
}
```

- [ ] **Step 4: Run activity tests**

Run: `cd gui && npm test -- --workers=1 -g "activity"`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gui/src/render.ts gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: improve activity scanning"
```

### Task 7: Settings

**Files:**
- Modify: `gui/src/render.ts:1-40,219-240`
- Modify: `gui/src/styles.css`
- Test: `gui/tests/app.spec.mjs:315-430,640-685`

**Interfaces:**
- Consumes: current Settings config bindings and raw TOML actions.
- Produces: no production Shared states fixture, a compact service row, and closed native `details[data-settings-advanced]` with unchanged raw actions.

- [ ] **Step 1: Write failing Settings hierarchy tests**

```js
test("settings hides developer fixtures and collapses Raw TOML", async ({ page }) => {
  await openApp(page, 1024, 768);
  await page.locator("[data-target='settings']").click();
  await expect(page.getByText("Shared states", { exact: true })).toHaveCount(0);
  const advanced = page.locator("details[data-settings-advanced]");
  await expect(advanced).not.toHaveAttribute("open", "");
  await expect(page.getByLabel("Raw TOML")).not.toBeVisible();
  await advanced.locator("summary").click();
  await expect(page.getByLabel("Raw TOML")).toBeVisible();
});

test("settings service actions share one bounded row", async ({ page }) => {
  await openApp(page, 1024, 768);
  await page.locator("[data-target='settings']").click();
  const row = await page.locator("[data-settings-service]").evaluate((card) => ({
    height: card.getBoundingClientRect().height,
    overflow: card.scrollWidth - card.clientWidth,
  }));
  expect(row.height).toBeLessThan(110);
  expect(row.overflow).toBeLessThanOrEqual(1);
});
```

- [ ] **Step 2: Run and verify RED**

Run: `cd gui && npm test -- --workers=1 -g "settings hides developer|settings service actions share"`

Expected: FAIL because Shared states is visible and Raw TOML is always open.

- [ ] **Step 3: Remove fixture markup and use native disclosure**

Remove `ServiceState` from the import, remove the `fixture` map, and replace `.settings-grid` with these exact siblings:

```ts
<div class="card compact-card service-card" data-settings-service><div class="section-heading"><div><p class="eyebrow">Service</p><h2>${title(state.app.service)}</h2></div><div class="toolbar service-actions"><button class="button" data-action="service-action" data-service-action="restart">Restart</button><button class="button" data-action="service-action" data-service-action="repair">Repair</button><button class="button danger-button" data-action="service-action" data-service-action="uninstall">Uninstall</button></div></div></div>
<details class="card advanced-settings" data-settings-advanced>
  <summary><span><small>Advanced</small><strong>Raw TOML</strong></span><span class="badge">${state.rawError ? "Invalid" : "Validated on import"}</span></summary>
  <div class="advanced-body"><textarea aria-label="Raw TOML" data-raw-toml data-focus="raw-toml" placeholder="Load or paste TOML">${escapeHtml(state.rawToml)}</textarea>${state.rawError ? `<p class="field-error" role="alert">${escapeHtml(state.rawError)}</p>` : ""}<div class="toolbar config-actions"><button class="button" data-action="load-raw">Load draft</button><button class="button primary" data-action="validate-raw">Validate and use</button><label class="button">Import<input class="sr-only" type="file" accept=".toml,text/plain" data-config-import></label><button class="button" data-action="export-safe">Export safely</button><button class="button danger-button" data-action="export-plaintext">Export plaintext…</button></div></div>
</details>
```

Add:

```css
.service-card .section-heading { gap: 16px; }
.service-card .service-actions { margin: 0; justify-content: flex-end; }
.advanced-settings { padding: 0; }
.advanced-settings summary { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 16px; cursor: pointer; }
.advanced-settings summary::marker { color: var(--muted); }
.advanced-settings summary span:first-child { display: grid; gap: 2px; }
.advanced-settings summary small { color: var(--danger); font-size: 11px; font-weight: 800; text-transform: uppercase; }
.advanced-settings .advanced-body { display: grid; gap: 12px; padding: 0 16px 16px; }
.advanced-settings .config-actions { margin: 0; }

@media (max-width: 759px) {
  .service-card .section-heading { align-items: flex-start; }
  .service-card .service-actions { width: 100%; justify-content: flex-start; }
  .advanced-settings .config-actions { grid-template-columns: 1fr; }
}
```

Update every raw-TOML test to click `details[data-settings-advanced] > summary` before locating hidden controls. Delete old tests that assert `.settings-grid`, `[data-settings-shared]`, `.fixture`, or side-by-side service/shared cards.

- [ ] **Step 4: Run all Settings and raw TOML tests**

Run: `cd gui && npm test -- --workers=1 -g "settings|raw TOML|service control"`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add gui/src/render.ts gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: simplify settings hierarchy"
```

### Task 8: Full Visual and Package Verification

**Files:**
- Modify if inspection finds a defect: `gui/src/styles.css`, `gui/src/render.ts`, `gui/tests/app.spec.mjs`
- Verify: all production and package targets

**Interfaces:**
- Consumes: completed page tasks.
- Produces: inspected screenshots and a buildable desktop package.

- [ ] **Step 1: Run the complete frontend suite**

Run: `cd gui && npm test -- --workers=1`

Expected: all tests PASS with zero failures.

- [ ] **Step 2: Run the frontend build and static checks**

Run: `cd gui && npm run build`

Expected: TypeScript and Vite exit 0.

Run: `git diff --check`

Expected: no output and exit 0.

- [ ] **Step 3: Capture every page at required viewports**

Using the existing local preview and browser automation, capture Setup,
Dashboard, Resolvers, Rules, Relay, Activity, and Settings at 420 by 720 and
1024 by 768. Capture Settings at 1440 by 900 with Raw TOML both closed and open.

- [ ] **Step 4: Inspect and fix only concrete visual defects**

Reject screenshots containing horizontal overflow, clipped essential text,
overlapping controls, inconsistent section gaps, controls hidden behind fixed
navigation, dead fixture content, or an always-open Raw TOML editor. For each
defect, add one focused failing Playwright assertion, run it RED, apply the
smallest CSS or markup correction, and run it GREEN.

- [ ] **Step 5: Run final desktop package verification**

Run: `cd gui && npm run tauri build`

Expected: exit 0 and refreshed Linux package artifacts on Linux.

Run: `git status --short`

Expected: only intended source/test changes plus the pre-existing untracked
`docs/assets/` directory if it remains outside this redesign.

- [ ] **Step 6: Commit final visual corrections**

```bash
git add gui/src/render.ts gui/src/styles.css gui/tests/app.spec.mjs
git commit -m "fix: finish gui visual QA"
```
