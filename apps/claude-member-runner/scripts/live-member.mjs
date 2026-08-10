import * as sdkmod from "@anthropic-ai/claude-agent-sdk";
import { createMemberRunner } from "./apps/claude-member-runner/src/member-runner.mjs";

const sdk = { query: sdkmod.query, tagSession: sdkmod.tagSession, renameSession: sdkmod.renameSession };
const turns = [];
let sessionId = null;

const runner = createMemberRunner({
  sdk,
  config: {
    teamRunId: "trun-live-1", memberRunId: "mrun-RuntimeBuilder",
    memberName: "RuntimeBuilder", roleLabel: "Runtime owner",
    cwd: process.cwd(), allowedTools: [], settingSources: [],
  },
  emit: (event, data) => {
    if (event === "session_bound") { sessionId = data.sessionId; console.log(`  [${event}] ${data.tag} | ${data.title}`); }
    else if (event === "turn_complete") { turns.push(Date.now()); console.log(`  [${event}] #${turns.length}`); }
    else if (event === "assistant_message") {
      const t = (data.content ?? []).filter(b=>b.type==="text").map(b=>b.text).join(" ");
      console.log(`  [reply] ${t.slice(0,70)}`);
    }
    else if (event === "member_closed") console.log(`  [${event}] reason=${data.reason}`);
  },
});

const done = runner.start();
const waitTurns = (n) => new Promise(r => { const i=setInterval(()=>{ if(turns.length>=n){clearInterval(i);r();} },200); });

console.log("→ 投递 durable Work");
runner.deliver({ id:"work-1", kind:"work", sender_runtime_id:"host",
                 body:"You are RuntimeBuilder. Reply with exactly: WORK-ACK" });
await waitTurns(1);

console.log("→ 空档 3 秒（旧设计在这里 member 就死了）");
await new Promise(r=>setTimeout(r,3000));
console.log(`   mailbox.pending=${runner.mailbox.pending} closed=${runner.mailbox.closed}`);

console.log("→ 空档后再投一条 peer 消息");
runner.deliver({ id:"m2", kind:"message", sender_runtime_id:"peer-Dashboard", work_id:"work-1", correlation_id:"corr-1",
                 body:"Peer here. Reply with exactly: SECOND-TURN-OK" });
await waitTurns(2);

runner.close("closed_by_host");
await done;
console.log(`\nSESSION_ID=${sessionId}`);
console.log(`turns=${turns.length}`);
