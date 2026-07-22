#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const contractRoot = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(contractRoot, "../../../..");
const runId = process.argv[2] ?? "iteration-1";
const outputRoot = resolve(repoRoot, ".visual-evidence/company-os-v4-standing-agent-workspace-v1/standing-agent-workspace-v1", runId);
const fixture = JSON.parse(await readFile(resolve(repoRoot, "docs/design/company-os-v1/fixtures/company-os-trademark-v1.json"), "utf8"));

const agent = fixture.actors.find((item) => item.id === "actor-agent-document-architecture");
Object.assign(agent, {
  role: "Document architecture",
  availability: "available",
  responsibility_summary: "Maintains company knowledge structure and routes durable results back into Docs.",
  system_prompt_ref: "document-agent-prompt-docs-governance",
  tool_refs: ["tool-docs-write", "tool-record-query"],
  skill_refs: ["skill-document-governance"],
  maintained_document_refs: ["document-trademark-application-cn-2026-018", "document-brand-a-content-operating-plan"],
  accepted_work_type_refs: ["work-type-document-governance"],
  permission_policy_refs: ["policy-docs-governance"],
  escalation_policy_ref: "policy-governance-escalation",
});
const governanceMembership = fixture.organization.memberships.find((item) => item.actor_id === agent.id);
governanceMembership.membership_role = "member";
const governanceUnit = fixture.organization.org_units.find((item) => item.id === governanceMembership.org_unit_id);
governanceUnit.agent_lead_actor_ref = { actor_type: "agent", actor_id: "actor-agent-ip-lead" };
fixture.work_items.push({
  id: "workitem-organize-trademark-knowledge",
  title: "Organize trademark filing knowledge",
  status: "in_progress",
  source_document_ref: "document-trademark-application-cn-2026-018",
  source_record_refs: [],
  result_document_ref: null,
  requested_by_ref: "actor-agent-ip-lead",
  submitted_by_ref: "actor-agent-document-architecture",
  accountable_owner_ref: "actor-agent-ip-lead",
  assignee_refs: ["actor-agent-document-architecture"],
  contributor_refs: [],
  approval_refs: [],
  evidence_refs: [],
  outcome_summary: null,
  updated_at: "2026-07-20T09:21:00+08:00",
});
fixture.assignments.push({
  id: "assignment-document-architecture",
  work_item_id: "workitem-organize-trademark-knowledge",
  recipient: { actor_type: "agent", actor_id: "actor-agent-document-architecture" },
  sender: { actor_type: "agent", actor_id: "actor-agent-ip-lead" },
  assigned_role: "Knowledge architecture owner",
  scope: "Organize trademark filing guidance and return a durable structure proposal to Docs.",
  delivery_state: "delivered",
  correlation_id: "corr-document-architecture",
  delivery_evidence_ref: "evidence-document-assignment-delivered",
  assigned_at: "2026-07-20T09:02:00+08:00",
});

const port = await new Promise((accept, reject) => {
  const server = createServer();
  server.once("error", reject);
  server.listen(0, "127.0.0.1", () => {
    const address = server.address();
    server.close((error) => error ? reject(error) : accept(address.port));
  });
});
const vite = spawn(process.execPath, [resolve(repoRoot, "node_modules/vite/bin/vite.js"), "--config", "apps/agent-dashboard/vite.config.ts", "--host", "127.0.0.1", "--port", String(port)], { cwd: repoRoot, stdio: "ignore" });
const base = `http://127.0.0.1:${port}`;
for (let attempt = 0; attempt < 100; attempt += 1) {
  try { if ((await fetch(base)).ok) break; } catch {}
  await new Promise((accept) => setTimeout(accept, 100));
}

const browser = await chromium.launch({ headless: true });
const results = [];
try {
  for (const viewport of [
    { name: "desktop-1536x1024", width: 1536, height: 1024 },
    { name: "tablet-900x1180", width: 900, height: 1180 },
    { name: "mobile-390x844", width: 390, height: 844 },
  ]) {
    const context = await browser.newContext({ viewport, deviceScaleFactor: 1, reducedMotion: "reduce", colorScheme: "light", locale: "en-US" });
    await context.route(/https:\/\/fonts\.(?:googleapis|gstatic)\.com\//, (route) => route.abort());
    await context.addInitScript((value) => { window.__COMPANY_OS_FIXTURE__ = value; }, fixture);
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
    const route = `${base}/?surface=organization&agent=actor-agent-document-architecture&api=http://127.0.0.1:9`;
    await page.goto(route, { waitUntil: "commit", timeout: 15_000 });
    const root = page.locator('[data-company-os-page="standing-agent-focus"][data-company-os-ready="true"]');
    await root.waitFor({ state: "visible", timeout: 15_000 }).catch(async (error) => {
      throw new Error(`${error.message}\nurl=${page.url()}\nerrors=${errors.join(" | ")}\nhtml=${(await page.content()).slice(0, 1000)}`);
    });
    await page.evaluate(() => document.fonts.ready);
    await root.locator('[data-standing-agent-workspace]').waitFor({ state: "visible" });
    if (await root.locator('[data-provider-thinking], [data-thinking-persisted]').count()) throw new Error("thinking appeared as durable state");
    const overflow = await page.evaluate(() => ({ client: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
    if (overflow.scroll > overflow.client) throw new Error(`${viewport.name}: horizontal overflow ${JSON.stringify(overflow)}`);

    const title = root.getByRole("button", { name: "Organize trademark filing knowledge", exact: true });
    await title.focus();
    await page.keyboard.press("Enter");
    await page.waitForURL(/surface=work.*workItem=workitem-organize-trademark-knowledge|workItem=workitem-organize-trademark-knowledge.*surface=work/);
    await page.goBack({ waitUntil: "commit" });
    await page.locator('[data-standing-agent-workspace]').waitFor({ state: "visible" });
    const source = page.locator('[data-company-os-ref="document-trademark-application-cn-2026-018"]').filter({ has: page.locator("button") });
    const sourceButton = page.getByRole("button", { name: /Trademark application CN-2026-018/ }).first();
    await sourceButton.click();
    await page.waitForURL(/surface=docs.*document=document-trademark-application-cn-2026-018|document=document-trademark-application-cn-2026-018.*surface=docs/);
    await page.goBack({ waitUntil: "commit" });
    await page.locator('[data-standing-agent-workspace]').waitFor({ state: "visible" });

    if (viewport.width < 1280) {
      const summary = page.getByText("Context & controls", { exact: true });
      await summary.click();
    }
    const screenshot = join(outputRoot, `standing-agent-focus--available--${viewport.name}.png`);
    await mkdir(dirname(screenshot), { recursive: true });
    await page.screenshot({ path: screenshot, fullPage: false });
    const bytes = await readFile(screenshot);
    results.push({
      viewport,
      route,
      screenshot,
      sha256: `sha256:${createHash("sha256").update(bytes).digest("hex")}`,
      journeys: ["content-reachability", "entity-deep-link", "return-context", "keyboard-path", "responsive-path", "motion-policy"],
      console_errors: errors,
    });
    await context.close();
  }
  await writeFile(join(outputRoot, "run-manifest.json"), JSON.stringify({ runId, fixture: "company-os-trademark-v1 + standing-agent-workspace overlay", reducedMotion: "reduce", results }, null, 2));
} finally {
  await browser.close();
  vite.kill("SIGTERM");
}

console.log(JSON.stringify({ runId, outputRoot, captures: results.length }, null, 2));
