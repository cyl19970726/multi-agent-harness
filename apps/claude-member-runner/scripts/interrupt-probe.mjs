import * as sdkmod from "@anthropic-ai/claude-agent-sdk";
import { createMemberRunner } from "../src/member-runner.mjs";
const SB = process.argv[2];
const sdk = { query: sdkmod.query, tagSession: sdkmod.tagSession, renameSession: sdkmod.renameSession };
const ev = []; let turns = 0;
const runner = createMemberRunner({
  sdk,
  config: { teamRunId:"t", memberRunId:"m", memberName:"IntProbe", cwd:SB,
            allowedTools:[], settingSources:[], model:"claude-haiku-4-5" },
  emit:(e,d)=>{ ev.push({e,d}); if(e==="turn_complete") turns++;
                if(e==="runner_error") console.log("EVENT runner_error:", String(d.error).slice(0,100)); },
});
const of = n => ev.filter(x=>x.e===n);
const wait = (n,ms=90000) => new Promise(r=>{const t0=Date.now();
  const i=setInterval(()=>{ if(turns>=n){clearInterval(i);r(true);}
    else if(Date.now()-t0>ms){clearInterval(i);console.log(`TIMEOUT turn ${n} (have ${turns})`);r(false);} },150);});

const done = runner.start().catch(e => console.log("START THREW:", String(e).slice(0,110)));

runner.deliver({id:"a1",kind:"assignment",from_member_id:"host",
  body:"Count slowly from 1 to 120, one number per line, nothing else."});
await new Promise(r=>setTimeout(r,3000));
console.log("PRE-INTERRUPT turns=", turns);
const receipt = await runner.interrupt().catch(e=>({THREW:String(e).slice(0,90)}));
console.log("INTERRUPT receipt:", JSON.stringify(receipt));

// 关键：不 close，继续投递。member 还活着吗？
await new Promise(r=>setTimeout(r,1500));
console.log("POST-INTERRUPT alive? closed=", runner.mailbox.closed, " turns=", turns);
try {
  runner.deliver({id:"a2",kind:"message",from_member_id:"host",body:"Reply with exactly: ALIVE-AFTER-INTERRUPT"});
  const ok = await wait(turns+1, 60000);
  console.log("POST-INTERRUPT delivery landed:", ok, " turns=", turns);
  const last = of("assistant_message").at(-1);
  console.log("last reply:", JSON.stringify(last?.d?.content?.[0]?.text ?? null).slice(0,80));
} catch (e) { console.log("DELIVER THREW:", String(e).slice(0,100)); }
runner.close("probe done");
await new Promise(r=>setTimeout(r,1500));
console.log("member_closed seen:", of("member_closed").length);
process.exit(0);
