export const meta = {
  name: 'generic-harness-object-model-build',
  description: 'Autonomous: implement the approved generic-harness object-model migration WP-A..WP-G, each gated (cargo test + pnpm check + tsc/vite), merged to master before the next',
  phases: [
    { title: 'WP-A schema spine' },
    { title: 'WP-B core extends' },
    { title: 'WP-C Review' },
    { title: 'WP-D Gap' },
    { title: 'WP-E Learning layer' },
    { title: 'WP-F Closeout gate' },
    { title: 'WP-G Docs governance' },
  ],
}

// Shared preamble: every WP agent gets the approved design + the same hard rules.
const APPROVED = `APPROVED DESIGN (owner signed off on the schema checkpoint):
- Versioning: ADDITIVE-OPTIONAL. New fields on existing objects are property-but-NOT-required, nullable (["T","null"]) or arrays; Rust uses Option<T>/Vec<T> with #[serde(default)]. Single schema file per object. NO schema_version field. Existing fixtures/JSONL stay valid (omission is allowed under additionalProperties:false when not required — this is the existing Evidence.task_id precedent).
- 6 NEW objects: Review, Gap (Bug = Gap with category=bug; NO separate Bug object), GoalDesign, GoalEvaluation, GoalCase, Vision. Phase = Task.phase label (NO Phase object).
- Open-enum pattern: verdict/decision/review_kind/evidence_kind/decision_kind are free "string" (minLength 1) in JSON Schema; Rust models the known set with an enum carrying #[serde(other)] Other(String) OR a validated String. Truly-closed harness-owned sets (Gap.severity p0/p1/p2, Gap.status) use hard JSON enum. HARNESS CORE MUST CONTAIN ZERO DOMAIN WORDS (no trading/Polymarket/InfluxDB/edge etc.) — domain vocabulary lives in adapters/skills.

Full approved spec is in the repo file .harness-genplan.md (sections 3 = schema, 4 = Rust, 5 = frontend, 7 = risks). READ IT FIRST for exact field lists.

HARD RULES (non-negotiable):
- NEVER use --no-verify. NEVER disable or weaken a test/check to make it pass. Fix the real cause.
- The gate must pass BEFORE you commit. If after honest effort (≈3 serious attempts) a gate still fails, STOP, do NOT commit/merge, and return a report whose first line is exactly "GATE_FAILED:" followed by the failing check name and the relevant log tail.
- Commit messages end with: Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
- You operate directly on a clean checkout of master in your cwd (NOT a worktree). Create your WP branch, implement, gate, commit, push, open PR, then MERGE it (gh pr merge --squash --delete-branch=false), then return to master and pull, so the next WP builds on your merged work.`

const GATE_RUST = `GATE for this WP (all must pass):
  cargo test 2>&1 | tail -40   (workspace must build AND tests pass)
  npx pnpm@9.15.4 check 2>&1 | tail -40   (validate:json, check:schema-fixtures, check:tool-descriptors, check:links, check:doc-size, check:skills, check:doc-governance, check:dashboard[tsc+vite])
Iterate until BOTH are green.`

const GATE_NODE = `GATE for this WP:
  npx pnpm@9.15.4 check 2>&1 | tail -40
Iterate until green.`

const FINISH = (branch, title) => `FINISH SEQUENCE (only after the gate is green):
  git add -A && git commit (clear message; title: "${title}"; end with the Co-Authored-By line)
  git push -u origin ${branch}
  gh pr create --base master --head ${branch} --title "${title}" --body "<concise summary of what changed + gate results>"
  gh pr merge ${branch} --squash   (merge it — autonomous mode, owner approved auto-merge on green gate)
  git checkout master && git pull --ff-only
Return a report: gate results (cargo test pass count + pnpm check EXIT), files changed, PR number/URL, and confirm master is updated. If the gate could not go green, first line "GATE_FAILED:" and do NOT merge.`

