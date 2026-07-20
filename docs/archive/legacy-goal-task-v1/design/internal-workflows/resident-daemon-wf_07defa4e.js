export const meta = {
  name: 'resident-daemon',
  description: 'Build a harness daemon that hosts a ResidentPool over a Unix socket for true cross-CLI-invocation Claude residency; revise ADR 0018; build+test green',
  phases: [
    { title: 'Design', detail: 'daemon architecture + IPC protocol + ADR-0018 revision' },
    { title: 'Implement', detail: 'daemon module + UDS server + CLI subcommands + client routing, in worktree' },
    { title: 'Review', detail: 'adversarial review of IPC/concurrency/lifecycle' },
    { title: 'Fix', detail: 'address confirmed findings' },
    { title: 'Verify', detail: 'cargo build + test + clippy, fix loop' },
  ],
}

const WT = '<WORKTREE>'

const DESIGN = {
  type: 'object',
  required: ['components', 'ipc_protocol', 'socket_path', 'concurrency', 'lifecycle', 'cli_routing', 'fallback', 'serde_changes', 'adr_changes', 'test_plan', 'risks'],
  properties: {
    components: { type: 'string', description: 'new modules/structs/functions and how they relate to existing resident.rs (ResidentPool/ResidentClaude/ResidentConfig)' },
    ipc_protocol: { type: 'string', description: 'request/response framing over the socket (line-delimited JSON); exact request and response field shapes' },
    socket_path: { type: 'string', description: 'where the socket lives + discovery + stale-socket cleanup' },
    concurrency: { type: 'string', description: 'serial accept loop vs thread-per-conn + Mutex<ResidentPool>; per-member serialization; chosen approach + why' },
    lifecycle: { type: 'string', description: 'daemon start/stop/status, idle reclaim trigger, graceful shutdown, SIGTERM/SIGINT handling without new crates' },
    cli_routing: { type: 'string', description: 'how run_claude_resident_delivery_real connects to the daemon and maps the response into the (success,events,session_id,stderr) tuple' },
    fallback: { type: 'string', description: 'behavior when HARNESS_CLAUDE_RESIDENT=1 but no daemon socket present (graceful degrade to inline)' },
    serde_changes: { type: 'string', description: 'which existing types (ResidentConfig/ResidentEvent) need Serialize/Deserialize; confirm additive' },
    adr_changes: { type: 'string', description: 'exact text plan: new ADR (e.g. 0021) amending 0018; what 0018 status note becomes; why the daemon does NOT violate "official headless exec-stream" (children still driven via stream-json)' },
    test_plan: { type: 'string', description: 'integration test that starts the daemon against the fake-claude and uses TWO separate socket connections for one member to prove the child stays warm across invocations; no network/auth' },
    risks: { type: 'array', items: { type: 'string' } },
  },
}

const REVIEW = {
  type: 'object',
  required: ['findings'],
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        required: ['severity', 'area', 'title', 'detail', 'fix'],
        properties: {
          severity: { type: 'string', enum: ['critical', 'high', 'medium', 'low'] },
          area: { type: 'string', enum: ['ipc', 'concurrency', 'lifecycle', 'error-handling', 'fallback', 'tests', 'style', 'other'] },
          title: { type: 'string' },
          detail: { type: 'string', description: 'concrete file:line + why it is wrong' },
          fix: { type: 'string' },
        },
      },
    },
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
    errors: { type: 'string' },
  },
}

// ---------------- Phase 1: Design ----------------
phase('Design')
const design = await agent(
  `Design a harness daemon that hosts the existing ResidentPool over a Unix domain socket, so a resident \`claude\` child stays warm across SEPARATE \`harness deliver\` CLI invocations (the harness CLI is short-lived per delivery; only a long-lived daemon can own the child's stdin).

Read the worktree at ${WT}: crates/harness-cli/src/resident.rs (ResidentClaude/ResidentPool/ResidentConfig/ResidentEvent — already implemented & tested; REUSE, don't rewrite), crates/harness-cli/src/main.rs run_claude_resident_delivery_real (~8169), build_resident_config, run_claude_delivery (~7877), deliver_agent_messages_value (~3830 for-queued loop), run_provider_delivery (~7761), and how subcommands are dispatched in main(). Also read docs/decisions/0018-exec-stream-primary-substrate.md (we are amending it).

Hard constraints:
- NO new crates. Use std::os::unix::net::{UnixListener, UnixStream}, std::thread, std::sync::{Arc, Mutex}, serde_json. Unix-only is fine (cfg(unix)).
- Children are STILL driven via the official headless stream-json contract (the daemon just keeps those exec-stream processes warm) — make the ADR argument that this does NOT reintroduce a custom *provider* protocol, so it amends rather than reverses ADR 0018.
- Opt-in: only used when HARNESS_CLAUDE_RESIDENT=1; when set but no daemon socket exists, degrade gracefully to the current inline path. Default behavior (flag unset) totally unchanged.
- The socket request must carry enough to call pool.run_turn(member_id, config, stderr_path, user_text, timeout); response carries (success, events, session_id) with stderr via a path. ResidentConfig/ResidentEvent likely need Serialize/Deserialize (additive derives).
- Keep it the simplest correct design. Prefer thread-per-connection with Arc<Mutex<ResidentPool>> (children serialize naturally under the lock); idle reclaim called opportunistically.

Produce a precise, buildable design.`,
  { label: 'design:daemon', phase: 'Design', schema: DESIGN }
)
log(`Design ready. Concurrency: ${design.concurrency?.slice(0, 80)}`)

