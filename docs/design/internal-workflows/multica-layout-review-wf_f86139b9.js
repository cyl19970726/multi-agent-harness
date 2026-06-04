export const meta = {
  name: 'multica-layout-review',
  description: 'Adversarial review of the Multica-style Agents layout (list + two-pane detail + stats)',
  phases: [
    { title: 'Review', detail: 'parallel review dimensions over the committed diff vs master' },
    { title: 'Verify', detail: 'adversarially verify each finding against the code' },
    { title: 'Synthesize', detail: 'prioritized fix list + verdict' },
  ],
}

const REPO = '<REPO>'
const DIFF = 'git diff master...feat/agents-multica-layout'

const FINDINGS = {
  type: 'object',
  additionalProperties: false,
  required: ['dimension', 'findings'],
  properties: {
    dimension: { type: 'string' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['title', 'severity', 'file', 'detail'],
        properties: {
          title: { type: 'string' },
          severity: { type: 'string', enum: ['blocker', 'major', 'minor', 'nit'] },
          file: { type: 'string' },
          detail: { type: 'string' },
          suggested_fix: { type: 'string' },
        },
      },
    },
  },
}

const DIMS = [
  {
    key: 'stats-and-projection',
    prompt: `Repo ${REPO}. Run \`${DIFF}\` and review the DATA layer of the Multica Agents layout. (1) computeAgentStats in apps/agent-dashboard/src/model/readModel.ts — verify the 7-day bucketing (oldest to newest, today = index 6), 30-day window, successRate (succeeded/(succeeded+failed), null when 0 terminal), avgDurationMs (only terminal sessions, end>=start), running/queued live-session selection. Edge cases: missing/NaN timestamps, "unix-ms:" parsing, zero sessions, future timestamps. (2) statsByMember built in buildWorkbenchModel using the current-time read (nowMs) inside a useMemo — is recomputing on each model build acceptable or does it cause churn/non-determinism? (3) crates/harness-cli/src/main.rs member_cards projection now includes provider_config — does this LEAK anything sensitive (env values, secrets) into the snapshot? Inspect AgentProviderConfig fields. Report findings with file:line.`,
  },
  {
    key: 'detail-shell',
    prompt: `Repo ${REPO}. Run \`${DIFF}\` and review the agent DETAIL two-pane shell in apps/agent-dashboard/src/surfaces/Surfaces.tsx (AgentDetail, AgentDetailShell usage, AgentConfigRail, CurrentWorkBanner, Tabs). Check: (1) the full-height container h-[calc(100vh-3.5rem-1px)] — is 3.5rem the real TopBar height? does it break on a different header height or when an error banner is shown above? (2) Tabs is controlled by selection.agentTab via onSelectionChange — does switching tabs preserve memberId/surface (WorkbenchShell.updateSelection merges)? does the back button/URL round-trip work? (3) ConversationStream changed min-h-[34rem] to h-full — does it still render/scroll correctly inside the flex TabsContent, and does the pinned composer stay visible? (4) the left rail is md:block (hidden under 768px) — is the agent detail usable on mobile with no config rail? (5) CurrentWorkBanner three states — any case where running session has no task_id, or queued>0 but offline. Report findings with file:line.`,
  },
  {
    key: 'list-and-reuse',
    prompt: `Repo ${REPO}. Run \`${DIFF}\` and review the AgentsList rewrite + reuse integrity in apps/agent-dashboard/src/surfaces/Surfaces.tsx. (1) agentMatchesFilter / agentIsWorking / agentIsUnstable — are the buckets coherent (e.g. can an agent be both Working and Unstable; does Idle correctly exclude both; is Offline mutually exclusive with Online)? Do the chip counts sum sensibly? (2) sort by Recent uses statsByMember lastActiveMs — undefined stats handling. (3) AgentSparkline with all-zero or empty data — renders without error? (4) the responsive grid (7 cols, lg:hidden on Workload/7-day) — does the header grid match the row grid at every breakpoint? (5) Reuse integrity: AgentRuntimeSection now appears BOTH in the left rail (RuntimeHealthPanel) and the Config tab (full AgentRuntimeSection) — any duplicate-key or double-render issue? Did the AgentDetail rework leave any dead code (old unused helpers/imports)? Report findings with file:line.`,
  },
]

phase('Review')
const reviews = (await parallel(
  DIMS.map((d) => () => agent(d.prompt, { label: `review:${d.key}`, phase: 'Review', schema: FINDINGS, agentType: 'Explore' })),
)).filter(Boolean)
const all = reviews.flatMap((r) => (r.findings || []).map((f) => ({ ...f, dimension: r.dimension })))
log(`collected ${all.length} findings`)

phase('Verify')
const VERDICT = {
  type: 'object',
  additionalProperties: false,
  required: ['real', 'severity', 'verdict'],
  properties: {
    real: { type: 'boolean' },
    severity: { type: 'string', enum: ['blocker', 'major', 'minor', 'nit'] },
    verdict: { type: 'string' },
  },
}
const verified = (await parallel(
  all.map((f) => () =>
    agent(
      `Repo ${REPO}. Adversarially verify this finding against the actual committed diff (run \`${DIFF}\`). Default real=false if you cannot confirm from the code. Finding: ${JSON.stringify(f)}`,
      { label: `verify:${f.title.slice(0, 36)}`, phase: 'Verify', schema: VERDICT, agentType: 'Explore' },
    ).then((v) => ({ ...f, ...v })),
  ),
)).filter(Boolean)
const confirmed = verified.filter((f) => f.real)
log(`confirmed ${confirmed.length}/${all.length}`)

phase('Synthesize')
const PLAN = {
  type: 'object',
  additionalProperties: false,
  required: ['must_fix', 'should_fix', 'optional', 'verdict'],
  properties: {
    must_fix: { type: 'array', items: { type: 'string' } },
    should_fix: { type: 'array', items: { type: 'string' } },
    optional: { type: 'array', items: { type: 'string' } },
    verdict: { type: 'string' },
  },
}
const plan = await agent(
  `Synthesize a prioritized fix list from these CONFIRMED findings for the Multica Agents layout in ${REPO}. Group into must_fix (block merge), should_fix (do now), optional (follow-up). One-line verdict on whether the layout is mergeable. Findings: ${JSON.stringify(confirmed, null, 2)}`,
  { label: 'synthesize', phase: 'Synthesize', schema: PLAN },
)
return { confirmed, plan }
