export const meta = {
  name: 'operator-drives-team',
  description: 'Make an external operator able to drive the team from the dashboard: WP-i operator identity (sender_kind), WP-ii HTTP create routes (teams/agents/goals/tasks), WP-iii dashboard affordances (team picker, new-team, new-agent, brief-the-Lead, operator composer), WP-iv real delivery (operator<->agent + agent<->agent). Each gated + auto-merged; final real browser+CLI acceptance.',
  phases: [
    { title: 'WP-i operator identity' },
    { title: 'WP-ii HTTP create routes' },
    { title: 'WP-iii dashboard affordances' },
    { title: 'WP-iv real delivery' },
    { title: 'Acceptance' },
  ],
}

const RULES = [
  'HARD RULES (non-negotiable):',
  '- NEVER --no-verify; NEVER weaken/skip/fixture-fake a test or check. Fix the real cause.',
  '- The gate must pass BEFORE you commit. If after ~3 honest attempts it cannot go green, STOP, do NOT merge, return a report whose FIRST LINE is exactly "GATE_FAILED:" + the failing check + log tail.',
  '- Work in an isolated git worktree off origin/master. Implement, gate, commit, push, open PR, merge (gh pr merge --squash), then git checkout master && git pull --ff-only.',
  '- Commit message ends with: Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>',
  '- Do NOT git-add scratch files .harness-ml-plan.md/.harness-genplan.md.',
  '- ADR 0011 provider-neutral; additive-optional schema (new fields default-valued so existing jsonl/fixtures validate, per the established convention).',
  '- VERIFICATION = REAL: "works" means a real CLI/HTTP call produced the change and `harness dashboard snapshot` reflects it. NEVER hand-write jsonl or a fixture to satisfy acceptance.',
].join('\n')

const RUST_GATE = 'GATE: cargo test 2>&1 | tail -40  AND  npx pnpm@9.15.4 check 2>&1 | tail -25 — both green before commit.'
const FE_GATE = 'GATE: npx tsc -p apps/agent-dashboard/tsconfig.json --noEmit AND npx vite build --config apps/agent-dashboard/vite.config.ts AND npx pnpm@9.15.4 check 2>&1 | tail -25 — all green before commit.'

const finish = (branch, title) => [
  `FINISH (after gate green): git add -A (not scratch files) && commit (title: "${title}"; end with Co-Authored-By line); git push -u origin ${branch}; gh pr create --base master --head ${branch} --title "${title}" --body "<summary + gate + real-run proof>"; gh pr merge ${branch} --squash; git checkout master && git pull --ff-only.`,
  'Return: gate results, files changed, the REAL-RUN proof (curl/CLI command + the snapshot delta you observed), PR number/URL. If gate cannot go green: first line "GATE_FAILED:".',
].join('\n')

// ---------- WP-i: operator identity ----------
phase('WP-i operator identity')
const wpi = await agent([
  RULES, '',
  'You are WP-i: add a first-class OPERATOR sender identity so external operators (humans / external agents) are not impersonated as the Lead.',
  'Design (approved): add an additive optional enum field to Message — sender_kind: agent | operator | system, default "agent" (so the 65 existing messages.jsonl records + fixtures still validate). Do NOT invent a synthetic operator AgentMember (it would pollute the roster/runtime). Use a reserved from id convention (e.g. "operator") when sender_kind=operator.',
  'Changes:',
  '- schemas/message.schema.json: add sender_kind property with enum [agent,operator,system] + default "agent"; NOT in required.',
  '- crates/harness-core/src/lib.rs: add `enum SenderKind { Agent, Operator, System }` (serde snake_case, #[serde(default)] helper) + `#[serde(default)] sender_kind` on Message; fix every Message struct-literal construction site (core round-trip tests, store helpers, cli builders) so cargo builds; add a round-trip test that sender_kind defaults to agent for old rows and persists operator when set.',
  '- crates/harness-cli/src/main.rs: create_message_value (~2456) reads optional sender_kind (default agent); `message send` / `agent send` accept an optional --sender-kind flag; snapshot emits sender_kind (direct serialization).',
  '- apps/agent-dashboard/src/types.ts: add sender_kind? to the Message interface. (Rendering distinction comes in WP-iii; here just carry the field so tsc is green.)',
  RUST_GATE,
  'REAL-RUN PROOF (in PR body): in a temp HARNESS dir, `message send --from operator --to <agent> --content "x" --kind task --sender-kind operator` then `dashboard snapshot` shows that message with sender_kind="operator"; an existing message (or a default send) shows sender_kind="agent". Confirm the repo .harness 65 messages still validate (pnpm check green).',
  finish('task/wp-i-operator-identity', 'WP-i: operator sender identity (additive Message.sender_kind agent|operator|system)'),
].join('\n'), { label: 'WP-i', phase: 'WP-i operator identity' })
if (typeof wpi === 'string' && wpi.startsWith('GATE_FAILED')) return { stoppedAt: 'WP-i', report: wpi }

