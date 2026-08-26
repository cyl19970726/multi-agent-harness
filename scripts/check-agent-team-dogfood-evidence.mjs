import Ajv2020 from "ajv/dist/2020.js";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { basename, join, resolve } from "node:path";

import { verifyCanonicalTrustLedgerJsonl } from "./lib/agent-team-trust-ledger.mjs";

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

function parseCliArguments(args) {
  const evidencePaths = [];
  let trustLedgerPath = null;
  let expectedExecutionSpaceId = null;
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--") continue;
    if (argument === "--trust-ledger") {
      if (trustLedgerPath !== null) throw new Error("--trust-ledger may be supplied only once");
      const value = args[index + 1];
      if (!value || value === "--" || value.startsWith("--")) {
        throw new Error("--trust-ledger requires a path");
      }
      trustLedgerPath = value;
      index += 1;
      continue;
    }
    if (argument === "--expected-execution-space-id") {
      if (expectedExecutionSpaceId !== null) {
        throw new Error("--expected-execution-space-id may be supplied only once");
      }
      const value = args[index + 1];
      if (!value || value === "--" || value.startsWith("--")) {
        throw new Error("--expected-execution-space-id requires an id");
      }
      expectedExecutionSpaceId = value;
      index += 1;
      continue;
    }
    if (argument.startsWith("--")) throw new Error(`unknown option ${argument}`);
    evidencePaths.push(argument);
  }
  return { evidencePaths, trustLedgerPath, expectedExecutionSpaceId };
}

assert.deepEqual(
  parseCliArguments(["first.json", "--", "second.json", "--", "third.json"]),
  {
    evidencePaths: ["first.json", "second.json", "third.json"],
    trustLedgerPath: null,
    expectedExecutionSpaceId: null,
  },
);
assert.deepEqual(
  parseCliArguments([
    "first.json",
    "--trust-ledger",
    "/space/agentfirm_trust_operations.jsonl",
    "--expected-execution-space-id",
    "space-fixture",
  ]),
  {
    evidencePaths: ["first.json"],
    trustLedgerPath: "/space/agentfirm_trust_operations.jsonl",
    expectedExecutionSpaceId: "space-fixture",
  },
);
assert.throws(() => parseCliArguments(["--trust-ledger"]), /requires a path/u);
assert.throws(
  () => parseCliArguments(["--trust-ledger", "one", "--trust-ledger", "two"]),
  /only once/u,
);
assert.throws(
  () => parseCliArguments(["--expected-execution-space-id"]),
  /requires an id/u,
);
assert.throws(
  () => parseCliArguments([
    "--expected-execution-space-id",
    "one",
    "--expected-execution-space-id",
    "two",
  ]),
  /only once/u,
);

function verifyTrustLedgerPath(evidence, trustLedgerPath, expectedExecutionSpaceId) {
  if (evidence.scenario_class !== "coding_dogfood") return [];
  if (!trustLedgerPath) return ["coding_dogfood requires --trust-ledger"];
  if (!expectedExecutionSpaceId) {
    return [
      "coding_dogfood requires --expected-execution-space-id from a trusted Execution Space selection",
    ];
  }

  const absoluteLedgerPath = resolve(trustLedgerPath);
  if (basename(absoluteLedgerPath) !== "agentfirm_trust_operations.jsonl") {
    return [
      "--trust-ledger must name the current Execution Space's agentfirm_trust_operations.jsonl",
    ];
  }
  try {
    return verifyCanonicalTrustLedgerJsonl(
      evidence,
      readFileSync(absoluteLedgerPath, "utf8"),
      expectedExecutionSpaceId,
    );
  } catch (error) {
    return [`trust ledger: cannot read ${trustLedgerPath}: ${error.message}`];
  }
}

function verifyManifestFixtureSuite() {
  const fixtureRoot = join(root, "fixtures/canonical-ledger");
  const manifest = readJson(join(fixtureRoot, "manifest.json"));
  const evidence = readJson(resolve(fixtureRoot, manifest.evidence_fixture));
  for (const fixtureCase of manifest.cases) {
    let jsonl = readFileSync(join(fixtureRoot, fixtureCase.path), "utf8");
    if (fixtureCase.input_transform) {
      const prefix = "append_unterminated:";
      if (!fixtureCase.input_transform.startsWith(prefix)) {
        throw new Error(`${fixtureCase.id}: unknown input_transform`);
      }
      jsonl += fixtureCase.input_transform.slice(prefix.length);
    }
    const failures = verifyCanonicalTrustLedgerJsonl(
      evidence,
      jsonl,
      manifest.execution_space_id,
    );
    if (fixtureCase.expect === "pass" && failures.length) {
      throw new Error(`${fixtureCase.id}: expected pass\n${failures.join("\n")}`);
    }
    if (
      fixtureCase.expect === "fail"
      && !failures.some((failure) => failure.includes(fixtureCase.expected_failure))
    ) {
      throw new Error(
        `${fixtureCase.id}: expected failure ${JSON.stringify(
          fixtureCase.expected_failure,
        )}\n${failures.join("\n")}`,
      );
    }
  }
  return manifest.cases.length;
}

const { evidencePaths, trustLedgerPath, expectedExecutionSpaceId } = parseCliArguments(
  process.argv.slice(2),
);
if (evidencePaths.length) {
  for (const path of evidencePaths) {
    const evidence = readJson(path);
    const failures = [
      ...verifyAgentTeamDogfoodEvidence(evidence),
      ...verifyRepositoryEvidence(evidence),
      ...verifyTrustLedgerPath(evidence, trustLedgerPath, expectedExecutionSpaceId),
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
  const coordinationFixture = readJson(join(validDir, "coordination-canary.json"));
  assert.deepEqual(
    verifyTrustLedgerPath(codingFixture, null, null),
    ["coding_dogfood requires --trust-ledger"],
  );
  assert.deepEqual(
    verifyTrustLedgerPath(codingFixture, "/space/agentfirm_trust_operations.jsonl", null),
    [
      "coding_dogfood requires --expected-execution-space-id from a trusted Execution Space selection",
    ],
  );
  assert.deepEqual(verifyTrustLedgerPath(coordinationFixture, null, null), []);
  const rejected = invalidCases(codingFixture);
  for (const [name, evidence] of rejected) {
    if (!verifyAgentTeamDogfoodEvidence(evidence).length) {
      throw new Error(`${name}: expected rejection`);
    }
  }
  const manifestCases = verifyManifestFixtureSuite();
  const validFixtures = readdirSync(validDir).length;
  const summary = [
    `agent team dogfood evidence PASS: ${validFixtures} valid fixtures,`,
    `${rejected.size} rejected variants, and ${manifestCases} canonical ledger cases`,
  ].join(" ");
  console.log(summary);
}
