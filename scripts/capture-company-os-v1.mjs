#!/usr/bin/env node

import { execFileSync, spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

import {
  companyOsApiProjection,
  loadCompanyOsFixture,
  resolveContractRoute,
} from "../apps/agent-dashboard/fixtures/company-os-trademark-v1/fixture.mjs";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const designRoot = join(repoRoot, "docs/design/company-os-v1");

function argument(name, fallback) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

function flag(name) {
  return process.argv.includes(name);
}

function safeRunId(value) {
  if (!/^[A-Za-z0-9._-]+$/.test(value)) throw new Error(`unsafe run id: ${value}`);
  return value;
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

function normalizedHttpBase(value, label) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error(`${label} must be an absolute HTTP(S) URL`);
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error(`${label} must use HTTP(S), got ${parsed.protocol}`);
  }
  return parsed.toString().replace(/\/$/, "");
}

async function readJson(url, label) {
  const response = await fetch(url, { headers: { accept: "application/json" } });
  if (!response.ok) throw new Error(`${label} returned HTTP ${response.status}`);
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("application/json")) {
    throw new Error(`${label} did not return application/json`);
  }
  return response.json();
}

async function inspectLiveSource(apiBaseUrl, requestedProjectId) {
  const projectsUrl = `${apiBaseUrl}/v1/projects`;
  const projectsPayload = await readJson(projectsUrl, "Harness projects API");
  const projects = Array.isArray(projectsPayload?.projects) ? projectsPayload.projects : [];
  const projectId = requestedProjectId || projectsPayload?.current;
  if (typeof projectId !== "string" || !projectId.trim()) {
    throw new Error("live capture needs --project-id or a current project from GET /v1/projects");
  }
  const project = projects.find((item) => item && typeof item === "object" && item.id === projectId);
  if (!project) throw new Error(`live project ${projectId} is absent from GET /v1/projects`);
  const snapshotUrl = `${apiBaseUrl}/v1/snapshot?project=${encodeURIComponent(projectId)}`;
  const snapshot = await readJson(snapshotUrl, "Harness project snapshot");
  if (!snapshot || typeof snapshot !== "object" || Array.isArray(snapshot)) {
    throw new Error("Harness project snapshot is not an object");
  }
  const companyOs = snapshot.company_os;
  if (!companyOs || typeof companyOs !== "object" || Array.isArray(companyOs)) {
    throw new Error("live Harness snapshot has no company_os projection; refusing prototype or fixture fallback evidence");
  }
  return {
    api_base_url: apiBaseUrl,
    projects_endpoint: projectsUrl,
    snapshot_endpoint: snapshotUrl,
    project_id: projectId,
    project,
    snapshot_generated_at: typeof snapshot.generated_at === "string" ? snapshot.generated_at : null,
    company_os_present: Boolean(companyOs && typeof companyOs === "object" && !Array.isArray(companyOs)),
    company_os_fixture_id:
      companyOs && typeof companyOs === "object" && typeof companyOs.fixture_id === "string"
        ? companyOs.fixture_id
        : null,
  };
}

function startVite(port, apiProxy = "") {
  const child = spawn(
    process.execPath,
    [join(repoRoot, "node_modules/vite/bin/vite.js"), "--config", "apps/agent-dashboard/vite.config.ts", "--host", "127.0.0.1", "--port", String(port)],
    {
      cwd: repoRoot,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        FORCE_COLOR: "0",
        ...(apiProxy ? { HARNESS_CAPTURE_API_PROXY: apiProxy } : {}),
      },
    },
  );
  let output = "";
  for (const stream of [child.stdout, child.stderr]) {
    stream.on("data", (chunk) => {
      output += chunk.toString();
      if (output.length > 20_000) output = output.slice(-20_000);
    });
  }
  child.done = new Promise((resolveDone) => child.once("exit", (code, signal) => resolveDone({ code, signal })));
  child.describeFailure = () => `Vite dashboard output:\n${output}`;
  return child;
}

async function stopProcess(child) {
  if (!child || child.exitCode !== null) return;
  child.kill("SIGTERM");
  const exited = await Promise.race([
    child.done.then(() => true),
    new Promise((resolveWait) => setTimeout(() => resolveWait(false), 2_000)),
  ]);
  if (!exited && child.exitCode === null) child.kill("SIGKILL");
}