// ---------- WP-ii: HTTP create routes ----------
phase('WP-ii HTTP create routes')
const wpii = await agent([
  RULES, '',
  'You are WP-ii: add HTTP write routes so the dashboard is not read-only. master has WP-i merged.',
  'The CLI already has the creation LOGIC (team create / agent create / goal create / task create+assign). Expose thin HTTP wrappers in handle_http_action (crates/harness-cli/src/main.rs ~2354-2438), mirroring the existing create_message_value pattern, reusing the same core/value functions the CLI commands call (refactor a shared value-fn if the CLI logic is inline in the command arm):',
  '- POST /v1/teams  {name, description, owner, member[]?} -> create team',
  '- POST /v1/agents {name, role, provider?, team?, skill[]?, prompt?, ...} -> create agent member (agent create logic, WITHOUT requiring --start; runtime start stays a separate action)',
  '- POST /v1/goals  {title, objective, owner, success[]?} -> create goal',
  '- POST /v1/tasks  {title, objective, owner, goal?, assignee?, reviewer?, ...} -> create task ; and POST /v1/tasks/{id}/assign {assignee} -> assign',
  'Each returns {ok:true, snapshot?} or {ok:true, id}; malformed body -> the existing CliError::Usage 400 path. Do NOT auto-start runtimes. Keep all existing routes working.',
  'Add cargo tests for at least create-team and create-agent value-fns (entity persisted + appears in snapshot).',
  RUST_GATE,
  'REAL-RUN PROOF (in PR body): start `serve` on a temp store; `curl -s -XPOST :PORT/v1/teams -d ...` then `curl :PORT/v1/snapshot` shows the new team; same for /v1/agents (member in roster), /v1/goals (goal in snapshot). Paste the before/after counts.',
  finish('task/wp-ii-http-create-routes', 'WP-ii: HTTP create routes (POST /v1/teams,/agents,/goals,/tasks[+assign]) — dashboard no longer read-only'),
].join('\n'), { label: 'WP-ii', phase: 'WP-ii HTTP create routes' })
if (typeof wpii === 'string' && wpii.startsWith('GATE_FAILED')) return { wpi, stoppedAt: 'WP-ii', report: wpii }

// ---------- WP-iii: dashboard affordances ----------
phase('WP-iii dashboard affordances')
const wpiii = await agent([
  RULES, '',
  'You are WP-iii: dashboard affordances so an operator can drive the team with ZERO CLI. master has WP-i + WP-ii merged (sender_kind + create routes exist). Frontend only (apps/agent-dashboard). Keep the dark operator-console design system + existing primitives + actions.ts seam.',
  'Implement (all wired to the WP-ii routes via src/api/actions.ts):',
  '1. TEAM PICKER: the snapshot returns all active teams but the UI has no switcher (readModel.ts ~245 selects teams[0]/?team=). Add a team selector control (e.g. in the TopBar or TeamRail header) that sets the ?team= selection.',
  '2. NEW TEAM form: a small dialog/inline form -> POST /v1/teams -> on success the team appears (refresh/SSE). ',
  '3. NEW AGENT form: name + role (+ optional provider codex|claude, skills) -> POST /v1/agents -> appears in the team roster.',
  '4. BRIEF THE LEAD / SET GOAL: a form that creates a Goal (POST /v1/goals, owner=Lead) AND optionally emits an operator Message(kind=task, sender_kind=operator, from="operator", to=Lead) so the objective shows both as durable Goal state and in the Lead conversation.',
  '5. OPERATOR COMPOSER: the Member chat-app composer must author as OPERATOR by default — set sender_kind=operator + from="operator" (remove the from=owner_agent_id Lead-impersonation in Surfaces.tsx ~2402 / WorkbenchShell.tsx ~637 / memberMessageDescriptor ~130). Render operator messages distinctly (right-aligned + "Operator" badge) vs agent messages (left, member name). Retire the stale placeholder message buttons.',
  'Gate on actionsEnabled (live) for all writes; offline = disabled with the existing tooltip.',
  FE_GATE,
  'Screenshot self-review: vite --port 5199 + a live serve backend on :8787 (start it: cargo run -q -p harness-cli -- serve --addr 127.0.0.1:8787 in background). Drive the real UI: create a team, add an agent, brief the Lead; screenshot /tmp/op-newteam.png, /tmp/op-newagent.png, /tmp/op-brieflead.png, /tmp/op-composer-operator.png. Confirm operator messages render with the Operator badge (not as Lead).',
  finish('task/wp-iii-operator-affordances', 'WP-iii: dashboard operator affordances (team picker, new-team, new-agent, brief-the-Lead, operator composer)'),
].join('\n'), { label: 'WP-iii', phase: 'WP-iii dashboard affordances' })
if (typeof wpiii === 'string' && wpiii.startsWith('GATE_FAILED')) return { wpi, wpii, stoppedAt: 'WP-iii', report: wpiii }

