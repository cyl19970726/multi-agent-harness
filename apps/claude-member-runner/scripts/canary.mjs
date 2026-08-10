import * as sdkmod from "@anthropic-ai/claude-agent-sdk";
import { createMemberRunner } from "../src/member-runner.mjs";
const SB = process.argv[2];
const sdk = { query: sdkmod.query, tagSession: sdkmod.tagSession, renameSession: sdkmod.renameSession };
const ev = [];
let turns = 0;
const runner = createMemberRunner({
  sdk,
  config: { teamRunId:"trun-canary", memberRunId:"mrun-canary", memberName:"Canary", roleLabel:"canary",
            cwd: SB, ownedPaths:["lane"], allowedTools:["Write","Read"], settingSources:[], model:"claude-haiku-4-5" },
  emit: (e,d) => { ev.push({e,d}); if (e==="turn_complete") turns++; },
});
const of = n => ev.filter(x=>x.e===n);
// Bounded. An unbounded spin here hung the first canary run for 10 minutes:
// if a turn never lands, the probe must report that, not wait forever.
const wait = (n, ms=120000) => new Promise(r=>{
  const t0=Date.now();
  const i=setInterval(()=>{
    if (turns>=n) { clearInterval(i); r(true); }
    else if (Date.now()-t0>ms) { clearInterval(i); console.log(`  !! timeout waiting for turn ${n} (have ${turns})`); r(false); }
  },150);
});
const done = runner.start();

// --- D1: durable Work enters the persistent member ---
runner.deliver({ id:"work-canary", kind:"work", sender_runtime_id:"host",
  body:`WORK: Propose a concise Markdown plan for writing the word READY into ${SB}/lane/ready.txt. Do not execute yet.` });
await wait(1);
const existsBefore = (await import("node:fs")).existsSync(`${SB}/lane/ready.txt`);
console.log(`D1 planned: file_exists_before_execute=${existsBefore}`);

// --- D2: Host replies with an ordinary execute message ---
runner.deliver({ id:"p2", kind:"message", sender_runtime_id:"host", work_id:"work-canary", correlation_id:"c1",
  body:`EXECUTE: The plan is accepted. Now write READY into ${SB}/lane/ready.txt using the Write tool.` });
await wait(2);
const existsAfter = (await import("node:fs")).existsSync(`${SB}/lane/ready.txt`);
console.log(`D2 executed: file_exists=${existsAfter}`);

// --- D3: steer ---
try { await runner.setPermissionMode("acceptEdits");
      console.log(`D3 steer:   ok  -> ${JSON.stringify(of("permission_mode_changed").at(-1)?.d)}`); }
catch (e) { console.log("D3 steer:   THREW", String(e).slice(0,90)); }

// --- D4: interrupt（真实终态确认） ---
runner.deliver({ id:"work-interrupt", kind:"work", sender_runtime_id:"host",
  body:"Count slowly from 1 to 120, one number per line, no other text." });
await new Promise(r=>setTimeout(r,2500));         // 让它真的在生成
try { const receipt = await runner.interrupt();
      console.log(`D4 interrupt: receipt=${JSON.stringify(receipt)}  emitted=${JSON.stringify(of("interrupted").at(-1)?.d)}`); }
catch (e) { console.log("D4 interrupt: THREW", String(e).slice(0,90)); }

runner.close("closed_by_host");
await done;
console.log(`closed: ${JSON.stringify(of("member_closed").at(-1)?.d?.reason)}  turns=${turns}`);