async function startFixtureApi(manifest, fixture) {
  const port = await freePort();
  const clients = new Set();
  const projection = companyOsApiProjection(manifest, fixture);
  const server = createServer((request, response) => {
    response.setHeader("access-control-allow-origin", "*");
    response.setHeader("access-control-allow-headers", "content-type");
    if (request.method === "OPTIONS") {
      response.writeHead(204);
      response.end();
      return;
    }
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "127.0.0.1"}`);
    if (url.pathname === "/v1/events") {
      response.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
      });
      response.write(`event: snapshot\ndata: ${JSON.stringify({ generated_at: manifest.capture_now })}\n\n`);
      clients.add(response);
      request.on("close", () => clients.delete(response));
      return;
    }
    let body;
    if (url.pathname === "/v1/snapshot") body = projection;
    else if (url.pathname === "/v1/company-os/fixture" || url.pathname === "/v1/company-os/bootstrap") body = fixture;
    else if (url.pathname === "/v1/docs") {
      const path = url.searchParams.get("path") ?? "";
      body = {
        path,
        content: path === "docs/registry.json"
          ? JSON.stringify({ version: 1, documents: [] })
          : "# Company OS deterministic capture fixture\n",
      };
    }
    else if (url.pathname === "/v1/projects") body = {
      current: manifest.project_id,
      projects: [{ id: manifest.project_id, name: "Company OS Trademark V1", active: true }],
    };
    else if (url.pathname === "/v1/workflows") body = [];
    else {
      response.writeHead(404, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: `fixture endpoint not found: ${url.pathname}` }));
      return;
    }
    response.writeHead(200, { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" });
    response.end(JSON.stringify(body));
  });
  await new Promise((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", resolveListen);
  });
  return {
    baseUrl: `http://127.0.0.1:${port}`,
    close: async () => {
      for (const client of clients) client.end();
      await new Promise((resolveClose) => server.close(resolveClose));
    },
  };
}

function artifactName(item, viewportName) {
  return `${item.page}--${item.state}--${viewportName}.png`;
}

function expectedActorTypes(fixture, refs) {
  const types = new Map(fixture.actors.map((actor) => [actor.id, actor.actor_type]));
  return refs.filter((ref) => types.has(ref)).map((ref) => [ref, types.get(ref)]);
}

async function verifyPage(page, root, item, viewportName, fixture, fixtureManifest) {
  const slice = fixture.page_slices[item.page];
  const missingRefs = [];
  for (const ref of slice.required_refs) {
    const locator = root.locator(`[data-company-os-ref="${ref}"]`);
    if (await locator.count() === 0) missingRefs.push(ref);
  }
  if (missingRefs.length) throw new Error(`missing required record markers: ${missingRefs.join(", ")}`);

  for (const [ref, actorType] of expectedActorTypes(fixture, slice.required_refs)) {
    const locator = root.locator(`[data-company-os-ref="${ref}"][data-actor-type="${actorType}"]`);
    if (await locator.count() === 0) throw new Error(`actor ${ref} is missing explicit type ${actorType}`);
  }

  const commitmentId = "financial-commitment-trademark-filing-fee-cn-2026-018";
  if (slice.required_refs.includes(commitmentId)) {
    const commitment = root.locator(
      `[data-company-os-ref="${commitmentId}"][${fixtureManifest.browser_contract.financial_record_type_attribute}="commitment"]`,
    );
    if (await commitment.count() === 0) throw new Error("¥3,000 record is not explicitly rendered as a Commitment");
  }
  if (await root.locator(`[${fixtureManifest.browser_contract.financial_record_type_attribute}="${fixtureManifest.browser_contract.forbidden_financial_record_type}"]`).count()) {
    throw new Error("page renders a Payment record before approval/settlement");
  }
  if (await root.locator(fixtureManifest.browser_contract.settlement_evidence_selector).count()) {
    throw new Error("page renders settlement evidence that is absent from the fixture");
  }
  if (await root.locator("[data-provider-thinking], [data-thinking-persisted]").count()) {
    throw new Error("page exposes provider thinking as durable product state");
  }

  const dimensions = await page.evaluate(() => ({
    document: {
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    },
    body: { scrollWidth: document.body.scrollWidth, clientWidth: document.body.clientWidth },
  }));
  if (dimensions.document.scrollWidth > dimensions.document.clientWidth || dimensions.body.scrollWidth > dimensions.body.clientWidth) {
    throw new Error(`${viewportName} has horizontal page overflow: ${JSON.stringify(dimensions)}`);
  }
}

