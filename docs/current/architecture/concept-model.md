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

The Team Host is not another executor object. `AgentTeam.host_agent_id`
resolves one active `TeamMembership(role=host)` for the same `AgentMember`
model. A current TeamRun has exactly one Host MemberRun. Managed Hosts follow
the ordinary MemberRun/AgentSession/NodeDaemon/runtime-adapter relation;
`external_interactive` is the same identity and role with a detached
user-driven runtime and an explicit pull-only delivery guarantee. See
[ADR 0057](../../decisions/0057-host-is-an-agent-member.md).

## Work graph mental model

Work is a flat responsibility node, never a container. Hard directed edges
connect prerequisite Work to successor Work and must remain acyclic. Fan-out
expresses parallel follow-up; fan-in expresses convergence after several
accepted prerequisites. The kernel derives readiness and explanations from
the graph plus Work lifecycle. Messages may discuss or propose graph changes
but never create edges. Provider plans and subagents remain inside one Work and
never become hidden graph authority.

```text
             ┌─> Work B ─┐
Work A ──────┤            ├─> Work D
             └─> Work C ─┘
```

The product mental model is maintained in Notion `02 Work & Message`; the
repository implementation crosswalk is
[ADR 0058](../../decisions/0058-work-dependency-dag-and-kernel-boundary.md).

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
