import { tagSession, renameSession, listSessions } from "@anthropic-ai/claude-agent-sdk";
const SID="dfda3000-4f1f-4a44-b38e-20479c745da7", DIR=process.cwd();
await tagSession(SID, "team-run-1785087229492-p4132-0:member-run-1785087229493-p4132-1", { dir: DIR });
await renameSession(SID, "SmokeMember · Smoke tester", { dir: DIR });
for (const m of (await listSessions({dir:DIR})).filter(s=>s.tag?.startsWith("team-run-")))
  console.log(`  ${m.sessionId.slice(0,8)}  tag=${m.tag}`);
