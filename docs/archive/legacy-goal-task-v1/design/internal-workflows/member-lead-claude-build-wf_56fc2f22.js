export const meta = {
  name: 'member-lead-claude-build',
  description: 'Two parallel autonomous tracks: FE (wire actions, real health, MemberWorkbench, Lead surfacing, polling, kanban) and BE (Claude provider via CLI shape). Each WP gated + auto-merged.',
  phases: [
    { title: 'FE track' },
    { title: 'BE track' },
    { title: 'Wrap' },
  ],
}

const RULES = [
  'HARD RULES (non-negotiable):',
  '- NEVER use --no-verify. NEVER disable/weaken a test or check to make a gate pass. Fix the real cause.',
  '- The gate must pass BEFORE you commit. If after honest effort (~3 serious attempts) a gate still fails, STOP, do NOT commit/merge, and return a report whose FIRST LINE is exactly "GATE_FAILED:" then the failing check + log tail.',
  '- You operate on a clean checkout in your cwd. Create your WP branch off the latest master, implement, gate, commit, push, open PR, MERGE it (gh pr merge --squash), then `git checkout master && git pull --ff-only` so the next WP builds on merged work.',
  '- Commit messages end with: Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>',
  '- The full approved design is in repo file .harness-ml-plan.md — READ IT for exact file:line anchors and field names. Do NOT git-add .harness-ml-plan.md or .harness-genplan.md (scratch files).',
  '- ADR 0011 provider-neutrality: harness core stays provider/domain-neutral; provider-specifics live in the CLI provider layer only.',
].join('\n')

const FE_GATE = [
  'GATE (all must pass before commit):',
  '  npx tsc -p apps/agent-dashboard/tsconfig.json --noEmit',
  '  npx vite build --config apps/agent-dashboard/vite.config.ts',
  '  npx pnpm@9.15.4 check  2>&1 | tail -30',
  'For UI changes, also screenshot self-review: start vite on --port 5199, agent-browser open, nav via button[aria-label="..."], screenshot to /tmp/, sanity-check it renders.',
].join('\n')

const BE_GATE = [
  'GATE (all must pass before commit):',
  '  cargo test 2>&1 | tail -40   (workspace builds AND tests pass; Codex path must stay regression-clean)',
  '  npx pnpm@9.15.4 check 2>&1 | tail -30',
].join('\n')

const finish = (branch, title) => [
  'FINISH (only after gate green):',
  `  git add -A (NOT the .harness-*.md scratch files) && git commit (title: "${title}"; end with Co-Authored-By line)`,
  `  git push -u origin ${branch}`,
  `  gh pr create --base master --head ${branch} --title "${title}" --body "<summary + gate results>"`,
  `  gh pr merge ${branch} --squash`,
  '  git checkout master && git pull --ff-only',
  'Return: gate results, files changed, PR number/URL, confirm master updated. If gate could not go green: first line "GATE_FAILED:" and do NOT merge.',
].join('\n')

