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

test("metrics failure does not erase service health", async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 720 });
  await page.goto(`${production}/?fixture=metrics-error`);
  await expect(page.getByText("Healthy", { exact: true })).toBeVisible();
  await expect(page.getByText(/Metrics unavailable:/)).toBeVisible();
  await expect(page.locator("[data-service-state]")).toHaveText("Stopped");
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

test("production service control reports its final state", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
  await page.getByRole("button", { name: "Stop DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Stopped");
  await expect(page.locator("[data-live-region]")).toContainText("stopped");
});

test("production draft can be reverted or applied", async ({ page }) => {
  await openApp(page);
  await page.locator("[data-target='settings']").click();
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
}
