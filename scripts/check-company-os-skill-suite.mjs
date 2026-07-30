#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const repo = process.cwd();

const suiteSkills = [
  "company-business-project-bootstrap",
  "dogfood-company-os",
  "connect-github-company-os",
  "company-docs-operator",
  "company-work-operator",
  "company-finance-operator",
  "company-org-operator",
  "company-module-designer",
  "company-page-builder",
];

const operatorSkills = [
  "company-docs-operator",
  "company-work-operator",
  "company-finance-operator",
  "company-org-operator",
];

const failures = [];

function read(rel) {
  return fs.readFileSync(path.join(repo, rel), "utf8");
}

function expect(condition, message) {
  if (!condition) failures.push(message);
}

for (const skill of suiteSkills) {
  const skillDir = path.join(repo, "skills", skill);
  const skillMd = path.join(skillDir, "SKILL.md");
  const openai = path.join(skillDir, "agents", "openai.yaml");
  expect(fs.existsSync(skillMd), `${skill} is missing SKILL.md`);
  expect(fs.existsSync(openai), `${skill} is missing agents/openai.yaml`);
  if (fs.existsSync(skillMd)) {
    const text = fs.readFileSync(skillMd, "utf8");
    expect(text.includes(`name: ${skill}`), `${skill} frontmatter name mismatch`);
    expect(
      /procedural[\s\S]{0,120}not\s+product\s+authority/i.test(text),
      `${skill} must state it is procedural, not authority`,
    );
  }
}

const installer = read("scripts/install-skill.sh");
expect(installer.includes("--suite"), "install-skill.sh does not expose --suite");
expect(installer.includes("company-os"), "install-skill.sh does not define company-os suite");
for (const skill of suiteSkills) {
  expect(installer.includes(skill), `install-skill.sh company-os suite missing ${skill}`);
}

const acceptance = read("scripts/acceptance-skill-install.sh");
expect(
  acceptance.includes("--suite company-os"),
  "acceptance-skill-install.sh does not install --suite company-os",
);
for (const skill of suiteSkills) {
  expect(acceptance.includes(skill), `acceptance-skill-install.sh missing ${skill}`);
}

const skillContracts = read("docs/company-os/skill-contracts.md");
expect(
  skillContracts.includes("scripts/install-skill.sh --agent both --suite company-os"),
  "skill-contracts.md missing company-os install command",
);
for (const skill of suiteSkills) {
  expect(skillContracts.includes(`../../skills/${skill}/SKILL.md`), `skill-contracts.md missing ${skill}`);
}
expect(
  skillContracts.includes("Docs, Work, Organization, Approval, and Finance baseline dedicated CLI implemented; governed OrgChangeProposal and deeper Finance lifecycle remain planned"),
  "skill-contracts.md must record baseline Docs/Work/Org/Approval/Finance CLI implementation",
);

const readme = read("docs/company-os/README.md");
expect(readme.includes("Skill and CLI Contracts"), "Company OS README missing skill-contracts reference");
expect(readme.includes("--suite company-os"), "Company OS README missing suite install command");
expect(
  readme.includes("Dedicated Docs, Work, Organization, Approval, and Finance baseline CLI commands"),
  "Company OS README must record baseline Company OS CLI implementation",
);

const governance = read("docs/company-os/governance-agent-workspaces.md");
for (const skill of operatorSkills) {
  expect(governance.includes(`../../skills/${skill}/SKILL.md`), `governance-agent-workspaces.md missing ${skill}`);
}

const dogfood = read("skills/dogfood-company-os/SKILL.md");
for (const skill of [
  "company-docs-operator",
  "company-work-operator",
  "company-org-operator",
  "connect-github-company-os",
  "orchestrate-mission-waves",
]) {
  expect(dogfood.includes(`$${skill}`), `dogfood-company-os must route to $${skill}`);
}
expect(
  dogfood.includes("A finding is not a commitment until Work owns it."),
  "dogfood-company-os must preserve the Work commitment boundary",
);
expect(
  dogfood.includes("it is neither the Execution Space nor the owner of Company Docs"),
  "dogfood-company-os must keep Git worktree, Project Binding, Execution Space, and Company Store distinct",
);
expect(
  !dogfood.includes("A Git worktree is an Execution Space or Project Binding"),
  "dogfood-company-os must not collapse a worktree into Execution Space or Project Binding identity",
);

const github = read("skills/connect-github-company-os/SKILL.md");
for (const phrase of [
  "Company Store",
  "Project Binding",
  "Use Existing Transport First",
  "$company-docs-operator",
  "$company-work-operator",
]) {
  expect(github.includes(phrase), `connect-github-company-os missing boundary: ${phrase}`);
}
expect(
  !github.includes("MCP is required"),
  "connect-github-company-os must not require MCP when gh/Git/API is sufficient",
);

const starHarnessSync = read("scripts/sync-star-harness-plugin-skills.mjs");
for (const skill of ["dogfood-company-os", "connect-github-company-os"]) {
  expect(
    !starHarnessSync.includes(`"${skill}"`),
    `${skill} belongs to the Company OS suite, not the Star Harness execution plugin`,
  );
}

const forbiddenAsImplemented = ["OrgChangeProposal"];
for (const phrase of forbiddenAsImplemented) {
  const docsClaim = new RegExp(`${phrase.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}[^\\n]*(implemented|available|stable)`, "i");
  expect(!docsClaim.test(skillContracts), `skill-contracts.md may overclaim planned command: ${phrase}`);
}

if (failures.length) {
  console.error("Company OS skill suite check failed:");
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log(`Company OS skill suite check passed (${suiteSkills.length} skills).`);
