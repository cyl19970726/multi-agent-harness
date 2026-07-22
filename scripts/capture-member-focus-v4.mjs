#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function argument(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

const base = argument("--base", "http://127.0.0.1:5173");
const outputRoot = resolve(argument("--output", join(repoRoot, ".visual-evidence/execution-workbench-v4/member-focus")));
const project = argument("--project", "multi-agent-harness");
const memberRun = argument("--member-run", "member-run-1784706739191-p29869-85");
const team = argument("--team", "team-run-1784706739191-p29869-83");
const mission = argument("--mission", "mission-1784705173148-p29869-0");
const wave = argument("--wave", "wave-1784705190397-p29869-1");
const viewport = { width: 1536, height: 1024 };

const search = new URLSearchParams({
  project,
  api: base,
  surface: "team",
  memberRun,
  team,
  mission,
  wave,
});
const url = `${base}/?${search}`;

const browser = await chromium.launch();
const context = await browser.newContext({
  viewport,
  deviceScaleFactor: 1,
  reducedMotion: "reduce",
  colorScheme: "light",
  locale: "en-US",
});
const page = await context.newPage();
const errors = [];
page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
page.on("pageerror", (error) => errors.push(error.message));
await page.route(/https:\/\/fonts\.googleapis\.com\//, (route) => route.fulfill({
  status: 200,
  contentType: "text/css",
  body: "/* deterministic capture uses the system font fallback */",
}));

try {
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await page.locator("h1").filter({ hasText: "WorkspaceFixer" }).waitFor({ state: "visible", timeout: 15_000 });
  await page.locator('[data-native-activity-state="ready"]').waitFor({ state: "visible", timeout: 15_000 });
  await page.evaluate(() => Promise.race([
    document.fonts.ready,
    new Promise((resolveWait) => setTimeout(resolveWait, 2_000)),
  ]));
  await page.waitForTimeout(500);

  const dimensions = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
  }));
  if (dimensions.scrollWidth > dimensions.clientWidth) {
    throw new Error(`horizontal overflow: ${JSON.stringify(dimensions)}`);
  }
  if (errors.length) throw new Error(`console errors: ${errors.join(" | ")}`);

  await mkdir(outputRoot, { recursive: true });
  const screenshot = join(outputRoot, "member-run-focus--completed-history--desktop-1536x1024.png");
  await page.screenshot({ path: screenshot });
  const journeys = [];

  const scrollOwner = page.locator('[data-member-history-scroll-owner="true"]');
  await scrollOwner.focus();
  await scrollOwner.press("End");
  await page.waitForTimeout(100);
  const scrollState = await scrollOwner.evaluate((element) => ({
    top: element.scrollTop,
    maximum: element.scrollHeight - element.clientHeight,
  }));
  if (scrollState.maximum > 0 && scrollState.top < scrollState.maximum * 0.8) {
    throw new Error(`member history End key did not reach the final region: ${JSON.stringify(scrollState)}`);
  }
  journeys.push("member-history-content-reachability");

  await page.getByRole("button", { name: "Focus", exact: true }).click();
  await page.getByRole("button", { name: "Return to complete", exact: true }).waitFor();
  await page.getByRole("button", { name: "Return to complete", exact: true }).click();
  await page.getByText("Complete history", { exact: false }).waitFor();
  journeys.push("member-history-focus-toggle");

  const toolDisclosure = page.locator("summary").filter({ hasText: /spawn_agent|send_message|wait/ }).first();
  await toolDisclosure.focus();
  await toolDisclosure.press("Enter");
  if (!await toolDisclosure.evaluate((summary) => summary.parentElement?.hasAttribute("open") ?? false)) {
    throw new Error("tool disclosure did not open from the keyboard");
  }
  await toolDisclosure.press("Enter");
  journeys.push("member-history-tool-disclosure");

  await page.getByRole("button", { name: "Back to team", exact: true }).click();
  await page.waitForURL((current) => !current.searchParams.has("memberRun"));
  await page.goBack({ waitUntil: "domcontentloaded" });
  await page.locator("h1").filter({ hasText: "WorkspaceFixer" }).waitFor();
  journeys.push("member-history-return-context");

  const manifest = {
    page: "member-run-focus",
    state: "completed-history",
    source: "named live store snapshot plus provider-native session",
    project,
    member_run_id: memberRun,
    team_run_id: team,
    mission_id: mission,
    wave_id: wave,
    url,
    viewport: { ...viewport, device_scale_factor: 1 },
    browser: browser.version(),
    reduced_motion: "reduce",
    git_revision: execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim(),
    git_dirty: Boolean(execFileSync("git", ["status", "--porcelain"], { cwd: repoRoot, encoding: "utf8" }).trim()),
    screenshot,
    journeys,
    console_errors: errors,
  };
  await writeFile(join(outputRoot, "capture-run.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(JSON.stringify(manifest, null, 2));
} finally {
  await context.close();
  await browser.close();
}
