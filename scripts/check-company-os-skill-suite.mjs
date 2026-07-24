#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const repo = process.cwd();

const suiteSkills = [
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
  skillContracts.includes("Docs dedicated CLI implemented; Work/Finance/Org dedicated CLI planned"),
  "skill-contracts.md must distinguish implemented Docs CLI from planned Work/Finance/Org CLI",
);

const readme = read("docs/company-os/README.md");
expect(readme.includes("Skill and CLI Contracts"), "Company OS README missing skill-contracts reference");
expect(readme.includes("--suite company-os"), "Company OS README missing suite install command");
expect(
  readme.includes("CLI families remain planned"),
  "Company OS README must not claim Work/Finance/Org dedicated CLI is implemented",
);

const governance = read("docs/company-os/governance-agent-workspaces.md");
for (const skill of operatorSkills) {
  expect(governance.includes(`../../skills/${skill}/SKILL.md`), `governance-agent-workspaces.md missing ${skill}`);
}

const forbiddenAsImplemented = [
  "harness company work query",
  "harness company finance query",
  "harness company org query",
];
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
