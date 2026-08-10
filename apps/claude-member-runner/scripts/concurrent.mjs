import * as sdkmod from "@anthropic-ai/claude-agent-sdk";
import { createMemberRunner } from "./apps/claude-member-runner/src/member-runner.mjs";
const sdk = { query: sdkmod.query, tagSession: sdkmod.tagSession, renameSession: sdkmod.renameSession };
let bound=null, turns=0;
const runner = createMemberRunner({
  sdk,
  config: { teamRunId:"trun-live-1", memberRunId:"mrun-RuntimeBuilder", memberName:"RuntimeBuilder",
            roleLabel:"Runtime owner", cwd:process.cwd(), allowedTools:[], settingSources:[],
            resumeSessionId:"851b37dd-98da-4701-a4dd-cc9ac2d76951" },
  emit:(e,d)=>{ if(e==="session_bound"){bound=d.sessionId;console.log("  [bound]",d.sessionId);}
                if(e==="turn_complete"){turns++;}
                if(e==="assistant_message"){const t=(d.content??[]).filter(b=>b.type==="text").map(b=>b.text).join(" ");console.log("  [reply]",t.slice(0,60));}
                if(e==="runner_error"||e==="registry_write_failed") console.log("  ["+e+"]",JSON.stringify(d).slice(0,120)); },
});
const done = runner.start();
runner.deliver({id:"m3",kind:"message",sender_runtime_id:"host",correlation_id:"corr-1",
                body:"Reply with exactly: RESUMED-AFTER-IMPORT"});
await new Promise(r=>{const i=setInterval(()=>{if(turns>=1){clearInterval(i);r();}},200);});
runner.close("test_done");
await done;
console.log("  resumed_session_id =", bound);
