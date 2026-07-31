#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const repo = process.cwd();

const suiteSkills = [
  "company-business-project-bootstrap",
  "company-docs-operator",
  "company-work-operator",
  "company-finance-operator",
  "company-org-operator",
  "company-module-designer",
  "company-page-builder",
  "dogfood-company-os",
  "connect-github-company-os",
];

const operatorSkills = [
  "company-docs-operator",
  "company-work-operator",
  "company-finance-operator",
  "company-org-operator",
];

const failures = [];

expectSuiteSize();

function expectSuiteSize() {
  if (suiteSkills.length !== 9) {
    failures.push(`company-os suite must contain exactly 9 skills, found ${suiteSkills.length}`);
  }
}

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
expect(
  acceptance.includes("for d in .claude/skills .agents/skills"),
  "acceptance-skill-install.sh must validate both Claude and Codex targets",
);
expect(
  acceptance.includes("company-os suite rejects a missing delegated Skill"),
  "acceptance-skill-install.sh must reject a missing delegated Skill",
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

const organization = read("docs/company-os/organization-and-actors.md");
for (const template of ["company_lead", "domain_lead", "execution_member"]) {
  expect(organization.includes(`| \`${template}\` |`), `organization-and-actors.md missing fixed role template ${template}`);
}
for (const moduleName of ["docs", "work", "org", "github"]) {
  expect(organization.includes(`| \`${moduleName}\` |`), `organization-and-actors.md missing simple permission module ${moduleName}`);
}
for (const verb of ["read", "write", "execute", "delegate"]) {
  expect(organization.includes(`\`${verb}\``), `organization-and-actors.md missing simple permission verb ${verb}`);
}
const protectedEffects = [
  "irreversible-destructive",
  "credential-root-security",
  "material-finance-legal-external",
  "major-public-production",
  "cross-domain-root-expansion",
  "policy-unknown",
];
for (const effect of protectedEffects) {
  expect(organization.includes(`| \`protected-effect/${effect}\` |`), `organization-and-actors.md missing protected effect ${effect}`);
}
expect(
  (organization.match(/^\| `protected-effect\//gm) ?? []).length === protectedEffects.length,
  "organization-and-actors.md must contain exactly the six simple-v1 protected effects",
);
expect(
  organization.includes("Target contract, not implemented on this base")
    && governance.includes("neither proves template Store/API enforcement"),
  "simple permission v1 must remain explicitly unimplemented in Store/API",
);
expect(
  /The Runtime Supervisor transports authenticated requests[\s\S]*?It does not select role\s+templates, issue envelopes, approve Actions, or own Company priority\/capacity\./.test(governance),
  "governance-agent-workspaces.md must keep Supervisor transport separate from Company authority",
);

const dogfoodCompany = read("skills/dogfood-company-os/SKILL.md");
const hardGateStart = dogfoodCompany.indexOf("## Enforce The Company Execution Hard Gate");
const hardGateEnd = dogfoodCompany.indexOf("\n## Run One Complete Cycle", hardGateStart);
expect(hardGateStart >= 0 && hardGateEnd > hardGateStart, "dogfood-company-os missing bounded execution hard gate");
const hardGate = hardGateStart >= 0 && hardGateEnd > hardGateStart
  ? dogfoodCompany.slice(hardGateStart, hardGateEnd)
  : "";
const requiredExecutionGateEvidence = [
  "Lead-originated exact correlated Assignment",
  "Domain-Agent-owned repository commit",
  "governed Company Action",
  "correlated Handoff with checks/evidence",
  "truthful Work lifecycle and result return",
  "temporary-member closure or durable roster carry-forward",
];
for (const evidence of requiredExecutionGateEvidence) {
  expect(hardGate.includes(evidence), `dogfood-company-os execution hard gate missing ${evidence}`);
}
expect(
  requiredExecutionGateEvidence.every((evidence, index) => index === 0
    || hardGate.indexOf(evidence) > hardGate.indexOf(requiredExecutionGateEvidence[index - 1])),
  "dogfood-company-os execution hard gate evidence must preserve Assignment-to-roster order",
);
expect(
  hardGate.includes("Store projection alone is insufficient")
    && hardGate.includes("do not advance the Wave"),
  "dogfood-company-os must reject Store-projection-only Wave advance",
);
expect(
  /Mission\/Wave and\s+Agent Team remain optional execution capabilities[\s\S]*?not Company truth/.test(hardGate),
  "dogfood-company-os must keep Mission/Wave and Agent Team optional and separate from Company truth",
);
expect(
  /Human Principal[\s\S]*?Company Lead[\s\S]*?Domain Lead[\s\S]*?Do not transfer those responsibilities/.test(hardGate),
  "dogfood-company-os execution hard gate must preserve Human and Lead responsibility boundaries",
);
for (const retiredModel of ["Plan Mode", "Plan Gate", "Task Graph", "Goal object"]) {
  expect(!hardGate.includes(retiredModel), `dogfood-company-os execution hard gate reintroduced ${retiredModel}`);
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
