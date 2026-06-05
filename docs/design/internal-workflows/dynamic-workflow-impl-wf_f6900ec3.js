export const meta = {
  name: 'dynamic-workflow-impl',
  description: 'Implement Stages 1-5 of the dynamic workflow runtime sequentially, each verified before commit; stop on failure',
  phases: [
    { title: 'Stage1', detail: 'Extract harness-workflow crate + JSON-IR spec + run-spec CLI' },
    { title: 'Stage2', detail: 'Real pipeline() + WP3 fields (args/final_output/result)' },
    { title: 'Stage3', detail: 'author-workflow skill' },
    { title: 'Stage4', detail: 'Dashboard per-node drill-in' },
    { title: 'Stage5', detail: 'End-to-end acceptance + docs + ADR' },
  ],
}

const STAGE_RESULT = {
  type: 'object',
  additionalProperties: false,
  required: ['stage', 'ok', 'summary', 'verification', 'filesChanged', 'commit', 'blockers'],
  properties: {
    stage: { type: 'string' },
    ok: { type: 'boolean', description: 'true ONLY if every verify command for this stage passed' },
    summary: { type: 'string', description: 'what was implemented' },
    verification: { type: 'string', description: 'each verify command run + PASS/FAIL + key output snippet' },
    filesChanged: { type: 'array', items: { type: 'string' } },
    commit: { type: 'string', description: 'commit sha + subject, or "NONE" if not committed' },
    blockers: { type: 'array', items: { type: 'string' }, description: 'anything that blocked completion' },
  },
}

const COMMON = `
You are implementing one stage of IMPLEMENTATION_PLAN.md in repo <REPO> on branch feat/dynamic-workflow-runtime. FIRST Read IMPLEMENTATION_PLAN.md and docs/design/orchestration-plugin-architecture.dot for full context.
Follow the repo's conventions (study neighbours before writing). Do NOT use --no-verify. Do NOT disable tests. Every commit must compile and pass this stage's verify commands. Commit with: git -c commit.gpgsign=false commit -m "<msg>" and end the message with a Co-Authored-By: Claude Opus 4.8 line. Only "git add" the files YOU changed for this stage — never add stray untracked files (screenshots, .harness-*.md, .claude/, *.png at repo root).
Set ok=true ONLY if every verify command passes. If you cannot make it pass after a genuine effort, set ok=false, do NOT commit broken code, and explain in blockers. Return the structured result with real verification output snippets.
`

