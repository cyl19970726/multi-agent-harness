import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import Ajv2020 from "ajv/dist/2020.js";

const root=process.cwd();
const schemaDir=path.join(root,"schemas/role-views/agentfirm.role_views.v1");
const names=["common","role-view","company-work","team-workspace","host-console","member-workbench","operator"];
const schemas=names.map(name=>JSON.parse(fs.readFileSync(path.join(schemaDir,`${name}.schema.json`),"utf8")));
const ajv=new Ajv2020({strict:false,allErrors:true});
for(const schema of schemas)ajv.addSchema(schema);
for(const name of names.slice(2))assert.equal(typeof ajv.getSchema(`agentfirm.role_views.v1/${name}.schema.json`),"function",`${name} schema compiles`);
const fixtureDir=path.join(root,"apps/agent-dashboard/fixtures/wave4-local-agentfirm-v1");
for(const name of names.slice(2)){
  const fixture=JSON.parse(fs.readFileSync(path.join(fixtureDir,`${name}.json`),"utf8"));
  const validate=ajv.getSchema(`agentfirm.role_views.v1/${name}.schema.json`);
  assert.equal(validate(fixture),true,`${name} fixture: ${ajv.errorsText(validate.errors)}`);
  const hostile=structuredClone(fixture);
  const closedTarget={
    "company-work":hostile.data,
    "team-workspace":hostile.data.team,
    "host-console":hostile.data.daemon_summary,
    "member-workbench":hostile.data.agent_member,
    operator:hostile.data.node,
  }[name];
  closedTarget.__browser_invented_truth=true;
  assert.equal(validate(hostile),false,`${name} must reject nested unknown fields`);
}

const operatorValidate=ajv.getSchema("agentfirm.role_views.v1/operator.schema.json");
const operatorFixture=JSON.parse(fs.readFileSync(path.join(fixtureDir,"operator.json"),"utf8"));
const daemonAction={kind:"start_daemon",target_ref:{kind:"execution_node",id:"node-1"},required_version:1,disabled_reason:null,authority_generation:0};
operatorFixture.allowed_actions=[daemonAction];
assert.equal(operatorValidate(operatorFixture),true,`daemon authority action: ${ajv.errorsText(operatorValidate.errors)}`);
delete daemonAction.authority_generation;
assert.equal(operatorValidate(operatorFixture),false,"daemon lifecycle actions require an exact authority generation");
daemonAction.authority_generation="1";
assert.equal(operatorValidate(operatorFixture),false,"daemon authority generation rejects browser-coerced string types");
operatorFixture.allowed_actions=[{kind:"diagnose",target_ref:{kind:"execution_node",id:"node-1"},required_version:1,disabled_reason:null,authority_generation:1}];
assert.equal(operatorValidate(operatorFixture),false,"non-daemon actions must not carry a daemon authority generation");
operatorFixture.allowed_actions=[{kind:"diagnose",target_ref:{kind:"execution_node",id:"node-1"},required_version:1,disabled_reason:null,__unknown_authority:true}];
assert.equal(operatorValidate(operatorFixture),false,"unknown action fields remain fail-closed");
const admissionBinding={provider:"codex",execution_mode:"codex_app_server",eligibility:"eligible",eligibility_fingerprint:"0123456789abcdef"};
operatorFixture.allowed_actions=[{kind:"admit_provider",target_ref:{kind:"execution_node",id:"node-1"},required_version:1,disabled_reason:null,intent_binding:admissionBinding}];
assert.equal(operatorValidate(operatorFixture),true,`tuple-bound admission action: ${ajv.errorsText(operatorValidate.errors)}`);
delete admissionBinding.eligibility_fingerprint;
assert.equal(operatorValidate(operatorFixture),false,"provider admission requires a complete server tuple fingerprint");
operatorFixture.allowed_actions=[{kind:"diagnose",target_ref:{kind:"execution_node",id:"node-1"},required_version:1,disabled_reason:null,intent_binding:{provider:"codex",execution_mode:"codex_app_server",eligibility:"eligible",eligibility_fingerprint:"0123456789abcdef"}}];
assert.equal(operatorValidate(operatorFixture),false,"only provider admission may carry a tuple binding");

const rust=fs.readFileSync(path.join(root,"crates/firm-cli/src/role_views_api.rs"),"utf8");
for(const endpoint of ["company-work","team-workspace/","host-console/","member-workbench/","operator/"])assert.ok(rust.includes(`/v1/views/${endpoint}`),`missing ${endpoint}`);
assert.ok(rust.includes("canonical_operations"),"views must use canonical event sequence");
assert.ok(!/POST \/v1\/views\//.test(rust),"page-specific mutations are forbidden");

const manifest=JSON.parse(fs.readFileSync(path.join(root,"schemas/role-views/role-action-manifest.v1.json"),"utf8"));
assert.equal(manifest.schema_version,"agentfirm.role_actions.v1");
assert.deepEqual(manifest.transport,{authentication:"X-AgentFirm-Token",idempotency:"Idempotency-Key",expected_version:"If-Match",identity_override:"forbidden"});
const actions=new Set(manifest.actions.map(item=>item.ui_action));
for(const required of [
  "create_work","assign_work","rebind_work","release_work","request_changes","accept_work","cancel_work",
  "send_message","reply_message","close_member_run","reopen_member_run","retire_member_run","resume_native_session",
  "provision_workspace","attach_workspace","archive_workspace","cleanup_workspace","request_gate_evaluation","evaluate_gate","waive_gate","revoke_waiver",
  "claim_work","start_work","block_work","unblock_work","submit_work","revise_work","write_report","write_finding","write_failure","request_decision",
  "reconcile_delivery","reconcile_message_delivery","start_daemon","stop_daemon","admit_provider","diagnose",
])assert.ok(actions.has(required),`action manifest missing ${required}`);
for(const item of manifest.actions){for(const field of ["http_endpoint","application_command","actor_policy","expected_version_source","resulting_event","returns"])assert.equal(typeof item[field],"string",`${item.ui_action}.${field}`)}
for(const kind of ["start_daemon","stop_daemon"])assert.equal(typeof manifest.actions.find(item=>item.ui_action===kind)?.authority_generation_source,"string",`${kind} must declare its server authority-generation source`);
assert.equal(typeof manifest.actions.find(item=>item.ui_action==="admit_provider")?.intent_binding_source,"string","provider admission must declare its server tuple-binding source");
for(const item of manifest.actions)assert.match(item.http_endpoint,/^(POST \/v1\/agentfirm\/|GET \/v1\/views\/operator\/)/,`${item.ui_action} must use a frozen authenticated route`);
const emitted=new Set([...rust.matchAll(/action\("([a-z_]+)"/g)].map(match=>match[1]));
for(const action of emitted)assert.ok(actions.has(action),`RoleView emits unimplemented action ${action}`);
assert.ok(rust.includes('daemon_action["authority_generation"]'),"server daemon actions must bind the generic authority generation");
assert.ok(!rust.includes('daemon_action["daemon_generation"]'),"daemon-specific action schema leakage is retired");
console.log("role-view contract check: PASS");