// ---- WP-A: schema spine (pure schema + fixtures + docs; no Rust) ----
phase('WP-A schema spine')
const wpa = await agent(
`${APPROVED}

You are WP-A: the back-compat SCHEMA SPINE. Pure schema + fixtures + docs. NO Rust changes in this WP.

Scope:
1. Read .harness-genplan.md §3 for exact field lists. Read existing schemas/ to match house style (schemas use $schema draft, additionalProperties:false, required arrays; fixtures live under schemas/fixtures/<obj>/{valid,invalid}/).
2. Add the new OPTIONAL properties (NOT in required[]) to the EXISTING schemas: goal.schema.json (+vision_id, goal_design_id, closed_by_decision_id), task.schema.json (+phase, scope_refs, requires_human_approval, verdict_decision_id), evidence.schema.json (+evidence_kind, goal_id), decision.schema.json (+decision_kind, goal_id, is_waiver, follow_up_task_id). Use nullable type unions for scalars, arrays for lists, boolean for flags. Keep additionalProperties:false. DO NOT add them to required.
3. Add the 6 NEW schema files (additionalProperties:false, full required for their OWN mandatory fields, per §3.5/3.8/3.9/3.10): review.schema.json, gap.schema.json, goal-design.schema.json, goal-evaluation.schema.json, goal-case.schema.json, vision.schema.json.
4. For EACH new schema add fixtures: schemas/fixtures/<obj>/valid/*.json (≥1) and schemas/fixtures/<obj>/invalid/*.json (≥1) — the checker (scripts/check-schema-fixtures.mjs) FAILS on empty dirs. Confirm how existing fixtures are wired (there may be an index/registry the checker reads — inspect check-schema-fixtures.mjs and mirror exactly). Add one new VALID fixture for each EXTENDED object exercising a new field. Do NOT modify existing fixtures.
5. Encode the versioning decision in docs: update docs/schemas.md, and add an ADR docs/decisions/0017-generic-object-model.md (follow existing ADR format; Status Accepted; summarize additive-optional + the 6 objects + open-enum; reference that it implements the .harness-genplan design). Register the ADR in docs/registry.json and docs/decisions/README.md so check:doc-governance + check:links pass. (Do NOT commit .harness-genplan.md itself — it's a scratch file; add it to .gitignore if needed, or just don't git add it.)

${GATE_NODE}
${FINISH('schema/wp-a-object-spine', 'WP-A: additive-optional schema spine + 6 new object schemas + ADR 0017')}`,
  { label: 'WP-A', phase: 'WP-A schema spine' })

if (typeof wpa === 'string' && wpa.startsWith('GATE_FAILED')) {
  return { stoppedAt: 'WP-A', report: wpa }
}

// ---- WP-B: Rust core extends existing objects ----
phase('WP-B core extends')
const wpb = await agent(
`${APPROVED}

You are WP-B: extend the Rust backend to carry the new OPTIONAL fields on EXISTING objects (Goal/Task/Evidence/Decision). New objects come in later WPs. master already has WP-A's schemas merged.

Scope (read .harness-genplan.md §4.1-4.4 for file/line anchors):
1. harness-core/src/lib.rs: add the new fields to Goal/Task/Evidence/Decision structs as Option<T>/Vec<T>/bool with #[serde(default)]. Match the exact field names from the merged schemas. No Validate changes needed (new fields optional). 
2. Fix EVERY struct-literal construction site so the workspace compiles: the round-trip tests in harness-core (~lines 533-663), harness-store test helpers (~496-549), and any harness-cli builders. Missing one = cargo test won't build. Add explicit None/vec![]/false for the new fields.
3. Snapshot producer (harness-cli dashboard snapshot, ~line 5982): the extended objects serialize directly so new fields auto-surface — VERIFY by running the snapshot command and grepping for a new field. Member cards are hand-built (~5955-5979) but none of our new fields touch AgentMember, so no card change.
4. Frontend type sync: add the same optional fields to apps/agent-dashboard/src/types.ts (Goal/Task/Evidence/Decision interfaces) so tsc stays green and the fields are available to the UI. (Rendering of these lands naturally; you may add lightweight display where trivial, but the gate is the priority.)

${GATE_RUST}
${FINISH('task/wp-b-core-fields', 'WP-B: Rust core carries new optional fields on Goal/Task/Evidence/Decision')}`,
  { label: 'WP-B', phase: 'WP-B core extends' })

if (typeof wpb === 'string' && wpb.startsWith('GATE_FAILED')) {
  return { wpa, stoppedAt: 'WP-B', report: wpb }
}

