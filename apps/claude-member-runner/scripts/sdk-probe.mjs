import { query } from "@anthropic-ai/claude-agent-sdk";

let sessionId = null;
try {
  for await (const m of query({
    prompt: "Reply with exactly: SDK-PONG",
    options: { cwd: process.cwd(), allowedTools: [], settingSources: [] },
  })) {
    if (m.type === "system" && m.subtype === "init") sessionId = m.session_id;
    if (m.type === "result") {
      sessionId = m.session_id ?? sessionId;
      console.log("subtype:", m.subtype);
      console.log("result :", String(m.result ?? "").slice(0, 120));
    }
  }
} catch (e) {
  console.log("THREW:", String(e).slice(0, 200));
}
console.log("session_id:", sessionId);