async function main() {
  const stage = argument("--stage", "implemented");
  if (!new Set(["baseline", "implemented"]).has(stage)) {
    throw new Error("--stage must be baseline or implemented");
  }
  const dataMode = argument("--data-mode", "fixture");
  if (!new Set(["fixture", "live"]).has(dataMode)) {
    throw new Error("--data-mode must be fixture or live");
  }
  const suppliedApiBase = argument("--api-base-url", "");
  const suppliedProjectId = argument("--project-id", "");
  if (dataMode === "live" && !suppliedApiBase) {
    throw new Error("--data-mode live requires --api-base-url pointing to a real Harness server");
  }
  if (dataMode === "fixture" && (suppliedApiBase || suppliedProjectId)) {
    throw new Error("--api-base-url/--project-id are live-only; fixture mode owns an isolated fixture API");
  }
  const timeoutMs = Number(argument("--timeout-ms", "6000"));
  const runId = safeRunId(argument("--run-id", `${dataMode}-${stage}-${new Date().toISOString().replace(/[:.]/g, "-")}`));
  const runRoot = resolve(argument("--output", join(repoRoot, ".visual-evidence/company-os-v1", runId)));
  const suppliedWebBase = argument("--web-base-url", "");
  const { manifest: fixtureManifest, fixture, sourcePath, sourceSha256 } = await loadCompanyOsFixture();
  const visual = JSON.parse(await readFile(join(designRoot, "visual-contract.json"), "utf8"));
  const liveSource = dataMode === "live"
    ? await inspectLiveSource(normalizedHttpBase(suppliedApiBase, "--api-base-url"), suppliedProjectId)
    : null;
  const fixtureApi = dataMode === "fixture" ? await startFixtureApi(fixtureManifest, fixture) : null;
  const apiBaseUrl = fixtureApi?.baseUrl ?? liveSource?.api_base_url;
  const projectId = fixtureApi ? fixtureManifest.project_id : liveSource?.project_id;
  if (!apiBaseUrl || !projectId) throw new Error(`unable to resolve ${dataMode} API/project source`);
  let vite;
  let browser;
  let webBase = suppliedWebBase ? normalizedHttpBase(suppliedWebBase, "--web-base-url") : "";

  try {
    if (!webBase) {
      const webPort = await freePort();
      webBase = `http://127.0.0.1:${webPort}`;
      vite = startVite(webPort, dataMode === "live" ? liveSource.api_base_url : "");
      await Promise.race([
        waitFor(webBase, "Dashboard"),
        vite.done.then(() => { throw new Error(vite.describeFailure()); }),
      ]);
    } else {
      await waitFor(webBase, "Dashboard", timeoutMs);
    }

    browser = await chromium.launch({ headless: !flag("--headed") });
    const results = [];
    const gaps = [];

    for (const item of visual.cases) {
      const viewportNames = ["desktop-1440x1000"];
      if (fixtureManifest.responsive_pages.includes(item.page)) {
        viewportNames.push("tablet-900x1180", "mobile-390x844");
      }
      let desktopFailure = null;
      for (const viewportName of viewportNames) {
        const fileName = artifactName(item, viewportName);
        const artifacts = {
          baseline: join(runRoot, "baseline", fileName),
          expected: resolve(designRoot, item.expected),
          implemented: join(runRoot, "implemented", fileName),
        };
        if (desktopFailure && viewportName !== "desktop-1440x1000") {
          const skipped = { case_id: item.id, page: item.page, viewport: viewportName, route: item.route, status: "blocked", reason: `desktop route failed first: ${desktopFailure}`, artifacts };
          results.push(skipped);
          gaps.push(skipped);
          continue;
        }

        const viewport = fixtureManifest.viewports[viewportName];
        const context = await browser.newContext({
          viewport: { width: viewport.width, height: viewport.height },
          deviceScaleFactor: viewport.device_scale_factor,
          reducedMotion: "reduce",
          colorScheme: "light",
          locale: fixtureManifest.locale,
        });
        if (dataMode === "fixture") {
          await context.addInitScript(({ fixtureValue, nowMs }) => {
            Object.defineProperty(window, "__COMPANY_OS_FIXTURE__", { value: fixtureValue, writable: false });
            const NativeDate = Date;
            class FixedDate extends NativeDate {
              constructor(...args) { super(...(args.length ? args : [nowMs])); }
              static now() { return nowMs; }
            }
            window.Date = FixedDate;
          }, { fixtureValue: fixture, nowMs: Date.parse(fixtureManifest.capture_now) });
        }
        const page = await context.newPage();
        const consoleErrors = [];
        page.on("console", (message) => { if (message.type() === "error") consoleErrors.push(message.text()); });
        page.on("pageerror", (error) => consoleErrors.push(error.message));

        const route = resolveContractRoute(item.route, fixtureManifest.route_tokens);
        const url = new URL(route, webBase);
        // The production server intentionally has no wildcard CORS. A locally
        // started Vite host proxies live `/v1` reads so the browser contract is
        // same-origin; preflight and evidence metadata still address the real
        // Harness server directly.
        const browserApiBaseUrl = dataMode === "live" ? webBase : apiBaseUrl;
        url.searchParams.set("api", browserApiBaseUrl);
        url.searchParams.set("project", projectId);
        const selector = dataMode === "live"
          ? `[data-company-os-page="${item.page}"][data-company-os-ready="true"][data-company-os-data-mode="store-live"][data-company-os-prototype="false"]`
          : `[data-company-os-page="${item.page}"][data-company-os-fixture="${fixtureManifest.id}"][data-company-os-ready="true"][data-company-os-prototype="true"]`;
        const performCapture = async () => {
          await page.goto(url.toString(), { waitUntil: "domcontentloaded", timeout: timeoutMs });
          if (dataMode === "live" && await page.evaluate(() => typeof window.__COMPANY_OS_FIXTURE__ !== "undefined")) {
            throw new Error("live page exposes window.__COMPANY_OS_FIXTURE__; refusing injected fixture evidence");
          }
          const root = page.locator(selector).first();
          await root.waitFor({ state: "visible", timeout: timeoutMs });
          await page.evaluate(() => document.fonts.ready);
          await verifyPage(page, root, item, viewportName, fixture, fixtureManifest);
          if (consoleErrors.length) throw new Error(`console errors: ${consoleErrors.join(" | ")}`);
          const output = artifacts[stage];
          await mkdir(dirname(output), { recursive: true });
          await page.screenshot({ path: output, fullPage: false });
        };
        try {
          await performCapture();
          results.push({ case_id: item.id, page: item.page, viewport: viewportName, route, final_url: page.url(), status: "captured", artifacts, console_errors: [] });
        } catch (error) {
          let failureError = error;
          const transientErrors = [...consoleErrors];
          // A new context can observe one in-flight project/SSE navigation or a
          // Chromium network-change event. Retry every failed capture once so
          // an incomplete first render is distinguishable from a durable page
          // defect. Acceptance still requires the complete second pass to be
          // clean; no console error or missing fact is filtered.
          consoleErrors.length = 0;
          try {
            await page.waitForTimeout(250);
            await performCapture();
            results.push({
              case_id: item.id,
              page: item.page,
              viewport: viewportName,
              route,
              final_url: page.url(),
              status: "captured",
              retry_count: 1,
              first_attempt_error: error.message,
              transient_errors: transientErrors,
              artifacts,
              console_errors: [],
            });
            continue;
          } catch (retryError) {
            failureError = retryError;
          }
          const reason = `${failureError.message}. Required root: ${selector}`;
          const failure = { case_id: item.id, page: item.page, viewport: viewportName, route, final_url: page.url(), status: "route_or_contract_missing", reason, artifacts, console_errors: consoleErrors };
          results.push(failure);
          gaps.push(failure);
          if (viewportName === "desktop-1440x1000") desktopFailure = reason;
        } finally {
          await context.close();
        }
      }
    }

    const runManifest = {
      workstream: "company-os-v1",
      stage,
      data_mode: dataMode,
      status: gaps.length ? "failed" : "passed",
      run_id: runId,
      captured_at: new Date().toISOString(),
      assertion_contract: {
        fixture: fixtureManifest.id,
        source: sourcePath,
        sha256: sourceSha256,
      },
      capture_now: dataMode === "fixture" ? fixtureManifest.capture_now : null,
      browser: browser.version(),
      web_base_url: webBase,
      data_source: dataMode === "fixture"
        ? {
            kind: "deterministic-fixture-api",
            api_base_url: fixtureApi.baseUrl,
            project_id: fixtureManifest.project_id,
            source: sourcePath,
            sha256: sourceSha256,
          }
        : { kind: "harness-store-live", ...liveSource, browser_api_base_url: webBase, browser_transport: "same-origin-vite-proxy" },
      git_revision: execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim(),
      git_dirty: Boolean(execFileSync("git", ["status", "--porcelain"], { cwd: repoRoot, encoding: "utf8" }).trim()),
      evidence_chain: {
        baseline: join(runRoot, "baseline"),
        expected: join(designRoot, "expected"),
        implemented: join(runRoot, "implemented"),
      },
      checks: [
        "semantic page/fixture readiness",
        dataMode === "live" ? "store-live data mode and prototype=false" : "prototype fixture identity",
        "required canonical refs",
        "explicit actor types",
        "pending commitment, no payment",
        "no settlement evidence",
        "no persisted thinking",
        "console errors",
        "horizontal overflow",
      ],
      results,
      gaps,
    };
    await mkdir(runRoot, { recursive: true });
    await writeFile(join(runRoot, "capture-run.json"), `${JSON.stringify(runManifest, null, 2)}\n`);
    console.log(JSON.stringify(runManifest, null, 2));
    if (gaps.length) {
      console.error(`\nCompany OS capture failed honestly: ${gaps.length} route/contract gaps. See ${join(runRoot, "capture-run.json")}`);
      process.exitCode = 1;
    }
  } finally {
    await browser?.close().catch(() => {});
    await fixtureApi?.close();
    await stopProcess(vite);
  }
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
