import Ajv2020 from "ajv/dist/2020.js";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const root = "schemas/agent-team-dogfood";
const schema = JSON.parse(readFileSync(join(root, "evidence.schema.json"), "utf8"));
const ajv = new Ajv2020({ allErrors: true, strict: false });
const validateSchema = ajv.compile(schema);

export function verifyAgentTeamDogfoodEvidence(evidence) {
  const failures = [];
  if (!validateSchema(evidence)) {
    failures.push(ajv.errorsText(validateSchema.errors, { separator: "\n" }));
    return failures;
  }
  if (evidence.scenario_class !== "coding_dogfood") return failures;

  if (evidence.revision.base === evidence.revision.candidate) {
    failures.push("coding_dogfood requires candidate revision to differ from base revision");
  }
  const implementer = evidence.team.implementer_agent_member_id;
  if (implementer === evidence.work.reviewer_agent_member_id) {
    failures.push("coding_dogfood reviewer must be independent from the implementer");
  }
  if (evidence.work.acceptance_actor_agent_member_id !== evidence.team.host_agent_member_id) {
    failures.push("coding_dogfood acceptance must bind to the exact Team Host");
  }
  const sessionMemberIds = evidence.sessions.map((session) => session.agent_member_id);
  if (new Set(sessionMemberIds).size !== sessionMemberIds.length) {
    failures.push("coding_dogfood Session evidence must contain one row per AgentMember");
  }
  if (!sessionMemberIds.includes(evidence.team.host_agent_member_id)) {
    failures.push("coding_dogfood requires a provider-native Session for the exact Team Host");
  }
  const implementerSession = evidence.sessions.find(
    (session) => session.agent_member_id === implementer,
  );
  if (!implementerSession) {
    failures.push("coding_dogfood requires a provider-native Session for the implementer");
  } else {
    if (implementerSession.tool_started < 1) {
      failures.push("coding_dogfood requires at least one implementer tool start");
    }
    if (implementerSession.tool_terminal < 1) {
      failures.push("coding_dogfood requires at least one implementer terminal tool result");
    }
  }
  return failures;
}

function verifyRepositoryEvidence(evidence) {
  if (evidence.scenario_class !== "coding_dogfood") return [];
  const failures = [];
  for (const revision of [evidence.revision.base, evidence.revision.candidate]) {
    const result = spawnSync("git", ["cat-file", "-e", `${revision}^{commit}`], {
      encoding: "utf8",
    });
    if (result.status !== 0) failures.push(`Git cannot resolve evidence revision ${revision}`);
  }
  if (failures.length) return failures;
  const diff = spawnSync(
    "git",
    ["diff", "--name-only", evidence.revision.base, evidence.revision.candidate],
    { encoding: "utf8" },
  );
  if (diff.status !== 0) {
    return [`Git cannot compare evidence revisions: ${diff.stderr.trim()}`];
  }
  const actual = diff.stdout.split("\n").map((value) => value.trim()).filter(Boolean).sort();
  const claimed = [...evidence.revision.changed_files].sort();
  if (JSON.stringify(actual) !== JSON.stringify(claimed)) {
    failures.push(
      `changed_files do not match git diff: claimed=${JSON.stringify(claimed)} actual=${JSON.stringify(actual)}`,
    );
  }
  return failures;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function verifyPath(path, expectedValid) {
  const failures = verifyAgentTeamDogfoodEvidence(readJson(path));
  if (expectedValid && failures.length) {
    throw new Error(`${path}: expected valid\n${failures.join("\n")}`);
  }
  if (!expectedValid && !failures.length) {
    throw new Error(`${path}: expected rejection`);
  }
  return failures;
}

function invalidCases(valid) {
  const variant = (mutate) => {
    const evidence = structuredClone(valid);
    mutate(evidence);
    return evidence;
  };
  return new Map([
    ["zero changed files", variant((value) => { value.revision.changed_files = []; })],
    ["same base and candidate", variant((value) => { value.revision.candidate = value.revision.base; })],
    ["reviewer is implementer", variant((value) => {
      value.work.reviewer_agent_member_id = value.team.implementer_agent_member_id;
    })],
    ["missing WorkReport", variant((value) => { delete value.work.work_report_id; })],
    ["missing review Message", variant((value) => { delete value.work.review_message_id; })],
    ["missing Host acceptance", variant((value) => {
      delete value.work.acceptance_event_id;
      delete value.work.acceptance_actor_agent_member_id;
    })],
    ["missing implementer tool terminal", variant((value) => {
      value.sessions.find(
        (session) => session.agent_member_id === value.team.implementer_agent_member_id,
      ).tool_terminal = 0;
    })],
  ]);
}

function normalizeEvidencePaths(args) {
  return args.filter((path) => path !== "--");
}

assert.deepEqual(
  normalizeEvidencePaths(["first.json", "--", "second.json", "--", "third.json"]),
  ["first.json", "second.json", "third.json"],
);

const paths = normalizeEvidencePaths(process.argv.slice(2));
if (paths.length) {
  for (const path of paths) {
    const evidence = readJson(path);
    const failures = [
      ...verifyAgentTeamDogfoodEvidence(evidence),
      ...verifyRepositoryEvidence(evidence),
    ];
    if (failures.length) {
      console.error(`${path}:\n${failures.join("\n")}`);
      process.exitCode = 1;
    } else {
      console.log(`${path}: ${evidence.scenario_class} evidence PASS`);
    }
  }
} else {
  const validDir = join(root, "fixtures/valid");
  for (const file of readdirSync(validDir).sort()) verifyPath(join(validDir, file), true);
  const codingFixture = readJson(join(validDir, "coding-dogfood.json"));
  const rejected = invalidCases(codingFixture);
  for (const [name, evidence] of rejected) {
    if (!verifyAgentTeamDogfoodEvidence(evidence).length) {
      throw new Error(`${name}: expected rejection`);
    }
  }
  console.log(
    `agent team dogfood evidence PASS: ${readdirSync(validDir).length} valid fixtures and ${rejected.size} rejected variants`,
  );
}
