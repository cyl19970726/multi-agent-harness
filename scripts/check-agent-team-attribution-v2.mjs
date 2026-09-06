import assert from 'node:assert/strict';
import { fixture, spaceId } from './fixtures/agent-team-v2.mjs';
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
console.log(`v2 attribution PASS: ${cases} deterministic cases; fixtures are not live dogfood`);