// =================== FE TRACK (serial WP1..WP5 + kanban) ===================
const feTrack = (async () => {
  phase('FE track')

  const wp1 = await agent([
    RULES, '',
    'You are FE-WP1: wire the dead write actions to the REAL backend routes. This is the P0 from the review.',
    'Today the UI posts /v1/actions/message-member, /v1/actions/deliver-queued, /v1/actions/request-review — none exist on the backend. Real routes (verified): POST /v1/messages (body has from/to/content/kind/task), POST /v1/agents/{id}/deliver, /v1/agents/{id}/retry-delivery, /v1/agents/{id}/reconcile-session, /v1/agents/{id}/close, POST /v1/tasks/{id}/request-review. The id/task goes in the URL PATH, not the body.',
    'Scope: add apps/agent-dashboard/src/api/actions.ts (or extend api.ts) mapping each UI action to the correct {method, path, body}; repoint the call sites in Surfaces.tsx (~433,443,1148,1858,1866) and the Inspector in WorkbenchShell.tsx; keep the actionsEnabled gating. Verify against the running backend if feasible (cargo run -p harness-cli -- serve on :8787 in background, then exercise one action), else verify the path/body shapes against handle_http_action in crates/harness-cli/src/main.rs (~2226-2311).',
    FE_GATE, finish('task/fe-wp1-wire-actions', 'FE-WP1: wire write actions to real backend routes (fix P0 dead buttons)'),
  ].join('\n'), { label: 'FE-WP1 actions', phase: 'FE track' })
  if (typeof wp1 === 'string' && wp1.startsWith('GATE_FAILED')) return { stoppedAt: 'FE-WP1', report: wp1 }

  const wp2 = await agent([
    RULES, '',
    'You are FE-WP2: replace fake runtime-health with the REAL four layers. master has FE-WP1 merged.',
    'Today HealthCell uses Boolean(field) presence checks (Surfaces.tsx ~1896-1899). The backend already emits member.runtime_health = {process_alive, socket_exists, protocol_probe, delivery_probe, checked_at} (main.rs ~6585). Render four separated rows: Process(process_alive+runtime_pid), Endpoint(socket_exists+control_endpoint), Protocol(protocol_probe string; null=unknown→amber), Delivery(delivery_probe; null=unknown→amber). Show checked_at freshness. A null/unknown probe must render amber "unknown", NOT green. Add runtime_health (+process_alive/socket_exists/protocol_probe/delivery_probe/checked_at) to the AgentMember type in types.ts if missing, and stop dropping runtime_pid/runtime_id.',
    FE_GATE, finish('task/fe-wp2-real-health', 'FE-WP2: render real runtime_health four layers (process/endpoint/protocol/delivery)'),
  ].join('\n'), { label: 'FE-WP2 health', phase: 'FE track' })
  if (typeof wp2 === 'string' && wp2.startsWith('GATE_FAILED')) return { wp1, stoppedAt: 'FE-WP2', report: wp2 }

  const wp3 = await agent([
    RULES, '',
    'You are FE-WP3: refactor MemberWorkbench to the spec (docs/dashboard/pages/agent-member-workbench.md). master has FE-WP1+WP2.',
    'Build: a routable member view (/members/:memberId reachable; if no router exists, keep the surface but make the member id URL-addressable via the existing selection state — do not add react-router unless trivial); identity header band (avatar toned by delivery health, name, role badge, provider badge neutral, status/runtime_status, Lead chip placeholder if role==lead or id==team.owner_agent_id); inbox/outbox split (messages filtered by to_agent_id/from_agent_id with delivery_status, distinct counts); merged chronological timeline (task assignment + reports + sessions + events + evidence + delivery + proposals + reviews authored by this member from reviews filtered by reviewer_agent_id), sorted so assignment precedes report (ascending then display), give warnings a synthetic timestamp so they do not sink, remove the hard slice(0,12) cap → scroll; render the already-computed sessionsByMember + childThreadsByMember (+ provider_child_thread_count, provider_agent_path/nickname/role), current_proposal_id, team_ids. readModel: add reviewsByMember, inboxMessages/outboxMessages; stop dropping the fields above. Backend already emits everything — this is frontend-only. Keep the dark operator-console design system + existing primitives.',
    FE_GATE, 'Screenshot /tmp/fe-wp3-member.png.',
    finish('task/fe-wp3-member', 'FE-WP3: MemberWorkbench refactor — real workbench, sessions/child-threads, sorted timeline'),
  ].join('\n'), { label: 'FE-WP3 member', phase: 'FE track' })
  if (typeof wp3 === 'string' && wp3.startsWith('GATE_FAILED')) return { wp1, wp2, stoppedAt: 'FE-WP3', report: wp3 }

  const wp4 = await agent([
    RULES, '',
    'You are FE-WP4: surface the Agent Lead without inventing schema. master has FE-WP1..3. NO dedicated 6th rail surface (owner decision).',
    'Derivation (frontend-only): leadMemberId = team.owner_agent_id (authoritative); treat member.role==="lead" as a Lead role. (a) Team-header "Lead band": in TeamWorkspace + TeamRail, stable-sort roleGroups lead→critic→worker→observer→other (Lead group first), badge the owner_agent_id member with a "Lead / Owner" chip; tie the active goal owner to the Lead chip. (b) Enriched Member view: when selected member is the Lead, add a Lead responsibilities lane from existing objects — goal_designs owned by this agent, outbox Message(kind=task) assignments, decisions authored, goal_evaluations owned, team member_ids composition. (c) Decision-queue: add an "Awaiting Lead decision" partition keyed to owner_agent_id. (d) Advisory warning when owner_agent_id member role !== "lead" (the unbound-Lead gap) — add to warnings.ts as a low/medium kind lead_owner_role_mismatch.',
    FE_GATE, 'Screenshot /tmp/fe-wp4-team.png and /tmp/fe-wp4-lead-member.png.',
    finish('task/fe-wp4-lead', 'FE-WP4: surface Agent Lead (team-header band + responsibilities lane + decision ownership)'),
  ].join('\n'), { label: 'FE-WP4 lead', phase: 'FE track' })
  if (typeof wp4 === 'string' && wp4.startsWith('GATE_FAILED')) return { wp1, wp2, wp3, stoppedAt: 'FE-WP4', report: wp4 }

  const wp5 = await agent([
    RULES, '',
    'You are FE-WP5: live polling + freshness, and consume backend kanban. master has FE-WP1..4.',
    'Scope: (1) opt-in interval polling of /v1/snapshot in App.tsx (a toggle next to "Load live"; when on, setInterval re-fetch every ~5s; clear on unmount/toggle-off; keep manual refresh). (2) a generated_at freshness chip in the TopBar (e.g. "updated 12s ago", amber if stale > N s). (3) consume the backend-emitted kanban map as the lane source of truth instead of rebuilding lanes from tasks in readModel (owner decision); if kanban is empty/absent fall back to the current local build.',
    FE_GATE, finish('task/fe-wp5-polling-kanban', 'FE-WP5: opt-in live polling + freshness chip + consume backend kanban'),
  ].join('\n'), { label: 'FE-WP5 polling', phase: 'FE track' })
  if (typeof wp5 === 'string' && wp5.startsWith('GATE_FAILED')) return { wp1, wp2, wp3, wp4, stoppedAt: 'FE-WP5', report: wp5 }

  return { wp1, wp2, wp3, wp4, wp5, status: 'FE-all-merged' }
})()

