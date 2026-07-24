#!/usr/bin/env node

import { execFileSync, spawn } from "node:child_process";
import { mkdtemp, mkdir, readFile, writeFile, copyFile, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = join(repoRoot, "apps/agent-dashboard/fixtures/workbench-layout-v2-native-v1");
const defaultOutput = join(repoRoot, ".visual-evidence/workbench-layout-v2/implemented");

function argument(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

async function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      server.close((error) => error ? reject(error) : resolvePort(port));
    });
  });
}

async function waitFor(url, label, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
      lastError = new Error(`${label} returned HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 200));
  }
  throw new Error(`${label} did not become ready: ${lastError?.message ?? "timeout"}`);
}

function start(command, args, name, env = {}) {
  const child = spawn(command, args, {
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, FORCE_COLOR: "0", ...env },
  });
  let output = "";
  for (const stream of [child.stdout, child.stderr]) {
    stream.on("data", (chunk) => {
      output += chunk.toString();
      if (output.length > 20_000) output = output.slice(-20_000);
    });
  }
  child.done = new Promise((resolveDone) => child.once("exit", (code, signal) => resolveDone({ code, signal })));
  child.describeFailure = () => `${name} output:\n${output}`;
  return child;
}

async function stop(child) {
  if (!child || child.exitCode !== null) return;
  child.kill("SIGTERM");
  const exited = await Promise.race([
    child.done.then(() => true),
    new Promise((resolveWait) => setTimeout(() => resolveWait(false), 2_000)),
  ]);
  if (!exited && child.exitCode === null) child.kill("SIGKILL");
}

async function materialize(storeRoot, manifest) {
  await mkdir(storeRoot, { recursive: true });
  for (const ledger of manifest.ledgers) {
    if (basename(ledger) !== ledger || !ledger.endsWith(".jsonl")) {
      throw new Error(`unsafe fixture ledger: ${ledger}`);
    }
    await copyFile(join(fixtureRoot, ledger), join(storeRoot, ledger));
  }
  await copyFile(join(fixtureRoot, "fixture-manifest.json"), join(storeRoot, "fixture-manifest.json"));
}

async function publishLivePreview(apiBase, manifest) {
  const response = await fetch(`${apiBase}/v1/live/member-activity?project=_store`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      team_run_id: manifest.team_run_id,
      member_run_id: manifest.member_run_id,
      preview: "Reviewing Mission/Wave exit criteria and responsive evidence. Live preview only; not saved.",
    }),
  });
  if (!response.ok) throw new Error(`live member preview failed: HTTP ${response.status} ${await response.text()}`);
}

async function main() {
  const outputRoot = resolve(argument("--output", defaultOutput));
  const manifest = JSON.parse(await readFile(join(fixtureRoot, "fixture-manifest.json"), "utf8"));
  const captureNowMs = Date.parse(manifest.capture_now);
  const storeRoot = await mkdtemp(join(tmpdir(), "workbench-layout-v2-native-v1-"));
  const apiPort = await freePort();
  const webPort = await freePort();
  const apiBase = `http://127.0.0.1:${apiPort}`;
  const webBase = `http://127.0.0.1:${webPort}`;
  let apiProcess;
  let webProcess;
  let browser;

  try {
    await materialize(storeRoot, manifest);
    apiProcess = start(
      join(repoRoot, "target/debug/harness"),
      ["--store", storeRoot, "serve", "--addr", `127.0.0.1:${apiPort}`],
      "harness serve",
    );
    webProcess = start(
      process.execPath,
      [join(repoRoot, "node_modules/vite/bin/vite.js"), "--config", "apps/agent-dashboard/vite.config.ts", "--host", "127.0.0.1", "--port", String(webPort)],
      "Vite dashboard",
      { HARNESS_CAPTURE_API_PROXY: apiBase },
    );
    await Promise.race([
      Promise.all([
        waitFor(`${apiBase}/v1/snapshot?project=_store`, "Harness API"),
        waitFor(webBase, "Dashboard"),
      ]),
      apiProcess.done.then(() => { throw new Error(apiProcess.describeFailure()); }),
      webProcess.done.then(() => { throw new Error(webProcess.describeFailure()); }),
    ]);

    browser = await chromium.launch();
    const cases = [
      ["agent-teams-home", "native-attempts", manifest.routes["agent-teams-home"], "Agent Teams"],
      ["mission-wave-canvas", "running-gate-pending", manifest.routes["mission-wave-canvas"], "Agent Team Console"],
      ["team-war-room", "running-needs-you", manifest.routes["team-war-room"], "Platform Foundation Team"],
      ["member-run-focus", "running-needs-you", manifest.routes["member-run-focus"], "Research Engineer"],
    ];
    const viewports = [
      [1440, 1000, "desktop-1440x1000", ""],
      [900, 1180, "tablet-900x1180", "responsive"],
      [390, 844, "mobile-390x844", "responsive"],
    ];
    const captures = [];
    const interactionChecks = [];

    for (const [width, height, viewportName, subdirectory] of viewports) {
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, reducedMotion: "reduce", colorScheme: "light", locale: "en-US" });
      await context.addInitScript((nowMs) => {
        const NativeDate = Date;
        class FixedDate extends NativeDate {
          constructor(...args) { super(...(args.length ? args : [nowMs])); }
          static now() { return nowMs; }
        }
        window.Date = FixedDate;
      }, captureNowMs);
      const page = await context.newPage();
      const errors = [];
      page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
      page.on("pageerror", (error) => errors.push(error.message));

      for (const [pageName, state, route, readyText] of cases) {
        const separator = route.includes("?") ? "&" : "?";
        // Keep browser reads and SSE same-origin. Vite forwards /v1 and /health
        // to this run's isolated Harness API; Node-side fixture writes still use
        // apiBase directly.
        const url = `${webBase}${route}${separator}api=${encodeURIComponent(webBase)}&project=_store`;
        await page.goto(url, { waitUntil: "domcontentloaded" });
        await page.getByRole("heading", { name: readyText, exact: false }).first().waitFor({ state: "attached", timeout: 15_000 });
        await page.evaluate(() => document.fonts.ready);
        if (pageName === "member-run-focus") {
          // A live preview is deliberately non-replayable and can be missed by
          // a screenshot client while its EventSource is reconnecting. Publish
          // one when possible, but keep deterministic visual evidence anchored
          // to durable history; SSE lifecycle tests own the transient contract.
          await publishLivePreview(apiBase, manifest);
          await page.waitForTimeout(250);
        }
        const dimensions = await page.evaluate(() => ({
          scrollWidth: document.documentElement.scrollWidth,
          clientWidth: document.documentElement.clientWidth,
        }));
        if (dimensions.scrollWidth > dimensions.clientWidth) {
          throw new Error(`${pageName} ${viewportName} has horizontal overflow: ${JSON.stringify(dimensions)}`);
        }
        if (errors.length) throw new Error(`${pageName} ${viewportName} console errors: ${errors.join(" | ")}`);

        const directory = join(outputRoot, subdirectory);
        await mkdir(directory, { recursive: true });
        const output = join(directory, `${pageName}--${state}--${viewportName}.png`);
        await page.screenshot({ path: output });
        captures.push({ page: pageName, state, viewport: viewportName, route, output });

        if (width <= 900 && pageName !== "agent-teams-home") {
          if (pageName === "mission-wave-canvas") {
            await page.getByRole("button", { name: "Context", exact: true }).click();
          } else {
            await page.getByText("Context & controls", { exact: true }).click();
          }
          await page.waitForTimeout(300);
          const contextOutput = join(directory, `${pageName}--context-open--${viewportName}.png`);
          await page.screenshot({ path: contextOutput });
          captures.push({ page: pageName, state: "context-open", viewport: viewportName, route, output: contextOutput });
        }

        if (width === 1440 && pageName === "mission-wave-canvas") {
          const missionRegion = page.getByRole("region", { name: "Mission detail", exact: true });
          const beforeScroll = await missionRegion.evaluate((element) => ({
            top: element.scrollTop,
            max: element.scrollHeight - element.clientHeight,
          }));
          await missionRegion.focus();
          await page.keyboard.press("PageDown");
          const afterScroll = await missionRegion.evaluate((element) => ({
            top: element.scrollTop,
            max: element.scrollHeight - element.clientHeight,
          }));
          if (beforeScroll.max <= 0 || afterScroll.top <= beforeScroll.top) {
            throw new Error(`Mission content is not keyboard-scrollable: ${JSON.stringify({ beforeScroll, afterScroll })}`);
          }
          interactionChecks.push({
            page: pageName,
            action: "mission-content-reachability",
            before: beforeScroll,
            after: afterScroll,
            result: "passed",
          });

          await page.getByRole("button", { name: /Open(?: member)? QA Engineer/, exact: true }).click();
          await page.locator("h1").filter({ hasText: "QA Engineer" }).waitFor({ state: "visible", timeout: 5_000 });
          const selected = new URL(page.url()).searchParams;
          if (
            selected.get("memberRun") !== "member-wave2-qa"
            || selected.get("team") !== manifest.team_run_id
            || selected.get("mission") !== manifest.mission_id
            || selected.get("wave") !== manifest.wave_id
          ) {
            throw new Error(`Mission member deep link lost execution context: ${page.url()}`);
          }
          interactionChecks.push({
            page: pageName,
            action: "mission-member-deep-link",
            expected_member_run_id: "member-wave2-qa",
            preserved_team_run_id: manifest.team_run_id,
            preserved_mission_id: manifest.mission_id,
            preserved_wave_id: manifest.wave_id,
            result: "passed",
          });
          await page.goBack({ waitUntil: "domcontentloaded" });
          await page.getByRole("region", { name: "Mission detail", exact: true }).waitFor({ state: "visible", timeout: 5_000 });
          interactionChecks.push({ page: pageName, action: "member-return-context", result: "passed" });
        }
      }
      await context.close();
    }

    const runManifest = {
      fixture: manifest.id,
      fixture_manifest: join(fixtureRoot, "fixture-manifest.json"),
      capture_now: manifest.capture_now,
      browser: browser.version(),
      git_revision: execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim(),
      git_dirty: Boolean(execFileSync("git", ["status", "--porcelain"], { cwd: repoRoot, encoding: "utf8" }).trim()),
      api_scope: "_store",
      captures,
      interaction_checks: interactionChecks,
    };
    await mkdir(outputRoot, { recursive: true });
    await writeFile(join(outputRoot, "capture-run.json"), `${JSON.stringify(runManifest, null, 2)}\n`);
    console.log(JSON.stringify(runManifest, null, 2));
  } finally {
    await browser?.close().catch(() => {});
    await Promise.all([stop(webProcess), stop(apiProcess)]);
    await rm(storeRoot, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
