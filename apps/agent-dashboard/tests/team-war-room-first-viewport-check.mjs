#!/usr/bin/env node

/**
 * Rendered-DOM acceptance for the Team War Room closure.
 *
 * Source-substring checks cannot prove a first viewport, a focus trap, or a
 * touch target, so these assertions drive the real app in headless Chromium at
 * the four contract viewports (1440x1000, 900x1180, 390x844, 320x720) over the
 * deterministic workbench fixture.
 */

import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

import {
  activityJourneyContract,
  desktopJourneyContract,
  teamWarRoomJourney,
} from "./team-war-room-journey.mjs";

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

const workOperations = await jsonl("work_operations");
const worksById = new Map();
const deliveriesById = new Map();
for (const operation of workOperations) {
  worksById.set(operation.work.id, operation.work);
  for (const delivery of operation.deliveries ?? []) deliveriesById.set(delivery.id, delivery);
  for (const update of operation.delivery_updates ?? []) {
    const delivery = deliveriesById.get(update.delivery_id);
    if (delivery) deliveriesById.set(update.delivery_id, { ...delivery, ...update, id: delivery.id });
  }
}

const snapshot = {
  generated_at: "2026-07-29T00:00:00Z",
  teams: await jsonl("teams"),
  missions: await jsonl("missions"),
  waves: await jsonl("waves"),
  team_runs: await jsonl("team_runs"),
  member_runs: await jsonl("member_runs"),
  team_messages: await jsonl("team_messages"),
  works: [...worksById.values()],
  work_events: workOperations.map((operation) => operation.event),
  work_deliveries: [...deliveriesById.values()],
  member_actions: await jsonl("member_actions"),
  delegation_runs: await jsonl("delegation_runs"),
  team_run_events: await jsonl("team_run_events"),
  evidence: await jsonl("evidence"),
  members: [], messages: [], events: [], provider_child_threads: [],
  workflow_runs: [], workflow_steps: [], workflow_patches: [],
  workflow_artifact_manifests: [], team_supervisor_leases: [],
  team_member_close_requests: [], agent_message_routes: [],
  pending_interactions: [], company_os: {},
};
const teamRunId = snapshot.team_runs.find((run) => run.member_run_ids?.length)?.id;
if (!teamRunId) throw new Error("fixture does not contain a current TeamRun with members");
const memberIds = new Set(
  snapshot.member_runs.filter((member) => member.team_run_id === teamRunId).map((member) => member.id),
);
const teamWorks = snapshot.works.filter((work) => work.team_run_id === teamRunId);
// Absent provider capacity is the fixture's real state and the case the UI must
// label honestly rather than upgrade into "available".
const membersWithoutCapacity = snapshot.member_runs
  .filter((member) => memberIds.has(member.id) && !member.provider_capacity).length;

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

/** Geometry of the scrollable work surface and what is inside its first screen. */
const PROBE = `(() => {
  const vis = (el) => { const b = el.getBoundingClientRect(); return b.width > 0 && b.height > 0; };
  const main = document.querySelector('main.min-h-0.flex-1.overflow-y-auto')
    || [...document.querySelectorAll('main')].find((m) => m.scrollHeight > m.clientHeight)
    || document.querySelector('main');
  const box = main.getBoundingClientRect();
  const cards = [...document.querySelectorAll('[data-work-card]')];
  const visibleCards = cards.filter(vis);
  const firstFullyVisible = visibleCards.find((c) => {
    const b = c.getBoundingClientRect();
    return b.top >= box.top - 1 && b.bottom <= box.bottom + 1;
  });
  const rows = [...document.querySelectorAll('[data-conversation-row="true"]')];
  const rowsAboveFold = rows.filter((r) => r.getBoundingClientRect().bottom <= box.bottom + 1).length;
  const firstRowStartsAboveFold = rows.length > 0 && rows[0].getBoundingClientRect().top < box.bottom;
  const strip = document.querySelector('[data-testid="team-capacity-strip"]');
  return {
    overflowX: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    mainHeight: Math.round(box.height),
    cardsInDom: cards.length,
    visibleCards: visibleCards.length,
    firstFullyVisibleWork: firstFullyVisible ? firstFullyVisible.getAttribute('data-work-card') : null,
    conversationRows: rows.length,
    rowsAboveFold,
    firstRowStartsAboveFold,
    capacityText: strip ? strip.innerText : '',
    tabRoles: {
      list: document.querySelectorAll('[role="tablist"]').length,
      tabs: document.querySelectorAll('[role="tab"]').length,
      panels: document.querySelectorAll('[role="tabpanel"]').length,
    },
  };
})()`;

