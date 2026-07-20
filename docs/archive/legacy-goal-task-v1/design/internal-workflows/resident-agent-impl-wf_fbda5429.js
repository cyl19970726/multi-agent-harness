export const meta = {
  name: 'resident-agent-impl',
  description: 'Implement Claude resident stream-json process + deep-explore codex exec-server vs app-server, in a worktree, build+test green',
  phases: [
    { title: 'Probe', detail: 'parallel: codex exec-server, codex app-server, claude code-path map' },
    { title: 'Decide', detail: 'codex exec-server vs app-server verdict' },
    { title: 'Implement', detail: 'claude resident module + wiring + codex decision doc, in worktree' },
    { title: 'Verify', detail: 'cargo build + test + clippy, fix loop' },
  ],
}

const WT = '<WORKTREE>'

const CODEX_PROBE = {
  type: 'object',
  required: ['service', 'transport', 'multi_conversation', 'resume_support', 'protocol_shape', 'startup_cost', 'maturity', 'pros', 'cons', 'fit_for_harness', 'evidence'],
  properties: {
    service: { type: 'string' },
    transport: { type: 'string', description: 'stdio/unix/ws and how to connect' },
    multi_conversation: { type: 'string', description: 'can one daemon host multiple concurrent conversations? evidence' },
    resume_support: { type: 'string' },
    protocol_shape: { type: 'string', description: 'JSON-RPC? methods/events observed; key message names' },
    startup_cost: { type: 'string' },
    maturity: { type: 'string', description: 'experimental? stability signals' },
    pros: { type: 'array', items: { type: 'string' } },
    cons: { type: 'array', items: { type: 'string' } },
    fit_for_harness: { type: 'string', description: 'how well it fits a per-member resident driver in a sync Rust harness' },
    evidence: { type: 'array', items: { type: 'string' }, description: 'concrete commands run + what they showed (verified vs inferred)' },
  },
}

const CODE_MAP = {
  type: 'object',
  required: ['delivery_entry', 'call_sites', 'state_owner', 'session_id_flow', 'stderr_handling', 'integration_point', 'risks', 'test_strategy'],
  properties: {
    delivery_entry: { type: 'string', description: 'fn names + line numbers for claude delivery path' },
    call_sites: { type: 'string', description: 'where run_claude_delivery is invoked (the loop that drives turns), file:line' },
    state_owner: { type: 'string', description: 'what long-lived struct/loop could own a per-member resident pool; file:line' },
    session_id_flow: { type: 'string', description: 'how session_id is extracted + stored on member.provider_thread_id' },
    stderr_handling: { type: 'string', description: 'current stderr draining; why it deadlocks for a resident (no EOF) and the fix (redirect to file)' },
    integration_point: { type: 'string', description: 'exact recommended seam to add an opt-in resident path without changing default behavior' },
    risks: { type: 'array', items: { type: 'string' } },
    test_strategy: { type: 'string', description: 'how to unit-test ResidentClaude/pool without network/auth (e.g. a fake claude script honoring stream-json), and how to gate a live test behind an env var' },
  },
}

const DECISION = {
  type: 'object',
  required: ['winner', 'rationale', 'runner_up_note', 'integration_sketch', 'adr_supersedes_0018'],
  properties: {
    winner: { type: 'string', enum: ['exec-server', 'app-server', 'neither-keep-respawn'] },
    rationale: { type: 'string' },
    runner_up_note: { type: 'string' },
    integration_sketch: { type: 'string', description: 'how the harness would drive the winner (transport, lifecycle, multi-conv)' },
    adr_supersedes_0018: { type: 'string', description: 'does choosing this conflict with / supersede ADR 0018? what the ADR revision should say' },
  },
}

const VERIFY = {
  type: 'object',
  required: ['build_passed', 'test_passed', 'clippy_clean', 'summary', 'errors'],
  properties: {
    build_passed: { type: 'boolean' },
    test_passed: { type: 'boolean' },
    clippy_clean: { type: 'boolean' },
    summary: { type: 'string' },
    errors: { type: 'string', description: 'raw compiler/test errors if any (empty if green)' },
  },
}

