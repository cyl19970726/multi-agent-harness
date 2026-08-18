# ADR 0051: Single Intent Spine — Mission Log Absorbs Wave, Host Native Goal Is Derived


> **Superseded by DOC-108 (legacy CompanyOS retirement, 2026-08-17).**
> This ADR is retained as historical evidence only; its object model is not
> current authority. See `docs/current/product/prd.md` and
> `docs/current/architecture/architecture-map.md`.

```text
status: accepted; staged breaking cutover (see Implementation Boundary)
owner_role: architecture
date: 2026-08-05
amends: ADR 0034 (Wave section)
completes: ADR 0028 removal policy
dissolves: issue #271 (rejected-Wave-advance semantics)
```

## Context

A session-forensics audit of the first work-board TeamRun (2026-08-04/05,
host session `019fac11`) traced how Mission, Wave, and the Host's own
provider-native state actually functioned as intent storage across one
evening of real scheduling.

- **Wave became a write-only narration log.** The member Skill never
  mentions Mission or Wave — Members cannot see either object by design.
  Every Host `wave show` in the transcript sat immediately adjacent to a
  `wave update`: a version-read protocol, not an input any visible decision
  consumed. At both re-orientation crises of the evening — post-compaction
  amnesia and supervisor-death recovery — the Host consulted `--help`
  output and CLI source to re-derive what to do next, never the Wave memo.
  In both cases the recovery decision was made first and narrated into the
  Wave afterwards.
- **`wave gate` has had zero invocations since 2026-07-31**, an
  executor-era leftover that outlived the executor hierarchy it gated.
- **The Host maintained four concurrent intent copies with no invalidation
  propagation between them**: the provider-native goal (`create_goal` ×8,
  `get_goal` ×34), the provider-native plan (`update_plan` ×167),
  Mission/Wave rows (mission writes ×168, wave writes ×384), and the
  window's own narrative context, which 4 compactions destroyed and forced
  a re-derivation from whichever of the other three the Host reached for.
- **The host session is the least durable component in the system**: 18
  `session_meta` restarts and 4 compactions in one evening, against Mission
  and Work rows that survived every one of them untouched.
- **ADR 0028 already retired the Goal/GoalPhase/task-graph stack**;
  Mission-plus-Wave was its successor. The retired stack had exactly one
  mechanism Wave lacks: its append-only ledger had a mandatory reader — the
  replan loop had to read the ledger before it was allowed to replan.
  Folding Wave into Mission must import that mechanism, or the fold
  recreates Wave's write-only failure one level up.

## Decision

### Mission absorbs Wave as an append-only Mission Log

Mission gains `MissionLogEntry` records: a monotonically increasing
`revision`, `kind` in {`judgment`, `replan`, `recovery`,
`closeout-evidence`}, a Markdown `body`, `created_at`, and `actor`. The Wave
object and its commands retire under the staged cutover in Implementation
Boundary below. An append-only log has no "advance" operation, so the
rejected-Wave-advance semantics question raised in issue #271 dissolves
along with the object it was about — there is no terminal Wave decision to
gate, only entries to append.

### Truth versus drive cache, at every level

Exactly two execution-intent truths exist: Mission plus its Log (host-level
durable intent and judgment), and Team-scoped Work (member-level current
responsibility). Company domain records such as Approval, Finance and Document
remain their own business truths but do not create a parallel Work lifecycle. Provider-native goal mode
and provider-native plan mode are derived drive caches, never truth. On Host
spawn, resume, or post-compaction re-entry, the Host's native goal text MUST
be derived from `mission show` plus the latest Log entries plus the board
summary; hand-authoring a second, independent intent into the
provider-native goal or plan is prohibited. This generalizes, at the Host
level, the rule ADR 0037 already states for Members — "Member Goal is a
Dashboard projection, not a new stored object. It is derived from the
current Work" — and the same no-double-drive discipline the Skill already
enforces against running a native Goal loop and ordinary Harness cycles on
the same Work.

### The Log gets mandatory readers

Wave's failure was never that it existed; it was that nothing was ever
required to read it before acting. The retired Goal/GoalPhase stack had
this mechanism, and the Mission Log imports it explicitly:

