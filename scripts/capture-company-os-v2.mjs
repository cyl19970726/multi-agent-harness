#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixturePath = join(repoRoot, "docs/design/company-os-v1/fixtures/company-os-trademark-v1.json");
const cases = [
  { id: "home--morning-operating-review--desktop", page: "home", route: "/?surface=home" },
  { id: "docs--company-knowledge-workspace--desktop", page: "docs-workspace", route: "/?surface=docs" },
  { id: "organization--lead-first-company--desktop", page: "agents-organization", route: "/?surface=organization" },
  { id: "lead-agent--coordinating-direct-reports--desktop", page: "standing-agent-focus", route: "/?surface=organization&agent=actor-agent-ip-lead" },
  { id: "business-module--trademark-operations--desktop", page: "business-module-focus", route: "/?surface=docs&module=module-trademark-management" },
  { id: "work--milestones-and-workitems--desktop", page: "workboard", route: "/?surface=work" },
];

function argument(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

async function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createNetServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      server.close((error) => error ? reject(error) : resolvePort(port));
    });
  });
}

async function waitFor(url) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {}
    await new Promise((resolveWait) => setTimeout(resolveWait, 200));
  }
  throw new Error(`Vite did not become ready at ${url}`);
}

function hash(buffer) {
  return `sha256:${createHash("sha256").update(buffer).digest("hex")}`;
}

async function main() {
  const runId = argument("--run-id", `v2-${Date.now()}`);
  if (!/^[A-Za-z0-9._-]+$/.test(runId)) throw new Error("unsafe run id");
  const outputRoot = resolve(argument("--output", join(repoRoot, ".visual-evidence/company-os-v2", runId)));
  const actualRoot = join(outputRoot, "actual");
  await mkdir(actualRoot, { recursive: true });
  const fixtureText = await readFile(fixturePath, "utf8");
  const fixture = JSON.parse(fixtureText);
  const port = await freePort();
  const vite = spawn(process.execPath, [join(repoRoot, "node_modules/vite/bin/vite.js"), "--config", "apps/agent-dashboard/vite.config.ts", "--host", "127.0.0.1", "--port", String(port)], { cwd: repoRoot, stdio: "ignore" });
  const base = `http://127.0.0.1:${port}`;
  let browser;
  try {
    await waitFor(base);
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({ viewport: { width: 1536, height: 1024 }, deviceScaleFactor: 1 });
    await context.addInitScript((value) => { window.__COMPANY_OS_FIXTURE__ = value; }, fixture);
    const page = await context.newPage();
    const results = [];
    for (const item of cases) {
      const url = `${base}${item.route}${item.route.includes("?") ? "&" : "?"}api=http%3A%2F%2F127.0.0.1%3A9`;
      await page.goto(url, { waitUntil: "networkidle" });
      const root = page.locator(`[data-company-os-page="${item.page}"][data-company-os-ready="true"]`).first();
      await root.waitFor({ state: "visible", timeout: 15_000 });
      const path = join(actualRoot, `${item.id}.png`);
      await page.screenshot({ path, fullPage: false });
      const bytes = await readFile(path);
      results.push({ ...item, viewport: "desktop-1536x1024", file: relative(repoRoot, path), sha256: hash(bytes), status: "captured" });
    }
    await context.close();
    const manifest = {
      contract: "company-os-v2-implementation-capture-v1",
      run_id: runId,
      status: "passed",
      captured_at: new Date().toISOString(),
      data_mode: "deterministic-fixture",
      fixture: relative(repoRoot, fixturePath),
      fixture_sha256: hash(Buffer.from(fixtureText)),
      truth: "Browser-rendered implementation evidence. This is not Store-live product evidence.",
      results,
    };
    await writeFile(join(outputRoot, "capture-run.json"), `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(JSON.stringify(manifest, null, 2));
  } finally {
    await browser?.close();
    vite.kill("SIGTERM");
  }
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
