#!/usr/bin/env node
import assert from "node:assert/strict";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vite = await createServer({ configFile: join(dashboardRoot, "vite.config.ts"), server: { host: "127.0.0.1", port: 0 }, logLevel: "silent" });
await vite.listen();
const address = vite.httpServer.address();
assert.ok(address && typeof address === "object");
const base = `http://127.0.0.1:${address.port}`;
const browser = await chromium.launch({ headless: true });

try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  await page.goto(`${base}/tests/agent-dock-browser-fixture.html`, { waitUntil: "networkidle" });
  await page.getByRole("button", { name: "Open Work" }).click();
  const dock = page.getByRole("complementary", { name: "Work and Messages dock" });
  await dock.waitFor();
  assert.equal(await page.getByTestId("session-canvas").count(), 1, "opening dock unmounted Session");
  await page.getByText("Map the Work and Message planes", { exact: true }).click();
  assert.equal(await page.evaluate(() => document.body.dataset.selectedWork), "work-open");
  await dock.getByRole("tab", { name: /Messages/ }).click();
  await dock.locator('[data-testid="messages-dock-list"] button').filter({ hasText: "Please connect the final review" }).click();
  assert.equal(await page.evaluate(() => document.body.dataset.selectedMessage), "message-in");
  await dock.getByRole("tab", { name: /Work/ }).click();
  assert.equal(await page.getByRole("button", { name: /Map the Work and Message planes/ }).getAttribute("aria-current"), "true", "Work selection was lost across module switch");
  const workListViewport = dock.locator('[data-testid="work-dock-list"] [data-radix-scroll-area-viewport]');
  await workListViewport.evaluate((element) => { element.scrollTop = 31; });
  await dock.getByRole("tab", { name: /Messages/ }).click();
  await dock.getByRole("button", { name: "unread", exact: true }).click();
  assert.equal(await page.getByText("The review is ready for Host judgment.", { exact: true }).count(), 0, "own Outbox was counted as unread");
  assert.equal(await page.getByText("Please connect the final review to the Work acceptance contract.", { exact: true }).count(), 2, "incoming unread Message was not retained in list and detail");
  await dock.getByRole("tab", { name: /Work/ }).click();
  assert.equal(await workListViewport.evaluate((element) => element.scrollTop), 31, "Work scroll was lost across module switch");
  const resize = page.getByRole("separator", { name: "Resize Work and Messages dock" });
  await resize.focus();
  const before = Number(await resize.getAttribute("aria-valuenow"));
  await page.keyboard.press("ArrowLeft");
  assert.equal(Number(await resize.getAttribute("aria-valuenow")), before + 24, "keyboard resize did not change width");
  await dock.getByRole("button", { name: "Expand dock" }).click();
  assert.ok(Number(await resize.getAttribute("aria-valuenow")) >= 520, "expanded dock did not reach readable detail width");
  await dock.getByRole("button", { name: "Close Work and Messages dock" }).click();
  assert.equal(await dock.count(), 0);
  assert.equal(await page.evaluate(() => document.activeElement?.textContent), "Open Work", "closing dock did not restore opener focus");
  await page.reload({ waitUntil: "networkidle" });
  assert.equal(await dock.count(), 0, "closed local preference was not restored");
  await page.getByRole("button", { name: "Open Work" }).click();
  assert.ok(Number(await page.getByRole("separator", { name: "Resize Work and Messages dock" }).getAttribute("aria-valuenow")) >= 520, "width local preference was not restored");
  await page.getByRole("button", { name: "Close Work and Messages dock" }).click();
  await page.getByRole("button", { name: "Simulate Messages error" }).click();
  await page.getByRole("alert").getByText("Canonical Message projection is temporarily unavailable.").waitFor();
  assert.equal(await page.getByTestId("session-canvas").count(), 1, "Messages failure replaced the Session canvas");

  const mobile = await browser.newPage({ viewport: { width: 390, height: 844 } });
  await mobile.goto(`${base}/tests/agent-dock-browser-fixture.html`, { waitUntil: "networkidle" });
  await mobile.getByRole("button", { name: "Open Messages" }).click();
  const mobileDock = mobile.getByRole("complementary", { name: "Work and Messages dock" });
  await mobileDock.waitFor();
  assert.equal(await mobileDock.evaluate((element) => getComputedStyle(element).position), "fixed", "mobile dock is not an overlay sheet");
  assert.equal(await mobileDock.boundingBox().then((box) => Math.round(box?.width ?? 0)), 390, "mobile dock is not full width");
  await mobile.keyboard.press("Escape");
  assert.equal(await mobileDock.count(), 0, "Escape did not close overlay dock");
  assert.equal(await mobile.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), true, "mobile dock caused horizontal overflow");
  await mobile.close();
} finally {
  await browser.close();
  await vite.close();
}

console.log("agent dock browser check passed");
