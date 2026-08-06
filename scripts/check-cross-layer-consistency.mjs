#!/usr/bin/env node
// Cross-layer consistency check: skill ↔ code CONTRACT prompt ↔ plugin manifests
// Exit 0 when consistent, 1 when gaps found.

import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..");

function read(path) {
  if (!existsSync(path)) return null;
  return readFileSync(path, "utf8");
}

function fail(msg) {
  console.error(`  ✗ ${msg}`);
  process.exitCode = 1;
}

function ok(msg) {
  console.log(`  ✓ ${msg}`);
}

// ── Rule 1: CONTRACT prompt must match skill invariants ──────────────────
console.log("Rule 1: CONTRACT prompt ↔ skills/shared-references");
const mainRs = read(join(ROOT, "crates/harness-cli/src/main.rs"));
const shared = read(join(ROOT, "skills/shared-references/SKILL.md"));

if (mainRs && shared) {
  // Key invariants that must appear in both
  const invariants = [
    { name: "no Plan Gate", skill: "No Plan Mode.*No Plan Gate", code: "Harness has no Plan Gate" },
    { name: "one execution driver", skill: "one.*execution driver", code: "one.*execution.driver|execution_driver" },
    { name: "messages never change Work", skill: "Messages Never Change Work", code: "never change.*Work|message.*not.*responsibility" },
    { name: "provider session is truth", skill: "Provider-Native Session.*Sole.*Truth", code: "provider.*session.*truth|native.*session.*sole" },
  ];

  for (const inv of invariants) {
    const inSkill = new RegExp(inv.skill, "i").test(shared);
    const inCode = new RegExp(inv.code, "i").test(mainRs);
    if (inSkill && inCode) {
      ok(`"${inv.name}" in both skill and CONTRACT prompt`);
    } else if (!inSkill) {
      fail(`"${inv.name}" MISSING from shared-references SKILL.md`);
    } else {
      fail(`"${inv.name}" MISSING from CONTRACT prompt in main.rs`);
    }
  }
} else {
  fail("Cannot read main.rs or shared-references SKILL.md");
}

// ── Rule 2: Plugin manifest must not reference retired concepts ──────────
console.log("\nRule 2: Plugin manifest (no Wave, no Plan Gate as feature)");
const pluginDir = join(ROOT, "plugins/star-harness");
const manifests = [
  join(pluginDir, "kimi.plugin.json"),
  join(pluginDir, ".codex-plugin/plugin.json"),
  join(pluginDir, ".claude-plugin/plugin.json"),
];

for (const mf of manifests) {
  const name = mf.split("/").slice(-2).join("/");
  const content = read(mf);
  if (!content) { fail(`${name}: not found`); continue; }
  
  try {
    const d = JSON.parse(content);
    const desc = (d.description || "").toLowerCase();
    const iface = d.interface || {};
    const longDesc = (iface.longDescription || "").toLowerCase();
    
    if (desc.includes("mission/wave")) fail(`${name}: description says "Mission/Wave"`);
    if (longDesc.includes("mission/wave")) fail(`${name}: longDescription references Wave as object`);
    
    const keywords = d.keywords || [];
    if (keywords.includes("wave")) fail(`${name}: keywords include "wave"`);
    
    const prompts = iface.defaultPrompt || [];
    for (const p of prompts) {
      if (p.toLowerCase().includes("wave")) fail(`${name}: defaultPrompt mentions Wave`);
    }
    
    ok(`${name}: clean`);
  } catch(e) {
    fail(`${name}: invalid JSON`);
  }
}

// ── Rule 3: Member skill matches CONTRACT prompt for key operations ──────
console.log("\nRule 3: Member skill ↔ CONTRACT prompt (key operations)");
const memberSkill = read(join(ROOT, "skills/collaborate-as-agent-team-member/SKILL.md"));

if (memberSkill && mainRs) {
  const ops = [
    { name: "work start", skill: "work start", code: "team-run work start" },
    { name: "work submit", skill: "work submit", code: "team-run work submit" },
    { name: "inbox read", skill: "team-run inbox", code: "team-run inbox" },
    { name: "send message", skill: "team-run send", code: "team-run send" },
    { name: "board summary", skill: "board-summary", code: "board-summary" },
  ];

  for (const op of ops) {
    const inSkill = new RegExp(op.skill, "i").test(memberSkill);
    const inCode = new RegExp(op.code, "i").test(mainRs);
    if (inSkill && inCode) {
      ok(`"${op.name}" in both`);
    } else if (!inSkill) {
      fail(`"${op.name}" missing from member skill`);
    } else {
      fail(`"${op.name}" missing from CONTRACT prompt`);
    }
  }

  // Special: no plan mode instruction consistency
  const skillBansPlan = /Do NOT use[\s\S]*?EnterPlanMode|Do NOT use[\s\S]*?plan.mode|no Plan Mode.*Gate/i.test(memberSkill);
  const codeBansPlan = /Do NOT use EnterPlanMode, ExitPlanMode/i.test(mainRs);
  if (skillBansPlan && codeBansPlan) {
    ok("plan mode ban consistent");
  } else {
    fail("plan mode ban inconsistent between skill and CONTRACT");
  }
}

// ── Summary ──────────────────────────────────────────────────────────────
console.log();
if (process.exitCode) {
  console.error("Consistency check FAILED — fix the gaps above.");
} else {
  console.log("Cross-layer consistency check PASSED.");
}
process.exit(process.exitCode || 0);
