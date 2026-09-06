import { verifyCanonicalWorkFacts } from './agent-team-trust-ledger.mjs';

const event = (record) => record.operation.event;
const projection = (record) => record.operation.resulting_projection;
const sequence = (record) => event(record).store_sequence;
function requireFact(condition, message) { if (!condition) throw new Error(message); }
function equal(actual, expected, label) {
  requireFact(actual === expected, `${label} mismatch: expected ${JSON.stringify(expected)}, found ${JSON.stringify(actual)}`);
}
function one(rows, label) {
  requireFact(rows.length === 1, `expected exactly one ${label}, found ${rows.length}`);
  return rows[0];
}
function time(value, label) {
  const result = /^unix-ms:\d+$/u.test(value) ? Number(value.slice(8)) : Date.parse(value);
  requireFact(Number.isFinite(result), `missing or invalid ${label} timestamp`);
  return result;
}
function facts(records, predicate) {
  return records.flatMap(record => [projection(record), ...(record.operation.immutable_side_records ?? []),
    ...(record.operation.initial_outbox_records ?? [])]
    .filter(row => row && predicate(row)).map(row => ({ row, record })));
}
function latest(rows, label) {
  requireFact(rows.length > 0, `missing ${label}`);
  const sorted = [...rows].sort((a, b) => sequence(a.record) - sequence(b.record));
  return sorted.at(-1).row;
}
function membershipAt(records, id, member, team, boundary, role) {
  const rows = records.filter(r => sequence(r) <= boundary && event(r).aggregate_kind === 'team_membership'
    && event(r).aggregate_id === id);
  const row = latest(rows.map(record => ({ row: projection(record), record })), 'TeamMembership');
  equal(row.id, id, 'membership id'); equal(row.team_id, team, 'membership Team');
  equal(row.agent_member_id, member, 'membership Member'); equal(row.state, 'active', 'membership state');
  if (role) equal(row.role, role, 'membership role');
}

// Select by canonical Session identity/generation BEFORE comparing any claimed
// native ID. Store sequence defines historical boundaries; later generations do
// not invalidate a completed delivery. No runtime recovery admission is repeated.
function sessionAt(records, session, boundary, startBoundary = boundary) {
  const history = records.filter(r => event(r).aggregate_kind === 'agent_session'
    && event(r).aggregate_id === session.agent_session_id && sequence(r) <= boundary);
  const atStart = latest(history.filter(r => sequence(r) <= startBoundary)
    .map(record => ({ row: projection(record), record })), 'AgentSession generation at execution boundary');
  equal(atStart.runtime_generation, session.session_generation, 'execution Session generation');
  const bindings = history.filter(r => event(r).transition === 'native_session_bound'
    && (projection(r)?.runtime_generation === session.session_generation
      || event(r).payload?.runtime_generation === session.session_generation));
  requireFact(bindings.length > 0, `missing native binding for ${session.agent_session_id} generation ${session.session_generation}`);
  for (const record of bindings) {
    const row = projection(record), payload = event(record).payload;
    equal(row?.id, session.agent_session_id, 'AgentSession id');
    equal(payload?.session_id, session.agent_session_id, 'native binding Session id');
    equal(row?.runtime_generation, session.session_generation, 'projection Session generation');
    equal(payload?.runtime_generation, session.session_generation, 'event Session generation');
    equal(row.agent_member_id, session.agent_member_id, 'AgentSession Member');
    equal(row.provider_kind, session.provider, 'AgentSession provider');
    for (const native of [row.native_session_ref, payload.native_session_ref]) {
      equal(native?.provider, session.provider, 'native provider');
      equal(native?.native_session_id, session.native_session_id, 'native identity');
    }
  }
  // A transition during the delivery interval cannot be hidden by choosing the
  // previous generation's native rows. Genuine successors after the interval pass.
  for (const record of history.filter(r => sequence(r) >= startBoundary)) {
    equal(projection(record)?.runtime_generation, session.session_generation, 'Session generation within execution interval');
    equal(projection(record)?.agent_member_id, session.agent_member_id, 'Session Member within execution interval');
    equal(projection(record)?.provider_kind, session.provider, 'Session provider within execution interval');
  }
}

