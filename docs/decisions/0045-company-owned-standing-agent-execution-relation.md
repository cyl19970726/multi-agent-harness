# ADR 0045: Company-owned Standing Agent execution relation


> **Superseded by DOC-108 (legacy CompanyOS retirement, 2026-08-17).**
> This ADR is retained as historical evidence only; its object model is not
> current authority. See `docs/current/product/prd.md` and
> `docs/current/architecture/architecture-map.md`.

Status: active; Agent Team responsibility projection amended by ADR 0050

## Decision

Company OS may link organization identity to reusable execution configuration
only through:

```text
StandingAgent.execution_agent_member_ref -> AgentMember.id
AgentMember.id <- MemberRun.agent_member_id
```

The first edge is optional, Company-owned, and one-to-one. Equal ids, names,
roles, providers, models, sessions, or timestamps never create the relation.
Duplicate latest StandingAgent refs are an integrity error, not last-write-wins.

## Authoring the first edge

The relation is authored by one explicit command per pair,
`harness company org link-execution` / `unlink-execution`, over records that
already exist. Both ids are typed explicitly, including when they are equal.
The StandingAgent is read latest-row-wins and re-appended through the Human
administrative governance envelope with only `execution_agent_member_ref` and
`updated_at` changed; no command edits raw JSONL. Re-running an identical pair
appends no row, and repointing an existing link requires `--replace`.

Idempotence is not an authorization exemption. Every invocation proves an active
Human `company_os.admin` authority before deciding whether anything changed, so
a no-op cannot become a bypass for an unknown or non-admin operator.

## Cross-store validation

AgentMember truth lives in an Execution Space, not in the Company Store
(ADR 0042), and `harness company ...` resolves the Company Store without ever
reaching the `--space` selector. `link-execution` therefore requires an explicit
`--execution-space <id>` and opens that space read-only to confirm the
AgentMember exists. There is no fallback to the active space and none to a
Project Binding, which describes provider cwd rather than identity.

The validating space id is a write-time assertion and is deliberately **not**
persisted on the StandingAgent: persisting it would move execution-space truth
into a Company OS row. The read projection therefore resolves the reference
against whichever Execution Space the reader selects, and a reader pointed
elsewhere sees empty participation rather than an error.

## Duplicate-link failure containment

Rejecting a duplicate on write and failing on read are different obligations. A
duplicate that already exists in a store is a defect in one pair of rows, so the
read projection must degrade locally: it withholds the ambiguous
`agent_member_id` from the join, names every claimant and every withheld
MemberRun in `standing_assignment_conflicts`, and lets the rest of the snapshot
succeed. One bad link must never take down the Dashboard for the whole company.

## Lifecycle boundary

StandingAgent owns organization identity, authority, and declared availability.
AgentMember owns reusable execution configuration. MemberRun owns one TeamRun
participation. MemberRun start, idle, failure, Close, Supervisor recovery, or
native-session changes never write lifecycle state back to either durable
identity.

The Company projection is read-only. It may expose Work-less participation,
chronological owned Agent Team Works, mailbox and pending
interaction counts, Supervisor/Close facts, evidence references, and navigation
without inferring business availability or authority.

Wave and Mission ids are optional navigation context. Agent Team Work and its
WorkEvents own member responsibility/status; TeamMessage is conversation only.