// =================== BE TRACK (serial WP6..WP8) ===================
const beTrack = (async () => {
  phase('BE track')

  const wp6 = await agent([
    RULES, '',
    'You are BE-WP6: the provider dispatch seam (Codex stays regression-clean; Claude routes to a stub). Claude shape = claude CLI (local process), owner decision.',
    'Read .harness-ml-plan.md §4 for the exact codex-pinned fns + file:lines. Scope: (A) core: add enum ProviderKind { Codex, Claude, Unknown(String) } with From<&str>/Display round-trip in harness-core (NO schema change — provider stays a String field; the enum is for dispatch). (B) CLI: route by member.provider at the seam call sites — runtime spawn (start_codex_runtime ~7024 via agent create --start ~118 / start_agent_runtime ~2689), delivery (deliver_agent_messages_value ~3275), protocol probe (agent_health ~2827), ingest (~3371): match provider { codex => existing fns, claude => claude stubs that return a clear "not yet implemented" Err for now, _ => unknown }. (C) generalize socket_path_from_endpoint (~4639) to accept a non-unix:// scheme (return endpoint as-is for non-unix). (D) replace hardcoded "codex" provider-default literals where they should follow member.provider (defaults ~563,634,6915,3781,4671,5307) — keep the create-time default = "codex" but downstream logic must read member.provider, not assume codex. Add cargo tests for ProviderKind round-trip and that a claude member dispatches to the claude path (even if stub).',
    BE_GATE, finish('task/be-wp6-provider-seam', 'BE-WP6: provider dispatch seam + ProviderKind enum (Codex regression-clean, Claude stub)'),
  ].join('\n'), { label: 'BE-WP6 seam', phase: 'BE track' })
  if (typeof wp6 === 'string' && wp6.startsWith('GATE_FAILED')) return { stoppedAt: 'BE-WP6', report: wp6 }

  const wp7 = await agent([
    RULES, '',
    'You are BE-WP7: implement the Claude runtime + delivery via the claude CLI shape. master has BE-WP6 (dispatch seam + claude stubs).',
    'Replace the claude stubs from WP6 with a real claude-CLI integration paralleling Codex: start_claude_runtime spawns/attaches the claude CLI as a local process (Command::new("claude") with appropriate args; keep a local pid; set provider="claude", command="claude", a control_endpoint, and a runtime_health with process_alive/etc.). run_claude_exchange delivers a queued Message to the claude process and records a ProviderSession (status lifecycle queued→running→succeeded/failed, thread/turn ids if available, terminal_source) + Evidence for the provider output, mirroring run_codex_delivery semantics. Reconciliation (reconcile-session) must work for claude sessions via the existing neutral path. If the local `claude` binary is unavailable in the build/test env, the code must compile and unit-test via a mockable seam (do NOT require the binary at test time; gate is cargo test + pnpm check, not a live claude call). Use the claude-code-guide knowledge for the claude CLI invocation/flags and session/turn shape, but keep all Claude specifics inside the CLI provider layer (ADR 0011).',
    BE_GATE, finish('task/be-wp7-claude-runtime', 'BE-WP7: Claude runtime + delivery via claude CLI (local-process shape)'),
  ].join('\n'), { label: 'BE-WP7 claude-runtime', phase: 'BE track', agentType: 'claude-code-guide' })
  if (typeof wp7 === 'string' && wp7.startsWith('GATE_FAILED')) return { wp6, stoppedAt: 'BE-WP7', report: wp7 }

  const wp8 = await agent([
    RULES, '',
    'You are BE-WP8: Claude ingest/child-threads + docs. master has BE-WP6+WP7.',
    'Scope: (1) a Claude parser for provider output that feeds the SAME ProviderChildThread / AgentEvent objects (Claude native subagents = child threads under the parent member, NOT promoted to members — doctrine). Map Claude session/turn/subagent shape onto the neutral objects; provider="claude". (2) docs/integration/claude.md paralleling docs/integration/codex.md (runtime model via claude CLI, delivery, claim/retry, event sources, reducer mapping, permission/workspace model, native multi-agent→child-threads, evidence extraction, health signals, fallback, unsupported surfaces). Register it in docs/registry.json + link from docs/integration/README.md so check:links + check:doc-governance pass. (3) ensure `--provider [codex|claude]` is documented in CLI help/usage. (4) hook config dispatch must NO-OP for Claude (Claude has no Codex-style hook CLI — do not fake it). Use claude-code-guide knowledge for Claude subagent/session shape.',
    BE_GATE, finish('docs-task/be-wp8-claude-ingest-docs', 'BE-WP8: Claude ingest/child-threads + docs/integration/claude.md'),
  ].join('\n'), { label: 'BE-WP8 claude-ingest+docs', phase: 'BE track', agentType: 'claude-code-guide' })
  if (typeof wp8 === 'string' && wp8.startsWith('GATE_FAILED')) return { wp6, wp7, stoppedAt: 'BE-WP8', report: wp8 }

  return { wp6, wp7, wp8, status: 'BE-all-merged' }
})()

const [fe, be] = await Promise.all([feTrack, beTrack])

phase('Wrap')
return { fe, be }