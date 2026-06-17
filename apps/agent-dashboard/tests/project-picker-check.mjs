#!/usr/bin/env node
// Dashboard multi-project verification (goal-multi-project P6, dashboard-browser-check).
//
// Proves the two user-visible guarantees of the project picker against a LIVE
// `harness serve` that multiplexes several project stores:
//
//   1. SSE channel isolation (the load-bearing guarantee): a client subscribed to
//      project B never receives a live event appended to project A's store, while
//      a live event appended to B IS delivered (positive control). This is exactly
//      what the picker relies on when it re-points the stream on switch.
//   2. Picker UI switch flow (best-effort, only when Playwright is installed): the
//      served dashboard lists `_global` + the registered projects, and selecting
//      A -> B -> _global re-points the scoped read model + SSE stream, persisting
//      the choice to the URL (`?project=<id>`).
//
// The isolation check uses a tiny dependency-free SSE reader over streaming fetch
// (Node has no global EventSource), so it runs everywhere — CI included. The
// Playwright leg degrades to SKIP (not FAIL) when Playwright is absent, so this
// never forces a heavyweight browser dependency into the repo.
//
// Usage (driven by scripts/verify-fixes.sh):
//   node apps/agent-dashboard/tests/project-picker-check.mjs \
//     --base http://127.0.0.1:8797 \
//     --project-a <idA> --store-a <storeRootA> \
//     --project-b <idB> --store-b <storeRootB> \
//     [--web-url http://127.0.0.1:5191]   # built+served dashboard, for the Playwright leg
//
// Exit code: 0 when every non-skipped check passes, 1 otherwise. Prints a
// PASS/FAIL/SKIP matrix that scripts/verify-fixes.sh folds into its own.

import { appendFile, mkdir } from "node:fs/promises";
import { join } from "node:path";

function arg(name, fallback = null) {
  const i = process.argv.indexOf(name);
  return i !== -1 && i + 1 < process.argv.length ? process.argv[i + 1] : fallback;
}

const base = (arg("--base") || "http://127.0.0.1:8797").replace(/\/$/, "");
const projectA = arg("--project-a");
const projectB = arg("--project-b");
const storeA = arg("--store-a");
const storeB = arg("--store-b");
const webUrl = arg("--web-url");

let pass = 0;
let fail = 0;
let skip = 0;
const ok = (m) => { console.log(`  PASS  ${m}`); pass += 1; };
const bad = (m) => { console.log(`  FAIL  ${m}`); fail += 1; };
const skipped = (m) => { console.log(`  SKIP  ${m}`); skip += 1; };

if (!projectA || !projectB || !storeA || !storeB) {
  console.error("project-picker-check: missing --project-a/--project-b/--store-a/--store-b");
  process.exit(2);
}

/**
 * Subscribe to `${base}/v1/events?project=<id>` and collect every `agent_event`
 * frame's id for `windowMs`, invoking `onReady` once the stream is open (the
 * initial `snapshot` frame arrived) so the caller can append AFTER subscription.
 * Returns the set of agent_event ids seen.
 */
async function collectAgentEventIds(projectId, windowMs, onReady) {
  const controller = new AbortController();
  const seen = new Set();
  const res = await fetch(`${base}/v1/events?project=${encodeURIComponent(projectId)}`, {
    headers: { Accept: "text/event-stream" },
    signal: controller.signal,
  });
  if (!res.ok || !res.body) throw new Error(`SSE connect failed: HTTP ${res.status}`);

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let readyFired = false;

  const parseFrames = () => {
    let idx;
    while ((idx = buffer.indexOf("\n\n")) !== -1) {
      const block = buffer.slice(0, idx);
      buffer = buffer.slice(idx + 2);
      let event = "message";
      let data = "";
      for (const line of block.split("\n")) {
        if (line.startsWith("event:")) event = line.slice(6).trim();
        else if (line.startsWith("data:")) data += line.slice(5).trim();
      }
      if (event === "snapshot" && !readyFired) {
        readyFired = true;
        Promise.resolve().then(onReady);
      }
      if (event === "agent_event" && data) {
        try {
          const obj = JSON.parse(data);
          if (obj && obj.id) seen.add(obj.id);
        } catch {
          // ignore an unparseable frame; never tears the stream down
        }
      }
    }
  };

  // End the window by aborting the stream — NOT by racing reader.read() against a
  // timeout. Racing leaves an orphaned read() pending across loop iterations; the
  // NEXT chunk resolves that abandoned promise and is silently dropped (the live
  // frame we are asserting on would vanish). Aborting makes a single in-flight
  // read() reject cleanly, so every byte is observed exactly once.
  const stop = setTimeout(() => controller.abort(), windowMs);
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      parseFrames();
    }
  } catch {
    // AbortError on window expiry (or a server close) — expected end of collection.
  } finally {
    clearTimeout(stop);
    try { await reader.cancel(); } catch { /* already aborted */ }
  }
  return seen;
}