const STAGES = [
  {
    id: 'Stage1',
    prompt: COMMON + `
STAGE 1 — Extract harness-workflow crate + WorkflowSpec JSON-IR + run-spec CLI.

Baseline: run \`cargo test --workspace\` first; it should be GREEN before you change anything. Note any pre-existing failures.

Verified code map:
- Runtime today: crates/harness-cli/src/workflow.rs (WorkflowScheduler ~L98; parallel() ~L163; pipeline() stub ~L229; AgentStepFn type ~L86; run_agent_step ~L90; investigate ~L261; WorkflowRegistry ~L332). It already \`use harness_core::{WorkflowRunStatus, WorkflowStepStatus}\`.
- harness-core (crates/harness-core/src/lib.rs ~L1236): WorkflowRun, WorkflowStep, WorkflowRunStatus, WorkflowStepStatus.
- Real driver workflow_real_agent_step (crates/harness-cli/src/main.rs ~L3724) -> claim_queued_message_delivery -> run_provider_delivery (~L7452). CLI workflow dispatcher ~L3848-3872; workflow run parser ~L3874.

Do:
1. Create crate crates/harness-workflow (lib). MOVE the runtime (scheduler/parallel/pipeline/AgentStepFn/run_agent_step/WorkflowOutcome/StepResult/AgentStepSpec + the WorkflowRegistry/investigate if you keep them) into crates/harness-workflow/src/lib.rs, keeping the injected AgentStepFn seam so the crate contains NO codex/claude code. Depend on harness-core. Add the crate to the workspace root Cargo.toml members. Make harness-cli depend on harness-workflow and update its \`use\` paths; keep workflow_real_agent_step IN harness-cli (it is the injected driver).
2. Add JSON-IR in harness-workflow: serde types WorkflowSpec { name, args: Option<serde_json::Value>, nodes: Vec<WorkflowNode> } and enum WorkflowNode { Agent { member, prompt, phase?, label? }, Phase { name, nodes }, Parallel { nodes }, Pipeline { stages } }. Add dispatch_spec(spec, member_resolver, driver: &AgentStepFn) -> WorkflowOutcome that walks the IR: Phase = serial in order, Parallel = barrier via existing parallel(), Pipeline = fall back to parallel() for now (real streaming is Stage 2). Member refs resolve via a passed map (member name -> harness member id).
3. Add schemas/workflow-spec.schema.json + fixtures schemas/fixtures/workflow-spec/{valid,invalid}/*.json. Read scripts/check-schema-fixtures.mjs and scripts/validate-json.mjs and an existing pair (schemas/agent-event.schema.json + schemas/fixtures/agent-event/) to wire it in so pnpm check:schema-fixtures validates them.
4. CLI: add a "run-spec" arm to the workflow dispatcher in main.rs: read a JSON file path, parse+validate into WorkflowSpec, build member resolver from existing --codex/--claude flags, call dispatch_spec with the real driver (or a dry-run mock when --dry-run), and journal WorkflowRun/WorkflowStep exactly like the existing \`workflow run\` path (reuse its journaling helpers).

Verify (record output for each): (a) cargo test --workspace ; (b) node scripts/validate-json.mjs ; (c) pnpm check:schema-fixtures ; (d) smoke: cargo run -p harness-cli -- workflow run-spec <a valid fixture> --dry-run  (with mock/sample members) runs without error and journals a run.
Commit subject suggestion: feat(workflow): extract harness-workflow crate + JSON-IR + run-spec CLI (Stage 1).
`,
  },
  {
    id: 'Stage2',
    prompt: COMMON + `
STAGE 2 — Real pipeline() + WP3 object fields. (Stage 1 must be committed and green first.)

Do:
1. In crates/harness-workflow, implement a real streaming pipeline(items, stages): each item flows through ALL stages independently with NO barrier between stages (item A may be in stage 3 while item B is in stage 1), using the existing WorkflowScheduler for concurrency. A stage that fails drops that item to a failed/None slot and skips its remaining stages. Replace the current parallel()-fallback stub. Add unit tests proving no-barrier ordering and failure-drop.
2. Add fields: harness-core WorkflowRun gains args: Option<serde_json::Value>, agents_spawned: u64, final_output: Option<serde_json::Value>; WorkflowStep gains result: Option<serde_json::Value>. Update crates/harness-store append/read, any serialization, and the dynamic run path (dispatch_spec) to populate them (args from the spec, agents_spawned from the scheduler counter, final_output as the collected results, step.result from each StepResult).
3. Keep the dashboard types in sync: update apps/agent-dashboard/src/types.ts WorkflowRun/WorkflowStep types with the new optional fields so tsc passes. Update readModel.ts only if needed to not break.

Verify: (a) cargo test --workspace ; (b) node scripts/validate-json.mjs + pnpm check:schema-fixtures (update fixtures for new fields) ; (c) pnpm check:dashboard (tsc + vite build) ; (d) smoke: a pipeline spec fixture runs via run-spec --dry-run and the run shows a pipeline shape.
Commit subject: feat(workflow): real streaming pipeline() + WP3 run/step fields (Stage 2).
`,
  },
  {
    id: 'Stage3',
    prompt: COMMON + `
STAGE 3 — author-workflow skill. (Stages 1-2 committed and green.)

Do: create .agents/skills/author-workflow/SKILL.md following the EXACT format of existing skills (study .agents/skills/generic-agent-harness/SKILL.md and .agents/skills/bootstrap-project-workflow/SKILL.md, and scripts/check-skills.mjs for the required frontmatter/structure). The skill teaches an agent to: (1) write a valid WorkflowSpec JSON (show one worked example spec, e.g. a scan->parallel-fix workflow), (2) invoke \`harness workflow run-spec <spec.json> --codex <member> --claude <member>\`, (3) read the run back (e.g. via the dashboard snapshot or store), and (4) the permission note: the member's profile must allow the \`harness\` binary. Include a runnable example spec file under the skill dir.

Verify: (a) pnpm check:skills ; (b) pnpm check:doc-governance ; (c) pnpm check:links (if it covers skills). Also validate the example spec against schemas/workflow-spec.schema.json with node scripts/validate-json.mjs or a quick ajv check.
Commit subject: feat(workflow): author-workflow skill teaching agents to write+run specs (Stage 3).
`,
  },
  {
    id: 'Stage4',
    prompt: COMMON + `
STAGE 4 — Dashboard per-node drill-in. (Stages 1-3 committed and green.)

Verified UI map: apps/agent-dashboard/src/surfaces/Workflows.tsx (StepCard ~L560-650 renders steps; uses TurnDrillIn inline). TurnDrillIn renders streamed provider tool calls (tool_use/tool_result) for a session — find its file under src/components/workbench/. readModel.ts buildWorkbenchModel (~L410-473) builds the snapshot; snapshot.live_turn_events is keyed by session_id but never passed down. useEventStream.ts pushes live_turn_events.

Do:
1. Make StepCard clickable to open a detail panel/modal/drawer that wraps TurnDrillIn for that step's provider_session_id.
2. Thread live_turn_events through: export it from buildWorkbenchModel if needed, and pass snapshot.live_turn_events down readModel -> Timeline -> StepCard -> TurnDrillIn.liveEvents so the node detail streams sub-second. Reuse existing UI primitives (the repo uses shadcn-style components in src/components/ui).
Keep it consistent with the existing dark-console design.

Verify: (a) pnpm check:dashboard (tsc -p apps/agent-dashboard/tsconfig.json --noEmit && vite build) MUST pass. (b) If feasible, do a quick reasoning check that clicking a step would mount TurnDrillIn with the right session id (read the wiring). Do NOT start a dev server.
Commit subject: feat(dashboard): per-node workflow drill-in via TurnDrillIn (Stage 4).
`,
  },
  {
    id: 'Stage5',
    prompt: COMMON + `
STAGE 5 — End-to-end acceptance + docs + ADR. (Stages 1-4 committed and green.)

Do:
1. Create scripts/acceptance-dynamic-workflow.mjs modeled on scripts/acceptance-mvp.mjs (study it + scripts/acceptance-autonomous-team.mjs). In mock/CI mode it should: build/run the harness, author a 2-provider dynamic WorkflowSpec, run it via \`harness workflow run-spec\` (mock providers / --dry-run acceptable for CI), assert a WorkflowRun + steps were journaled with the expected serial->parallel(->pipeline) shape and final_output, and exit nonzero on any failure. Add a package.json script "acceptance:dynamic-workflow".
2. Update docs/research/dynamic-workflow-runtime-design.md for the JSON-IR + skill+CLI path, and add one ADR under docs/decisions/ recording the locked decisions (skill+CLI not MCP/plugin; JSON-IR not embedded JS; new harness-workflow crate). Follow the existing ADR format in docs/decisions/.
3. Update IMPLEMENTATION_PLAN.md: set every stage Status to Complete.

Verify: (a) node scripts/acceptance-dynamic-workflow.mjs (mock mode) exits 0 ; (b) pnpm check (the full governance suite) passes ; (c) cargo test --workspace green.
Commit subject: feat(workflow): end-to-end acceptance + docs + ADR (Stage 5).
`,
  },
]

