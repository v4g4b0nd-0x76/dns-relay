import { test, expect } from "@playwright/test";

const production = "http://127.0.0.1:1420";

async function openApp(page, width = 420) {
  await page.setViewportSize({ width, height: 720 });
  await page.goto(production);
}

test("first launch installs and starts through one setup action", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=first-launch`);
  await expect(page.locator("[data-view='setup']")).toBeVisible();
  await page.getByRole("button", { name: "Install and start" }).click();
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
});

test("cancelled elevation leaves first launch recoverable", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=install-error`);
  await page.getByRole("button", { name: "Install and start" }).click();
  await expect(page.locator("[data-view='setup']")).toBeVisible();
  await expect(page.locator("[data-toast]")).toContainText("cancelled");
  await expect(page.getByRole("button", { name: "Install and start" })).toBeEnabled();
});

test("existing install is protected until explicit adoption", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=existing`);
  await expect(page.getByRole("heading", { name: "Adopt DNS Relay" })).toBeVisible();
  await expect(page.getByText("protected from default replacement")).toBeVisible();
  await page.getByRole("button", { name: "Adopt existing configuration" }).click();
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
  await expect(page.locator("[data-toast]")).toContainText("adopted");
});

test("partial install exposes a safe repair path", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=partial-install`);
  await expect(page.getByRole("heading", { name: "Repair DNS Relay" })).toBeVisible();
  await page.getByRole("button", { name: "Repair installation" }).click();
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
});

test("repair preserves an existing config behind adoption", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=partial-existing`);
  await page.getByRole("button", { name: "Repair installation" }).click();
  await expect(page.getByRole("heading", { name: "Adopt DNS Relay" })).toBeVisible();
  await page.getByRole("button", { name: "Adopt existing configuration" }).click();
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
});

test("stopped service keeps non-connection telemetry errors visible", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=metrics-error`);
  await expect(page.locator("[data-service-state]")).toHaveText("Stopped");
  await expect(page.getByText("Service stopped", { exact: true })).toBeVisible();
  await expect(page.getByText(/Metrics unavailable:/)).toBeVisible();
});

test("stopped service suppresses connection-refused endpoint noise", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=connection-refused`);
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
  await expect(page.getByText(/Health unavailable: Connection refused/)).toBeVisible();
  await page.getByRole("button", { name: "Stop DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Stopped");
  await expect(page.getByText("Service stopped", { exact: true })).toBeVisible();
  await expect(page.getByText(/unavailable:/)).toHaveCount(0);
});

test("restart failure is reported without a false success state", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=restart-error`);
  await page.locator("[data-target='settings']").click();
  await page.getByRole("button", { name: "Restart" }).click();
  await expect(page.locator("[data-toast]")).toContainText("restart failed");
  await expect(page.locator("[data-header-state]")).toHaveText("Error");
});