// ---- WP-C: Review object (full stack) ----
phase('WP-C Review')
const wpc = await agent(
`${APPROVED}

You are WP-C: the Review object, full stack. Schema already merged in WP-A. master has WP-A+WP-B.

Scope (read .harness-genplan.md §3.5, §4, §5.4):
1. harness-core: Review struct + ReviewVerdict (open enum w/ #[serde(other)] Other(String)). Validate impl for required fields. Round-trip test.
2. harness-store: append_review/reviews() following the existing append_jsonl/read_jsonl<T> pattern; add Review to the use import; filename reviews.jsonl.
3. harness-cli: imports; "review create/list" command arms following existing command patterns; add "reviews" key to the dashboard snapshot (direct serialization); extend goal_learning_status to count reviews if natural.
4. Frontend: types.ts Review interface + DashboardSnapshot.reviews; readModel reviewsByTask/reviewsByGoal; demoSnapshot.ts add one Review(verdict=pass) on the demo task; render Reviews in TaskDocument (verdict/blockers/residual_risk/missing_validation) — this fills the biggest current gap (reviews are unstructured today); also surface in the DecisionCenter/Warnings decision area. Keep the dark-console aesthetic + existing primitives.
5. Screenshot self-review: build, run dev server on port 5199, screenshot the Task surface (nav via button[aria-label="Tasks"] then open a task, or load the demo task) to /tmp/wpc-task.png; sanity-check it renders the Review panel.

${GATE_RUST}
Also run: npx tsc -p apps/agent-dashboard/tsconfig.json --noEmit && npx vite build --config apps/agent-dashboard/vite.config.ts
${FINISH('task/wp-c-review', 'WP-C: Review object (schema+Rust+CLI+frontend) — structured evaluator output')}`,
  { label: 'WP-C', phase: 'WP-C Review' })

if (typeof wpc === 'string' && wpc.startsWith('GATE_FAILED')) {
  return { wpa, wpb, stoppedAt: 'WP-C', report: wpc }
}

// ---- WP-D: Gap object (full stack) ----
phase('WP-D Gap')
const wpd = await agent(
`${APPROVED}

You are WP-D: the Gap object (absorbs the bug ledger; Bug = Gap with category=bug). Schema merged in WP-A. master has WP-A..C.

Scope (read .harness-genplan.md §3.9, §2.3, §5.4):
1. harness-core: Gap struct + GapSeverity (hard enum p0/p1/p2) + GapStatus (hard enum open/in_progress/fixed/blocked/deferred/wontfix); category is a free string (open enum). Validate + round-trip test.
2. harness-store: append_gap/gaps(); gaps.jsonl.
3. harness-cli: "gap create/list" (+ optional "gap export" that prints a markdown projection of the gap ledger); add "gaps" key to snapshot.
4. Frontend: types.ts Gap + DashboardSnapshot.gaps; readModel gapsByGoal/gapsBySeverity; demoSnapshot add one Gap(p1/open); render the Gap ledger in the Warnings surface (sortable/grouped by severity+status) — this is the natural home; add new warning kinds gap_p0_open. Dark-console aesthetic.
5. Screenshot self-review: screenshot Warnings surface to /tmp/wpd-warnings.png.

${GATE_RUST}
Also tsc + vite build for the dashboard.
${FINISH('task/wp-d-gap', 'WP-D: Gap object (incl. bug ledger) + Warnings ledger surface')}`,
  { label: 'WP-D', phase: 'WP-D Gap' })

if (typeof wpd === 'string' && wpd.startsWith('GATE_FAILED')) {
  return { wpa, wpb, wpc, stoppedAt: 'WP-D', report: wpd }
}

// ---- WP-E: Learning layer (GoalDesign/GoalEvaluation/GoalCase/Vision) ----
phase('WP-E Learning layer')
const wpe = await agent(
`${APPROVED}

You are WP-E: the learning layer — GoalDesign, GoalEvaluation, GoalCase, Vision. Schemas merged in WP-A. master has WP-A..D.

Scope (read .harness-genplan.md §3.8, §3.10, §4, §5.4, §7.3):
1. harness-core: GoalDesign, GoalEvaluation (+EvaluationOutcome open enum), GoalCase, Vision structs; Validate + round-trip tests.
2. harness-store: append_/read_ for goal_designs.jsonl, goal_evaluations.jsonl, goal_cases.jsonl, visions.jsonl.
3. harness-cli: "goal-design", "goal-evaluation", "goal-case", "vision" create/list arms; add snapshot keys goal_designs/goal_evaluations/goal_cases/visions; extend goal_learning_status to read BOTH the new objects AND the legacy Evidence(source_type=goal_design|goal_evaluation) rows (dual-read, union by goal_id — no backfill).
4. Frontend: types.ts interfaces + DashboardSnapshot keys; readModel goalDesignByGoal/goalEvaluationByGoal/goalCases/visions; demoSnapshot add one of each linked to the demo goal; render GoalDesign (scenario/non_goals/acceptance_gates) + GoalEvaluation (what_worked/failed/patterns) as REAL sections in GoalDocument (today: counts only); render Vision list + goal↔vision link in VisionOverview.
5. Screenshots: /tmp/wpe-goal.png, /tmp/wpe-vision.png.

${GATE_RUST}
Also tsc + vite build.
${FINISH('task/wp-e-learning', 'WP-E: learning layer (GoalDesign/GoalEvaluation/GoalCase/Vision) + Goal/Vision rendering')}`,
  { label: 'WP-E', phase: 'WP-E Learning layer' })

