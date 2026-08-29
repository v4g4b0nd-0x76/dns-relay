import { test, expect } from "@playwright/test";
import { pathToFileURL } from "node:url";
import path from "node:path";

const prototype = pathToFileURL(path.resolve("prototype/index.html")).href;
const views = ["dashboard", "resolvers", "rules", "relay", "activity", "settings"];

async function openPrototype(page, width = 420) {
  await page.setViewportSize({ width, height: 720 });
  await page.goto(prototype);
}

test("prototype exposes the six operational views", async ({ page }) => {
  await openPrototype(page);
  await expect(page.locator("[data-app-shell]")).toBeVisible();
  await expect(page.locator("[data-view-nav]")).toHaveCount(6);
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Primary" })).toBeVisible();
});

test("power control moves between running and stopped", async ({ page }) => {
  await openPrototype(page);
  await page.getByRole("button", { name: "Stop DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Stopped");
  await page.getByRole("button", { name: "Start DNS Relay" }).click();
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
});

test("resolver testing updates health and latency", async ({ page }) => {
  await openPrototype(page);
  await page.locator("[data-target='resolvers']").click();
  await page.locator("[data-action='test-resolver']").first().click();
  await expect(page.locator("[data-resolver-result]").first()).toContainText("Healthy");
  await expect(page.locator("[data-resolver-result]").first()).toContainText("ms");
});

test("rules can be added, searched, toggled, edited, and deleted", async ({ page }) => {
  await openPrototype(page);
  await page.locator("[data-target='rules']").click();
  await page.getByRole("button", { name: "Add rule" }).click();
  await page.getByLabel("Domain pattern").fill("ads.example");
  await page.getByLabel("Rule target").fill("drop");
  await page.getByRole("button", { name: "Save rule" }).click();
  const row = page.locator("[data-rule-row]", { hasText: "ads.example" });
  await expect(row).toBeVisible();
  await row.getByRole("button", { name: "Disable ads.example" }).click();
  await expect(row).toHaveAttribute("data-enabled", "false");
  await row.getByRole("button", { name: "Edit ads.example" }).click();
  await page.getByLabel("Domain pattern").fill("tracker.example");
  await page.getByRole("button", { name: "Save rule" }).click();
  await page.getByRole("searchbox", { name: "Search rules" }).fill("tracker");
  const edited = page.locator("[data-rule-row]", { hasText: "tracker.example" });
  await expect(edited).toBeVisible();
  await edited.getByRole("button", { name: "Delete tracker.example" }).click();
  await expect(edited).toHaveCount(0);
});

test("dirty changes can be reverted or applied", async ({ page }) => {
  await openPrototype(page);
  await page.locator("[data-target='settings']").click();
  await page.getByLabel("Secure resolvers only").uncheck();
  await expect(page.locator("[data-dirty-bar]")).toBeVisible();
  await page.getByRole("button", { name: "Revert changes" }).click();
  await expect(page.getByLabel("Secure resolvers only")).toBeChecked();
  await page.getByLabel("Secure resolvers only").uncheck();
  await page.getByRole("button", { name: "Apply changes" }).click();
  await expect(page.locator("[data-dirty-bar]")).toBeHidden();
  await expect(page.locator("[data-toast]")).toContainText("applied");
});

test("relay secret stays masked until explicitly revealed", async ({ page }) => {
  await openPrototype(page);
  await page.locator("[data-target='relay']").click();
  await expect(page.locator("[data-relay-secret]")).toHaveText("••••••••••••");
  await page.getByRole("button", { name: "Reveal relay key" }).click();
  await expect(page.locator("[data-relay-secret]")).toContainText("rk_demo_");
  await page.getByRole("button", { name: "Test relay" }).click();
  await expect(page.locator("[data-relay-result]")).toContainText("Reachable");
});

test("setup can adopt the installed-state flow", async ({ page }) => {
  await openPrototype(page);
  await page.locator("[data-target='settings']").click();
  await page.getByRole("button", { name: "Restart setup" }).click();
  await expect(page.locator("[data-view='setup']")).toBeVisible();
  await page.getByRole("button", { name: "Install and start" }).click();
  await expect(page.locator("[data-view='dashboard']")).toBeVisible();
  await expect(page.locator("[data-service-state]")).toHaveText("Running");
});

test("keyboard focus, reduced motion, and icon names remain accessible", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openPrototype(page);
  await page.keyboard.press("Tab");
  await expect(page.locator(":focus")).toBeVisible();
  const duration = await page.locator("[data-power]").evaluate((node) =>
    getComputedStyle(node).transitionDuration,
  );
  expect(duration).toBe("0.001s");
  const unnamed = await page.locator("[data-icon-only]").evaluateAll((nodes) =>
    nodes.filter((node) => !node.getAttribute("aria-label") || !node.getAttribute("title")).length,
  );
  expect(unnamed).toBe(0);
});

for (const width of [420, 1024]) {
  test(`primary states render without overlap at ${width}px`, async ({ page }) => {
    await openPrototype(page, width);
    for (const view of views) {
      await page.locator(`[data-target='${view}']`).click();
      await expect(page.locator(`[data-view='${view}']`)).toBeVisible();
      const overlaps = await page.locator("[data-no-overlap]").evaluateAll((nodes) =>
        nodes.flatMap((node, index) => {
          const a = node.getBoundingClientRect();
          return nodes.slice(index + 1).filter((other) => {
            const b = other.getBoundingClientRect();
            return a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
          }).map((other) => [node.dataset.noOverlap, other.dataset.noOverlap]);
        }),
      );
      expect(overlaps).toEqual([]);
      await page.screenshot({
        path: `tests/screenshots/prototype-${view}-${width}.png`,
        fullPage: true,
        animations: "disabled",
      });
    }
    await page.locator("[data-target='settings']").click();
    await page.getByRole("button", { name: "Restart setup" }).click();
    await page.screenshot({
      path: `tests/screenshots/prototype-setup-${width}.png`,
      fullPage: true,
      animations: "disabled",
    });
  });
}
