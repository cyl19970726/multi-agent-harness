#!/usr/bin/env node
// Agent-config single-source gate (AGENTS.md maintenance rule 14):
//  - root CLAUDE.md is a thin import of AGENTS.md;
//  - .claude/skills is a symlink resolving to .agents/skills;
//  - no real skill directories shadow the symlink under .claude/;
//  - .gitignore keeps the symlink trackable (.claude/* + !.claude/skills).

import { existsSync, lstatSync, readFileSync, readlinkSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const failures = [];

const claudeMdPath = join(repoRoot, "CLAUDE.md");
if (!existsSync(claudeMdPath)) {
  failures.push("CLAUDE.md missing at repo root; it must import AGENTS.md.");
} else {
  const body = readFileSync(claudeMdPath, "utf8");
  if (!/^@AGENTS\.md\s*$/m.test(body)) {
    failures.push("CLAUDE.md must contain the import line `@AGENTS.md`.");
  }
}

const skillsLink = join(repoRoot, ".claude", "skills");
const agentsSkills = join(repoRoot, ".agents", "skills");
if (!existsSync(join(repoRoot, ".claude"))) {
  failures.push(".claude/ directory missing; expected .claude/skills symlink.");
} else {
  let stat;
  try {
    stat = lstatSync(skillsLink);
  } catch {
    failures.push(
      ".claude/skills missing; create it with `ln -s ../.agents/skills .claude/skills`.",
    );
  }
  if (stat) {
    if (!stat.isSymbolicLink()) {
      failures.push(
        ".claude/skills must be a symlink to .agents/skills, not a real directory; " +
          "move any real skill directories into .agents/skills/ and relink.",
      );
    } else {
      const target = resolve(dirname(skillsLink), readlinkSync(skillsLink));
      if (target !== agentsSkills) {
        failures.push(
          `.claude/skills points at ${target}; expected ${agentsSkills}.`,
        );
      }
    }
  }
}

const gitignore = readFileSync(join(repoRoot, ".gitignore"), "utf8");
const lines = gitignore.split("\n").map((line) => line.trim());
if (!lines.includes(".claude/*") || !lines.includes("!.claude/skills")) {
  failures.push(
    ".gitignore must contain `.claude/*` plus `!.claude/skills` so the skills symlink stays tracked.",
  );
}

if (failures.length > 0) {
  console.error("check-agent-config-sync: FAILED");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
console.log(
  "check-agent-config-sync: OK (CLAUDE.md imports AGENTS.md; .claude/skills -> .agents/skills)",
);
