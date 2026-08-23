#!/usr/bin/env node

/**
 * Rendered-DOM acceptance for MemberRun Focus at phone widths.
 *
 * The hero header and composer were authored as a desktop composition: a fixed
 * 152px hero with 44px side padding, a 130px portrait, and a single-row
 * composer whose two selects and send button are laid out beside the textarea.
 * At 390px that left the identity block overlapping its own controls and
 * collapsed the message input to roughly 60px.
 *
 * Overlap and collapse are geometry, so they are asserted against real
 * rectangles rather than source text.
 */

import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = join(dashboardRoot, "fixtures/workbench-layout-v2-native-v1");

let passed = 0;
let failed = 0;
function check(condition, message) {
  if (condition) {
    console.log(`  PASS  ${message}`);
    passed += 1;
  } else {
    console.log(`  FAIL  ${message}`);
    failed += 1;
  }
}

async function jsonl(name) {
  return (await readFile(join(fixtureRoot, `${name}.jsonl`), "utf8"))
    .split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

const manifest = JSON.parse(await readFile(join(fixtureRoot, "fixture-manifest.json"), "utf8"));
const workOperations = await jsonl("work_operations");
const workDeliveries = await jsonl("current_work_deliveries");
const worksById = new Map();
for (const operation of workOperations) {
  worksById.set(operation.work.id, operation.work);
}

const snapshot = {
  generated_at: "2026-07-29T00:00:00Z",
  teams: await jsonl("teams"),
  missions: await jsonl("missions"),
  legacy_waves: await jsonl("waves"),
  team_runs: await jsonl("team_runs"),
  member_runs: await jsonl("member_runs"),
  team_messages: await jsonl("team_messages"),
  works: [...worksById.values()],
  work_events: workOperations.map((operation) => operation.event),
  work_deliveries: workDeliveries,
  member_actions: await jsonl("member_actions"),
  delegation_runs: await jsonl("delegation_runs"),
  team_run_events: await jsonl("team_run_events"),
  evidence: await jsonl("evidence"),
  members: [], messages: [], events: [], provider_child_threads: [],
  team_supervisor_leases: [],
  team_member_close_requests: [],
  company_os: {},
};
const teamRunId = snapshot.team_runs.find((run) => run.member_run_ids?.length)?.id;

const VIEWPORTS = [
  { width: 1440, height: 1000, label: "desktop-1440x1000", mobile: false },
  { width: 900, height: 1180, label: "tablet-900x1180", mobile: false },
  { width: 390, height: 844, label: "mobile-390x844", mobile: true },
  { width: 320, height: 720, label: "mobile-320x720", mobile: true },
];

const vite = await createServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "silent",
});
await vite.listen();
const base = `http://127.0.0.1:${vite.httpServer.address().port}`;
const browser = await chromium.launch({ headless: true });

const PROBE = `(() => {
  const vis = (el) => { if (!el) return false; const b = el.getBoundingClientRect(); return b.width > 0 && b.height > 0; };
  const rect = (el) => { const b = el.getBoundingClientRect(); return { left: b.left, right: b.right, top: b.top, bottom: b.bottom, w: Math.round(b.width), h: Math.round(b.height) }; };
  const overlaps = (a, b) => a.left < b.right - 1 && b.left < a.right - 1 && a.top < b.bottom - 1 && b.top < a.bottom - 1;

  const header = document.querySelector('header');
  const title = header ? header.querySelector('h1') : null;
  const controls = header ? [...header.querySelectorAll('button, a[href]')].filter(vis) : [];
  const titleBox = title && vis(title) ? rect(title) : null;
  const controlBoxes = controls.map((el) => ({
    label: (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 24),
    box: rect(el),
  }));
  const titleOverlaps = titleBox
    ? controlBoxes.filter((c) => overlaps(titleBox, c.box)).map((c) => c.label)
    : [];

  const form = document.querySelector('footer form');
  const textarea = document.getElementById('member-run-message');
  const send = document.querySelector('footer button[type="submit"]');
  const formBox = form && vis(form) ? rect(form) : null;
  const textareaBox = textarea && vis(textarea) ? rect(textarea) : null;
  const sendBox = send && vis(send) ? rect(send) : null;

  // Anything in the header or footer that is clipped by its own scroll box.
  const clipped = [];
  for (const el of [...document.querySelectorAll('header'), form].filter(Boolean)) {
    if (!vis(el)) continue;
    if (el.scrollWidth > el.clientWidth + 1) {
      const heading = el.querySelector('h1, h2');
      clipped.push((heading ? heading.textContent.trim().slice(0, 18) : el.tagName.toLowerCase()));
    }
  }

  const smallControls = [...document.querySelectorAll('header button, header a[href], footer button, footer select')]
    .filter(vis)
    .map((el) => ({ label: (el.getAttribute('aria-label') || el.textContent || '').trim().slice(0, 24), h: Math.round(el.getBoundingClientRect().height) }))
    .filter((entry) => entry.h < 44);

  const goalSummary = document.querySelector('[data-goal-summary="true"]');
  const goalSummaryWidth = goalSummary && vis(goalSummary) ? Math.round(goalSummary.getBoundingClientRect().width) : 0;

  return {
    goalSummaryWidth,
    docOverflowX: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    hasTitle: Boolean(titleBox),
    titleWidth: titleBox ? titleBox.w : 0,
    portraitVisible: Boolean(header && [...header.querySelectorAll('img, svg, span')].some((el) => vis(el))),
    controlCount: controlBoxes.length,
    controlLabels: controlBoxes.map((c) => c.label),
    titleOverlaps,
    formWidth: formBox ? formBox.w : 0,
    textareaWidth: textareaBox ? textareaBox.w : 0,
    textareaRatio: formBox && textareaBox ? textareaBox.w / formBox.w : 0,
    sendHeight: sendBox ? sendBox.h : 0,
    clipped,
    smallControls,
  };
})()`;