// ---------- WP-iv: real delivery ----------
phase('WP-iv real delivery')
const wpiv = await agent([
  RULES, '',
  'You are WP-iv: make message DELIVERY actually work from the dashboard, and prove operator<->agent and agent<->agent end-to-end. master has WP-i..iii merged.',
  'Findings to act on: POST /v1/messages only ENQUEUES (Queued); the UI Deliver button calls deliverQueued(member.id) with NO start_runtime (WorkbenchShell.tsx ~654), and serve runs no background gateway loop, so messages sit Queued.',
  'Implement:',
  '1. FE: the UI Deliver action passes start_runtime:true (so a runtime is started if not alive) via actions.ts deliverQueued; surface delivery status transitions (Queued->Delivered/Acknowledged) live via the existing SSE.',
  '2. OPTIONAL Rust (do if it keeps acceptance reliable): add an opt-in background gateway tick to serve (a flag or interval) so queued messages get delivered without a manual click — keep it OFF by default if it risks test flakiness; a manual /v1/gateway/tick already exists.',
  '3. Do NOT require a real codex/claude binary in the GATE (cargo test + pnpm check must pass without a live provider). The live delivery proof happens in Acceptance against whatever runtime is available; if no provider binary exists in the env, the proof is that the message reaches Delivered via gateway/deliver against a stub/mock or is clearly reported as "runtime unavailable in env" (NOT faked).',
  FE_GATE + '  (+ cargo test/pnpm check if Rust touched)',
  'REAL-RUN PROOF (in PR body): describe the delivery path now wired (start_runtime flag passed); if a provider runtime is available, show a message going Queued->Delivered; agent<->agent is already proven in messages.jsonl (cite it) — re-prove via a UI/HTTP-triggered gateway tick if possible.',
  finish('task/wp-iv-real-delivery', 'WP-iv: real message delivery from the dashboard (start_runtime on deliver; operator<->agent + agent<->agent)'),
].join('\n'), { label: 'WP-iv', phase: 'WP-iv real delivery' })
if (typeof wpiv === 'string' && wpiv.startsWith('GATE_FAILED')) return { wpi, wpii, wpiii, stoppedAt: 'WP-iv', report: wpiv }

// ---------- Acceptance: real browser + CLI ----------
phase('Acceptance')
const accept = await agent([
  'You are the ACCEPTANCE agent: prove an external OPERATOR can drive the team from the dashboard, end to end, on master. REAL browser + REAL snapshot — never fake.',
  'SETUP (clean master checkout in cwd): git checkout master && git pull --ff-only; npx pnpm@9.15.4 install; cargo test 2>&1 | tail -15 (pass); npx pnpm@9.15.4 check 2>&1 | tail -10 (EXIT 0).',
  'Start backend: cargo run -q -p harness-cli -- serve --addr 127.0.0.1:8787 (background). Start frontend: npx vite --config apps/agent-dashboard/vite.config.ts --host 127.0.0.1 --port 5199 (background). Wait for 200 on both.',
  'Use a TEMP harness store for create-actions if you do not want to mutate the repo .harness, OR accept that creates land in the repo store (note which). Drive via agent-browser (nav by button[aria-label], click "Load live"):',
  '  A. Open :5199, Load live -> chip "live (SSE)". Screenshot /tmp/acc-op-live.png.',
  '  B. CREATE A TEAM from the UI -> confirm it appears (in rail + in /v1/snapshot via curl). Screenshot /tmp/acc-op-team.png.',
  '  C. CREATE AN AGENT from the UI -> confirm it appears in the team roster + snapshot. Screenshot /tmp/acc-op-agent.png.',
  '  D. BRIEF THE LEAD / SET A GOAL from the UI -> confirm a Goal exists in snapshot AND the Lead has an operator Message(kind=task, sender_kind=operator). Screenshot /tmp/acc-op-goal.png.',
  '  E. OPERATOR MESSAGE distinction: send a message via the composer -> it renders right-aligned with an "Operator" badge (NOT as the Lead). Confirm in snapshot the message has sender_kind=operator and from is the operator id, not the Lead. Screenshot /tmp/acc-op-composer.png.',
  '  F. DELIVERY: trigger Deliver (with start_runtime) or /v1/gateway/tick; report whether a message reached Delivered/Acknowledged. If no provider binary is available in this env, say so explicitly and show the Queued state + the delivery path being invoked (do NOT fake Delivered). agent<->agent: cite the existing delivered pairs in .harness/messages.jsonl.',
  'Kill both servers when done. Read each screenshot with the Read tool and judge honestly.',
  'VERDICT: if A-E pass (operator can create team+agent+goal and send a distinct operator message, all reflected in the real snapshot), return "ACCEPT_PASS:" with per-check evidence + the snapshot deltas (teams/members/goals counts up, a sender_kind=operator message present). For F, report the real delivery state honestly. If a create path does not actually work from the UI, return "ACCEPT_FAIL:" naming which, with screenshot evidence; if it is a SMALL fix, fix-forward (worktree, gate, PR, merge) then re-verify. NEVER fake a pass.',
].join('\n'), { label: 'acceptance', phase: 'Acceptance' })

return { wpi, wpii, wpiii, wpiv, accept }