// ---------------- Phase 2: Implement ----------------
phase('Implement')
const impl = await agent(
  `Implement the harness resident daemon in the worktree at ${WT} (branch feat/resident-agent-process). Build on the EXISTING resident.rs (do not rewrite it; extend minimally — e.g. add Serialize/Deserialize derives, make pool/fields used so #[allow(dead_code)] can be removed where now used). Do NOT git commit. Small, compiling, conventional changes matching code style.

Follow this design (verify against real files; correct it where the code disagrees):
${JSON.stringify(design, null, 2)}

Required deliverables:
1. A daemon module (e.g. crates/harness-cli/src/resident_daemon.rs, cfg(unix)) implementing: a UnixListener server hosting Arc<Mutex<ResidentPool>>; per-connection handling that reads a JSON request, calls pool.run_turn(...), writes a JSON response; stale-socket cleanup on start; opportunistic idle reclaim; graceful shutdown.
2. CLI subcommands wired into main()'s dispatch: \`harness daemon start [--socket <path>]\` (runs the server; foreground), \`harness daemon status\`, \`harness daemon stop\`. Default socket path under the store root (.harness/resident.sock). Match how existing subcommands are parsed/dispatched.
3. Client routing: change run_claude_resident_delivery_real so that when a daemon socket exists it sends the delivery over the socket and maps the response into the SAME (success, events, session_id, stderr) tuple; when no socket exists, keep the current inline single-turn behavior as the graceful fallback. run_claude_delivery and ProviderSession recording stay untouched.
4. Add Serialize/Deserialize to ResidentConfig and ResidentEvent if needed (additive).
5. Tests (no network/auth): an integration-style test that starts the daemon (a thread or subprocess) pointed at the fake-claude binary, then makes TWO SEPARATE socket connections for the SAME member_id and asserts ONE child served both turns (PID file has one line) and session_id continuity — proving cross-invocation warmth. Reuse/extend the fake-claude fixture pattern from resident.rs. Do not weaken existing tests.
6. Docs: a new ADR docs/decisions/0021-resident-daemon.md amending 0018 (pick the next free number; update docs/decisions/README.md and add a status note to 0018 pointing to 0021). Update docs/resident-claude.md to document the daemon, the socket, the lifecycle commands, and the hot(daemon)/cold(--resume)/idle model.

Report every file created/modified and the daemon's request/response JSON shape.`,
  { label: 'impl:daemon', phase: 'Implement' }
)
log('Daemon implemented. Running adversarial review.')

// ---------------- Phase 3: Review ----------------
phase('Review')
const review = await agent(
  `Adversarially review the resident daemon implementation in the worktree at ${WT}. Read the new daemon module, the resident.rs changes, the CLI routing in run_claude_resident_delivery_real, and the daemon subcommand dispatch in main(). Hunt for REAL defects, default to skepticism:
- IPC: partial reads/writes, missing newline framing, request/response desync, large stderr, non-UTF8, malformed JSON handling.
- Concurrency: deadlock holding the pool Mutex across a blocking send_turn (does one slow member block all others? is that acceptable / documented?); poisoned mutex; thread leaks.
- Lifecycle: stale socket on crash, double-start, EADDRINUSE, shutdown races, zombie/leaked claude children on daemon exit, idle reclaim correctness.
- Fallback: flag set + no socket + socket present-but-dead daemon (connect refused) — does it degrade cleanly or hang?
- Tests: do they actually prove cross-invocation warmth (separate connections, one child), or do they accidentally reuse one connection?
Only report defects you can tie to specific code (file:line) with a concrete fix. Empty findings list is allowed if it is genuinely clean.`,
  { label: 'review:daemon', phase: 'Review', schema: REVIEW }
)
const blocking = (review.findings || []).filter(f => f.severity === 'critical' || f.severity === 'high')
log(`Review: ${review.findings?.length || 0} findings (${blocking.length} blocking).`)

// ---------------- Phase 4: Fix ----------------
if (blocking.length > 0) {
  phase('Fix')
  await agent(
    `Fix these confirmed review findings in the worktree at ${WT}. Do NOT git commit. Minimal, correct fixes matching code style; do not weaken or skip tests. Re-verify each fix addresses the root cause.\n\n${JSON.stringify(blocking, null, 2)}`,
    { label: 'fix:review', phase: 'Fix' }
  )
}

// ---------------- Phase 5: Verify (build+test+clippy loop) ----------------
phase('Verify')
let green = false
let lastErrors = ''
for (let attempt = 1; attempt <= 6 && !green; attempt++) {
  const v = await agent(
    `In ${WT} run the gate and report (do not fix here):
\`cd ${WT} && cargo build --workspace 2>&1 | tail -80\`
\`cargo test --workspace 2>&1 | tail -140\`
\`cargo clippy --workspace --all-targets 2>&1 | tail -60\` (clippy_clean=no warnings; if clippy missing set true + note).
Put RAW failing errors verbatim into errors.`,
    { label: `verify:attempt-${attempt}`, phase: 'Verify', schema: VERIFY }
  )
  if (v.build_passed && v.test_passed) {
    green = true
    log(`Verify GREEN on attempt ${attempt} (clippy_clean=${v.clippy_clean}): ${v.summary?.slice(0,100)}`)
    break
  }
  lastErrors = v.errors
  log(`Attempt ${attempt} red: ${v.summary?.slice(0, 120)}`)
  await agent(
    `Fix the build/test failures in the worktree at ${WT}. Do NOT git commit. Minimal, root-cause fixes; do not skip tests. Errors:\n\n${lastErrors}`,
    { label: `fix:attempt-${attempt}`, phase: 'Verify' }
  )
}

return {
  verify_green: green,
  review_findings: review.findings?.length || 0,
  blocking_fixed: blocking.length,
  impl_summary: impl?.slice(0, 1000),
  remaining_errors: green ? '' : lastErrors.slice(0, 2000),
}