test("initial service status failure keeps independent dashboard data", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=service-error`);
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
  await expect(page.locator("[data-service-state]")).toHaveText("Error");
  await expect(page.getByText("Healthy", { exact: true })).toBeVisible();
  await expect(page.getByText(/Service status unavailable/)).toBeVisible();
});

test("polling leaves an active modal and its focus intact", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='rules']").click();
  await page.getByRole("button", { name: "Add rule" }).click();
  await page.getByLabel("Domain pattern").fill("editing.example");
  await page.waitForTimeout(5200);
  await expect(page.getByRole("dialog", { name: "Add rule" })).toBeVisible();
  await expect(page.getByLabel("Domain pattern")).toHaveValue("editing.example");
  await expect(page.getByLabel("Domain pattern")).toBeFocused();
});

test("production shell exposes the six operational views", async ({ page }) => {
  await openApp(page);
  await expect(page.locator("[data-app-shell]")).toBeVisible();
  await expect(page.locator("[data-view-nav]")).toHaveCount(6);
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
  await page.locator("[data-target='settings']").click();
  await expect(page.locator("[data-view='settings']")).toBeVisible();
});

test("desktop shell uses the available window width", async ({ page }) => {
  await openApp(page, 1440);
  const shell = await page.locator("[data-app-shell]").boundingBox();

  expect(shell).not.toBeNull();
  expect(shell.width).toBeGreaterThan(1360);
  expect(shell.x).toBeLessThan(4);
});

test("wide desktop content starts near the navigation rail", async ({ page }) => {
  await openApp(page, 1920);
  const view = await page.locator("[data-view='dashboard']").boundingBox();

  expect(view).not.toBeNull();
  expect(view.x).toBeLessThan(180);
});

test("boolean config fields render as app switches", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='settings']").click();

  const checkboxCount = await page.locator(".check-row input[type='checkbox']").count();
  await expect(page.locator(".check-row .switch")).toHaveCount(checkboxCount);
});

test("compact dashboard keeps service facts readable", async ({ page }) => {
  await openApp(page);

  const narrowFacts = await page.locator(".detail-list > div").evaluateAll((items) =>
    items
      .map((item) => item.getBoundingClientRect().width)
      .filter((width) => width < 130),
  );

  expect(narrowFacts).toEqual([]);
});

test("desktop forms have breathing room inside cards", async ({ page }) => {
  await openApp(page, 1440);
  await page.locator("[data-target='relay']").click();

  const spacing = await page.locator("[data-view='relay'] .card.form-grid").first().evaluate((card) => {
    const cardStyle = getComputedStyle(card);
    const formStyle = getComputedStyle(card);
    return {
      padding: Number.parseFloat(cardStyle.paddingTop),
      rowGap: Number.parseFloat(formStyle.rowGap),
      columnGap: Number.parseFloat(formStyle.columnGap),
    };
  });

  expect(spacing.padding).toBeGreaterThanOrEqual(22);
  expect(spacing.rowGap).toBeGreaterThanOrEqual(18);
  expect(spacing.columnGap).toBeGreaterThanOrEqual(18);
});

test("file action buttons do not stretch across toolbars", async ({ page }) => {
  await openApp(page, 1440);
  await page.locator("[data-target='rules']").click();

  const addButton = await page.getByRole("button", { name: "Add rule" }).boundingBox();
  const importButton = await page.getByText("Import blocklist", { exact: true }).boundingBox();

  expect(addButton).not.toBeNull();
  expect(importButton).not.toBeNull();
  expect(importButton.width).toBeLessThan(220);
  expect(importButton.x - (addButton.x + addButton.width)).toBeLessThan(24);
});

test("empty operational views are actionable", async ({ page }) => {
  await openApp(page, 1440);

  await expect(page.getByRole("button", { name: "Open activity" })).toBeVisible();
  await page.getByRole("button", { name: "Open activity" }).click();
  await expect(page.locator("[data-view='activity']")).toBeVisible();
  await page.locator("[data-target='rules']").click();
  await expect(page.getByText("No rules yet", { exact: true })).toBeVisible();
  await page.locator("[data-target='relay']").click();
  await expect(page.getByText("No relay endpoints", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Add relay" })).toBeVisible();
});

test("dashboard activity preview stays compact", async ({ page }) => {
  await openApp(page, 1440);

  const preview = await page.locator("[data-dashboard-activity]").boundingBox();

  expect(preview).not.toBeNull();
  expect(preview.height).toBeLessThan(180);
});

test("settings listener field uses a full row", async ({ page }) => {
  await openApp(page, 1440);
  await page.locator("[data-target='settings']").click();

  const card = await page.locator("[data-settings-core]").boundingBox();
  const listener = await page.getByLabel("Listener address").boundingBox();

  expect(card).not.toBeNull();
  expect(listener).not.toBeNull();
  expect(listener.width).toBeGreaterThan(card.width * 0.8);
});

test("production service control reports its final state", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
  await page.getByRole("button", { name: "Stop DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Stopped");
  await expect(page.locator("[data-live-region]")).toContainText("stopped");
});

test("failed service can be stopped from the power control", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=service-error`);
  await page.getByRole("button", { name: "Stop DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Stopped");
});

test("production draft can be reverted or applied", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='resolvers']").click();
  await page.getByLabel("Secure resolvers only").uncheck();
  await expect(page.getByLabel("Secure resolvers only")).toBeFocused();
  await expect(page.locator("[data-dirty-bar]")).toBeVisible();
  await page.getByRole("button", { name: "Revert changes" }).click();
  await expect(page.getByLabel("Secure resolvers only")).toBeChecked();
  await page.getByLabel("Secure resolvers only").uncheck();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.getByLabel("Secure resolvers only")).toBeDisabled();
  await expect(page.getByRole("button", { name: "Revert changes" })).toBeDisabled();
  await expect(page.locator("[data-dirty-bar]")).toBeHidden();
  await expect(page.locator("[data-toast]")).toContainText("applied");
});

test("production dialog restores focus and closes with Escape", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='rules']").click();
  const trigger = page.getByRole("button", { name: "Add rule" });
  await trigger.click();
  await expect(page.getByRole("dialog", { name: "Add rule" })).toBeVisible();
  await expect(page.getByLabel("Domain pattern")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
});