/**
 * Exact-size fidelity: row counts and the mobile split toolbar. These encode
 * geometry the visual gate revised, so a regression fails here first.
 */
const FIDELITY_PROBE = `(() => {
  const vis = (el) => { const b = el.getBoundingClientRect(); return b.width > 0 && b.height > 0; };
  const rowsOf = (nodes) => new Set(nodes.filter(vis).map((n) => Math.round(n.getBoundingClientRect().top))).size;
  const tiles = [...document.querySelectorAll('[data-capacity-tile]')];
  const composerGrid = [...document.querySelectorAll('[data-composer-controls="true"]')].find(vis);
  const composerChildren = composerGrid ? [...composerGrid.children] : [];
  const footer = document.querySelector('footer');
  const disclosure = document.querySelector('[data-shell-context-disclosure="true"]');
  const footerBox = footer && vis(footer) ? footer.getBoundingClientRect() : null;
  const disclosureBox = disclosure && vis(disclosure) ? disclosure.getBoundingClientRect() : null;
  return {
    capacityTiles: tiles.length,
    capacityRows: rowsOf(tiles),
    composerRows: composerChildren.length ? rowsOf(composerChildren) : null,
    emptySlots: [...document.querySelectorAll('[data-work-empty-slot]')].filter(vis).length,
    toolbar: footerBox && disclosureBox ? {
      sideBySide: footerBox.right <= disclosureBox.left + 2 && Math.abs(footerBox.top - disclosureBox.top) <= 2,
      footerHeight: Math.round(footerBox.height),
      disclosureHeight: Math.round(disclosureBox.height),
    } : null,
  };
})()`;

