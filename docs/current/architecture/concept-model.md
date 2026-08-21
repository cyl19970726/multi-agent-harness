# Concept Model

## Authority

Product doctrine for this topic — core object relationships, the active
coordination vocabulary, executor kinds, and anti-drift invariants — is
canonical in Notion; see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location. Source-of-truth rules and
gate invariants stay in [data-model.md](data-model.md). This repository file
survives only as the implementation-bound remainder below.

The active coordination vocabulary excludes Dynamic Workflow. Its historical
objects are archive evidence only; `AgentTeam`, `Work`, identity-first
`Message`, provider-native sessions, and fenced `RuntimeCommand` effects form
the current model.

## Implementation-bound invariants

Open-enum vocabularies: harness defines a canonical starter set in Rust,
JSON keeps the field as `string`, and adapters may add values without a
schema bump. Only truly closed, harness-owned sets should use hard JSON
enums.

| Field | Object | Canonical values |
| --- | --- | --- |
| `review_kind` | Review | `acceptance`, `correctness`, `safety`, `design`, `data_flow`, `docs`, `other` |
| `verdict` | Review | `pass`, `fail`, `blocked`, `needs_changes` |
| `decision` | Decision | `accept`, `reject`, `revise`, `split`, `block`, `promote`, `waive`, `follow_up`, `stop_approved`, `continue_required` |
| `decision_kind` | Decision | `verdict`, `gate`, `stop_gate`, `waiver`, `closeout`, `promotion`, `other` |
| `evidence_kind` | Evidence | `check`, `log`, `session`, `diff`, `review_note`, `screenshot`, `artifact`, `snapshot`, `historical work design`, `outcome evaluation`, `other` |
| `category` | Gap | `ux`, `data`, `observability`, `parity`, `tooling`, `workflow`, `docs`, `bug`, `other` |
| `outcome` | outcome evaluation | `success`, `partial`, `failed`, `blocked` |
