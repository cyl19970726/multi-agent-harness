import assert from 'node:assert/strict';
import { fixture, overlapGenerationsFixture, spaceId } from './fixtures/agent-team-v2.mjs';
import { verifyAttributionV2 } from './lib/agent-team-attribution-v2.mjs';
const verify = f => verifyAttributionV2(f.evidence,f.records,f.sources,spaceId);
const native = f => f.records.find(r=>r.operation.event.aggregate_id==='agent-session-member');
const at = (f,kind) => f.records.find(r=>r.operation.event.aggregate_kind===kind);
let cases=0;
function check(name,mutate,pattern,external=false){ const f=fixture(external); mutate(f);const errors=verify(f);if(pattern)assert.match(errors.join('\n'),pattern,name);else assert.deepEqual(errors,[],name);cases++;}
check('managed',()=>{});
check('external',()=>{},null,true);
check('same generation metadata updates',f=>{const row=structuredClone(native(f));row.operation.event.store_sequence=12;row.operation.resulting_projection.native_session_ref.native_locator='updated';f.records.push(row);});
for(const [label,mutate,pattern] of [
  ['native',r=>{r.operation.event.payload.native_session_ref.native_session_id='wrong';},/native identity/],
  ['Member',r=>{r.operation.resulting_projection.agent_member_id='wrong';},/Member/],
  ['provider',r=>{r.operation.resulting_projection.provider_kind='wrong';},/provider/],
  ['generation',r=>{r.operation.event.payload.runtime_generation=9;},/generation/],
])check(`conflicting ${label} cannot be prefiltered`,f=>{const row=structuredClone(native(f));row.operation.event.store_sequence=12;mutate(row);f.records.push(row);},pattern);
check('wrong claimed generation',f=>{f.evidence.sessions[1].session_generation=2;},/generation/);
check('missing binding',f=>{f.records=f.records.filter(r=>r.operation.event.aggregate_kind!=='work_execution_binding');at(f,'work_report').operation.immutable_side_records=[];},/missing exact WorkExecutionBinding/);
check('wrong delivery',f=>{at(f,'work_delivery_receipt').operation.resulting_projection.recipient_session_id='wrong';},/delivery Session/);
check('no provider receipt',f=>{delete at(f,'work_delivery_receipt').operation.resulting_projection.provider_receipt_id;},/provider-received/);
check('later canonical generation',f=>{const row=structuredClone(native(f));row.operation.event.store_sequence=23;row.operation.event.transition='resumed';row.operation.resulting_projection.runtime_generation=2;f.records.push(row);});
check('generation changes during delivery',f=>{const row=structuredClone(native(f));row.operation.event.store_sequence=12;row.operation.event.transition='resumed';row.operation.resulting_projection.runtime_generation=2;f.records.push(row);},/within execution interval/);
check('managed missing Session',f=>{f.evidence.sessions=f.evidence.sessions.slice(1);},/reviewer native Session|managed Host Session/);
check('released after acceptance retains old interval',f=>{f.sources.hostLeases.push({...f.sources.hostLeases[0],status:'released',heartbeat_unix_ms:1200,expires_unix_ms:1200,released_unix_ms:1200});},null,true);
check('release before acceptance',f=>{f.sources.hostLeases.push({...f.sources.hostLeases[0],status:'released',heartbeat_unix_ms:1001,expires_unix_ms:1001,released_unix_ms:1001});},/at acceptance/,true);
for(const field of ['owner_id','generation','lease_id'])check(`wrong lease ${field}`,f=>{f.evidence.host[field]=field==='generation'?2:'wrong';},/Host lease/,true);
check('expired lease',f=>{f.sources.hostLeases[0].expires_unix_ms=1002;},/expired/,true);
check('missing historical active interval',f=>{f.sources.hostLeases=[{...f.sources.hostLeases[0],status:'released',heartbeat_unix_ms:1200,expires_unix_ms:1200,released_unix_ms:1200}];},/history before/,true);
check('fake discovery',f=>{f.sources.validateHost=()=>({host_surface:'codex',host_thread_id:'native-external',owner_id:f.evidence.host.owner_id,discovery_source:'self_asserted'});},/trusted native discovery/,true);
check('wrong Host mode',f=>{f.sources.teamRuns[0].host_control_mode='managed';},/Host mode/,true);
check('wrong Host member',f=>{f.sources.teamRuns[0].host_actor.id='wrong';},/Host Member/,true);
check('wrong acceptance actor',f=>{at(f,'work').operation.event.performed_by_actor.id='wrong';},/acceptance actor/);
check('missing reviewer native evidence',f=>{f.evidence.sessions=f.evidence.sessions.slice(1);},/reviewer native Session/,true);
check('foreign source',f=>{native(f).execution_space_id='foreign';},/Execution Space/);
check('duplicate consistent binding without reattach', f => { const r=structuredClone(native(f));r.operation.event.store_sequence=12;f.records.push(r); });
check('missing original bound event', f => { f.records=f.records.filter(r=>r.operation.event.aggregate_kind!=='work_execution_binding'); }, /canonical WorkExecutionBinding bound event/);
check('missing reviewer sender Session', f => { delete at(f,'message').operation.resulting_projection.sender_session_id; }, /review Message sender Session/);
check('wrong reviewer sender Session', f => { at(f,'message').operation.resulting_projection.sender_session_id='other'; }, /review Message sender Session/);
check('old Claude external cannot be repaired', f => { f.evidence.host.surface='claude';f.sources.teamRuns[0].host_surface='claude';f.sources.hostLeases[0].host_surface='claude';f.sources.validateHost=()=>{throw new Error('unsupported native Host discovery');}; }, /unsupported native Host discovery/, true);
check('native discovery wrong owner', f => { f.sources.validateHost=(surface,thread)=>({host_surface:surface,host_thread_id:thread,owner_id:'wrong',discovery_source:'codex_rollout_session_meta'}); }, /native discovery owner/, true);
check('native discovery wrong thread', f => { f.sources.validateHost=()=>({host_surface:'codex',host_thread_id:'wrong'}); }, /native discovery thread/, true);
check('missing Host membership', f => { f.records=f.records.filter(r=>r.operation.event.aggregate_id!=='host-membership'); }, /TeamMembership/, true);
check('wrong Host membership owner', f => { f.records.find(r=>r.operation.event.aggregate_id==='host-membership').operation.resulting_projection.agent_member_id='wrong'; }, /membership Member/, true);
check('future lease renewal does not repair expired interval', f => { f.sources.hostLeases[0].expires_unix_ms=1002;f.sources.hostLeases.push({...f.sources.hostLeases[0],heartbeat_unix_ms:1100,expires_unix_ms:1600}); }, /expired/, true);
check('successor lease after acceptance', f => { f.sources.hostLeases.push({...f.sources.hostLeases[0],lease_id:'successor',generation:2,acquired_unix_ms:1600,heartbeat_unix_ms:1600,expires_unix_ms:2000}); }, null, true);
check('later native generation with new identity', f => { const r=structuredClone(native(f));r.operation.event.store_sequence=23;r.operation.event.payload.runtime_generation=2;r.operation.resulting_projection.runtime_generation=2;r.operation.event.payload.native_session_ref.native_session_id='successor-native';r.operation.resulting_projection.native_session_ref.native_session_id='successor-native';f.records.push(r); });
check('unrelated foreign history is not folded', f => { f.records.push({execution_space_id:'other',operation:{event:{aggregate_kind:'message',aggregate_id:'unrelated'},resulting_projection:{id:'unrelated'}}}); });
check('released predecessor without persisted Report association is a gap', f => {
  const released=structuredClone(at(f,'work_execution_binding'));released.operation.event.store_sequence=12;
  released.operation.event.transition='released';released.operation.resulting_projection.status='released';released.operation.resulting_projection.ended_at='unix-ms:990';f.records.push(released);
  at(f,'work_report').operation.immutable_side_records=[];
}, /direct released WorkExecutionBinding association.*evidence gap/);
check('multiple historical predecessor candidates cannot be latest selected', f => {
  const candidate=structuredClone(at(f,'work_execution_binding'));candidate.operation.event.store_sequence=13;
  candidate.operation.event.aggregate_id='second-binding';candidate.operation.resulting_projection.id='second-binding';
  candidate.operation.resulting_projection.status='released';candidate.operation.resulting_projection.ended_at='unix-ms:995';f.records.push(candidate);
  at(f,'work_report').operation.immutable_side_records=[];
}, /direct released WorkExecutionBinding association.*evidence gap/);
check('ambiguous Report binding side records cannot be selected by evidence id', f => {
  const side=at(f,'work_report').operation.immutable_side_records[0];
  at(f,'work_report').operation.immutable_side_records.push({...side,id:'other-binding'});
}, /direct released WorkExecutionBinding association.*found 2/);
// Submission02: role boundaries are independent, and Host selection is made
// from canonical run associations before inspecting the claimed Session ID.
function separateManaged(f) {
  const managed=fixture(false);
  const host=managed.evidence.sessions.find(s=>s.agent_member_id==='host-fixture');
  const source=managed.records.find(r=>r.operation.event.aggregate_id===host.agent_session_id);
  source.operation.event.store_sequence=6;
  f.records.push(source);f.evidence.sessions.push(host);
  f.evidence.host={mode:'managed',team_membership_id:'host-membership'};
  f.sources.teamRuns[0].host_control_mode='managed';f.sources.teamRuns[0].host_thread_id=null;
}
function advanceHostBetweenRoles(f, claimGeneration) {
  f.records.forEach(r=>{r.operation.event.store_sequence*=2;});
  const host=f.evidence.sessions.find(s=>s.agent_member_id==='host-fixture');
  const source=f.records.find(r=>r.operation.event.aggregate_id===host.agent_session_id);
  const successor=structuredClone(source);successor.operation.event.id='later-host-generation';
  successor.operation.event.store_sequence=43;successor.operation.event.payload.runtime_generation=2;
  successor.operation.resulting_projection.runtime_generation=2;f.records.push(successor);
  host.session_generation=claimGeneration;
}
check('B1 overlap Review gen1 cannot borrow Host acceptance gen2',f=>advanceHostBetweenRoles(f,2),/reviewer native Session claim.*generation 1/);
check('overlap older generation cannot claim later acceptance',f=>advanceHostBetweenRoles(f,1),/managed Host associated Session claim.*generation 2/);
check('overlap unchanged generation remains positive',()=>{});
check('separate reviewer and managed Host remain positive',separateManaged,null,true);
check('Host canonical successor after acceptance preserves history',f=>{
  const source=f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host');
  const successor=structuredClone(source);successor.operation.event.store_sequence=23;
  successor.operation.event.payload.runtime_generation=2;successor.operation.resulting_projection.runtime_generation=2;
  f.records.push(successor);
});
check('B2 genuine same-Member foreign node/run Session cannot be selected',f=>{
  separateManaged(f);const source=f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host');
  source.operation.event.aggregate_id='foreign-session';source.operation.event.payload.session_id='foreign-session';
  const row=source.operation.resulting_projection;row.id='foreign-session';row.node_id='other-node';row.control_state.driver_ref.team_run_id='other-run';
  row.native_session_ref.native_session_id='foreign-native';source.operation.event.payload.native_session_ref.native_session_id='foreign-native';
  const claim=f.evidence.sessions.find(s=>s.agent_member_id==='host-fixture');claim.agent_session_id='foreign-session';claim.native_session_id='foreign-native';
},/historically associated managed Host Session/,true);
check('Host associated with run but foreign node refuses',f=>{
  separateManaged(f);f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host').operation.resulting_projection.node_id='other-node';
},/managed Host Session node/,true);
check('Host same node but foreign run refuses',f=>{
  separateManaged(f);f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host').operation.resulting_projection.control_state.driver_ref.team_run_id='other-run';
},/historically associated managed Host Session/,true);
check('Host missing run association is a named gap',f=>{
  separateManaged(f);delete f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host').operation.resulting_projection.control_state;
},/evidence gap/,true);
check('Host ambiguous run association is a named gap',f=>{
  separateManaged(f);const other=structuredClone(f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host'));
  other.operation.event.store_sequence=7;other.operation.event.aggregate_id='other-session';other.operation.resulting_projection.id='other-session';
  other.operation.event.payload.session_id='other-session';f.records.push(other);
},/evidence gap.*found 2/,true);
check('unrelated same-Member Session cannot replace resolved Host',f=>{
  separateManaged(f);const other=structuredClone(f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host'));
  other.operation.event.store_sequence=7;other.operation.event.aggregate_id='other-session';other.operation.resulting_projection.id='other-session';
  other.operation.event.payload.session_id='other-session';other.operation.resulting_projection.control_state.driver_ref.team_run_id='other-run';f.records.push(other);
  f.evidence.sessions.find(s=>s.agent_member_id==='host-fixture').agent_session_id='other-session';
},/managed Host associated Session/,true);
check('closed predecessor does not make current Host ambiguous',f=>{
  separateManaged(f);const old=structuredClone(f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host'));
  old.operation.event.store_sequence=7;old.operation.event.aggregate_id='closed-session';old.operation.resulting_projection.id='closed-session';
  old.operation.event.payload.session_id='closed-session';old.operation.resulting_projection.lifecycle='closed';f.records.push(old);
},null,true);
function tupleCase(name, mutate, pattern) {
  const f=overlapGenerationsFixture();mutate(f);const errors=verify(f);
  if(pattern)assert.match(errors.join('\n'),pattern,name);else assert.deepEqual(errors,[],name);cases++;
}
tupleCase('Review84 gen1 and acceptance88 gen2 both prove exact tuples',()=>{});
tupleCase('role tuple selection is independent of evidence order',f=>f.evidence.sessions.reverse());
tupleCase('missing reviewer generation claim refuses',f=>{f.evidence.sessions=f.evidence.sessions.filter(s=>s.agent_member_id!=='host-fixture'||s.session_generation!==1);},/reviewer native Session claim.*generation 1/);
tupleCase('missing Host acceptance generation claim refuses',f=>{f.evidence.sessions=f.evidence.sessions.filter(s=>s.agent_member_id!=='host-fixture'||s.session_generation!==2);},/managed Host associated Session claim.*generation 2/);
tupleCase('duplicate exact tuple refuses',f=>f.evidence.sessions.push({...f.evidence.sessions[0]}),/duplicate or conflicting exact Session tuple/);
tupleCase('conflicting duplicate tuple refuses before choosing',f=>f.evidence.sessions.push({...f.evidence.sessions[0],native_session_id:'conflicting'}),/duplicate or conflicting exact Session tuple/);
tupleCase('wrong member cannot occupy exact tuple',f=>{f.evidence.sessions.find(s=>s.session_generation===2).agent_member_id='other';},/managed Host associated Session Member/);
tupleCase('swapped generation native identities refuse',f=>{for(const s of f.evidence.sessions.filter(s=>s.agent_member_id==='host-fixture'))s.session_generation=s.session_generation===1?2:1;},/native identity/);
tupleCase('missing canonical reviewer boundary refuses',f=>{f.records=f.records.filter(r=>r.operation.event.aggregate_id!=='agent-session-host'||r.operation.event.store_sequence>84);},/review Message sender Session canonical boundary/);
tupleCase('missing new-generation native binding refuses',f=>{f.records=f.records.filter(r=>r.operation.event.store_sequence!==86);},/missing native binding/);
tupleCase('old role native conflict cannot be hidden by successor',f=>{const r=structuredClone(f.records.find(r=>r.operation.event.aggregate_id==='agent-session-host'));r.operation.event.store_sequence=83;r.operation.event.payload.native_session_ref.native_session_id='wrong';f.records.push(r);},/native identity/);
tupleCase('later successor after acceptance preserves both role claims',f=>{const r=structuredClone(f.records.find(r=>r.operation.event.store_sequence===86));r.operation.event.store_sequence=89;r.operation.resulting_projection.runtime_generation=3;r.operation.event.payload.runtime_generation=3;f.records.push(r);});
for(const field of ['tool_started','tool_terminal'])tupleCase(`other generation cannot lend ${field} to implementer`,f=>{
  const actual=f.evidence.sessions.find(s=>s.agent_member_id==='member-fixture');
  f.evidence.sessions.unshift({...actual,session_generation:2,tool_started:99,tool_terminal:99});actual[field]=0;
},/exact implementer binding requires/);
tupleCase('first unrelated generation zero counters does not hide actual implementer',f=>{
  const actual=f.evidence.sessions.find(s=>s.agent_member_id==='member-fixture');
  f.evidence.sessions.unshift({...actual,session_generation:2,tool_started:0,tool_terminal:0});
});
console.log(`v2 attribution PASS: ${cases} deterministic cases; fixtures are not live dogfood`);