function deliveryAt(records, evidence, reportRecord) {
  const { work, team } = evidence;
  const bounded = records.filter(r => sequence(r) <= sequence(reportRecord));
  const rows = facts(bounded, row => row.id === work.work_execution_binding_id
    && 'agent_session_generation' in row && 'binding_generation' in row);
  requireFact(rows.length > 0, 'missing exact WorkExecutionBinding');
  const admission = one(rows.filter(({ record }) => event(record).aggregate_kind === 'work_execution_binding'
    && event(record).aggregate_id === work.work_execution_binding_id && event(record).transition === 'bound'), 'canonical WorkExecutionBinding bound event');
  const binding = admission.row;
  for (const { row } of rows) {
    for (const field of ['id', 'work_id', 'work_revision', 'team_id', 'team_membership_id', 'agent_member_id',
      'agent_session_id', 'agent_session_generation', 'delivery_id', 'binding_generation', 'bound_at']) {
      equal(row[field], binding[field], `WorkExecutionBinding ${field}`);
    }
  }
  equal(binding.work_id, work.work_id, 'binding Work'); equal(binding.team_id, team.agent_team_id, 'binding Team');
  equal(binding.agent_member_id, team.implementer_agent_member_id, 'binding implementer');
  equal(binding.delivery_id, work.delivery_id, 'binding delivery');
  const reportSides = reportRecord.operation.immutable_side_records ?? [];
  const reportBinding = one(reportSides.filter(row => row.work_id === binding.work_id
    && 'agent_session_generation' in row && 'binding_generation' in row && row.ended_at),
  'WorkReport direct released WorkExecutionBinding association (historical predecessor without it is an evidence gap)');
  equal(reportBinding.id, binding.id, 'WorkReport execution binding');
  equal(reportBinding.delivery_id, binding.delivery_id, 'WorkReport delivery');
  equal(reportBinding.agent_session_generation, binding.agent_session_generation, 'WorkReport Session generation');
  const deliveries = facts(bounded, row => row.id === work.delivery_id && 'work_execution_binding_id' in row);
  requireFact(deliveries.length > 0, 'missing canonical Work delivery');
  for (const { row } of deliveries) {
    equal(row.work_execution_binding_id, binding.id, 'delivery binding');
    equal(row.work_id, binding.work_id, 'delivery Work'); equal(row.work_revision, binding.work_revision, 'delivery Work revision');
    equal(row.recipient_agent_member_id, binding.agent_member_id, 'delivery Member');
    equal(row.recipient_session_id, binding.agent_session_id, 'delivery Session');
    equal(row.recipient_session_generation, binding.agent_session_generation, 'delivery Session generation');
  }
  requireFact(deliveries.some(({ row }) => row.status === 'provider_received' && row.provider_receipt_id),
    'missing provider-received Work delivery');
  membershipAt(records, binding.team_membership_id, binding.agent_member_id, binding.team_id, sequence(admission.record));
  return { binding, start: sequence(admission.record) };
}

function externalLeaseAt(leases, host, runId, acceptedAt) {
  // Reconstruct from the last fact recorded at/before acceptance. A Released
  // row overwrites expires on release, so cannot itself prove a past interval.
  const history = leases.filter(row => row.team_run_id === runId);
  requireFact(history.length > 0, 'missing HostBindingLease history');
  const prior = history.filter(row => Number.isSafeInteger(row.heartbeat_unix_ms)
    && row.heartbeat_unix_ms <= acceptedAt);
  requireFact(prior.length > 0, 'missing HostBindingLease history before acceptance');
  const lease = prior.at(-1);
  for (const field of ['lease_id', 'owner_id', 'generation']) equal(lease[field], host[field], `Host lease ${field}`);
  equal(lease.host_surface, host.surface, 'Host lease surface'); equal(lease.host_thread_id, host.thread_id, 'Host lease thread');
  equal(lease.owner_kind, 'interactive', 'Host lease owner kind'); equal(lease.status, 'active', 'Host lease at acceptance');
  requireFact(lease.acquired_unix_ms <= acceptedAt && lease.expires_unix_ms > acceptedAt,
    'Host lease expired or not acquired at acceptance');
}