test("resolver workflow preserves order, probes, and rejects insecure empty state", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='resolvers']").click();
  await expect(page.getByLabel("Secure resolvers only")).toHaveCount(1);
  await expect(page.locator(".toolbar").getByLabel("Secure resolvers only")).toBeVisible();
  await page.getByLabel("New resolver transport").selectOption("quic://9.9.9.9:853");
  await page.getByRole("button", { name: "Add resolver" }).click();
  const resolvers = page.locator("[data-config-path^='resolvers.']");
  await expect(resolvers).toHaveCount(2);
  await expect(resolvers.nth(1)).toHaveValue("quic://9.9.9.9:853");
  await page.getByRole("button", { name: "Move resolver 2 up" }).click();
  await expect(resolvers.nth(0)).toHaveValue("quic://9.9.9.9:853");
  await page.getByRole("button", { name: "Test resolver 1" }).click();
  await expect(page.getByText(/Resolver is reachable/)).toBeVisible();
  await page.getByRole("button", { name: "Delete resolver 1" }).click();
  await page.getByRole("button", { name: "Delete resolver 1" }).click();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator("[data-toast]")).toContainText("authenticated resolver");
});

test("rule workflow validates, creates, edits, deletes, and imports", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='rules']").click();
  await page.getByRole("button", { name: "Add rule" }).click();
  await expect(page.getByLabel("Rule type")).toHaveValue("drop");
  await expect(page.getByLabel("IPv4 addresses")).toBeHidden();
  await page.getByLabel("Rule type").selectOption("redirect");
  await expect(page.getByLabel("IPv4 addresses")).toBeVisible();
  await page.getByLabel("Domain pattern").fill("bad");
  await page.getByLabel("IPv4 addresses").fill("999.1.1.1");
  await page.getByRole("button", { name: "Save rule" }).click();
  await expect(page.locator("[data-rule-error]")).toContainText("valid domain and IPv4");
  await page.getByLabel("Domain pattern").fill("ads.example");
  await page.getByLabel("IPv4 addresses").fill("10.0.0.1");
  await page.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText("ads.example", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Edit ads.example" }).click();
  await expect(page.getByLabel("Rule type")).toHaveValue("redirect");
  await expect(page.getByLabel("IPv4 addresses")).toHaveValue("10.0.0.1");
  await page.getByLabel("Domain pattern").fill("tracker.example");
  await page.getByRole("button", { name: "Save rule" }).click();
  await page.getByRole("button", { name: "Delete tracker.example" }).click();
  await expect(page.getByText("tracker.example", { exact: true })).toHaveCount(0);
  await page.locator("[data-blocklist-import]").setInputFiles({ name: "block.txt", mimeType: "text/plain", buffer: Buffer.from("one.example\n# ignored\ntwo.example\n") });
  await expect(page.getByText("one.example", { exact: true })).toBeVisible();
});

test("drop rule hides the redirect target", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='rules']").click();
  await page.getByRole("button", { name: "Add rule" }).click();
  await page.getByLabel("Domain pattern").fill("drop.example");
  await page.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText("drop.example", { exact: true })).toBeVisible();
  await expect(page.getByText("drop", { exact: true })).toBeVisible();
});

test("relay secrets stay vaulted and probes report latency", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='relay']").click();
  await page.getByRole("button", { name: "Add relay" }).click();
  await page.getByLabel("HTTPS URL").fill("https://relay.example/dns-query");
  await page.getByRole("button", { name: "Generate key" }).click();
  await expect(page.locator("[data-toast]")).toContainText("stored in Keychain");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Reveal relay key" }).click();
  await expect(page.getByText("fixture-secret-1", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Test relay" }).click();
  await expect(page.getByText(/Relay responded with 200 OK/)).toBeVisible();
  await page.getByRole("button", { name: "Mask relay key" }).click();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator("[data-toast]")).toContainText("applied");
  await page.getByRole("button", { name: "Replace key" }).click();
  await page.getByRole("button", { name: "Revert changes" }).click();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Reveal relay key" }).click();
  await expect(page.getByText("fixture-secret-1", { exact: true })).toBeVisible();
});

