#!/usr/bin/env node
import assert from "node:assert/strict";

import {
  parseTrustOperationJsonl,
  verifyCanonicalTrustLedger,
  verifyCanonicalTrustLedgerJsonl,
} from "./lib/agent-team-trust-ledger.mjs";

const workId = "work-fixture";
const reportId = "work-report-fixture";
const reviewMessageId = "message-fixture";
const acceptanceEventId = "trust-event-fixture";
const executionSpaceId = "space-fixture";
const candidate = "a".repeat(40);
const candidateFingerprint = "sha256:candidate-fixture";

const evidence = {
  scenario_class: "coding_dogfood",
  team: {
    agent_team_id: "team-fixture",
    team_run_id: "team-run-fixture",
    host_agent_member_id: "host-fixture",
    implementer_agent_member_id: "member-fixture",
  },
  revision: { base: "b".repeat(40), candidate, changed_files: ["scripts/example.mjs"], check_refs: ["node check"] },
  work: {
    work_id: workId,
    work_report_id: reportId,
    review_message_id: reviewMessageId,
    reviewer_agent_member_id: "host-fixture",
    acceptance_event_id: acceptanceEventId,
    acceptance_actor_agent_member_id: "host-fixture",
    accepted_version: 4,
    phase: "closed",
    resolution: "accepted",
  },
  sessions: [
    {
      agent_member_id: "host-fixture",
      provider: "codex",
      agent_session_id: "agent-session-host",
      native_session_id: "native-host",
      tool_started: 1,
      tool_terminal: 1,
    },
    {
      agent_member_id: "member-fixture",
      provider: "codex",
      agent_session_id: "agent-session-member",
      native_session_id: "native-member",
      tool_started: 1,
      tool_terminal: 1,
    },
  ],
};

function envelope(event, resultingProjection, commandName = "fixture") {
  return {
    execution_space_id: executionSpaceId,
    authenticated_actor_kind: "service",
    authenticated_actor_id: "fixture",
    command_name: commandName,
    operation: { event, resulting_projection: resultingProjection, immutable_side_records: [], initial_outbox_records: [] },
  };
}

function nativeBinding(session) {
  const native = {
    provider: session.provider,
    execution_mode: `${session.provider}_managed`,
    native_session_id: session.native_session_id,
    native_locator_kind: "provider_native",
    adapter_contract_version: "fixture-v1",
    availability: "available",
    supports_resume: true,
  };
  return envelope(
    {
      id: `bind:${session.agent_session_id}`,
      aggregate_kind: "agent_session",
      aggregate_id: session.agent_session_id,
      transition: "native_session_bound",
      resulting_version: 4,
      performed_by_actor: { kind: "service", id: "node-daemon:fixture" },
      payload: { session_id: session.agent_session_id, native_session_ref: native },
    },
    {
      id: session.agent_session_id,
      agent_member_id: session.agent_member_id,
      provider_kind: session.provider,
      native_session_ref: native,
      version: 4,
    },
    "runtime_fabric.session_native_session.bind",
  );
}

function validRecords() {
  return [
    envelope(
      {
        id: "report-created-fixture",
        aggregate_kind: "work_report",
        aggregate_id: reportId,
        transition: "created",
        resulting_version: 1,
        performed_by_actor: { kind: "agent_member", id: "member-fixture" },
        payload: {},
      },
      {
        id: reportId,
        work_id: workId,
        work_revision: 3,
        authored_by: { kind: "agent_member", id: "member-fixture" },
        candidate: { kind: "git_commit", value: candidate },
        candidate_fingerprint: candidateFingerprint,
      },
      "work_report.create",
    ),
    envelope(
      {
        id: reviewMessageId,
        aggregate_kind: "message",
        aggregate_id: reviewMessageId,
        transition: "authored",
        resulting_version: 1,
        performed_by_actor: { kind: "service", id: "node-daemon:fixture" },
        payload: {},
      },
      {
        id: reviewMessageId,
        work_id: workId,
        team_run_id: "team-run-fixture",
        sender_agent_member_id: "host-fixture",
        body: "REVIEW_RESULT\nVerdict: Pass",
      },
      "runtime.authormessage.effect",
    ),
    envelope(
      {
        id: acceptanceEventId,
        aggregate_kind: "work",
        aggregate_id: workId,
        transition: "accepted",
        resulting_version: 4,
        performed_by_actor: { kind: "agent_member", id: "host-fixture" },
        payload: { work_report_id: reportId, candidate_fingerprint: candidateFingerprint },
      },
      { id: workId, version: 4, phase: "closed", resolution: "accepted" },
      "work.accept",
    ),
    ...evidence.sessions.map(nativeBinding),
    envelope(
      { id: "unrelated", aggregate_kind: "message", aggregate_id: "message:other", transition: "authored" },
      { id: "message:other" },
    ),
  ];
}