/** Append one AgentEvent row to a project's store; the watcher broadcasts it as an
 * `agent_event` SSE frame on THAT project's channel only. Returns the row id. */
async function emitAgentEvent(storeRoot, label) {
  const id = `picker-check-${label}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  const row = {
    id,
    agent_member_id: "picker-check",
    event_type: "verification",
    summary: `dashboard-browser-check ${label}`,
    created_at: new Date().toISOString(),
  };
  await mkdir(storeRoot, { recursive: true });
  await appendFile(join(storeRoot, "agent_events.jsonl"), `${JSON.stringify(row)}\n`);
  return id;
}

async function checkSseIsolation() {
  // Subscribe to B; once open, append a live event to A (must NOT arrive) and one
  // to B (must arrive). A ~2.5s window comfortably exceeds the ~150ms watcher poll.
  let idA = null;
  let idB = null;
  const seenByB = await collectAgentEventIds(projectB, 2500, async () => {
    idA = await emitAgentEvent(storeA, "A");
    // Small gap so ordering is unambiguous, then the positive control on B.
    await new Promise((r) => setTimeout(r, 300));
    idB = await emitAgentEvent(storeB, "B");
  });

  if (idB && seenByB.has(idB)) {
    ok(`B subscriber received B's live event (positive control: ${idB})`);
  } else {
    bad(`B subscriber did NOT receive B's live event (positive control failed)`);
  }
  if (idA && !seenByB.has(idA)) {
    ok(`B subscriber did NOT receive A's live event (isolation holds: ${idA})`);
  } else {
    bad(`B subscriber LEAKED A's live event ${idA} (isolation broken)`);
  }
}

async function checkProjectsApi() {
  const res = await fetch(`${base}/v1/projects`);
  if (!res.ok) { bad(`GET /v1/projects HTTP ${res.status}`); return; }
  const data = await res.json();
  const ids = new Set((data.projects || []).map((p) => p.id));
  if (ids.has(projectA) && ids.has(projectB) && ids.has("_global")) {
    ok(`GET /v1/projects lists A, B and _global (${[...ids].join(", ")})`);
  } else {
    bad(`GET /v1/projects missing one of A/B/_global (got ${[...ids].join(", ")})`);
  }
}

/**
 * Drive the served dashboard UI through A -> B -> _global with Playwright, if it
 * is installed. SKIPs (does not FAIL) when Playwright or --web-url is absent so
 * the repo never has to carry a browser dependency for the gate.
 */
async function checkPickerUi() {
  if (!webUrl) { skipped("Playwright UI switch (no --web-url provided)"); return; }
  let chromium;
  try {
    ({ chromium } = await import("playwright"));
  } catch {
    skipped("Playwright UI switch (playwright not installed)");
    return;
  }
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(`${webUrl}/?api=${encodeURIComponent(base)}&project=${encodeURIComponent(projectA)}`, {
      waitUntil: "domcontentloaded",
    });
    const picker = page.locator('select[aria-label="Active project"]');
    await picker.waitFor({ state: "attached", timeout: 15000 });
    for (const target of [projectB, "_global", projectA]) {
      await picker.selectOption(target);
      await page.waitForFunction(
        (id) => new URLSearchParams(location.search).get("project") === id,
        target,
        { timeout: 10000 },
      );
    }
    const shot = join(process.cwd(), "apps/agent-dashboard/tests", "project-picker-evidence.png");
    await page.screenshot({ path: shot });
    ok(`Playwright UI switched A -> B -> _global -> A; screenshot ${shot}`);
  } catch (error) {
    bad(`Playwright UI switch failed: ${error.message}`);
  } finally {
    await browser.close();
  }
}

async function main() {
  console.log("== Dashboard multi-project picker checks ==");
  console.log(`   base=${base} A=${projectA} B=${projectB}`);
  await checkProjectsApi();
  await checkSseIsolation();
  await checkPickerUi();
  console.log(`\n   picker checks: ${pass} pass, ${fail} fail, ${skip} skip`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`project-picker-check crashed: ${error.stack || error}`);
  process.exit(1);
});
