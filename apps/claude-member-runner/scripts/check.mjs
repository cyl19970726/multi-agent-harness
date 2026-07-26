import { listSessions, getSessionInfo, tagSession, renameSession } from "@anthropic-ai/claude-agent-sdk";
const SID="c465a253-f7b0-4c0e-a276-aae34bb485d4", DIR=process.cwd();
const info = await getSessionInfo(SID, { dir: DIR });
console.log("  getSessionInfo:", info ? `cwd=${info.cwd}` : "NOT FOUND");
await tagSession(SID, "trun-demo:mrun-RuntimeBuilder", { dir: DIR });
await renameSession(SID, "RuntimeBuilder · SDK member", { dir: DIR });
const mine = (await listSessions({ dir: DIR })).filter(s => s.tag?.startsWith("trun-demo:"));
for (const m of mine) console.log(`  listSessions: ${m.sessionId.slice(0,8)} tag=${m.tag} title=${m.customTitle}`);
