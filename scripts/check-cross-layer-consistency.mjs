#!/usr/bin/env node
// Cross-layer consistency check: skill ↔ code CONTRACT prompt ↔ plugin manifests
// Exit 0 when consistent, 1 when gaps found.

import { readFileSync, existsSync, readdirSync } from "node:fs";
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
const mainModuleRoot = join(ROOT, "crates/firm-cli/src/main_modules");
const collaborationRoot = join(ROOT, "crates/firm-cli/src/collaboration");
const mainRs = [
  read(join(ROOT, "crates/firm-cli/src/main.rs")),
  ...readdirSync(mainModuleRoot)
    .filter((name) => name.endsWith(".rs"))
    .sort()
    .map((name) => read(join(mainModuleRoot, name))),
  ...readdirSync(collaborationRoot)
    .filter((name) => name.endsWith(".rs"))
    .sort()
    .map((name) => read(join(collaborationRoot, name))),
].filter(Boolean).join("\n");
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
      fail(`"${inv.name}" MISSING from the CLI CONTRACT prompt`);
    }
  }
} else {
  fail("Cannot read the CLI command source or shared-references SKILL.md");
}

// ── Rule 2: Mission + Wave are Legacy read/export-only (DOC-108) ─────────
console.log("\nRule 2: Mission + Wave retired to Legacy read-only (DOC-108); no writers remain");
const missionSchema = read(join(ROOT, "schemas/mission.schema.json"));
const waveSchema = read(join(ROOT, "schemas/wave.schema.json"));
const schemasReadme = read(join(ROOT, "schemas/README.md"));
const currentMissionFixture = read(join(ROOT, "schemas/fixtures/mission/valid/basic.json"));
const legacyMissionFixture = read(join(ROOT, "schemas/fixtures/mission/valid/legacy-wave-ids.json"));

if (!missionSchema || !waveSchema || !schemasReadme || !currentMissionFixture || !legacyMissionFixture) {
  fail("Cannot read Mission/Wave compatibility contracts");
} else {
  try {
    const mission = JSON.parse(missionSchema);
    const wave = JSON.parse(waveSchema);
    const currentFixture = JSON.parse(currentMissionFixture);
    const legacyFixture = JSON.parse(legacyMissionFixture);
    const waveIds = mission.properties?.wave_ids;

    if (waveIds?.deprecated !== true || waveIds?.readOnly !== true ||
        waveIds?.["x-agentfirm-authority"] !== "legacy-historical-read-only") {
      fail("Mission.wave_ids is not marked deprecated Legacy read-only compatibility");
    } else {
      ok("Mission.wave_ids is explicitly Legacy read-only compatibility");
    }

    const missionIsLegacy = mission.deprecated === true && mission.readOnly === true &&
      mission["x-agentfirm-authority"] === "legacy-historical-read-only" &&
      mission["x-agentfirm-new-writes"] === false;
    if (!missionIsLegacy) {
      fail("Mission schema is not fully fenced as Legacy historical read-only (DOC-108)");
    } else {
      ok("Mission is Legacy read/export-only with no writer surface");
    }

    if (Object.hasOwn(currentFixture, "wave_ids")) {
      fail("current Mission fixture still writes wave_ids");
    } else if (!Array.isArray(legacyFixture.wave_ids) || legacyFixture.wave_ids.length === 0) {
      fail("Legacy Mission fixture no longer proves wave_ids read compatibility");
    } else {
      ok("current Mission fixture omits wave_ids; Legacy fixture preserves read compatibility");
    }

    const waveIsLegacy = wave.deprecated === true && wave.readOnly === true &&
      wave["x-agentfirm-authority"] === "legacy-historical-read-only" &&
      wave["x-agentfirm-new-writes"] === false &&
      wave["x-agentfirm-new-gates"] === false;
    if (!waveIsLegacy) {
      fail("Wave schema is not fully fenced as Legacy historical read-only");
    } else if (!wave.properties?.gate_status) {
      fail("Wave schema lost historical gate fields needed for compatibility validation");
    } else {
      ok("Wave is Legacy-only while historical gate rows remain validation-compatible");
    }

    const [beforeLegacy, legacySchemas = ""] = schemasReadme.split("## Legacy historical compatibility");
    const currentRegistry = beforeLegacy.split("## Current schema registry")[1] ?? "";
    if (currentRegistry.includes("[mission.schema.json]") || currentRegistry.includes("[wave.schema.json]")) {
      fail("schemas README lists Mission/Wave in the current schema registry");
    } else if (!legacySchemas.includes("[mission.schema.json]") || !legacySchemas.includes("[wave.schema.json]")) {
      fail("schemas README no longer documents the Mission/Wave compatibility schemas");
    } else {
      ok("schemas README keeps Mission/Wave in the Legacy historical compatibility section");
    }
  } catch (error) {
    fail(`Mission/Wave compatibility contract is invalid JSON: ${error.message}`);
  }
}