if (typeof wpe === 'string' && wpe.startsWith('GATE_FAILED')) {
  return { wpa, wpb, wpc, wpd, stoppedAt: 'WP-E', report: wpe }
}

// ---- WP-F: closeout + stop-gate enforcement ----
phase('WP-F Closeout gate')
const wpf = await agent(
`${APPROVED}

You are WP-F: closeout + stop-gate enforcement. master has WP-A..E (Review, Gap, GoalEvaluation, Decision fields all exist).

Scope (read .harness-genplan.md §3.6, §3.7, §2.5):
1. harness-cli: extend "goal close" (or the goal status-transition path) to ENFORCE: a Goal may become complete only if there exists Decision(goal_id=G, decision_kind=closeout) with >=1 evidence_id AND a GoalEvaluation(goal_id=G) — OR an explicit Decision(is_waiver=true) with follow_up_task_id + >=1 evidence_id. Return a clear error otherwise.
2. Waiver enforcement: is_waiver=true requires follow_up_task_id and >=1 evidence_ids (CLI validation).
3. Stop-gate: support decision_kind=stop_gate with decision in {stop_approved, continue_required} (no domain semantics).
4. goal_learning_status: surface closeout readiness (has_closeout_decision, has_evaluation, may_close).
5. Frontend: warnings.ts new kinds goal_close_without_evaluation, waiver_without_follow_up; GoalDocument closeout-gate ProofRow (decision+evaluation present → may close). 
6. Tests: add cargo tests for the closeout gate (allowed when both present; blocked when missing; allowed via waiver).
7. Screenshot: /tmp/wpf-goal.png.

${GATE_RUST}
Also tsc + vite build.
${FINISH('task/wp-f-closeout', 'WP-F: goal closeout + stop-gate + waiver enforcement')}`,
  { label: 'WP-F', phase: 'WP-F Closeout gate' })

if (typeof wpf === 'string' && wpf.startsWith('GATE_FAILED')) {
  return { wpa, wpb, wpc, wpd, wpe, stoppedAt: 'WP-F', report: wpf }
}

// ---- WP-G: docs + registry governance sweep ----
phase('WP-G Docs governance')
const wpg = await agent(
`${APPROVED}

You are WP-G: documentation + registry governance sweep for the whole object-model migration. master has WP-A..F merged.

Scope (read .harness-genplan.md §6 WP-G, §7.4):
1. Update the canonical docs to describe the new/extended objects: docs/concept-model.md, docs/data-model.md, docs/core-modules.md, docs/schemas.md, docs/goal-learning-loop.md — add Review, Gap (Bug=Gap), GoalDesign, GoalEvaluation, GoalCase, Vision; the closeout gate; the open-enum vocabularies; document the Evidence→object dual-read graduation (§7.3).
2. docs/registry.json: ensure every new doc/schema is registered (canonicalFor/machineConsumers/lastVerifiedWith/reviewAfter) so check:doc-governance + check:links pass. 
3. Update the dashboard page specs that changed: docs/dashboard/pages/{task-document,goal-document,vision-overview,warnings-repair,evidence-review-decision}.md to reflect Review/Gap/learning-layer rendering now implemented.
4. Update IMPLEMENTATION_PLAN.md to mark WP-A..G done and note remaining frontend roadmap (WP4 Member, WP5 Graph canvas, WP6 Docs wiring) as the next phase.

${GATE_NODE}
${FINISH('docs/wp-g-object-model-governance', 'WP-G: docs + registry governance for the generic object model')}`,
  { label: 'WP-G', phase: 'WP-G Docs governance' })

return {
  status: (typeof wpg === 'string' && wpg.startsWith('GATE_FAILED')) ? 'stopped-at-WP-G' : 'all-merged',
  wpa, wpb, wpc, wpd, wpe, wpf, wpg,
}