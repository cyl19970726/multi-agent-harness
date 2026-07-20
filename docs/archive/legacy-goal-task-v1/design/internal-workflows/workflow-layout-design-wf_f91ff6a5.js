export const meta = {
  name: 'workflow-layout-design',
  description: 'Design a complete layout for the Workflow surface: deep-understand the project workflow model + existing design system, run a 3-way layout tournament, judge and synthesize ONE complete layout spec (wireframes + regions + states + design-system mapping). No implementation.',
  phases: [
    { title: 'Understand', detail: 'workflow domain semantics + dashboard design system' },
    { title: 'Propose', detail: '3 distinct complete layout proposals' },
    { title: 'Synthesize', detail: 'judge + merge into one complete layout doc' },
  ],
}

const WT = '<WORKTREE>'

const UNDERSTAND_DOMAIN = {
  type: 'object',
  required: ['objects', 'lifecycle', 'composition', 'control_flow', 'investigate_shape', 'what_layout_must_show', 'data_availability'],
  properties: {
    objects: { type: 'string', description: 'WorkflowRun / WorkflowStep exact fields + meaning' },
    lifecycle: { type: 'string', description: 'run + step status state machines and what each state means visually' },
    composition: { type: 'string', description: 'how a step targets an agent (member_id) and links a provider_session (the raw turn)' },
    control_flow: { type: 'string', description: 'serial vs parallel vs barrier — how phase + step_ids ordering encodes shape' },
    investigate_shape: { type: 'string', description: 'the concrete investigate workflow shape, step by step' },
    what_layout_must_show: { type: 'array', items: { type: 'string' }, description: 'the concrete information the layout must surface, prioritized' },
    data_availability: { type: 'string', description: 'what is already in snapshot/SSE vs needs the read-only source endpoint; note there is likely NO live run data yet (empty state matters)' },
  },
}

const UNDERSTAND_DESIGN = {
  type: 'object',
  required: ['layout_primitives', 'master_detail_pattern', 'agentdetail_anatomy', 'tokens', 'reusable_atoms', 'drillin_pattern', 'nav', 'constraints'],
  properties: {
    layout_primitives: { type: 'string', description: 'DocumentSurface / DocSection / SurfaceHeader / grid conventions, max-width, spacing scale' },
    master_detail_pattern: { type: 'string', description: 'how AgentsList -> AgentDetail navigation/selection works; the list+detail spatial model' },
    agentdetail_anatomy: { type: 'string', description: 'AgentDetail regions (header, DocProperties, DocSection blocks: task/conversation/runtime) — the template to echo' },
    tokens: { type: 'string', description: 'StatusTone palette, Badge tones, status-{tone} classes, typography sizes used' },
    reusable_atoms: { type: 'array', items: { type: 'string' }, description: 'atoms available to compose (StatusDot, EmptyState, TimelineRow, MonoId, Avatar, Markdown, DocProperties...)' },
    drillin_pattern: { type: 'string', description: 'the #60 TurnDrillIn lazy-fetch chevron pattern, to reuse for per-step provider turns' },
    nav: { type: 'string', description: 'how the left rail / navItems / surface switching works' },
    constraints: { type: 'array', items: { type: 'string' }, description: 'visual/consistency constraints any proposal MUST respect (dark console aesthetic, no new heavy deps, etc.)' },
  },
}

const PROPOSAL = {
  type: 'object',
  required: ['name', 'philosophy', 'index_view', 'run_detail_view', 'structural_graph', 'code_view', 'per_step_drillin', 'live_state', 'empty_loading_states', 'responsive', 'wireframes', 'tradeoffs'],
  properties: {
    name: { type: 'string' },
    philosophy: { type: 'string', description: 'the core idea / what this layout optimizes for' },
    index_view: { type: 'string', description: 'the Workflows list/index: what regions, columns, grouping, how registered defs vs runs are shown' },
    run_detail_view: { type: 'string', description: 'the run detail page overall composition and information hierarchy' },
    structural_graph: { type: 'string', description: 'how the serial/parallel phase->step shape is visualized (pipeline diagram? columns? nested timeline?) and where it sits' },
    code_view: { type: 'string', description: 'how BOTH the structural graph and the Rust source are presented (tabs? side panel? collapsible?)' },
    per_step_drillin: { type: 'string', description: 'how each step exposes its agent + provider-turn drill-in inline' },
    live_state: { type: 'string', description: 'how Running/Queued/Completed/Failed and live progress are conveyed at a glance' },
    empty_loading_states: { type: 'string' },
    responsive: { type: 'string', description: 'narrow vs wide behavior' },
    wireframes: { type: 'string', description: 'ASCII wireframes for BOTH the index view and the run detail view — concrete, labeled regions' },
    tradeoffs: { type: 'string', description: 'what this layout sacrifices' },
  },
}

const SYNTHESIS = {
  type: 'object',
  required: ['winner_rationale', 'doc_path', 'summary'],
  properties: {
    winner_rationale: { type: 'string', description: 'which proposal(s) won on which dimensions and why; what was grafted from runners-up' },
    doc_path: { type: 'string', description: 'path to the written layout design doc' },
    summary: { type: 'string', description: 'a tight prose summary of the final chosen layout for both views, including the key ASCII wireframes inline, so it can be shown to the user verbatim' },
  },
}