test("apply locks secret replacement before delayed validation", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=validation-delay`);
  await page.locator("[data-target='relay']").click();
  await page.getByRole("button", { name: "Add relay" }).click();
  await page.getByLabel("HTTPS URL").fill("https://relay.example/dns-query");
  await page.getByRole("button", { name: "Generate key" }).click();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator("[data-dirty-bar]")).toBeHidden();

  await page.getByLabel("HTTPS URL").fill("https://relay-two.example/dns-query");
  await page.keyboard.press("Tab");
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator("[data-view-host]")).toHaveAttribute("inert", "");
  await page.getByRole("button", { name: "Replace key" }).click({ force: true });
  await expect(page.locator("[data-dirty-bar]")).toBeHidden();

  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Reveal relay key" }).click();
  await expect(page.getByText("fixture-secret-1", { exact: true })).toBeVisible();
});

test("replacing one shared generated reference keeps the other live", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='relay']").click();
  await page.getByRole("button", { name: "Add relay" }).click();
  await page.getByRole("button", { name: "Generate key" }).click();
  await page.locator("[data-target='settings']").click();
  await page.getByLabel("Raw TOML").fill("fixture_duplicate_generated_secret = true");
  await page.getByRole("button", { name: "Validate and use" }).click();
  await page.locator("[data-target='relay']").click();
  await page.getByRole("button", { name: "Replace key" }).first().click();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Reveal relay key" }).nth(1).click();
  await expect(page.getByText("fixture-secret-1", { exact: true })).toBeVisible();
});

test("first history edit creates a complete optional retention object", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='settings']").click();
  await page.getByLabel("History matched domains").fill("*.example");
  await page.locator("[data-target='dashboard']").click();
  await page.locator("[data-target='settings']").click();
  await expect(page.getByLabel("History line retention")).toHaveValue("1000");
  await page.getByRole("button", { name: "Revert changes" }).click();
  await page.getByLabel("History line retention").fill("250");
  await page.locator("[data-target='dashboard']").click();
  await page.locator("[data-target='settings']").click();
  await expect(page.getByLabel("History matched domains")).toHaveValue("");
  await expect(page.getByLabel("History line retention")).toHaveValue("250");
});

test("activity sources filter, pause, export, and clear independently", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='activity']").click();
  await expect(page.getByText(/resolver ready/)).toBeVisible();
  await page.getByLabel("Filter activity").fill("tracker");
  await expect(page.getByText(/blocked tracker/)).toBeVisible();
  await expect(page.getByText(/resolver ready/)).toHaveCount(0);
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByRole("button", { name: "Resume" })).toBeVisible();
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export" }).click();
  await expect(await download).toBeTruthy();
  await page.getByRole("button", { name: "Clear view" }).click();
  await expect(page.getByText("No matching logs")).toBeVisible();
});

test("raw TOML validation and secret-free export remain explicit", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='settings']").click();
  await page.getByRole("button", { name: "Load draft" }).click();
  await expect(page.getByLabel("Raw TOML")).toContainText("dns_target");
  await page.getByLabel("Raw TOML").fill("invalid = true");
  await page.getByRole("button", { name: "Validate and use" }).click();
  await expect(page.getByRole("alert")).toContainText("invalid config");
  await page.getByLabel("Raw TOML").fill("secure_only = false");
  await page.getByRole("button", { name: "Validate and use" }).click();
  await expect(page.locator("[data-toast]")).toContainText("valid");
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export safely" }).click();
  await expect(await download).toBeTruthy();
});

for (const width of [420, 1024]) {
  test(`production navigation does not overlap at ${width}px`, async ({ page }) => {
    await openApp(page, width);
    const navigation = await page.getByRole("navigation", { name: "Primary" }).boundingBox();
    expect(navigation).not.toBeNull();
    expect(navigation.y + navigation.height).toBeLessThanOrEqual(720);
    const overlaps = await page.locator("[data-no-overlap]").evaluateAll((nodes) =>
      nodes.flatMap((node, index) => {
        const a = node.getBoundingClientRect();
        return nodes.slice(index + 1).filter((other) => {
          const b = other.getBoundingClientRect();
          return a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
        });
      }),
    );
    expect(overlaps).toEqual([]);
  });

  test(`settings actions fit their cards at ${width}px`, async ({ page }) => {
    await openApp(page, width);
    await page.locator("[data-target='settings']").click();
    const overflow = await page.locator(".card input, .card select, .card textarea, .card button, .card label.button").evaluateAll((controls) =>
      controls.filter((control) => {
        const box = control.getBoundingClientRect();
        const card = control.closest(".card")?.getBoundingClientRect();
        return card && (box.left < card.left || box.right > card.right || box.width > card.width);
      }).map((control) => control.outerHTML),
    );
    expect(overflow).toEqual([]);
    await expect(page.locator("[data-config-import]").locator("..")).toHaveCSS("flex-grow", "0");
  });
}