async function mockRoutes(page) {
  await page.route("**/v1/**", async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === `/v1/team-runs/${encodeURIComponent(teamRunId)}/snapshot` || url.pathname === "/v1/snapshot") {
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(snapshot) });
    }
    if (url.pathname === "/v1/projects") return route.fulfill({ status: 200, contentType: "application/json", body: '{"projects":[],"current":""}' });
    if (url.pathname === "/v1/spaces") return route.fulfill({ status: 200, contentType: "application/json", body: '{"spaces":[],"current":""}' });
    if (url.pathname === "/v1/companies") return route.fulfill({ status: 200, contentType: "application/json", body: '{"companies":[],"current":""}' });
    if (url.pathname === "/v1/events") return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
    if (url.pathname.endsWith("/native-activity")) return route.fulfill({ status: 200, contentType: "application/json", body: '{"items":[],"truncated":false,"availability":"unknown","native_session_id":"","provider":"","execution_mode":""}' });
    // The persistent provenance footer (issue #307) polls this on its own;
    // stub it like every other endpoint so this check stays free of
    // console-logged network errors.
    if (url.pathname === "/v1/meta") {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          git_rev: "fixture0",
          built_at: null,
          store_root: "/fixture/store",
          latest_op_seq: 0,
          server_version: "0.0.0-fixture",
        }),
      });
    }
    return route.fulfill({ status: 404, contentType: "application/json", body: '{"error":"not_found"}' });
  });
}

try {
  for (const viewport of VIEWPORTS) {
    const context = await browser.newContext({
      viewport: { width: viewport.width, height: viewport.height },
      deviceScaleFactor: 1,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await mockRoutes(page);
    const route = manifest.routes["member-run-focus"];
    const separator = route.includes("?") ? "&" : "?";
    await page.goto(`${base}${route}${separator}api=${encodeURIComponent(base)}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
    await page.locator("h1").first().waitFor({ timeout: 20_000 });

    const state = await page.evaluate(PROBE);

    check(!state.docOverflowX, `${viewport.label}: no horizontal overflow`);
    check(pageErrors.length === 0, `${viewport.label}: no page errors (${pageErrors.slice(0, 1).join("")})`);
    check(state.hasTitle && state.titleWidth > 0, `${viewport.label}: member title is rendered (${state.titleWidth}px wide)`);
    check(state.portraitVisible, `${viewport.label}: member portrait is rendered`);
    check(
      state.titleOverlaps.length === 0,
      `${viewport.label}: header controls do not overlap the member title (${state.titleOverlaps.join(", ") || "no overlap"})`,
    );
    check(
      state.clipped.length === 0,
      `${viewport.label}: header and composer are not clipped by their own width (${state.clipped.join(", ") || "none"})`,
    );
    check(
      state.controlLabels.some((label) => label.startsWith("Back to team")),
      `${viewport.label}: Back to team control is present (${state.controlCount} header controls)`,
    );

    if (viewport.mobile) {
      // The message input is the point of the surface; it must keep a usable
      // share of the composer rather than being squeezed by its own controls.
      check(
        state.textareaWidth >= 200 && state.textareaRatio >= 0.8,
        `${viewport.label}: composer input stays full-width usable (${state.textareaWidth}px, ${Math.round(state.textareaRatio * 100)}% of the form)`,
      );
      check(
        state.goalSummaryWidth >= 200,
        `${viewport.label}: Current Work summary keeps a readable column (${state.goalSummaryWidth}px)`,
      );
      check(
        state.sendHeight >= 44,
        `${viewport.label}: send action is at least 44px tall (${state.sendHeight}px)`,
      );
      check(
        state.smallControls.length === 0,
        `${viewport.label}: header and composer controls are at least 44px tall (${state.smallControls.map((c) => `${c.label}:${c.h}`).join(", ") || "all pass"})`,
      );
    }

    await context.close();
  }
  console.log(`\n${passed} passed, ${failed} failed`);
} finally {
  await browser.close();
  await vite.close();
}

process.exit(failed === 0 ? 0 : 1);
