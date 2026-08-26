#!/usr/bin/env node
import assert from "node:assert/strict";

import {
  parseTrustOperationJsonl,
  verifyCanonicalTrustLedger,
  verifyCanonicalTrustLedgerJsonl,
} from "./lib/agent-team-trust-ledger.mjs";

const workId = "work-624";
const reportId = "work-report:624";
const reviewMessageId = "message:review-624";
const acceptanceEventId = "trust-event-624";
const candidate = "a".repeat(40);
const candidateFingerprint = "sha256:candidate-624";

const evidence = {
  scenario_class: "coding_dogfood",
  team: {
    agent_team_id: "team-624",
    team_run_id: "team-run-624",
    host_agent_member_id: "host-624",
    implementer_agent_member_id: "implementer-624",
  },
  revision: { base: "b".repeat(40), candidate, changed_files: ["scripts/example.mjs"], check_refs: ["node check"] },
  work: {
    work_id: workId,
    work_report_id: reportId,
    review_message_id: reviewMessageId,
    reviewer_agent_member_id: "reviewer-624",
    acceptance_event_id: acceptanceEventId,
    acceptance_actor_agent_member_id: "host-624",
    accepted_version: 4,
    phase: "closed",
    resolution: "accepted",
  },
  sessions: [
    {
      agent_member_id: "host-624",
      provider: "codex",
      agent_session_id: "agent-session:host-624:1",
      native_session_id: "native-host-624",
      tool_started: 1,
      tool_terminal: 1,
    },
    {
      agent_member_id: "implementer-624",
      provider: "claude",
      agent_session_id: "agent-session:implementer-624:1",
      native_session_id: "native-implementer-624",
      tool_started: 1,
      tool_terminal: 1,
    },
  ],
};

function envelope(event, resultingProjection, commandName = "fixture") {
  return {
    execution_space_id: "space-624",
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
        id: "report-created-624",
        aggregate_kind: "work_report",
        aggregate_id: reportId,
        transition: "created",
        resulting_version: 1,
        performed_by_actor: { kind: "agent_member", id: "implementer-624" },
        payload: {},
      },
      {
        id: reportId,
        work_id: workId,
        work_revision: 3,
        authored_by: { kind: "agent_member", id: "implementer-624" },
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
        team_run_id: "team-run-624",
        sender_agent_member_id: "reviewer-624",
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
        performed_by_actor: { kind: "agent_member", id: "host-624" },
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

function expectRejected(name, records, mutateEvidence = () => {}) {
  const candidateEvidence = clone(evidence);
  mutateEvidence(candidateEvidence);
  const failures = verifyCanonicalTrustLedger(candidateEvidence, records);
  assert.ok(failures.length > 0, `${name}: expected rejection`);
}

const valid = validRecords();
assert.deepEqual(verifyCanonicalTrustLedger(evidence, valid), []);
assert.deepEqual(verifyCanonicalTrustLedgerJsonl(evidence, `${valid.map(JSON.stringify).join("\n")}\n\n`), []);
assert.equal(parseTrustOperationJsonl("\n\r\n").length, 0);

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

assert.match(verifyCanonicalTrustLedgerJsonl(evidence, "{not-json\n")[0], /line 1 is malformed JSON/u);
assert.match(verifyCanonicalTrustLedgerJsonl(evidence, "[]\n")[0], /line 1 must contain a JSON object/u);
assert.match(
  verifyCanonicalTrustLedgerJsonl(evidence, '{"operation":{}}\n')[0],
  /no canonical operation\.event envelope/u,
);

console.log("agent team trust-ledger validator PASS: valid ledger and 21 adversarial cases");
