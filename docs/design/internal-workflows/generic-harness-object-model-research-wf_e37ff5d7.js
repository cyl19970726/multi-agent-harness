export const meta = {
  name: 'generic-harness-object-model-research',
  description: 'Compare ALL let-me-try v1 skills against our schema/docs/Rust/frontend; produce a detailed generic-harness object-model + schema/backend/frontend migration plan',
  phases: [
    { title: 'Enumerate' },
    { title: 'Gather' },
    { title: 'Synthesize' },
  ],
}

phase('Enumerate')

const enumPrompt = `Use the authenticated gh CLI to enumerate EVERY skill in cyl19970726/let-me-try on branch v1.

Run: gh api "repos/cyl19970726/let-me-try/git/trees/v1?recursive=1" --jq '.tree[].path' | grep -E "\\.agents/skills/"

Return a clean newline-separated list of every path under .agents/skills/ (SKILL.md files AND any reference/*.md or template files they contain). Group by skill directory. Also give a one-line guess of each skill's topic from its directory name. Output ONLY the structured list — this drives the next phase.`

const skillList = await agent(enumPrompt, { label: 'enumerate:lmt-skills', phase: 'Enumerate' })

phase('Gather')

const lmtSkillsPrompt = `Use the authenticated gh CLI to read the let-me-try v1 skills and extract their OBJECT/DATA model exhaustively. Here is the file list discovered:

${skillList}

For EVERY skill directory (read each SKILL.md and the reference/template .md files it points to; fetch raw via: gh api "repos/cyl19970726/let-me-try/contents/<PATH>?ref=v1" --jq '.content' | base64 --decode  OR curl -s https://raw.githubusercontent.com/cyl19970726/let-me-try/v1/<PATH>):

Produce, per skill:
- skill name + one-line purpose
- every DATA OBJECT it defines or operates on (e.g. Task, Goal, Trial, Evidence, Review, Decision, Agent/role, Workflow, Project-state, Backlog, Bug-ledger, GoalCase, etc.)
- for each object: the COMPLETE field/section list (exact names), required-vs-optional if stated, value vocabularies/enums, lifecycle/status states
- invariants / definition-of-done / gates the skill enforces
- relationships (goal->task->subtask, depends_on/blocks, owner/assignee/reviewer/challenger/evaluator roles, evidence->proposal->review->decision)
- explicitly flag which parts are CODE-TRIAL / let-me-try-DOMAIN-SPECIFIC (e.g. trading, code execution, specific gap taxonomies) vs which are GENERIC multi-agent coordination concepts.

Be exhaustive and quote exact field names. This is the reference contract we will generalize. If a path 404s, note and continue. Cover ALL skills, not just task/goal.`

const ourSchemaDocsPrompt = `Read the LOCAL repo <REPO>. Map our CURRENT canonical object model from schemas + docs.

1. schemas/: list every *.json schema file; for EACH, list every field with type, required/optional, enums, and additionalProperties setting. Note the constraint that schemas use additionalProperties:false AND list all properties as required (so any new field is a breaking change).
2. docs: read docs/concept-model.md, docs/data-model.md, docs/core-modules.md, docs/prd.md, docs/schemas.md, AGENTS.md. Extract every harness object (Goal, GoalDesign, AgentTeam, AgentMember, AgentRuntime, AgentEvent, Task, Message, Proposal, Evidence, Decision, ProviderSession, GoalEvaluation, GoalCase, NextRoundPlan, Vision, etc.), its defined fields, lifecycle/states, and invariants. Note which objects are described in docs but have NO schema yet (e.g. GoalDesign, GoalEvaluation, GoalCase, Vision).

Return structured markdown: "Schemas (concrete)" and "Docs-defined objects (some schema-less)", with exact field names and the schema-vs-docs gaps called out.`

const ourBackendPrompt = `Read the LOCAL repo <REPO> Rust backend. Map what EXISTS so we can scope backend changes for new schema fields/objects.

Explore crates/: harness-core, harness-store, harness-cli.
- List the Rust structs/enums that mirror harness objects (Goal, Task, AgentMember, Message, Proposal, Evidence, Decision, etc.) — file + struct name + fields + serde attributes.
- How is the dashboard snapshot produced? Find the command (harness-cli ... dashboard snapshot) and the struct that serializes the DashboardSnapshot (which arrays/fields it emits). 
- How is state stored (harness-store: file store / jsonl)? What would adding a new object type or field touch (struct, serialization, store read/write, snapshot assembly, any validation against schemas/)?
- Note any place schemas/*.json are validated against (scripts or Rust) so we know what a schema change must keep in sync.

Return structured markdown: object->Rust-struct map, the snapshot producer path, and a "what a new field/object touches in Rust" checklist.`

