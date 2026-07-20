# AI Company OS V1 visual contract

This workstream defines expected visual direction for the AI Company OS. It does not replace the current Mission/Wave Workbench implementation or its `workbench-layout-v2` contract; the Workbench remains its execution-tool visual baseline. Company OS V1 defines the product layer above it: documents as company memory and action entry, mixed human/agent organization, typed operational records, and governed growth.

## Product center

```text
Docs -> Work -> People + Standing Agents -> results and records -> Docs
                           |                         |
                           +---- Finance / approvals -+
                                      |
                                  Governance
```

Primary navigation:

```text
Home
Docs
Organization
```

Every page shows the same grouped secondary navigation:

```text
OPERATIONS  Work · Approvals · Finance
EXECUTION   Missions · Workflows · Agent Teams
PLATFORM    Providers · Plugins · Settings
```

These are execution, control, and platform tools; they are not the product center.

## Visual grammar

- Warm light-gray application background, white document/work surfaces, fine neutral borders, and coral-red accent for selection and primary action.
- Compact but calm enterprise density; readable English labels; no decorative illustration or dark control-room aesthetic.
- Every expected image is desktop `1536x1024`. Actual browser hierarchy is validated at `1440x1000` desktop plus `900x1180` tablet and `390x844` mobile for the seven focus/decision pages.
- Documents are rich Notion-like surfaces: prose, databases, views, charts, relation chips, people/agent cards, and inline action entry may coexist.
- Human, standing-agent, and external participants share a participant reference style, but retain an explicit `Human`, `Standing Agent`, or `External` type label.
- Finance, work, approval, and governance panels display connected typed records. A document does not become a static duplicate of a record it embeds.

## Safety and truth boundaries

- Never infer availability from a healthy runtime or idle session.
- Never infer assignment ownership from names, timing, or ordinary chat.
- Never persist or replay provider thinking; only sanitized, transient live state may later be displayed.
- WorkItem and explicit assignments carry responsibility; Mission/Wave and Workflow remain optional execution mechanisms.
- Money movement, legal filing, organization/permission change, and other high-risk actions retain visible human approval and durable evidence.

## Per-page visual lifecycle

Each page records one truthful lifecycle stage, in this order:

```text
spec_ready
-> candidate_generated_needs_revision
-> expected_generated
-> expected_approved
-> implemented
-> actual_captured
-> compared
-> accepted
```

`spec_ready` means its prompt and product contract are ready but no corrected expected image exists. `candidate_generated_needs_revision` is explicitly non-authoritative. `expected_approved` requires an expected-image hash plus approval identity and date. Browser evidence and a comparison file are required before `accepted`.

The obsolete-IA and truth-defect generations are retained under `candidates/`
as non-authoritative design history. All twelve corrected images now exist under
`expected/`, have recorded SHA-256 hashes, and are marked `expected_generated`.
They remain pending product approval; generation is not approval.

## Evidence lifecycle

```text
spec-ready prompt -> candidate -> approved expected image
  -> browser implementation capture -> labeled comparison -> review decision
```

The complete set is visible in
[`contact-sheet--12-core-pages.png`](contact-sheet--12-core-pages.png). Individual
images, prompts, hashes, approval state, future browser captures, and comparison
paths are tracked in [`visual-contract.json`](visual-contract.json). See
[page-matrix.md](page-matrix.md) for coverage.

The durable implementation evidence is in [`actual/`](actual/). Open
[`expected-vs-actual.html`](expected-vs-actual.html) for the audited three-way
comparison: missing pre-implementation route, expected design, and actual live
Store-backed browser render. [`comparison-manifest.json`](comparison-manifest.json)
pins all 26 actual image hashes and their source capture; expected images remain
pending Human visual approval.
