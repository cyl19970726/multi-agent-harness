export const meta = {
  name: 'agent-live-tui-design',
  description: 'Design a Claude-Code-TUI-style live agent activity view (tool calls, thinking, output) for the chat',
  phases: [
    { title: 'Audit', detail: 'streaming feasibility + ingest/SSE + frontend live + real event shapes' },
    { title: 'Design', detail: 'backend live-stream + frontend TUI rendering' },
    { title: 'Synthesize', detail: 'one implementable design + WP plan' },
  ],
}

const REPO = '<REPO>'

const AUDIT = {
  type: 'object',
  additionalProperties: false,
  required: ['area', 'findings', 'feasibility'],
  properties: {
    area: { type: 'string' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['claim', 'files'],
        properties: { claim: { type: 'string' }, files: { type: 'array', items: { type: 'string' } } },
      },
    },
    feasibility: { type: 'string', description: 'what is achievable, what needs new code' },
  },
}

phase('Audit')
const AUDITS = [
  {
    key: 'backend-streaming',
    prompt: `Repo ${REPO}. Determine whether the agent delivery can stream events to the frontend MID-TURN (for a live TUI), or only at turn end. With file:line:
- crates/harness-cli/src/main.rs run_claude_exec_delivery_real (~8252): it parses claude stream-json line-by-line via parse_claude_stream_json but COLLECTS into a Vec and writes the ndjson file ONCE at ~8134 (fs::write(&ndjson_ref, ...)). Confirm. Could it instead APPEND each parsed line to the session jsonl as it reads (incremental), and is the ProviderSession row created at START (status=running) with jsonl_ref set early, or only at end? Look at the claim/running-session journaling (build_claimed_provider_session / record_claimed_delivery_terminal).
- run_codex_exec_process: same question (does it stream stdout to a file live?).
State exactly what backend change enables incremental streaming (append-as-parsed + early session row with jsonl_ref). Report feasibility.`,
  },
  {
    key: 'ingest-and-sse',
    prompt: `Repo ${REPO}. Audit the existing ingest + SSE machinery for reuse in a live TUI. With file:line:
- crates/harness-cli/src/main.rs ingest_claude_stream_json (~7313) — what does it produce (AgentEvents? one per stream event?), and WHO calls it (line ~4826 — is that the resident path or a separate ingest command)? Does it run during the one-shot delivery?
- crates/harness-cli/src/sse.rs — the watcher: which jsonl files does it tail, what frames does it broadcast (AgentEvent/Message/ProviderSession/WorkflowRun/WorkflowStep)? Could it tail a session's claude.stream-json.ndjson and broadcast raw-turn-event frames live, OR is per-event AgentEvent emission the better channel?
- The GET /v1/provider-sessions/{id}/events route (read_provider_session_events) — does it re-read the file each call (so polling a growing file works for live)?
Report which channel (poll the events route while running vs SSE per-event) is lower-risk for live.`,
  },
  {
    key: 'frontend-live',
    prompt: `Repo ${REPO}. Audit the frontend for a live TUI turn view. With file:line:
- apps/agent-dashboard/src/surfaces/Surfaces.tsx TurnDrillIn (fetches /v1/provider-sessions/{id}/events ONCE on expand) + summarizeRawEvent (handles system/assistant/user/result + codex item). What it would take to (a) auto-open + LIVE-poll the events for a RUNNING session (re-fetch every ~1s until the session is no longer running), (b) render a TUI: assistant text, tool_use (name + input), tool_result (output), THINKING blocks (currently unhandled), result.
- CurrentWorkBanner / the running-session detection (model.sessionsByMember status==="running"). Where would the live turn view mount (in the Conversation tab above the composer? a dedicated live region?).
- The SSE handler (api.ts) — if we go SSE per-event, what frame type to add.
Report the cleanest frontend approach (poll-while-running vs SSE) and the TUI component shape.`,
  },
  {
    key: 'real-event-shapes',
    prompt: `Repo ${REPO}. Read an ACTUAL captured claude stream-json file to get the REAL event shapes for the TUI renderer. Find a file under ${REPO}/.harness/provider-sessions/*/claude.stream-json.ndjson (ls the dir, pick the largest/most recent) and read it. Extract the exact JSON shape of each event type: system/init, assistant (message.content[] with type text / thinking / tool_use — show the tool_use fields name/input/id), user (tool_result content), result. ALSO check codex: a codex stdout ndjson under .harness/provider-sessions (item.completed with item.type agent_message / reasoning / command_execution). Report the precise field paths the frontend summarizeRawEvent must read to render tool calls + thinking + results faithfully. If no thinking/tool_use events exist in captured files (simple PONG turns), say so and give the shapes from the claude stream-json spec.`,
  },
]
const audits = (await parallel(
  AUDITS.map((a) => () => agent(a.prompt, { label: `audit:${a.key}`, phase: 'Audit', schema: AUDIT, agentType: 'Explore' })),
)).filter(Boolean)
log(`audited ${audits.length} areas`)