// ---------------- Phase 1: Probe (parallel) ----------------
phase('Probe')
const [execSrv, appSrv, codeMap] = await parallel([
  () => agent(
    `Deep-probe Codex CLI's \`codex exec-server\` (codex-cli 0.135.0 is installed at $(which codex)). Goal: decide if it can host a persistent, programmatically-driven multi-turn Codex conversation for a Rust harness.
Do this WITHOUT hanging: prefer offline introspection first — \`codex exec-server --help\`, any \`codex app-server generate-json-schema\`/\`generate-ts\` output that documents the shared protocol, codex docs. For a LIVE probe, never block: run the server in the background and hard-kill it, e.g. \`(codex exec-server --listen stdio < /dev/null & p=$!; sleep 8; kill $p 2>/dev/null)\` or use \`perl -e 'alarm 12; exec @ARGV' -- codex exec-server ...\`. Try to observe the initial handshake/framing on stdio.
Answer: transport options (stdio vs ws), whether one server hosts multiple concurrent conversations, resume support, the protocol shape (JSON-RPC? method/event names), startup cost, maturity/experimental status, and how well it fits a per-member resident driver. Mark each claim verified-from-command vs inferred-from-docs.`,
    { label: 'probe:exec-server', phase: 'Probe', schema: CODEX_PROBE }
  ),
  () => agent(
    `Deep-probe Codex CLI's \`codex app-server\` (codex-cli 0.135.0). Goal: decide if it can host a persistent, programmatically-driven multi-turn Codex conversation for a Rust harness.
Offline first: \`codex app-server --help\`, \`codex app-server daemon --help\`, \`codex app-server proxy --help\`, and CRUCIALLY \`codex app-server generate-json-schema\` and \`codex app-server generate-ts\` (these emit the FULL protocol — capture the method/notification names, conversation lifecycle, how a new conversation is created, how user turns are sent, how events stream back, resume). For a LIVE probe never block: background + hard-kill (\`(codex app-server --listen stdio:// < /dev/null & p=$!; sleep 8; kill $p)\` or perl alarm). 
Answer: transport (stdio/unix/ws), multi-conversation hosting, resume, protocol shape (concrete method names), startup cost, maturity, and fit for a per-member resident driver in a sync Rust harness. Mark verified vs inferred.`,
    { label: 'probe:app-server', phase: 'Probe', schema: CODEX_PROBE }
  ),
  () => agent(
    `Map the Claude delivery code path in the Rust repo at ${WT} so a resident stream-json process can be added without breaking the default path. The code is SYNCHRONOUS std::process (NO tokio).
Read precisely: crates/harness-cli/src/main.rs functions run_claude_delivery (~7876), run_claude_exec_delivery_real (~8006), extract_session_id_from_claude_events (~7098), claude_recorded_args (~7591), start_claude_runtime (~7830), parse_claude_stream_json; and crates/harness-core/src/lib.rs LaunchSpec (~222) + build_launch_spec (~347). Find WHERE run_claude_delivery is called from (the turn-driving loop) and what long-lived struct/loop could own a per-member resident pool (check workflow.rs and the run loop). 
Critically analyze: current code passes the prompt via argv \`-p "<content>"\` and closes stdin (Stdio::null) — the resident model instead needs \`--input-format stream-json\` with stdin held open and the user message written as a JSON frame per turn; and stderr is currently drained to EOF AFTER stdout (works only because the process exits) — for a resident process this deadlocks, so stderr must be redirected to a file. Recommend the exact seam for an OPT-IN resident path (env flag e.g. HARNESS_CLAUDE_RESIDENT=1) that leaves default behavior untouched, and a test strategy that does NOT need network/auth (a fake 'claude' script honoring the stream-json line protocol; gate any live test behind an env var).`,
    { label: 'map:claude-path', phase: 'Probe', schema: CODE_MAP }
  ),
])

log(`Probe done. exec-server fit=${execSrv?.fit_for_harness?.slice(0,60)} | app-server fit=${appSrv?.fit_for_harness?.slice(0,60)}`)

// ---------------- Phase 2: Decide ----------------
phase('Decide')
const decision = await agent(
  `Decide which Codex persistent service is better for THIS harness: exec-server vs app-server. Base it strictly on these two probe reports (JSON):

EXEC-SERVER:
${JSON.stringify(execSrv, null, 2)}

APP-SERVER:
${JSON.stringify(appSrv, null, 2)}

Context: the harness is a sync Rust runtime that drives one conversation per AgentMember and wants a persistent, programmatically-driven process. ADR 0018 previously DELETED the codex app-server WS path in favor of headless exec-stream (commit 4772af2). So picking app-server would partly reverse that decision — weigh that cost. Pick the winner (or 'neither-keep-respawn' if neither beats the current \`codex exec\` + \`exec resume\` respawn model), give the rationale, a runner-up note, an integration sketch, and whether/what ADR revision of 0018 is needed.`,
  { label: 'decide:codex', phase: 'Decide', schema: DECISION }
)
log(`Codex verdict: ${decision.winner}`)