const ourFrontendPrompt = `Read the LOCAL repo <REPO> frontend (apps/agent-dashboard, on master after the shadcn rebuild + WP1/WP2 merges).

Map current object usage:
- src/types.ts: every interface and its fields (Goal, Task, AgentMember, AgentTeam, Message, Proposal, Evidence, Decision, ProviderSession, AutonomousProposal, GoalLearningStatus, DashboardSnapshot, WorkflowWarning, etc.).
- src/model/readModel.ts: what WorkbenchModel exposes and derives.
- src/model/demoSnapshot.ts: the shape of the offline fixture (which fields are populated).
- src/surfaces/Surfaces.tsx + src/app/WorkbenchShell.tsx: per-surface, which object fields are rendered. Current rail is Team/Vision/Tasks/Member/Warnings (Goal/Task drill-in; Docs/Decisions/Debug demoted).

Return: the frontend object field inventory, what each surface consumes, and which fields are typed-but-unused / shown-but-thin. This tells us the frontend delta when schema grows.`

const [lmtSkills, ourSchemaDocs, ourBackend, ourFrontend] = await parallel([
  () => agent(lmtSkillsPrompt, { label: 'lmt:object-model', phase: 'Gather' }),
  () => agent(ourSchemaDocsPrompt, { label: 'ours:schema+docs', phase: 'Gather' }),
  () => agent(ourBackendPrompt, { label: 'ours:rust-backend', phase: 'Gather' }),
  () => agent(ourFrontendPrompt, { label: 'ours:frontend', phase: 'Gather' }),
])

phase('Synthesize')

const synthPrompt = `You are the architect for Star Harness, a GENERIC multi-agent coordination system. The owner wants to adopt the good design from let-me-try's skills (which are referenced but code-trial-specific) and GENERALIZE it into our schema + Rust backend + frontend, as one coherent object-model migration plan.

=== INPUT A: let-me-try v1 ALL skills — object/data model (mark domain-specific vs generic) ===
${lmtSkills}

=== INPUT B: our current schemas + docs object model (note schema-vs-docs gaps) ===
${ourSchemaDocs}

=== INPUT C: our Rust backend reality (struct map, snapshot producer, what a change touches) ===
${ourBackend}

=== INPUT D: our frontend object usage (types/readModel/surfaces) ===
${ourFrontend}

Produce a VERY DETAILED, decision-ready markdown plan with these sections:

## 1. Side-by-side object/field comparison
A table per major object (Goal, Task, and any others let-me-try defines: Evidence, Review/Challenger, Decision, Project-state/Backlog, Bug-ledger, GoalCase, roles, Workflow): let-me-try field/concept | our field (or —) | keep / adopt / generalize / drop | note. Cover EVERY let-me-try object.

## 2. Genericization principles
For each let-me-try concept that is code-trial/domain-specific (e.g. specific gap/bug taxonomies, trading/code-exec assumptions, fixed role names), state how to abstract it into a generic harness concept (e.g. typed enums with an "other/extensible" escape, adapter-provided vocabularies, optional domain metadata). The harness core must stay domain-neutral; domain specifics belong in adapters/skills.

## 3. Proposed unified schema
Per object: the target field set (exact snake_case names), required vs optional, enums, and NEW objects to add (e.g. GoalDesign, GoalEvaluation, GoalCase, Vision, Review, Phase/Subtask). Address the hard constraint that current schemas use additionalProperties:false + all-required: specify the versioning strategy (relax required to optional? introduce *.v2? add a schema_version field?) and the exact migration so existing snapshots/fixtures don't break.

## 4. Backend (Rust) change plan
Concrete: which structs/enums in harness-core change or are added, serde impacts, harness-store read/write, the DashboardSnapshot producer, and any schema-validation sync. Sequence + size each.

## 5. Frontend change plan
Per surface (Team/Vision/Tasks/Goal/Task/Member/Warnings/Docs/Decisions/Debug): the types.ts additions, readModel derivations, demoSnapshot fixture updates, and rendering changes each new field/object unlocks. Tie to the existing page specs in docs/dashboard/pages/*.md.

## 6. Sequenced work packages
WP list with: scope, layers touched (schema/Rust/frontend/docs), size (S/M/L), dependencies, and the gate (pnpm check / tsc+build / cargo test). Order so each WP is independently mergeable and green. Make the schema/back-compat WP first.

## 7. Risks, migration, back-compat
Breaking-change risks, fixture/snapshot migration, doc-governance (registry.json), and how to keep cargo test + pnpm check green throughout.

Be concrete and faithful; prefer exact field names; explicitly separate "generic harness core" from "let-me-try/domain-specific". This goes to the owner to approve before any code changes.`

const synthesis = await agent(synthPrompt, { label: 'synthesis:migration-plan', phase: 'Synthesize' })

return synthesis