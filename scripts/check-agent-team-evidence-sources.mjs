import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, realpathSync, rmSync, writeFileSync, symlinkSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fixture, spaceId } from './fixtures/agent-team-v2.mjs';
import { loadSelectedSpaceSources } from './lib/agent-team-evidence-sources.mjs';
import { verifyAttributionV2 } from './lib/agent-team-attribution-v2.mjs';
const root=realpathSync(mkdtempSync(join(tmpdir(),'f1-v2-sources-')));
const jsonl=rows=>rows.map(JSON.stringify).join('\n')+'\n';
try {
  const f=fixture(true), ledger=join(root,'agentfirm_trust_operations.jsonl');
  writeFileSync(ledger,jsonl(f.records));writeFileSync(join(root,'team_runs.jsonl'),jsonl(f.sources.teamRuns));writeFileSync(join(root,'host_binding_leases.jsonl'),jsonl(f.sources.hostLeases));
  const calls=[];const runJson=(bin,args)=>{calls.push(args);return args[0]==='space'?{id:spaceId,store_root:root}:f.sources.validateHost('codex','native-external');};
  const sources=loadSelectedSpaceSources(spaceId,ledger,'trusted-test-cli',runJson);
  assert.deepEqual(verifyAttributionV2(f.evidence,sources.records,sources,spaceId),[]);
  assert.deepEqual(calls[0],['space','show',spaceId]);assert.deepEqual(calls[1],['team-run','validate-host-session','--surface','codex','--thread-id','native-external']);
  assert.throws(()=>loadSelectedSpaceSources(spaceId,ledger,'bin',()=>({id:'wrong',store_root:root})),/identity/);
  assert.throws(()=>loadSelectedSpaceSources(spaceId,join(root,'other','agentfirm_trust_operations.jsonl'),'bin',runJson),/canonical ledger path/);
  unlinkSync(ledger);symlinkSync(join(root,'team_runs.jsonl'),ledger);
  assert.throws(()=>loadSelectedSpaceSources(spaceId,ledger,'bin',runJson),/regular canonical/);
  unlinkSync(ledger);writeFileSync(ledger,jsonl(f.records)+'{"partial":');
  assert.equal(loadSelectedSpaceSources(spaceId,ledger,'bin',runJson).records.length,f.records.length);
  writeFileSync(ledger,jsonl(f.records)+'bad\n');assert.throws(()=>loadSelectedSpaceSources(spaceId,ledger,'bin',runJson),/malformed/);

  const binary=process.argv[2];
  if(binary){
    const home=join(root,'isolated-home');mkdirSync(home);
    const env={...process.env,HOME:home,FIRM_HOME:join(home,'.firm')};
    for(const key of ['FIRM_ROOT','FIRM_PROJECT','FIRM_SPACE','HARNESS_HOME','HARNESS_ROOT'])delete env[key];
    const cmd=args=>spawnSync(resolve(binary),args,{cwd:root,env,encoding:'utf8',timeout:30000});
    const init=cmd(['space','init','--id',spaceId]);assert.equal(init.status,0,init.stderr);
    const selected=JSON.parse(cmd(['space','show',spaceId]).stdout), selectedRoot=realpathSync(selected.store_root);
    writeFileSync(join(selectedRoot,'agentfirm_trust_operations.jsonl'),jsonl(f.records));
    writeFileSync(join(selectedRoot,'team_runs.jsonl'),jsonl(f.sources.teamRuns));
    // Include a subsequent Released row: actual CLI must read the prior active interval.
    writeFileSync(join(selectedRoot,'host_binding_leases.jsonl'),jsonl([...f.sources.hostLeases,{...f.sources.hostLeases[0],status:'released',heartbeat_unix_ms:1200,expires_unix_ms:1200,released_unix_ms:1200}]));
    const sessions=join(home,'.codex','sessions');mkdirSync(sessions,{recursive:true});
    const rollout=join(sessions,'rollout-native-external.jsonl');
    writeFileSync(rollout,JSON.stringify({type:'session_meta',payload:{id:'native-external'}})+'\n');
    const ledgerNames=['agentfirm_trust_operations.jsonl','team_runs.jsonl','host_binding_leases.jsonl'];
    const beforeDiscovery=ledgerNames.map(name=>readFileSync(join(selectedRoot,name),'utf8'));
    const discover=cmd(['team-run','validate-host-session','--surface','codex','--thread-id','native-external']);
    assert.equal(discover.status,0,discover.stderr);assert.equal(JSON.parse(discover.stdout).discovery_source,'codex_rollout_session_meta');
    assert.deepEqual(ledgerNames.map(name=>readFileSync(join(selectedRoot,name),'utf8')),beforeDiscovery,'metadata bridge must not write coordination records');
    assert.notEqual(cmd(['team-run','validate-host-session','--surface','claude','--thread-id','native-external']).status,0);
    assert.notEqual(cmd(['team-run','validate-host-session','--surface','codex','--thread-id','wrong']).status,0);
    const git=args=>spawnSync('git',args,{encoding:'utf8'}).stdout.trim();
    f.evidence.revision.base=git(['rev-parse','HEAD~1']);f.evidence.revision.candidate=git(['rev-parse','HEAD']);
    f.evidence.revision.changed_files=git(['diff','--name-only',f.evidence.revision.base,f.evidence.revision.candidate]).split('\n');
    f.records.find(r=>r.operation.event.aggregate_kind==='work_report').operation.resulting_projection.candidate.value=f.evidence.revision.candidate;
    writeFileSync(join(selectedRoot,'agentfirm_trust_operations.jsonl'),jsonl(f.records));
    const evidence=join(root,'evidence.json');writeFileSync(evidence,JSON.stringify(f.evidence));
    const verify=ledgerPath=>spawnSync(process.execPath,['scripts/check-agent-team-dogfood-evidence.mjs',evidence,'--trust-ledger',ledgerPath,'--expected-execution-space-id',spaceId,'--harness-bin',resolve(binary)],{env,encoding:'utf8',timeout:30000});
    const pass=verify(join(selectedRoot,'agentfirm_trust_operations.jsonl'));assert.equal(pass.status,0,pass.stderr);assert.match(pass.stdout,/structure and trusted attribution PASS/);assert.match(pass.stdout,/separate native-store review/);
    writeFileSync(ledger,jsonl(f.records));assert.notEqual(verify(ledger).status,0,'same IDs in a forged sibling ledger must fail');
    // CODEX_HOME is deliberately not the Host discovery root.
    const other=join(root,'caller-codex');mkdirSync(join(other,'sessions'),{recursive:true});
    writeFileSync(join(other,'sessions','rollout-native-external.jsonl'),readFileSync(rollout));unlinkSync(rollout);env.CODEX_HOME=other;
    assert.notEqual(verify(join(selectedRoot,'agentfirm_trust_operations.jsonl')).status,0,'caller CODEX_HOME cannot repair missing canonical metadata');
    console.log('v2 real CLI source integration PASS: selected Space, historical lease, canonical native discovery, forged path/refusal; deterministic fixtures only');
  }
  console.log('v2 selected-source unit checks PASS');
} finally {rmSync(root,{recursive:true,force:true});}
