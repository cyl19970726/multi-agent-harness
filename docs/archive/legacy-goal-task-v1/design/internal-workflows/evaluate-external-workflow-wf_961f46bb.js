export const meta = {
  name: 'evaluate-external-workflow',
  description: 'Meta-evaluation: use an internal workflow to evaluate the external dynamic-workflow runtime across observability / execution / evaluability / production-readiness, adversarially verify, synthesize a roadmap',
  phases: [{ title: 'Assess' }, { title: 'Verify' }, { title: 'Synthesize' }],
}

const ASSESS = {
  type: 'object',
  properties: {
    dimension: { type: 'string' },
    score: { type: 'string', enum: ['pass', 'partial', 'gap'] },
    evidence: { type: 'array', items: { type: 'string' } },
    gaps: { type: 'array', items: { type: 'string' } },
    recommendations: { type: 'array', items: { type: 'string' } },
  },
  required: ['dimension', 'score', 'evidence', 'gaps', 'recommendations'],
}
const VERIFY = {
  type: 'object',
  properties: {
    dimension: { type: 'string' },
    holds: { type: 'boolean' },
    corrections: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['dimension', 'holds', 'corrections'],
}
const REPORT = {
  type: 'object',
  properties: {
    overall: { type: 'string' },
    scorecard: { type: 'array', items: { type: 'object', properties: { dimension: { type: 'string' }, score: { type: 'string' } }, required: ['dimension', 'score'] } },
    top_findings: { type: 'array', items: { type: 'string' } },
    roadmap: { type: 'array', items: { type: 'object', properties: { priority: { type: 'string' }, item: { type: 'string' }, why: { type: 'string' } }, required: ['priority', 'item', 'why'] } },
    production_verdict: { type: 'string' },
  },
  required: ['overall', 'scorecard', 'top_findings', 'roadmap', 'production_verdict'],
}

const CTX = `Subject: the EXTERNAL dynamic-workflow runtime in <REPO> — \`harness workflow run-script <prog.star>\` (Starlark front-end) + ephemeral codex/claude workers. The standing rubric is docs/research/dynamic-workflow-evaluation.md (READ it first). You have shell + read tools. Ground EVERY claim in real evidence you actually read; do not speculate. Two real dogfood runs exist to inspect: live-review (wfrun-1780470595069-0, 2 codex parallel review, 527.9k tok) and dogfood-schema (wfrun-1780473555264-0, codex implemented schema output then claude verified). Evidence lives in: the snapshot API \`curl -s http://127.0.0.1:8787/v1/snapshot\`; the store .harness/workflow_runs.jsonl + workflow_steps.jsonl; durable traces .harness/provider-sessions/*/{codex,claude}.stream-json.ndjson + last-message.md; and the code crates/harness-workflow/src/starlark_front.rs + crates/harness-cli/src/main.rs (workflow_run_script_value, workflow_real_agent_step, parse_claude_model/parse_codex_model/build_step_details) + apps/agent-dashboard/src/surfaces/Workflows.tsx.`

const DIMS = [
  { key: 'observability', prompt: `Assess OBSERVABILITY. Are status / design_intent / tokens / cost / model / log() lines / worktree-diff / durable trace ALL captured AND surfaced (store + API + dashboard), live (running) AND post-hoc? SPECIFICALLY investigate: on the dogfood-schema run both steps showed model=None — read parse_claude_model + build_step_details + the run-script driver path and determine whether claude's model is actually wired through (or is parse_claude_model effectively dead for run-script). Score pass/partial/gap.` },
  { key: 'execution-effectiveness', prompt: `Assess EXECUTION EFFECTIVENESS. The dogfood-schema run had codex IMPLEMENT a real feature (optional schema output; git diff shows ~337 insertions across 3 files; 2 new tests pass; builds clean) then claude VERIFY. Read the codex diff (git diff crates/), the claude verify final message (the verify step's last-message.md), and the steps. Judge: did it accomplish real correct work? quality of the implement->verify handoff? failure handling? AND cost-effectiveness — codex burned ~10.9M input tokens / ~30 min for this one feature. Score pass/partial/gap.` },
  { key: 'evaluability', prompt: `Assess EVALUABILITY — can a run be GRADED, ideally automatically, against its intent? Read WorkflowRun/WorkflowStep fields (crates/harness-core) and how success is represented (status, summary, verdict). Today a verify step returns PROSE, not a typed pass/fail. Assess whether the schema feature codex just added (crates/harness-workflow/src/starlark_front.rs) now lets a verify worker return a machine-readable verdict the program can branch on, and what is STILL missing (a declared success criterion on the run? an evidence ledger linking criterion->step->verdict?). Score pass/partial/gap.` },
  { key: 'production-readiness', prompt: `Assess PRODUCTION-READINESS. Read starlark_front.rs + workflow_run_script_value + workflow_real_agent_step + the worktree isolation/cleanup code. Assess: determinism/resume (hermetic Starlark); worktree isolation + cleanup (any orphan risk?); SECURITY (workers EDIT files and RUN commands for real — what sandbox/permission actually bounds them?); concurrency caps; governance (mandatory design_intent); error handling (stuck Running rows finalized?); COST controls (is there ANY per-run token/cost budget ceiling? the dogfood burned 10.9M tokens unbounded); --timeout-ms default (3000ms vs real provider turns). Score pass/partial/gap.` },
]

phase('Assess')
const assessed = await pipeline(
  DIMS,
  (d) => agent(`${CTX}\n\n${d.prompt}`, { schema: ASSESS, phase: 'Assess', label: `assess:${d.key}` }),
  (a, d) => agent(
    `${CTX}\n\nAdversarially CHECK this assessment of the external workflow's ${d.key}. Read the actual code/artifacts and try to REFUTE each claimed gap — is it REAL, or already handled / overstated? Return holds=true only if the assessment stands; list corrections for anything wrong.\n\nASSESSMENT:\n${JSON.stringify(a)}`,
    { schema: VERIFY, phase: 'Verify', label: `verify:${d.key}` },
  ).then((v) => ({ ...a, verdict: v })),
)

const valid = assessed.filter(Boolean)
phase('Synthesize')
const report = await agent(
  `${CTX}\n\nSynthesize these four adversarially-verified dimension assessments into a production-readiness evaluation of the external dynamic-workflow runtime. Give: overall (2-3 sentences), a scorecard (dimension -> pass/partial/gap, using the verified score), top_findings, a PRIORITIZED roadmap (priority P0..P3, item, why — ground each in the assessments; P0 should be the single biggest blocker to production), and a production_verdict (is it production-ready yet, and the 1-2 things that gate it). Honor the verifiers' corrections where holds=false.\n\nVERIFIED ASSESSMENTS:\n${JSON.stringify(valid)}`,
  { schema: REPORT, phase: 'Synthesize', label: 'synthesize' },
)

return { assessed: valid, report }
