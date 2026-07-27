import * as sdkmod from "@anthropic-ai/claude-agent-sdk";
import { createMemberRunner } from "../src/member-runner.mjs";

const SANDBOX = process.argv[2];
const MODE = process.argv[3];            // default | bypassPermissions
const sdk = { query: sdkmod.query, tagSession: sdkmod.tagSession, renameSession: sdkmod.renameSession };
let turns = 0, denials = 0;

const runner = createMemberRunner({
  sdk,
  config: {
    teamRunId: "trun-gate", memberRunId: "mrun-gate", memberName: "GateProbe", roleLabel: "gate",
    cwd: SANDBOX,
    ownedPaths: ["owned"],               // 只允许写 owned/
    allowedTools: ["Write", "Read"],
    permissionMode: MODE,
    settingSources: [],
    model: "claude-haiku-4-5",
  },
  emit: (e, d) => {
    if (e === "owned_path_violation") { denials++; console.log(`  [DENY] ${d.path}`); }
    if (e === "turn_complete") turns++;
    if (e === "runner_error") console.log("  [err]", JSON.stringify(d).slice(0, 120));
  },
});
const done = runner.start();
runner.deliver({ id:"m1", kind:"assignment", from_member_id:"host",
  body:`Create a file at ${SANDBOX}/owned/allowed.txt containing the word INSIDE-LANE. Use the Write tool. Do it now.` });
await new Promise(r => { const i = setInterval(() => { if (turns >= 1) { clearInterval(i); r(); } }, 200); });
runner.close("probe done");
await done;
console.log(`  mode=${MODE}  denials=${denials}`);