export function verifyAttributionV2(evidence, records, sources, expectedSpaceId) {
  const failures = verifyCanonicalWorkFacts(evidence, records, expectedSpaceId);
  try {
    requireFact(evidence?.host && Array.isArray(evidence.sessions), 'missing v2 Host/Session evidence');
    // Never discard conflicting foreign records with the same identity through
    // space prefiltering. The selected ledger must own every relevant fact.
    const { team, work, host } = evidence;
    const referenced = new Set([team.agent_team_id, work.work_id, work.work_report_id,
      work.review_message_id, work.work_execution_binding_id, work.delivery_id, host.team_membership_id,
      ...evidence.sessions.map(s => s.agent_session_id)]);
    for (const record of records) {
      const related = referenced.has(event(record).aggregate_id)
        || [...(record.operation.immutable_side_records ?? []), ...(record.operation.initial_outbox_records ?? [])]
          .some(row => referenced.has(row?.id));
      if (related) equal(record.execution_space_id, expectedSpaceId, 'selected ledger Execution Space');
      if (record.execution_space_id === expectedSpaceId) {
        requireFact(Number.isSafeInteger(sequence(record)) && sequence(record) > 0, 'missing canonical store sequence');
      }
    }
    records = records.filter(record => record.execution_space_id === expectedSpaceId);
    const accept = one(records.filter(r => event(r).id === work.acceptance_event_id), 'acceptance event');
    const report = one(records.filter(r => event(r).aggregate_kind === 'work_report'
      && event(r).aggregate_id === work.work_report_id && event(r).transition === 'created'), 'WorkReport');
    const review = one(records.filter(r => event(r).aggregate_kind === 'message'
      && event(r).aggregate_id === work.review_message_id && event(r).transition === 'authored'), 'review Message');
    requireFact(sequence(report) < sequence(review) && sequence(review) < sequence(accept), 'report/review/acceptance canonical order mismatch');
    const acceptedAt = time(event(accept).created_at, 'acceptance');
    const run = sources.teamRuns.filter(row => row.id === team.team_run_id
      && time(row.updated_at, 'TeamRun') <= acceptedAt).at(-1);
    requireFact(run, 'missing TeamRun at acceptance');
    equal(run.agent_team_id, team.agent_team_id, 'TeamRun Team'); equal(run.host_control_mode, host.mode, 'Host mode');
    equal(run.host_actor?.kind, 'host', 'TeamRun Host actor kind');
    equal(run.host_actor?.id, team.host_agent_member_id, 'TeamRun Host Member');
    const teams = records.filter(r => event(r).aggregate_kind === 'agent_team'
      && event(r).aggregate_id === team.agent_team_id && sequence(r) <= sequence(accept));
    const actualTeam = latest(teams.map(record => ({ row: projection(record), record })), 'AgentTeam');
    equal(actualTeam.node_id, run.execution_node_id, 'Team/TeamRun node');
    membershipAt(records, host.team_membership_id, team.host_agent_member_id, team.agent_team_id, sequence(accept), 'host');
    const { binding, start } = deliveryAt(records, evidence, report);
    const implementer = one(evidence.sessions.filter(s => s.agent_member_id === team.implementer_agent_member_id), 'implementer Session');
    equal(implementer.agent_session_id, binding.agent_session_id, 'implementer binding Session');
    equal(implementer.session_generation, binding.agent_session_generation, 'implementer binding generation');
    const reviewer = one(evidence.sessions.filter(s => s.agent_member_id === work.reviewer_agent_member_id), 'reviewer native Session');
    equal(projection(review).sender_session_id, reviewer.agent_session_id, 'review Message sender Session');
    requireFact(implementer.agent_member_id !== reviewer.agent_member_id, 'reviewer must be independent');
    requireFact(new Set(evidence.sessions.map(s => s.agent_session_id)).size === evidence.sessions.length, 'duplicate Session identity');
    requireFact(new Set(evidence.sessions.map(s => s.agent_member_id)).size === evidence.sessions.length, 'duplicate Session Member');
    for (const session of evidence.sessions) {
      const isImplementer = session === implementer;
      const boundary = isImplementer ? sequence(report) : session.agent_member_id === team.host_agent_member_id
        ? sequence(accept) : sequence(review);
      sessionAt(records, session, boundary, isImplementer ? start : boundary);
    }
    if (host.mode === 'managed') {
      one(evidence.sessions.filter(s => s.agent_member_id === team.host_agent_member_id), 'managed Host Session');
    } else {
      requireFact(!evidence.sessions.some(s => s.agent_member_id === team.host_agent_member_id), 'external Host must not fabricate an AgentSession');
      equal(run.host_surface, host.surface, 'external Host surface'); equal(run.host_thread_id, host.thread_id, 'external Host thread');
      externalLeaseAt(sources.hostLeases, host, run.id, acceptedAt);
      const receipt = sources.validateHost(host.surface, host.thread_id);
      equal(receipt?.host_surface, host.surface, 'native discovery surface'); equal(receipt?.host_thread_id, host.thread_id, 'native discovery thread');
      equal(receipt?.owner_id, host.owner_id, 'native discovery owner');
      equal(receipt?.owner_id, `interactive:codex:${host.thread_id}`, 'canonical discovery owner');
      equal(receipt?.discovery_source, 'codex_rollout_session_meta', 'trusted native discovery source');
    }
  } catch (error) { failures.push(`v2 attribution: ${error.message}`); }
  return failures;
}