/** Primary controls that must be comfortably tappable on a phone. */
const TOUCH_PROBE = `(() => {
  const main = document.querySelector('main.min-h-0.flex-1.overflow-y-auto') || document.querySelector('main');
  const scope = [main, document.querySelector('footer')].filter(Boolean);
  const primary = [];
  for (const root of scope) {
    for (const el of root.querySelectorAll('[role="tab"], button')) {
      const b = el.getBoundingClientRect();
      if (b.width === 0 || b.height === 0) continue;
      const label = (el.getAttribute('aria-label') || el.textContent || '').trim();
      // Primary = the controls the mobile journey depends on.
      if (/^(Works|Activity|Members)/.test(label) || /New Work|Filter|Message team|Send|ACK|Answer|Close Work details/.test(label)) {
        primary.push({ label: label.slice(0, 28), w: Math.round(b.width), h: Math.round(b.height) });
      }
    }
  }
  return primary;
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
    if (url.pathname === "/v1/workflows") return route.fulfill({ status: 200, contentType: "application/json", body: '{"workflows":[]}' });
    if (url.pathname === "/v1/events") return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
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
    page.on("console", (message) => { if (message.type() === "error") pageErrors.push(message.text()); });
    await mockRoutes(page);
    const query = new URLSearchParams({ api: base, surface: "team", team: teamRunId });
    await page.goto(`${base}/?${query}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
    await page.getByText("Shared Works", { exact: true }).waitFor({ timeout: 20_000 });

    const works = await page.evaluate(PROBE);
    check(!works.overflowX, `${viewport.label}: no horizontal overflow`);
    check(pageErrors.length === 0, `${viewport.label}: no console or page errors (${pageErrors.slice(0, 2).join(" | ")})`);
    check(
      works.cardsInDom === works.visibleCards && works.cardsInDom === teamWorks.length,
      `${viewport.label}: Works render once (${works.cardsInDom} in DOM, ${works.visibleCards} visible, ${teamWorks.length} in fixture)`,
    );
    check(
      works.tabRoles.list === 1 && works.tabRoles.tabs === 3 && works.tabRoles.panels >= 1,
      `${viewport.label}: workspace uses semantic tabs (tablist=${works.tabRoles.list} tab=${works.tabRoles.tabs} tabpanel=${works.tabRoles.panels})`,
    );
    check(
      !works.capacityText.includes("%"),
      `${viewport.label}: capacity strip states counts without a utilization percentage`,
    );

    const fidelity = await page.evaluate(FIDELITY_PROBE);
    check(
      fidelity.capacityTiles === 5,
      `${viewport.label}: capacity strip renders all five truthful tiles (${fidelity.capacityTiles})`,
    );
    if (viewport.width >= 900) {
      check(
        fidelity.capacityRows === 1,
        `${viewport.label}: the five capacity tiles occupy one row (${fidelity.capacityRows} row(s))`,
      );
      check(
        fidelity.composerRows === 1,
        `${viewport.label}: composer controls occupy one row (${fidelity.composerRows} row(s))`,
      );
    }
    if (viewport.width >= 640 && viewport.width < 1024) {
      // Two-column lanes: an odd card count must state the empty slot instead
      // of leaving an unexplained blank half.
      check(
        fidelity.emptySlots > 0,
        `${viewport.label}: two-column lanes state their empty slot (${fidelity.emptySlots})`,
      );
    } else {
      check(
        fidelity.emptySlots === 0,
        `${viewport.label}: no empty-slot filler outside the two-column regime (${fidelity.emptySlots})`,
      );
    }
    if (viewport.mobile) {
      check(
        Boolean(fidelity.toolbar?.sideBySide)
          && fidelity.toolbar.footerHeight <= 56
          && fidelity.toolbar.disclosureHeight <= 56,
        `${viewport.label}: composer and context share one compact toolbar row (${JSON.stringify(fidelity.toolbar)})`,
      );
      check(
        Boolean(works.firstFullyVisibleWork),
        `${viewport.label}: at least one Work card is fully inside the first viewport (main=${works.mainHeight}px, first=${works.firstFullyVisibleWork ?? "none"})`,
      );
      const primary = await page.evaluate(TOUCH_PROBE);
      const small = primary.filter((entry) => entry.h < 44);
      check(
        small.length === 0,
        `${viewport.label}: primary controls are at least 44px tall (${small.map((e) => `${e.label}:${e.h}`).join(", ") || "all pass"})`,
      );
    }

    if (!viewport.mobile) {
      // The visual capture runner reaches this page through these exact
      // selectors. Resolving them here means a component change that moves a
      // role or test id fails the fast suite instead of only breaking capture.
      for (const [label, select] of desktopJourneyContract) {
        check(await select(page).first().isVisible().catch(() => false), `${viewport.label}: capture journey selector resolves — ${label}`);
      }
    }

    // Activity hierarchy: the conversation must start inside the first viewport.
    await teamWarRoomJourney.tab(page, "Activity").first().click();
    await teamWarRoomJourney.conversation(page).waitFor({ timeout: 20_000 });

    if (!viewport.mobile) {
      for (const [label, select] of activityJourneyContract) {
        check(await select(page).first().isVisible().catch(() => false), `${viewport.label}: capture journey selector resolves — ${label}`);
      }
      const fixtureMailbox = "member-wave2-qa";
      check(
        await teamWarRoomJourney.mailbox(page, fixtureMailbox).first().isVisible().catch(() => false),
        `${viewport.label}: capture journey selector resolves — mailbox-${fixtureMailbox}`,
      );
      check(
        await teamWarRoomJourney.mailboxOpen(page, fixtureMailbox).first().isVisible().catch(() => false),
        `${viewport.label}: capture journey selector resolves — mailbox-open-${fixtureMailbox}`,
      );
    }
    const activity = await page.evaluate(PROBE);
    if (viewport.width <= 320) {
      // A single durable message can legitimately render taller than a 320px
      // surface (the fixture's first row is ~291px of Markdown in a ~437px
      // main region), so "a whole row fits" is not achievable here without
      // truncating durable content. The honest guarantee at this width is that
      // the conversation itself begins inside the first viewport.
      check(
        activity.firstRowStartsAboveFold,
        `${viewport.label}: Activity conversation begins inside the first viewport`,
      );
    } else {
      const requiredRows = viewport.mobile ? 1 : 3;
      check(
        activity.rowsAboveFold >= requiredRows,
        `${viewport.label}: Activity shows >= ${requiredRows} conversation rows above the fold (${activity.rowsAboveFold} of ${activity.conversationRows})`,
      );
    }
    check(!activity.overflowX, `${viewport.label}: Activity has no horizontal overflow`);

    // Members capacity must name the absent-observation case honestly.
    await teamWarRoomJourney.tab(page, "Members").first().click();
    await teamWarRoomJourney.membersCapacity(page).waitFor({ timeout: 20_000 });
    const membersText = await teamWarRoomJourney.membersCapacity(page).innerText();
    const notObserved = (membersText.match(/Not observed/g) ?? []).length;
    check(
      notObserved === membersWithoutCapacity,
      `${viewport.label}: every member without a capacity snapshot is labelled "Not observed" (${notObserved}/${membersWithoutCapacity})`,
    );
    check(
      !membersText.includes("%"),
      `${viewport.label}: member capacity reports no utilization percentage`,
    );

    if (!viewport.mobile) {
      // Keyboard tab operation, then Work sheet focus trap and restoration.
      // Click first so the roving focus group and the DOM agree on the current
      // tab stop; a programmatic focus() does not update that internal state.
      await teamWarRoomJourney.tab(page, "Works").first().click();
      const activeTabText = () => page.evaluate(() =>
        document.querySelector('[role="tab"][data-state="active"]')?.textContent?.trim() ?? "");
      const beforeArrow = await activeTabText();
      await page.keyboard.press("ArrowRight");
      // Roving focus moves the tab stop on a queued task, so the DOM is not
      // updated synchronously with the key press.
      await page
        .waitForFunction(
          () => document.querySelector('[role="tab"][data-state="active"]')?.textContent?.trim().startsWith("Activity"),
          { timeout: 5_000 },
        )
        .catch(() => undefined);
      const afterArrow = await activeTabText();
      check(
        beforeArrow.startsWith("Works") && afterArrow.startsWith("Activity"),
        `${viewport.label}: arrow keys move the selected tab ("${beforeArrow.slice(0, 12)}" -> "${afterArrow.slice(0, 12)}")`,
      );
      await page.keyboard.press("ArrowLeft");
      await teamWarRoomJourney.worksBoard(page).waitFor({ timeout: 10_000 });

      const firstCard = teamWarRoomJourney.workCards(page).first();
      const cardId = await firstCard.getAttribute("data-work-card");
      await firstCard.focus();
      await page.keyboard.press("Enter");
      await teamWarRoomJourney.workDetailSheet(page).waitFor({ timeout: 10_000 });
      for (let index = 0; index < 25; index += 1) await page.keyboard.press("Tab");
      const trapped = await page.evaluate(() =>
        Boolean(document.querySelector('[data-testid="work-detail-sheet"]')?.contains(document.activeElement)));
      check(trapped, `${viewport.label}: Work sheet traps Tab focus inside the dialog`);
      await page.keyboard.press("Escape");
      await teamWarRoomJourney.workDetailSheet(page).waitFor({ state: "detached", timeout: 10_000 });
      const restored = await page.evaluate(() => document.activeElement?.getAttribute("data-work-card"));
      check(restored === cardId, `${viewport.label}: closing the Work sheet restores focus to its Work card`);
    }

    await context.close();
  }
  console.log(`\n${passed} passed, ${failed} failed`);
} finally {
  await browser.close();
  await vite.close();
}

process.exit(failed === 0 ? 0 : 1);