function clone(value) {
  return structuredClone(value);
}

let rejectedCases = 0;

function expectRejected(name, records, mutateEvidence = () => {}, expectedFailure = null) {
  const candidateEvidence = clone(evidence);
  mutateEvidence(candidateEvidence);
  const failures = verifyCanonicalTrustLedger(candidateEvidence, records, executionSpaceId);
  assert.ok(failures.length > 0, `${name}: expected rejection`);
  if (expectedFailure) {
    assert.match(failures.join("\n"), expectedFailure, `${name}: wrong rejection reason`);
  }
  rejectedCases += 1;
}

const valid = validRecords();
const validJsonl = `${valid.map(JSON.stringify).join("\n")}\n`;
assert.deepEqual(verifyCanonicalTrustLedger(evidence, valid, executionSpaceId), []);
assert.deepEqual(verifyCanonicalTrustLedgerJsonl(evidence, validJsonl, executionSpaceId), []);
assert.deepEqual(verifyCanonicalTrustLedgerJsonl(evidence, `${validJsonl}{"incomplete":`, executionSpaceId), []);
assert.deepEqual(
  verifyCanonicalTrustLedgerJsonl(evidence, `${valid.map(JSON.stringify).join("\r\n")}\r\n`, executionSpaceId),
  [],
);
assert.deepEqual(parseTrustOperationJsonl(""), []);
assert.deepEqual(parseTrustOperationJsonl(JSON.stringify(valid[0])), []);
const unrelatedForeignSpace = clone(valid);
unrelatedForeignSpace.at(-1).execution_space_id = "space-unrelated";
assert.deepEqual(verifyCanonicalTrustLedger(evidence, unrelatedForeignSpace, executionSpaceId), []);
assert.match(
  verifyCanonicalTrustLedger(evidence, valid, "")[0],
  /expected execution_space_id must be a non-empty string/u,
);

