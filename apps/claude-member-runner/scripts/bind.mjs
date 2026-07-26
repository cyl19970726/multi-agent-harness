import { tagSession, renameSession, listSessions } from "@anthropic-ai/claude-agent-sdk";
const SID="851b37dd-98da-4701-a4dd-cc9ac2d76951", DIR=process.cwd();
await tagSession(SID, "trun-live-1:mrun-RuntimeBuilder", { dir: DIR });
await renameSession(SID, "RuntimeBuilder · Runtime owner", { dir: DIR });
for (const m of (await listSessions({dir:DIR})).filter(s=>s.tag?.startsWith("trun-live-1:")))
  console.log(`  ${m.sessionId.slice(0,8)}  tag=${m.tag}  title=${m.customTitle}`);