- the recovery entrypoint (issue #304) prints the current Mission Log tail
  before any mutation;
- session re-entry injection (the issue #306 mechanism) includes Mission
  intent, the latest Log revision, and the board summary; and
- new-Host takeover contract: a replacement Host must be able to resume
  correct scheduling from three reads or fewer (`mission show`, Log tail,
  board summary) totaling 4,000 characters or fewer.

### Log-before-act discipline

At material decision points — a new Work tranche, a composition change,
recovery, or a model/provider switch — the Host appends the judgment entry
to the Mission Log before mutating runs or Works, not as after-the-fact
narration. This directly targets the ordering failure the audit found
twice: the Wave memo was written after the recovery decision, at both
crises.

### Goal-stack removal completes

Per ADR 0028, the CLI `goal *` command tree, its store surfaces, and the
star-goal Skill copies are removal debt, not an active product surface;
they are deleted in a dedicated follow-up PR, not this one. `wave gate`
retires immediately with the Wave-to-Log fold: it has had zero invocations
since 2026-07-31, and the Log has no analogous gate to replace it with —
only entries.

## Consequences

- The Host's operating history becomes one append-only stream instead of
  four independently drifting copies; the recovery entrypoint, session
  re-entry injection, and any replacement Host all read the same Log.
- Issue #271 closes by dissolution: the rejected-Wave-advance question no
  longer has an object to be about.
- `wave gate`, `wave advance`, and per-Wave terminal-decision bookkeeping
  retire with no replacement mechanism, because an append-only log has
  nothing analogous to a gate.
- Mission closeout evidence becomes a `kind = closeout-evidence` Log entry
  instead of a separate Wave-outcome convention.
- The Member-facing contract is unchanged: Members still never see Mission,
  Wave, or the Log, by design.
- Provider-native goal/plan state becomes explicitly disposable:
  discarding and regenerating it on every Host re-entry is correct, because
  it was never truth.
- `docs/product/mission-wave-host-plan.md`, Dashboard Wave panels, and any
  store code that reads Wave for a decision become cutover debt tracked by
  the dedicated MissionLog PR named in Implementation Boundary.
- Goal-stack deletion (CLI `goal *`, store surfaces, star-goal Skill) is
  scheduled as its own PR under ADR 0028's existing removal policy, not
  bundled into the MissionLog cutover.

## Rejected Alternatives

### Keep Wave but wire mandatory readers onto it

Rejected. Two ledger objects — Mission context and Wave judgment — would
still record one judgment stream, and the change keeps the #271-class
terminal-decision lifecycle burden (advance, gate, rejected-vs-accepted)
that an append-only log does not need in the first place.

### Make the provider-native goal the truth

Rejected. It is unreadable by Members, Dashboard, and any replacement Host,
and the host session is the least durable store in the system — 18
`session_meta` restarts and 4 compactions in one evening. Anchoring truth
there would recreate the write-only-narration failure one layer down, not
fix it.

### Resurrect the retired Goal object

Rejected under ADR 0028's active removal policy. Mission already owns
durable intent, team linkage, and closeout; reviving a Goal object would
duplicate that ownership instead of fixing Wave's missing-reader problem.

## Implementation Boundary

This ADR is operational immediately for documentation, Skills, and Host
behavior rules: the Host Scheduling Policy addition to
`orchestrate-mission-waves` (both copies) and the truth-versus-drive-cache
rule above apply as soon as this ADR merges.

MissionLog storage, schema, CLI surface, and Wave command retirement land in
a dedicated cutover PR, under the same breaking-cutover discipline ADR 0050
used for Assignment Messages: no dual-read, no dual-write, no silent
inference path. Until that PR lands:

- Wave writes remain functional but deprecated;
- no new code may read Wave for a scheduling or recovery decision; and
- the recovery entrypoint (#304) and re-entry injection (#306) land against
  whichever intent surface is current at merge time, and are re-pointed at
  the Mission Log by the cutover PR if they ship first.

Goal-stack deletion (CLI `goal *`, store surfaces, star-goal Skill copies)
is its own PR under ADR 0028's existing removal policy and is not gated on
the MissionLog cutover landing first.

**Cutover status:** the MissionLog cutover PR (#318, branch
`feat/mission-log-cutover`, "Mission Log absorbs Wave — append-only judgment
log, wave-write retirement, recover reads log") landed the storage, CLI/HTTP/
MCP surface, and Wave write retirement described above; the deprecated-but-
functional Wave write window this section describes is now closed.
