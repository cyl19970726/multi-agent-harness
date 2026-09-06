# Agent Team Dogfood Loop

```text
status: canonical
owner_role: execution-foundation
canonical_for: implementation-bound remainder of the dogfood loop (baseline gates, provider modes, delivery receipts)
architecture: ADR 0031 + ADR 0032 + ADR 0037 + ADR 0039 + ADR 0041 + ADR 0044
```

## Authority

Product doctrine for this topic — the dogfood method, defect-to-repair loop,
evidence bundle shape, and exit criteria — is canonical in Notion; see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location. This repository file survives only as the
implementation-bound remainder below.

## Implementation-bound invariants

### Name the claim before running it

Every live scenario declares exactly one class before launch:

- `coordination_canary` proves one bounded coordination, authority, delivery,
  or lifecycle claim. A read-only SHA check, echo, no-edit task, or provider
  receipt can be valid here, but the completion report must name the focused
  claim and its limitations. It is never evidence that an Agent Team can
  perform repository development.
- `coding_dogfood` proves real coding delivery. It requires a changed candidate
  revision, at least one changed file and one real check, a canonical Member
  WorkReport, independent review by an AgentMember other than the implementer,
  exact Host acceptance, and provider-native evidence containing both a tool
  start and terminal tool result for the implementer.

Validate the response-local evidence bundle and its canonical coordination
records before claiming coding dogfood:

```bash
pnpm verify:agent-team-dogfood -- /path/to/evidence.json \
  --trust-ledger /path/to/agentfirm_trust_operations.jsonl \
  --expected-execution-space-id <trusted-execution-space-id>
```

For new managed/external Host attribution use
`agentfirm.agent_team_dogfood_evidence.v2` and
`schemas/agent-team-dogfood/evidence.v2.schema.json`. Keep the same explicit
Space and ledger arguments; `--harness-bin /absolute/path/to/firm` selects the
trusted operator-installed binary when it is not named `harness` on PATH.
The verifier calls `space show <id>`, checks that exact ID, and derives the
canonical ledger, `team_runs.jsonl`, and `host_binding_leases.jsonl` paths from
the returned Store root. An evidence file cannot select a replacement source
or provide a native discovery receipt.

The v2 `host` branch declares `managed` plus `team_membership_id`, or
`external_interactive` plus that membership, `surface`, `thread_id`, `lease_id`,
`owner_id`, and `generation`. Every managed Session row adds
`session_generation`. `work` adds `work_execution_binding_id` and `delivery_id`.
These are claims checked against the selected Space records, not new authority
objects. The exact implementer delivery and its released binding in the Result
submission establish the execution interval. All native bindings in that Session
generation up to its submission boundary are checked before comparing native
IDs; consistent duplicates and locator metadata changes need no reattach event.
A canonical successor after the interval leaves historical attribution intact.
Missing binding history or an uncorrelated Result is an evidence gap, never an
invitation to synthesize old records.