const results = []
for (const s of STAGES) {
  phase(s.id)
  let res = await agent(s.prompt, { label: s.id, phase: s.id, schema: STAGE_RESULT })
    .catch((e) => ({ stage: s.id, ok: false, summary: 'agent threw', verification: '', filesChanged: [], commit: 'NONE', blockers: [String(e)] }))
  if (res && !res.ok) {
    log(`${s.id} failed first attempt — one repair pass`)
    const repair = await agent(
      COMMON + `\nREPAIR PASS for ${s.id}. The previous attempt failed. Blockers reported:\n- ` +
      (res.blockers || []).join('\n- ') +
      `\nVerification output was:\n` + (res.verification || '(none)') +
      `\nDiagnose and FIX so all of ${s.id}'s verify commands pass, then commit. Re-read IMPLEMENTATION_PLAN.md ${s.id}. If still blocked, set ok=false with precise blockers — do not commit broken code.`,
      { label: s.id + ':repair', phase: s.id, schema: STAGE_RESULT }
    ).catch((e) => ({ stage: s.id, ok: false, summary: 'repair threw', verification: '', filesChanged: [], commit: 'NONE', blockers: [String(e)] }))
    res = repair
  }
  results.push(res)
  if (!res || !res.ok) {
    log(`${s.id} could not be completed — STOPPING the chain so the human can intervene.`)
    break
  }
  log(`${s.id} complete: ${res.commit}`)
}

return { stagesAttempted: results.length, results }