phase('Design')
const AUDIT_CTX = JSON.stringify(audits)
const DESIGN = {
  type: 'object',
  additionalProperties: false,
  required: ['angle', 'backend_plan', 'frontend_plan', 'mockup', 'tradeoffs'],
  properties: {
    angle: { type: 'string' },
    backend_plan: { type: 'string' },
    frontend_plan: { type: 'string' },
    mockup: { type: 'string', description: 'ASCII mockup of the live TUI in the chat' },
    tradeoffs: { type: 'string' },
  },
}
const ANGLES = [
  { key: 'poll-incremental', angle: 'Incremental jsonl write + frontend polls the events route while the session is running (no new SSE frame type). Simplest path to live.' },
  { key: 'sse-per-event', angle: 'Backend emits a per-stream-event SSE frame (raw turn event) during delivery; frontend renders as it arrives. Truest live, more backend.' },
  { key: 'post-turn-rich', angle: 'No backend change: keep one-shot, but render the full turn richly (tool_use/thinking/tool_result/text) and auto-open the drill-in the moment the turn completes. Near-live, lowest risk.' },
]
const designs = (await parallel(
  ANGLES.map((a) => () =>
    agent(
      `Repo ${REPO}. Design a Claude-Code-TUI-style live agent activity view for the chat from this angle: "${a.angle}".
Owner intent: after sending a message, SEE the agent working — tool calls, thinking, tool results, output — like Claude Code's own TUI, showing all the content. REUSE the existing TurnDrillIn / summarizeRawEvent / events route / #60 chat where possible.
Audit context: ${AUDIT_CTX}
Give a concrete backend_plan (what changes, or "none"), frontend_plan (the TUI component + how it goes live), an ASCII mockup of the live TUI in the conversation, and tradeoffs (latency, risk, effort).`,
      { label: `design:${a.key}`, phase: 'Design', schema: DESIGN },
    ),
  ),
)).filter(Boolean)
log(`got ${designs.length} designs`)

phase('Synthesize')
const FINAL = {
  type: 'object',
  additionalProperties: false,
  required: ['recommendation', 'design_doc_markdown', 'wps', 'open_questions'],
  properties: {
    recommendation: { type: 'string', description: 'which angle (or hybrid) and why' },
    design_doc_markdown: { type: 'string', description: 'complete implementable design: backend stream changes (if any), frontend TUI component, event-shape rendering table, live mechanism, ASCII mockup, sequenced WPs with files' },
    wps: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['wp', 'title', 'kind', 'files'],
        properties: { wp: { type: 'string' }, title: { type: 'string' }, kind: { type: 'string' }, files: { type: 'array', items: { type: 'string' } } },
      },
    },
    open_questions: { type: 'array', items: { type: 'string' } },
  },
}
const final = await agent(
  `Synthesize ONE implementable design for a Claude-Code-TUI-style live agent activity view in ${REPO}. Pick the best angle or a staged hybrid (e.g. ship post-turn-rich first, then incremental-poll live) and justify. Reuse-first. Be concrete about the exact claude/codex event field paths to render (tool_use name+input, thinking, tool_result, text). 
Audit context: ${AUDIT_CTX}
Designs: ${JSON.stringify(designs)}
Return recommendation, the full design_doc_markdown (with ASCII mockup + event-shape rendering table + sequenced WPs with exact files), wps[], and open_questions.`,
  { label: 'synthesize', phase: 'Synthesize', schema: FINAL },
)
return final