// ---------------- Phase 1: Understand (parallel) ----------------
phase('Understand')
const [domain, system] = await parallel([
  () => agent(
    `Deeply understand the WORKFLOW domain in the Rust project at ${WT} so a UI layout can faithfully express it. Read crates/harness-core/src/lib.rs (WorkflowRun/WorkflowStep/status enums ~1220-1309), crates/harness-cli/src/workflow.rs (WorkflowRegistry, WorkflowDef, the investigate fn, serial + parallel + barrier helpers), crates/harness-cli/src/main.rs (dashboard_snapshot — workflow_runs/workflow_steps; how steps link provider_session_id; run_workflow_with_driver). Output the precise semantics the layout must convey.`,
    { label: 'understand:domain', phase: 'Understand', schema: UNDERSTAND_DOMAIN }
  ),
  () => agent(
    `Catalog the EXISTING dashboard design system at ${WT}/apps/agent-dashboard so a new Workflow surface layout is visually consistent. Read src/components/workbench/atoms.tsx (DocumentSurface/DocSection/SurfaceHeader/StatusDot/EmptyState/TimelineRow/DocProperties/MonoId/Avatar/Markdown), src/surfaces/Surfaces.tsx (AgentsList ~531, AgentDetail ~2499, TurnDrillIn ~2923), src/app/WorkbenchShell.tsx (rail/navItems/SurfaceSwitch), src/app/selection.ts, and the tailwind tokens/status-{tone} usage. Output the layout primitives, the master-detail pattern, the AgentDetail anatomy to echo, and the hard consistency constraints.`,
    { label: 'understand:design-system', phase: 'Understand', schema: UNDERSTAND_DESIGN }
  ),
])
log('Understanding done. Running 3-way layout tournament.')

// ---------------- Phase 2: Propose (parallel, 3 distinct philosophies) ----------------
phase('Propose')
const philosophies = [
  { key: 'pipeline-centric', brief: 'GRAPH/PIPELINE-CENTRIC: the run-detail page makes the serial->parallel pipeline diagram the hero (phases as columns/stages left-to-right, parallel steps stacked within a stage, each node a compact agent card with live status); list view is a dense runs table. Optimize for "see the shape and live progress at a glance".' },
  { key: 'master-detail-timeline', brief: 'MASTER-DETAIL TIMELINE: a two-pane surface — left a runs rail (grouped running/recent), right a vertical phase->step timeline (echoing AgentDetail conversation/runtime sections), each step expandable to its provider turn. Optimize for consistency with the existing AgentDetail/master-detail muscle memory.' },
  { key: 'document-narrative', brief: 'DOCUMENT/NARRATIVE: a DocumentSurface page like AgentDetail — header + DocProperties (workflow name/status/timing) + DocSection blocks (Structure, Steps, Code, Output), reading top-to-bottom like a run report; structural graph rendered inline as an indented nested list. Optimize for calm readability and minimal new visual vocabulary.' },
]
const proposals = (await parallel(philosophies.map(p => () =>
  agent(
    `Propose a COMPLETE layout for the read-only Workflow surface following this philosophy:\n${p.brief}\n\nIt must cover BOTH the Workflows index view AND the run-detail view, and must reflect this domain:\n${JSON.stringify(domain, null, 2)}\n\nand stay consistent with this design system (reuse its atoms; respect its constraints):\n${JSON.stringify(system, null, 2)}\n\nRequirements to satisfy: (1) list running/completed/failed runs + show registered workflow defs; (2) run detail must show the serial/parallel phase->step structure, each step's target AGENT + status + timing + output, with a per-step drill-in to the raw provider turn (reuse TurnDrillIn); (3) a code view showing BOTH the structural graph AND the Rust source (lazy-fetched); (4) clean empty/loading states (likely no live run data yet); (5) responsive narrow/wide. Provide concrete ASCII wireframes for BOTH views.`,
    { label: `propose:${p.key}`, phase: 'Propose', schema: PROPOSAL }
  )
))).filter(Boolean)
log(`${proposals.length} layout proposals generated. Judging + synthesizing.`)

// ---------------- Phase 3: Judge + Synthesize ----------------
phase('Synthesize')
const synthesis = await agent(
  `You are the design lead. Here are ${proposals.length} complete layout proposals for the read-only Workflow surface:\n\n${JSON.stringify(proposals, null, 2)}\n\nDomain it must serve:\n${JSON.stringify(domain.what_layout_must_show, null, 2)}\nDesign-system constraints:\n${JSON.stringify(system.constraints, null, 2)}\n\nScore them on: (1) faithfulness to the workflow semantics (serial/parallel/barrier, agent composition, live state), (2) consistency with the existing dashboard design language, (3) scalability (many runs, many steps, deep parallelism), (4) clarity of the code view (structural + source), (5) graceful empty/loading states. Then SYNTHESIZE ONE complete, opinionated final layout — pick the strongest base and graft the best ideas from the others. 

Write the final layout design to ${WT}/docs/design/workflow-surface-layout.md (create dirs). The doc must contain: the chosen information architecture; the Workflows INDEX view (regions + ASCII wireframe); the RUN DETAIL view (regions + ASCII wireframe), including exactly how the serial/parallel structure, per-step agent + status + output, provider-turn drill-in, and the code view (structural graph + collapsible Rust source) are laid out; all empty/loading/error states; responsive behavior; and a mapping of every region to the existing design-system atoms/components it will reuse. Make it concrete enough to implement directly. Return a tight summary (with the two key ASCII wireframes inline) suitable to show the user verbatim.`,
  { label: 'synthesize:final-layout', phase: 'Synthesize', schema: SYNTHESIS }
)

return {
  doc_path: synthesis.doc_path,
  winner_rationale: synthesis.winner_rationale,
  summary: synthesis.summary,
}
