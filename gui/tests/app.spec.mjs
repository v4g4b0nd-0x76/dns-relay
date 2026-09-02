import { test, expect } from "@playwright/test";

const production = "http://127.0.0.1:1420";

async function openApp(page, width = 420, height = 720) {
  await page.setViewportSize({ width, height });
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

test("first launch explains the setup without dead navigation", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 720 });
  await page.goto(`${production}/?fixture=first-launch`);

  await expect(page.locator("[data-view-nav]")).toHaveCount(1);
  await expect(page.getByText("Confirm locations", { exact: true })).toBeVisible();
  await expect(page.getByText("Choose resolver", { exact: true })).toBeVisible();
  await expect(page.getByText("Install service", { exact: true })).toBeVisible();
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

test("service state suppresses connection-refused endpoint noise", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=connection-refused`);
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
  await expect(page.getByText(/unavailable:/)).toHaveCount(0);
  await expect(page.getByText("Health unavailable", { exact: true })).toHaveCount(0);
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

test("desktop shell uses the available workspace without stretching indefinitely", async ({ page }) => {
  await openApp(page, 1440);
  const shell = await page.locator("[data-app-shell]").boundingBox();

  expect(shell).not.toBeNull();
  expect(shell.width).toBeGreaterThanOrEqual(1180);
  expect(shell.width).toBeLessThanOrEqual(1200);
  expect(Math.abs(shell.x - (1440 - shell.width) / 2)).toBeLessThan(2);
});

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

test("dashboard fits the default desktop height without initial scrolling", async ({ page }) => {
  await openApp(page, 1024);
  const layout = await page.locator("[data-view-host]").evaluate((view) => ({
    clientHeight: view.clientHeight,
    scrollHeight: view.scrollHeight,
  }));

  expect(layout.scrollHeight).toBeLessThanOrEqual(layout.clientHeight + 1);
});

test("boolean config fields render as app switches", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='settings']").click();

  const checkboxCount = await page.locator(".check-row input[type='checkbox']").count();
  await expect(page.locator(".check-row .switch")).toHaveCount(checkboxCount);
});

test("compact dashboard keeps service facts readable in two columns", async ({ page }) => {
  await openApp(page);

  const facts = await page.locator(".detail-list > div").evaluateAll((items) => items.map((item) => ({
    clipped: item.scrollWidth > item.clientWidth,
    width: item.getBoundingClientRect().width,
  })));

  expect(facts).toHaveLength(4);
  expect(facts.every(({ clipped, width }) => !clipped && width >= 85)).toBe(true);
});

test("desktop forms use the compact screenshot spacing", async ({ page }) => {
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

  expect(spacing.padding).toBeGreaterThanOrEqual(14);
  expect(spacing.padding).toBeLessThanOrEqual(18);
  expect(spacing.rowGap).toBeGreaterThanOrEqual(10);
  expect(spacing.rowGap).toBeLessThanOrEqual(14);
  expect(spacing.columnGap).toBeGreaterThanOrEqual(10);
  expect(spacing.columnGap).toBeLessThanOrEqual(14);
});

test("compact dashboard keeps flat cards and the horizontal service summary", async ({ page }) => {
  await openApp(page);

  const hero = await page.locator(".hero").evaluate((element) => ({
    backgroundImage: getComputedStyle(element).backgroundImage,
    columns: getComputedStyle(element).gridTemplateColumns.split(" ").length,
    height: element.getBoundingClientRect().height,
  }));

  expect(hero.backgroundImage).toBe("none");
  expect(hero.columns).toBe(2);
  expect(hero.height).toBeLessThan(280);
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

test("compact resolver controls leave the address readable", async ({ page }) => {
  await openApp(page, 420, 690);
  await page.locator("[data-target='resolvers']").click();

  const layout = await page.locator(".resolver-row").first().evaluate((row) => {
    const main = row.querySelector(".row-main").getBoundingClientRect();
    const actions = row.querySelector(".row-actions").getBoundingClientRect();
    return { mainWidth: main.width, actionsTop: actions.top, mainBottom: main.bottom };
  });

  expect(layout.mainWidth).toBeGreaterThan(220);
  expect(layout.actionsTop).toBeGreaterThanOrEqual(layout.mainBottom);
});

test("compact empty cards do not double their padding", async ({ page }) => {
  await openApp(page, 420, 690);
  await page.locator("[data-target='rules']").click();

  await expect(page.locator(".empty-card")).toHaveCSS("padding", "0px");
});

test("compact relay action stays above the bottom navigation", async ({ page }) => {
  await openApp(page, 420, 690);
  await page.locator("[data-target='relay']").click();

  const action = await page.getByRole("button", { name: "Add relay" }).boundingBox();
  const navigation = await page.getByRole("navigation", { name: "Primary" }).boundingBox();
  expect(action).not.toBeNull();
  expect(navigation).not.toBeNull();
  expect(action.y + action.height).toBeLessThanOrEqual(navigation.y - 4);
});

test("compact relay visible content stays above the bottom navigation", async ({ page }) => {
  await openApp(page, 420, 720);
  await page.locator("[data-target='relay']").click();

  const layout = await page.evaluate(() => {
    const nav = document.querySelector("nav").getBoundingClientRect();
    const last = document.querySelector("[data-view='relay'] > :last-child").getBoundingClientRect();
    return { navTop: nav.top, contentBottom: last.bottom };
  });

  expect(layout.contentBottom).toBeLessThanOrEqual(layout.navTop - 4);
});

test("compact pending changes stay clear of scrolled settings", async ({ page }) => {
  await openApp(page, 420, 690);
  await page.locator("[data-target='settings']").click();
  await page.getByLabel("Listener address").fill("127.0.0.1:5300");
  await page.keyboard.press("Tab");
  await expect(page.locator("[data-dirty-bar]")).toBeVisible();
  await page.locator("[data-view-host]").evaluate((view) => { view.scrollTop = view.scrollHeight; });

  const gap = await page.evaluate(() => {
    const lastCard = document.querySelector("[data-view='settings'] .card:last-of-type").getBoundingClientRect();
    const dirtyBar = document.querySelector("[data-dirty-bar]").getBoundingClientRect();
    return dirtyBar.top - lastCard.bottom;
  });

  expect(gap).toBeGreaterThanOrEqual(12);
});

test("dashboard activity preview stays compact", async ({ page }) => {
  await openApp(page, 1440);

  const preview = await page.locator("[data-dashboard-activity]").boundingBox();

  expect(preview).not.toBeNull();
  expect(preview.height).toBeLessThan(180);
});

test("dashboard activity preview shows recent activity", async ({ page }) => {
  await openApp(page, 1440);

  await expect(page.locator("[data-dashboard-activity]")).toContainText("blocked tracker.example");
  await expect(page.locator("[data-dashboard-activity]")).not.toContainText("No recent events");
});

test("dashboard metric row stays separated from activity preview", async ({ page }) => {
  await openApp(page, 1440);

  const gap = await page.evaluate(() => {
    const metrics = document.querySelector(".metrics").getBoundingClientRect();
    const preview = document.querySelector("[data-dashboard-activity]").getBoundingClientRect();
    return preview.top - metrics.bottom;
  });

  expect(gap).toBeGreaterThanOrEqual(12);
});

test("settings listener field stays aligned in the field column", async ({ page }) => {
  await openApp(page, 1440);
  await page.locator("[data-target='settings']").click();

  const listener = await page.getByLabel("Listener address").boundingBox();
  const interval = await page.getByLabel("Hot reload interval (ms)").boundingBox();

  expect(listener).not.toBeNull();
  expect(interval).not.toBeNull();
  expect(Math.abs(listener.x - interval.x)).toBeLessThan(1);
  expect(Math.abs(listener.width - interval.width)).toBeLessThan(1);
});

test("resolver discovery keeps switches and text fields aligned", async ({ page }) => {
  await openApp(page, 1440);
  await page.locator("[data-target='resolvers']").click();

  await expect(page.locator("[data-resolver-options] .option-column .check-row")).toHaveCount(3);
  await expect(page.locator("[data-resolver-options] .field-column input, [data-resolver-options] .field-column textarea")).toHaveCount(3);
});

test("settings groups keep switches and text fields aligned", async ({ page }) => {
  await openApp(page, 1440);
  await page.locator("[data-target='settings']").click();

  await expect(page.locator("[data-settings-core] .option-column .check-row")).toHaveCount(3);
  await expect(page.locator("[data-settings-core] .field-column input")).toHaveCount(2);
  await expect(page.locator("[data-settings-metrics] .option-column .check-row")).toHaveCount(2);
  await expect(page.locator("[data-settings-metrics] .field-column input, [data-settings-metrics] .field-column select, [data-settings-metrics] .field-column textarea")).toHaveCount(4);
});

test("settings lower utility cards use compact desktop spacing", async ({ page }) => {
  await openApp(page, 1440, 900);
  await page.locator("[data-target='settings']").click();

  await expect(page.locator("[data-settings-obfs] .key-tools")).toBeVisible();
  const layout = await page.evaluate(() => {
    const obfs = document.querySelector("[data-settings-obfs]").getBoundingClientRect();
    const service = document.querySelector("[data-settings-service]").getBoundingClientRect();
    const shared = document.querySelector("[data-settings-shared]").getBoundingClientRect();
    const advanced = document.querySelector("[data-settings-advanced]").getBoundingClientRect();
    const keyLabel = document.querySelector("[data-settings-obfs] .key-tools .muted").getBoundingClientRect();
    const keyButton = document.querySelector("[data-action='generate-obfs-secret']").getBoundingClientRect();
    return {
      advancedGap: advanced.top - Math.max(service.bottom, shared.bottom),
      obfsGap: service.top - obfs.bottom,
      sameRow: Math.abs(service.top - shared.top),
      keyAligned: Math.abs(keyLabel.top - keyButton.top),
      serviceHeight: service.height,
      sharedHeight: shared.height,
    };
  });

  expect(layout.obfsGap).toBeGreaterThanOrEqual(10);
  expect(layout.obfsGap).toBeLessThanOrEqual(14);
  expect(layout.sameRow).toBeLessThan(2);
  expect(layout.advancedGap).toBeGreaterThanOrEqual(10);
  expect(layout.advancedGap).toBeLessThanOrEqual(14);
  expect(layout.keyAligned).toBeLessThan(8);
  expect(layout.serviceHeight).toBeLessThan(150);
  expect(layout.sharedHeight).toBeLessThan(150);
});

test("settings metrics history fields avoid a blank left column", async ({ page }) => {
  await openApp(page, 1440, 900);
  await page.locator("[data-target='settings']").click();

  await expect(page.locator("[data-settings-metrics] .history-fields")).toBeVisible();
  const layout = await page.evaluate(() => {
    const card = document.querySelector("[data-settings-metrics]").getBoundingClientRect();
    const history = document.querySelector("[data-settings-metrics] .history-fields").getBoundingClientRect();
    const options = document.querySelector("[data-settings-metrics] .option-column").getBoundingClientRect();
    const fields = document.querySelector("[data-settings-metrics] .metrics-fields").getBoundingClientRect();
    return {
      startsAtCardLeft: Math.abs(history.left - (card.left + 16)),
      spansMostOfCard: history.width / (card.width - 32),
      firstRowAligned: Math.abs(options.top - fields.top),
      firstRowHeightGap: Math.abs(options.height - fields.height),
    };
  });

  expect(layout.startsAtCardLeft).toBeLessThan(2);
  expect(layout.spansMostOfCard).toBeGreaterThan(.95);
  expect(layout.firstRowAligned).toBeLessThan(2);
  expect(layout.firstRowHeightGap).toBeLessThan(24);
});

test("settings action buttons do not crowd their fields", async ({ page }) => {
  await openApp(page, 1440, 900);
  await page.locator("[data-target='settings']").click();

  const layout = await page.evaluate(() => {
    const serviceTitle = document.querySelector("[data-settings-service] h2").getBoundingClientRect();
    const serviceButtons = document.querySelector("[data-settings-service] .service-actions").getBoundingClientRect();
    const firstServiceButton = document.querySelector("[data-settings-service] .service-actions .button").getBoundingClientRect();
    const raw = document.querySelector("[data-settings-advanced] textarea").getBoundingClientRect();
    const rawButtons = document.querySelector("[data-settings-advanced] .config-actions").getBoundingClientRect();
    return {
      serviceIndent: firstServiceButton.left - serviceTitle.left,
      serviceGap: serviceButtons.top - serviceTitle.bottom,
      rawGap: rawButtons.top - raw.bottom,
    };
  });

  expect(Math.abs(layout.serviceIndent)).toBeLessThan(4);
  expect(layout.serviceGap).toBeGreaterThanOrEqual(8);
  expect(layout.rawGap).toBeGreaterThanOrEqual(10);
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

test("unchanged draft does not keep pending changes", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='resolvers']").click();
  await page.getByLabel("Secure resolvers only").uncheck();
  await expect(page.locator("[data-dirty-bar]")).toBeVisible();
  await page.getByLabel("Secure resolvers only").check();
  await expect(page.locator("[data-dirty-bar]")).toBeHidden();
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

test("existing relay keys can be stored in the vault", async ({ page }) => {
  const key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
  await openApp(page);
  await page.locator("[data-target='relay']").click();
  await page.getByRole("button", { name: "Add relay" }).click();
  await page.getByRole("button", { name: "Set key" }).click();
  await page.getByRole("dialog", { name: "Set relay key" }).getByRole("textbox", { name: "Relay key" }).fill(key);
  await page.getByRole("button", { name: "Store key" }).click();
  await expect(page.locator("[data-toast]")).toContainText("stored in Keychain");

  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Reveal relay key" }).click();
  await expect(page.getByText(key, { exact: true })).toBeVisible();
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