// ---------------- Phase 3: Implement ----------------
phase('Implement')
const implReport = await agent(
  `Implement in the worktree at ${WT} (branch feat/resident-agent-process). Make small, compiling, conventional changes that match the existing code style. Do NOT git commit — leave changes in the working tree.

GROUND TRUTH (use this map, verify against the real files):
${JSON.stringify(codeMap, null, 2)}

DELIVERABLE 1 — Claude resident stream-json process (the core code change), OPT-IN and additive so default behavior is unchanged:
- Add a new module crates/harness-cli/src/resident.rs with:
  * struct ResidentClaude holding the child process + ChildStdin (held open = keep-alive) + a buffered stdout reader + the session_id. Use SYNCHRONOUS std::process (no tokio). Redirect the child's stderr to a file (Stdio from an opened File) so the never-ending process can't deadlock on an undrained stderr pipe.
  * spawn(): \`claude -p --input-format stream-json --output-format stream-json --verbose\` plus the SAME flag mapping that run_claude_exec_delivery_real builds (model/permission/mcp/add-dir/system-prompt/resume). The user prompt is NOT argv anymore — it is written to stdin as a JSON frame.
  * send_turn(user_text) -> reads NDJSON from stdout, appends each event, returns when it sees type=="result"; extracts/updates session_id. Frame format: {"type":"user","message":{"role":"user","content":[{"type":"text","text":...}]}}\\n then flush.
  * shutdown(): drop stdin (EOF) and wait, OR kill on timeout.
  * A ResidentPool keyed by (member_id, config fingerprint) with: get-or-spawn, idle reclaim (a max-idle duration; reclaim drops stdin), and crash recovery (if the child died, respawn with --resume <session_id>). Keep it simple and obvious.
- Wire it into the delivery path at the recommended seam as an OPT-IN governed by env HARNESS_CLAUDE_RESIDENT=1. When unset/false, the existing run_claude_exec_delivery_real path runs unchanged. When set, route through the resident pool. Keep the returned (success, events, session_id, stderr) shape identical so run_claude_delivery and ProviderSession recording are untouched.
- Refactor the flag-building in run_claude_exec_delivery_real into a small shared helper if that avoids duplication (only if clean).

DELIVERABLE 2 — Tests (must not need network/auth):
- Unit-test ResidentClaude/ResidentPool against a FAKE claude: a small shell/script fixture that speaks the stream-json line protocol (reads user frames from stdin, emits a system/init then a result line per turn, stays alive until stdin EOF). Make the spawned command path configurable (e.g. via env or a constructor param) so tests inject the fake. Assert: same child across two turns, session_id continuity, clean shutdown on stdin close, and crash→resume respawn.
- Do not weaken or skip existing tests.

DELIVERABLE 3 — Codex decision doc: write docs/decisions/<next-number>-codex-persistent-service-exploration.md (look at docs/decisions/ to pick the next ADR number after 0018; match the existing ADR format). Capture the exec-server vs app-server deep exploration and this verdict:
${JSON.stringify(decision, null, 2)}
Include the two probe reports' key facts and the ADR-0018 relationship. State clearly that this PR does NOT implement codex persistence (only claude); codex remains on respawn + a follow-up spike.

DELIVERABLE 4 — A short docs/ note (docs/resident-claude.md) explaining the keep-alive principle (hold stdin = no EOF), the hot/cold hybrid (resident hot path + --resume cold recovery + idle reclaim), and the HARNESS_CLAUDE_RESIDENT flag.

Report exactly which files you created/modified and the public API of resident.rs.`,
  { label: 'impl:resident+docs', phase: 'Implement' }
)
log('Implementation written. Starting verify loop.')

// ---------------- Phase 4: Verify (build+test+clippy, fix loop) ----------------
phase('Verify')
let green = false
let lastErrors = ''
for (let attempt = 1; attempt <= 5 && !green; attempt++) {
  const v = await agent(
    `In ${WT} run the build/test gate and report results. Run, capturing output:
\`cd ${WT} && cargo build --workspace 2>&1 | tail -80\`
then \`cargo test --workspace 2>&1 | tail -120\`
then \`cargo clippy --workspace --all-targets 2>&1 | tail -60\` (clippy_clean = no warnings/errors; if clippy isn't installed, set clippy_clean true and note it in summary).
Set build_passed/test_passed accordingly and put the RAW failing errors (compiler errors, failing test names + assertion output) into errors verbatim. Do not fix anything in this step — only report.`,
    { label: `verify:attempt-${attempt}`, phase: 'Verify', schema: VERIFY }
  )
  if (v.build_passed && v.test_passed) {
    green = true
    log(`Verify GREEN on attempt ${attempt} (clippy_clean=${v.clippy_clean})`)
    break
  }
  lastErrors = v.errors
  log(`Attempt ${attempt} red: ${v.summary?.slice(0, 120)}`)
  await agent(
    `Fix the build/test failures in the worktree at ${WT}. Do NOT git commit. Keep changes minimal and matching code style; do not delete or skip tests to make them pass — fix the real cause. Errors:\n\n${lastErrors}`,
    { label: `fix:attempt-${attempt}`, phase: 'Verify' }
  )
}

return {
  codex_winner: decision.winner,
  verify_green: green,
  impl_summary: implReport?.slice(0, 800),
  remaining_errors: green ? '' : lastErrors.slice(0, 2000),
}