expectRejected("missing WorkReport", valid.filter((record) => record.command_name !== "work_report.create"));
expectRejected("wrong WorkReport", valid, (value) => { value.work.work_report_id = "work-report:wrong"; });
expectRejected("duplicate WorkReport", [...valid, clone(valid[0])]);
expectRejected("wrong Work", clone(valid).map((record, index) => {
  if (index === 0) record.operation.resulting_projection.work_id = "work:wrong";
  return record;
}));
expectRejected("missing review Message", valid.filter((record) => record.command_name !== "runtime.authormessage.effect"));
expectRejected("wrong review Message", valid, (value) => { value.work.review_message_id = "message:wrong"; });
expectRejected("duplicate review Message", [...valid, clone(valid[1])]);
expectRejected("non-Pass review Message", clone(valid).map((record, index) => {
  if (index === 1) record.operation.resulting_projection.body = "Verdict: Changes Required";
  return record;
}));
expectRejected("noncanonical Pass review Message", clone(valid).map((record, index) => {
  if (index === 1) record.operation.resulting_projection.body = "Looks good\nVerdict: Pass";
  return record;
}), () => {}, /canonical REVIEW_RESULT/u);
expectRejected("negated Pass review Message", clone(valid).map((record, index) => {
  if (index === 1) record.operation.resulting_projection.body = "REVIEW_RESULT\nVerdict: Pass\nThis is not Pass";
  return record;
}), () => {}, /canonical REVIEW_RESULT/u);
expectRejected("mixed review verdicts", clone(valid).map((record, index) => {
  if (index === 1) {
    record.operation.resulting_projection.body = "REVIEW_RESULT\nVerdict: Pass\nVerdict: Changes Required";
  }
  return record;
}), () => {}, /canonical REVIEW_RESULT/u);
expectRejected("missing acceptance", valid.filter((record) => record.command_name !== "work.accept"));
expectRejected("wrong acceptance", valid, (value) => { value.work.acceptance_event_id = "trust-event:wrong"; });
expectRejected("duplicate acceptance", [...valid, clone(valid[2])]);
expectRejected("wrong identity", valid, (value) => { value.work.acceptance_actor_agent_member_id = "member:wrong"; });
expectRejected("wrong Work version", valid, (value) => { value.work.accepted_version = 5; });
expectRejected("wrong AgentSession member", valid, (value) => { value.sessions[0].agent_member_id = "member:wrong"; });
expectRejected("wrong session id", valid, (value) => { value.sessions[0].agent_session_id = "agent-session:wrong"; });
expectRejected("wrong native session", valid, (value) => { value.sessions[0].native_session_id = "native:wrong"; });
expectRejected("wrong provider", valid, (value) => { value.sessions[0].provider = "wrong-provider"; });
expectRejected("duplicate native binding", [...valid, clone(valid[3])]);
expectRejected("duplicate Session member", valid, (value) => { value.sessions[1].agent_member_id = value.sessions[0].agent_member_id; });
expectRejected("missing execution space", clone(valid).map((record, index) => {
  if (index === 0) record.execution_space_id = "";
  return record;
}), () => {}, /no non-empty execution_space_id/u);
expectRejected("foreign execution space substitution", clone(valid).map((record, index) => {
  if (index === 3) record.execution_space_id = "space-foreign";
  return record;
}), () => {}, /execution_space_id mismatch/u);
expectRejected("all-foreign execution space substitution", clone(valid).map((record, index) => {
  if (index < valid.length - 1) record.execution_space_id = "space-foreign";
  return record;
}), () => {}, /execution_space_id mismatch/u);
expectRejected("transcript mirror field", clone(valid).map((record, index) => {
  if (index === valid.length - 1) record.operation.resulting_projection.tool_calls = [];
  return record;
}), () => {}, /forbidden transcript-mirror field/u);
expectRejected("transcript mirror aggregate", [
  ...valid,
  envelope(
    { id: "provider-event-fixture", aggregate_kind: "provider_transcript", aggregate_id: "native-member", transition: "appended" },
    { id: "provider-event-fixture", text: "mirrored provider output" },
  ),
], () => {}, /forbidden transcript-mirror aggregate/u);

assert.match(verifyCanonicalTrustLedgerJsonl(evidence, "{not-json\n", executionSpaceId)[0], /line 1 is malformed JSON/u);
assert.match(verifyCanonicalTrustLedgerJsonl(evidence, `${validJsonl}{not-json\n`, executionSpaceId)[0], /line 7 is malformed JSON/u);
assert.match(verifyCanonicalTrustLedgerJsonl(evidence, "\n", executionSpaceId)[0], /line 1 is malformed JSON/u);
assert.match(verifyCanonicalTrustLedgerJsonl(evidence, "[]\n", executionSpaceId)[0], /line 1 must contain a JSON object/u);
assert.match(
  verifyCanonicalTrustLedgerJsonl(evidence, '{"operation":{}}\n', executionSpaceId)[0],
  /no canonical operation\.event envelope/u,
);

console.log(`agent team trust-ledger validator PASS: valid ledger and ${rejectedCases} adversarial cases`);