// Rule 3: Plugin manifest must not reference retired concepts.
console.log("\nRule 3: Plugin manifest (no Wave, no Plan Gate as feature)");
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

// ── Rule 4: Member skill matches CONTRACT prompt for key operations ─────
console.log("\nRule 4: Member skill ↔ CONTRACT prompt (key operations)");
const memberSkill = read(join(ROOT, "skills/collaborate-as-agent-team-member/SKILL.md"));

if (memberSkill && mainRs) {
  const ops = [
    { name: "work start", skill: "member work start", code: "member work start" },
    { name: "work submit", skill: "member work submit", code: "member work submit" },
    { name: "inbox read", skill: "member inbox", code: "member inbox" },
    {
      name: "send message",
      skill: "authenticated[\\s\\S]{0,80}Member Role Action",
      code: "authenticated Member Role Action",
    },
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
console.log("\nRule 5: four-layer development governance stays aligned");
const rootAgents = read(join(ROOT, "AGENTS.md"));
const operations = read(join(ROOT, "docs/current/operations/operations.md"));
const operatingRules = read(join(ROOT, "docs/current/product/agent-operating-rules.md"));
const developmentSkill = read(join(ROOT, ".agents/skills/agentfirm-development-loop/SKILL.md"));
const brainContract = read(join(ROOT, ".agents/skills/agentfirm-development-loop/references/brain.md"));
const reviewerContract = read(join(ROOT, ".agents/skills/agentfirm-development-loop/references/reviewer.md"));
const notionContract = read(join(ROOT, ".agents/skills/agentfirm-development-loop/references/notion.md"));

const governanceContracts = [
  ["AGENTS four layers", rootAgents, /Intent[\s\S]*Task[\s\S]*Acceptance[\s\S]*Feedback/],
  ["AGENTS Observer boundary", rootAgents, /Observer[\s\S]*never[\s\S]*(edits the artifact|owns Task status|replaces Reviewer)/],
  ["Skill Issue Pool batching", developmentSkill, /Issue does not automatically create a Task/],
  ["Brain bounded hot-fix", brainContract, /prevents the[\s\S]*run[\s\S]*invalidates its evidence[\s\S]*authority[\s\S]*boundary/],
  ["Reviewer scope boundary", reviewerContract, /out-of-scope finding is a non-blocking suggestion/],
  ["Notion Cancelled state", notionContract, /`Cancelled`[\s\S]*never means Pass/],
];

for (const [name, content, pattern] of governanceContracts) {
  if (content && pattern.test(content)) ok(name);
  else fail(`${name} is missing or drifted`);
}

for (const [name, content] of [
  ["operations", operations],
  ["agent operating rules", operatingRules],
]) {
  if (/dogfood remains suspended for repository repair|execution is suspended for repository repair/i.test(content ?? "")) {
    fail(`${name} still suspends repository progress behind dogfood admission`);
  } else {
    ok(`${name} has no global dogfood suspension`);
  }
}

console.log();
if (process.exitCode) {
  console.error("Consistency check FAILED — fix the gaps above.");
} else {
  console.log("Cross-layer consistency check PASSED.");
}
process.exit(process.exitCode || 0);