One existing historical path remains an explicit evidence gap ([#879](https://github.com/cyl19970726/multi-agent-harness/issues/879)): submitting a
Result after a formal Close/Reopen can use an already Released predecessor
binding. The Store does not persist the selected binding in that Report
operation again; Work/version/author and a HostAttention MemberRun ID do not
identify its Session generation. V2 therefore refuses that uncorrelated Report,
including when a reader sees only one apparent candidate. It neither chooses
the latest predecessor nor repeats runtime recovery admission. This limitation
is distinct from a normal Report followed by a later canonical generation,
which remains verifiable. Preserve the old records; produce a new normally
correlated delivery if complete fresh evidence is required.

An independent managed reviewer must author its existing canonical Pass Message
through `member message send|reply`; its `sender_session_id` must name the
claimed reviewer Session. A managed Host also needs a genuine Session at the
acceptance boundary. Review and Host acceptance are checked separately even
when one Member holds both roles: a later acceptance generation cannot be
credited with an earlier Review. Session claims are unique by `(agent_session_id, session_generation)`, so one
Member may provide both its Review generation and its later acceptance
generation. The verifier derives each role from canonical facts before
selecting its exact claim; a shared exact tuple may serve two roles. Missing,
duplicate or conflicting tuples refuse. Only the exact WorkExecutionBinding
generation supplies the implementer tool-count checks. Managed Host selection comes from the
historical Session's TeamSupervisor TeamRun association at acceptance, before
comparing the claimed Session/native ID; its node and Space must match the
TeamRun. Missing or ambiguous associations are evidence gaps. These are
historical identity checks, not live lease/recovery admission revalidation.
An external Host has no fabricated Session: v2 checks its
historical TeamRun, Host membership and lease interval, then invokes the read-only
`team-run validate-host-session --surface codex --thread-id <id>` bridge. This
reuses canonical `<HOME>/.codex` metadata discovery, ignoring `CODEX_HOME`, and
never binds a Host or writes a lease. Discovery proves same-user metadata
existence, not an active window or cross-user authentication. A subsequent
Released lease row does not erase a prior recorded valid interval; a future
renewal cannot repair expiry at acceptance. Unsupported old Claude discovery
cannot be retrospectively replaced with a Codex receipt.

**A v2 success proves structure and trusted attribution only.** Counts remain
necessary claims; they do not prove native tool execution. Before claiming full
coding dogfood, separately inspect the real implementer and independent
reviewer's provider-native records, including tool start/terminal evidence,
actual checks and exact revision. The verifier never imports native transcript
content into Harness or the evidence bundle. Deterministic fixtures do not
constitute a live dogfood run.

The following describes the preserved v1 contract. Old v1 files keep their
original schema, checks and failure history; they are not silently upgraded.

The evidence schema is
`schemas/agent-team-dogfood/evidence.schema.json`; canonical ledger examples and
adversarial cases are indexed by
`schemas/agent-team-dogfood/fixtures/canonical-ledger/manifest.json`. For
`coding_dogfood`, both options are required. `--trust-ledger` identifies the
current Execution Space's canonical `agentfirm_trust_operations.jsonl`, while
`--expected-execution-space-id` must come from the trusted Execution Space
selection that located the ledger, never from the ledger contents or path. The
verifier fails closed unless it finds exactly one matching WorkReport,
independent Pass review Message, Host acceptance event, and native-session
binding for each evidenced AgentSession. Those records must agree on the
evidence bundle's exact Work, Work version, candidate revision, AgentTeam,
AgentMember identities, TeamRun, provider, AgentSession, and native-session
ids. Missing, malformed, foreign, wrong, or ambiguous records fail
verification.

The ledger is append-only coordination evidence, so unrelated complete rows do
not invalidate an exact match. Read only complete newline-terminated frames:
ignore an unterminated trailing fragment left by an append crash, but fail
closed on malformed complete frames. Skip whitespace-only rows, never read a
sibling `.next` file, and never fold records from another
`execution_space_id` into the match. The ledger and evidence bundle carry only
ids, counts, digests, and `NativeSessionRef` pointers. Provider conversation
content remains solely in the provider-native store; a transcript mirror is a
contract violation, not additional proof.

The verifier also rejects a no-edit candidate, a same-revision candidate, an
implementer Session without terminal tool evidence, or changed files that do
not exactly match Git's base-to-candidate diff.

Passing a coordination canary can close only its focused claim. A Task or
report may say `coding_dogfood` or “full Agent Team dogfood” only after the
coding evidence verifier passes against the exact candidate and durable Team
records.

- Known-baseline gates before a dogfood run:

  ```bash
  firm member providers --fail-on-review
  npx pnpm@9.15.4 acceptance:legacy-retirement
  node scripts/check-cross-layer-consistency.mjs
  firm governance check
  ```

- Persistent Team execution modes: Codex → `codex_app_server`, Claude →
  `claude_agent_sdk`, Kimi → `kimi_acp`. Bounded `codex_exec`/`claude_cli`
  describe retired Dynamic Workflow records only, never current Team members.
- Provider delivery/terminal-state receipts differ by adapter: Codex's
  `turn/start` response is the WorkDelivery provider receipt (persist it
  before `turn/completed` to avoid a crash window that can execute the same
  writable Work twice); Claude uses the Agent SDK delivery receipt; Kimi ACP
  has no separate prompt-start receipt, so the first update, provider
  request, or terminal response for that prompt is the earliest honest
  runtime receipt and must publish before that turn's Member-to-Host or peer
  communication.
- For every driven mode, inspect the settled `StartCycle` RuntimeCommand and
  require one exact `cycle_correlation`: the accepted provider input, terminal
  provider input (where the native protocol exposes one), native session,
  AgentSession generation, and transport attempt must agree. Run two rounds
  and an interrupt/follow-up boundary to prove an old terminal cannot complete
  a new input. Loss after the input receipt is `Unknown`/reconciliation and
  must never be automatically replayed. Empty output is not semantic success.
