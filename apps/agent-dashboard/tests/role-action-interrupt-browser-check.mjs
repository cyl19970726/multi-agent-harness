#!/usr/bin/env node
import assert from "node:assert/strict";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vite = await createServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "silent",
});
await vite.listen();
const address = vite.httpServer.address();
assert.ok(address && typeof address === "object");
const base = `http://127.0.0.1:${address.port}`;
const browser = await chromium.launch({ headless: true });

try {
  const page = await browser.newPage();
  await page.goto(`${base}/tests/role-action-interrupt-browser-fixture.html`, {
    waitUntil: "networkidle",
  });

  await page.getByRole("button", { name: "interrupt member run" }).click();
  await page.getByRole("button", { name: "Execute action" }).click();
  await page.getByRole("status").getByText("reason is required.").waitFor();
  assert.equal(
    await page.evaluate(() => window.__interruptActionCalls.length),
    0,
    "missing reason crossed the RoleAction boundary",
  );

  await page.getByLabel("Interrupt reason").fill("Stop the current provider turn only");
  await page.getByRole("button", { name: "Execute action" }).click();
  await page
    .getByRole("status")
    .getByText("Completed interrupt_member_run. Refetching canonical RoleView.")
    .waitFor();
  const [successCall] = await page.evaluate(() => window.__interruptActionCalls);
  assert.equal(
    successCall.path,
    "/v1/agentfirm/member-runs/member%2Frun%20one/interrupt",
  );
  assert.deepEqual(successCall.body, {
    action: "interrupt_member_run",
    reason: "Stop the current provider turn only",
  });
  assert.equal(successCall.headers["If-Match"], "7");
  assert.match(successCall.headers["Idempotency-Key"], /^[0-9a-f-]{36}$/);
  assert.equal("X-AgentFirm-Confirm" in successCall.headers, false);
  assert.equal(await page.evaluate(() => window.__interruptCompleted), 1);

  await page.evaluate(() => {
    window.__interruptActionResult = {
      ok: false,
      error: {
        status: 409,
        code: "runtime_not_active",
        message: "No provider turn is active.",
        resource_kind: "member_run",
        resource_id: "member/run one",
        current_version: 8,
      },
    };
  });
  await page.getByLabel("Interrupt reason").fill("Second exact interrupt attempt");
  await page.getByRole("button", { name: "Execute action" }).click();
  await page
    .getByRole("status")
    .getByText(
      "runtime_not_active: No provider turn is active. (member_run member/run one v8)",
    )
    .waitFor();
  assert.equal(await page.evaluate(() => window.__interruptActionCalls.length), 2);
  assert.equal(
    await page.evaluate(() => window.__interruptCompleted),
    1,
    "rejected Interrupt incorrectly triggered a canonical refetch completion",
  );
} finally {
  await browser.close();
  await vite.close();
}

console.log("role-action interrupt browser check passed");
