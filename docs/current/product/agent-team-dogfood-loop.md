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
  --trust-ledger /path/to/agentfirm_trust_operations.jsonl
```

The evidence schema is
`schemas/agent-team-dogfood/evidence.schema.json`; canonical ledger examples and
adversarial cases are indexed by
`schemas/agent-team-dogfood/fixtures/canonical-ledger/manifest.json`. For
`coding_dogfood`, `--trust-ledger` is required and identifies the current
Execution Space's canonical `agentfirm_trust_operations.jsonl`. The verifier
fails closed unless it finds exactly one matching WorkReport, independent Pass
review Message, Host acceptance event, and native-session binding for each
evidenced AgentSession. Those records must agree on the evidence bundle's exact
Work, Work version, candidate revision, AgentMember identities, TeamRun,
provider, AgentSession, and native-session ids. Missing, malformed, foreign,
wrong, or ambiguous records fail verification.

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
  npx pnpm@9.15.4 check:star-harness-plugin
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
