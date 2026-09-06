import { readFileSync } from 'node:fs';
export const spaceId = 'space-fixture';
export function fixture(external = false) {
  const evidence = JSON.parse(readFileSync('schemas/agent-team-dogfood/fixtures/valid/coding-dogfood.json', 'utf8'));
  evidence.schema_version = 'agentfirm.agent_team_dogfood_evidence.v2';
  evidence.host = { mode: external ? 'external_interactive' : 'managed', team_membership_id: 'host-membership' };
  evidence.work.work_execution_binding_id = 'binding'; evidence.work.delivery_id = 'delivery';
  evidence.sessions.forEach(s => { s.session_generation = 1; });
  const records = readFileSync('schemas/agent-team-dogfood/fixtures/canonical-ledger/valid/canonical.jsonl', 'utf8').trim().split('\n').map(JSON.parse);
  records.forEach((r, i) => { r.operation.event.store_sequence = i < 3 ? 20 + i : i + 1; });
  for (const r of records.slice(3)) {
    r.operation.event.payload.runtime_generation = 1; r.operation.resulting_projection.runtime_generation = 1;
    Object.assign(r.operation.resulting_projection, {node_id:'node',execution_space_id:spaceId,lifecycle:'active',
      control_state:{driver_ref:{kind:'team_supervisor',team_run_id:'team-run-fixture',team_supervisor_id:'supervisor',team_supervisor_generation:1}}});
  }
  const row = (seq, kind, data) => ({execution_space_id:spaceId, operation:{event:{id:`event-${seq}`,aggregate_kind:kind,
    aggregate_id:data.id,store_sequence:seq,created_at:'unix-ms:900',transition:'created'},resulting_projection:data,immutable_side_records:[],initial_outbox_records:[]}});
  records.push(row(1, 'agent_team', {id:'team-fixture', node_id:'node'}));
  records.push(row(2, 'team_membership', {id:'host-membership',team_id:'team-fixture',agent_member_id:'host-fixture',state:'active',role:'host'}));
  records.push(row(3, 'team_membership', {id:'member-membership',team_id:'team-fixture',agent_member_id:'member-fixture',state:'active',role:'member'}));
  const binding = {id:'binding',work_id:'work-fixture',work_revision:2,team_id:'team-fixture',team_membership_id:'member-membership',agent_member_id:'member-fixture',agent_session_id:'agent-session-member',agent_session_generation:1,delivery_id:'delivery',binding_generation:1,status:'active',version:1,bound_at:'unix-ms:950',ended_at:null};
  records.push(row(10,'work_execution_binding',binding));
  records.at(-1).operation.event.transition='bound';
  records.push(row(11,'work_delivery_receipt',{id:'delivery',work_id:'work-fixture',work_revision:2,work_execution_binding_id:'binding',recipient_agent_member_id:'member-fixture',recipient_session_id:'agent-session-member',recipient_session_generation:1,status:'provider_received',provider_receipt_id:'receipt'}));
  records[0].operation.immutable_side_records.push({...binding,status:'released',version:2,ended_at:'unix-ms:1000'});
  const teamRuns = [{id:'team-run-fixture',agent_team_id:'team-fixture',execution_node_id:'node',host_control_mode:evidence.host.mode,
    host_actor:{kind:'host',id:'host-fixture'},host_surface:'codex',host_thread_id:external?'native-external':null,updated_at:'unix-ms:900'}];
  const hostLeases = [];
  if (external) {
    Object.assign(evidence.host,{surface:'codex',thread_id:'native-external',owner_id:'interactive:codex:native-external',lease_id:'lease',generation:1});
    evidence.work.reviewer_agent_member_id='reviewer'; evidence.sessions[0].agent_member_id='reviewer';
    evidence.sessions[0].agent_session_id='reviewer-session'; evidence.sessions[0].native_session_id='reviewer-native';
    const review=records[1]; review.operation.event.authority_actor.id='reviewer'; review.operation.resulting_projection.sender_agent_member_id='reviewer';
    const native=records[3]; native.operation.event.aggregate_id='reviewer-session'; native.operation.event.payload.session_id='reviewer-session';
    native.operation.resulting_projection.id='reviewer-session'; native.operation.resulting_projection.agent_member_id='reviewer';
    native.operation.resulting_projection.native_session_ref.native_session_id='reviewer-native'; native.operation.event.payload.native_session_ref.native_session_id='reviewer-native';
    hostLeases.push({team_run_id:'team-run-fixture',host_surface:'codex',host_thread_id:'native-external',owner_kind:'interactive',owner_id:evidence.host.owner_id,lease_id:'lease',generation:1,acquired_unix_ms:800,heartbeat_unix_ms:900,expires_unix_ms:1500,status:'active'});
  }
  records[1].operation.resulting_projection.sender_session_id=evidence.sessions[0].agent_session_id;
  records.sort((a,b)=>a.operation.event.store_sequence-b.operation.event.store_sequence);
  const validateHost=(surface,thread)=>({host_surface:surface,host_thread_id:thread,owner_id:`interactive:codex:${thread}`,discovery_source:'codex_rollout_session_meta'});
  return {evidence,records,sources:{teamRuns,hostLeases,validateHost}};
}
