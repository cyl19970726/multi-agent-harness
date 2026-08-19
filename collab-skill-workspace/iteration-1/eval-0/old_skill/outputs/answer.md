# Host Playbook: "add `--json` to `harness export`, with a test"

Role: Host of AgentTeam `builders`, `team_run_id = tr-123`. Roster: member `kiwi`
(`member_run_id = mr-k1`, Kimi ACP, currently idle).

## 0. A note on the skill I actually have

The skill loaded (`collaborate-as-agent-team-member`) is explicitly the **Member**
skill — its own header says "Do not use for Host orchestration." It does not
document a Host-side `work accept` / `work assign` verb table. What it does give
me, and what I am building this sequence from:

- The exact **Work object model** ("What / Mental Model / Workspace / Boundary /
  Gates / Evidence" + a completion checklist) that a Host must write when
  creating Work.
- The **CLI verb family** (`team-run work create`, `team-run work show`,
  `team-run work list`, `team-run board-summary`) shown from the Member side,
  which I use unchanged from the Host side.
- The **shared hard invariants** it repeatedly cites (§1 no assignment via
  chat/Message, §4 Message never changes Work state, §5 only Host acceptance
  moves Work to `done`, §9 ownership survives process exit) — these pin down
  what the missing Host verbs (`work assign`, `work accept`,
  `work request-changes`, `send-message`) must do even though the skill
  doesn't spell their flags out for a Host caller.
- The Member-side submission contract (Rule Zero: merged PR + green CI is the
  only "done"; `--artifact-ref` / `--check-ref` are not decoration) — which
  tells me exactly what I must inspect before I, as Host, accept.

Where the skill is silent on a Host-only verb, I use the same
`team-run work <noun-verb>` / `--team-run-id` / `--work-id` /
`--expected-version` / `--idempotency-key` shape it uses everywhere else,
because that is the one CLI grammar the skill/context actually documents.

---

## 1. Orient before decomposing

```bash
"$HARNESS_BIN" team-run status --id tr-123
"$HARNESS_BIN" team-run board-summary --id tr-123
"$HARNESS_BIN" team-run work list --team-run-id tr-123 --brief
```

Check before doing anything else:
- kiwi's board-summary line reads `kiwi: idle` (confirms the roster fact I was
  told, don't just trust the prompt).
- no existing open/in_progress Work already covers "export --json" (avoid
  creating a duplicate).

## 2. Decomposition

This is a single-crate, single-flag change with one test — it does **not**
need fan-out into multiple Work items or multiple members. Splitting a task
this small across Work items would only buy coordination overhead (two Works
touching the same file, sequencing risk) for no parallelism benefit. One Work,
owned end-to-end by kiwi, is the right granularity.

I write the Work using the skill's own mental-model shape:

```
What:        harness export currently only prints human-readable text.
             Add a --json flag that prints machine-parseable JSON instead.
Mental Model: existing precedent in this codebase: `harness governance check`
             already has this exact pattern (--json flag + a
             print_json/print_<x>_report(..., json: bool) branch) — mirror it,
             don't invent a new convention.
Workspace:   kiwi's own worktree, outside the main checkout.
Boundary:    crates/firm-cli/src/** and crates/firm-cli/tests/** only.
             Do NOT touch other crates or unrelated CLI subcommands.
Gates:       artifact-exists (PR URL) + check-pass (named test, see below).
Evidence:    a merged-ready PR + a named passing test command; default
             (non---json) output byte-for-byte unchanged.
```

Completion criteria must name **observable evidence**, not "add the flag":

```
1. `harness export --json` prints valid JSON to stdout on success; the
   existing non-flag path is byte-for-byte unchanged (regression-safe).
2. A new test asserts this behavior — e.g.
   `cargo test -p firm-cli --test export_json` — and that test actually
   parses the JSON (serde_json::from_str) and asserts on real fields, not
   just that the flag parses.
3. A PR is opened against master containing the diff, scoped to
   crates/firm-cli/src/** and crates/firm-cli/tests/**.
4. Named CI check green: `cargo test -p firm-cli --test export_json` (or the
   project's CI job that runs it) passes on the PR's head commit.
```

## 3. Create the Work — host-assigned directly to kiwi

This single call is the entire assignment act:

```bash
"$HARNESS_BIN" team-run work create \
  --team-run-id tr-123 \
  --id w-201 \
  --title "harness export: add --json output flag" \
  --context "$(cat <<'CTX'
Add a --json flag to the `harness export` subcommand. Precedent to mirror:
`harness governance check` already has --json (crates/firm-cli/src/main.rs,
governance_command) using a bool `json` flag + print_*(..., json) branch.
Follow that same shape for export instead of inventing a new one.
CTX
)" \
  --completion-criteria "$(cat <<'CRIT'
1. `harness export --json` prints valid JSON on success; non-flag output is
   unchanged (regression-safe).
2. New test `cargo test -p firm-cli --test export_json` exists, parses the
   JSON output with serde_json, and asserts real fields (not just "flag
   parses").
3. PR opened against master, diff scoped to crates/firm-cli/src/** and
   crates/firm-cli/tests/** only.
4. Named CI check `cargo test -p firm-cli --test export_json` green on the
   PR head commit.
CRIT
)" \
  --claim-mode host_assign \
  --owner-member-run-id mr-k1 \
  --idempotency-key "create-export-json-flag-1"
```

Check the response: `work_id = w-201`, `version = 1`,
`owner_member_run_id = mr-k1`, `claim_mode = host_assign`.

**Trap avoided:** I do *not* also send kiwi a chat message saying "this one's
yours" — `--claim-mode host_assign --owner-member-run-id mr-k1` on
`work create` is the *only* act that establishes ownership. A Message never
changes Work state (shared invariant §4), so treating a Message as an
assignment vector — the exact "consider it yours" pattern the skill warns
against on the Member side — would be just as wrong coming from the Host.

## 4. Optional: send a pointer, explicitly informational

Host-authored `message` mail defaults to requiring a response round (it comes
from the coordination plane, not a peer). For a pure FYI I do not want to
force an interrupt, so I set the intent explicitly:

```bash
"$HARNESS_BIN" team-run send-message \
  --team-run-id tr-123 \
  --work-id w-201 \
  --to mr-k1 \
  --kind message \
  --response-intent informational \
  --body "Pointer: crates/firm-cli/src/main.rs governance_command already has a --json flag + print_json/print_governance_report(report, json) pattern — mirror that for export instead of a new convention."
```

This is a hint, not an assignment (already done in step 3) and not a
completion-criteria change (already recorded on the Work itself).

## 5. Wait — event-driven, not a sleep loop

What I am actually waiting on: kiwi's ProviderWorkDispatch wakes its runtime,
kiwi runs its own `member work start --work-id w-201 --expected-version 1 ...`,
then either (a) sends a plan Message first (this Work is small/single-file, so
plan-first isn't mandatory, but I watch for it) or (b) goes straight to
implementation.

I do not poll in a tight loop. I check state at safe checkpoints — when
notified, or at coarse human-driven intervals — using cheap delta reads:

```bash
"$HARNESS_BIN" team-run board-summary --id tr-123
"$HARNESS_BIN" team-run work list --team-run-id tr-123 --since <last-cursor>
"$HARNESS_BIN" team-run events --team-run-id tr-123 --after-seq <last-seen-seq>
```

`board-summary` tells me kiwi's line flipped `idle → working`; `--since` /
`--after-seq` give me only what changed since the last check instead of
re-reading the whole board. If kiwi sends a plan Message, I read it via
`team-run work show --work-id w-201 --team-run-id tr-123 --json` (Messages
linked to a `work_id` show up there) and reply once, preserving correlation:

```bash
"$HARNESS_BIN" team-run send-message \
  --team-run-id tr-123 \
  --work-id w-201 \
  --to mr-k1 \
  --kind message \
  --response-intent informational \
  --causation-id <kiwi_plan_message_id> \
  --body "Plan approved, proceed. One addition: emit [] not blank stdout when export has zero records, so `--json` output is always parseable."
```

If instead kiwi calls `member work block` (real blocker, not silence), the
Work's status will read `blocked` with a reason in `work show`. I resolve the
blocker via a Message, then:

```bash
"$HARNESS_BIN" team-run work resume \
  --team-run-id tr-123 \
  --work-id w-201 \
  --expected-version <latest> \
  --resolution "<how the blocker was resolved>" \
  --idempotency-key "resume-w201-1"
```

I never treat a tool status of "completed" on kiwi's side as done — only a
Work in `review` with a submitted result_summary counts.

## 6. Work reaches `review` — inspect before accepting

```bash
"$HARNESS_BIN" team-run work show --work-id w-201 --team-run-id tr-123 --json
```

Checklist, line by line, before I accept anything:

1. `result_summary` follows the RESULT/SUMMARY/COVERAGE/KEY DECISIONS/
   WORKTREE/ARTIFACTS template and `RESULT` literally says `done` (not
   `blocked`/`failed`).
2. `artifact_refs` contains a real PR URL. I open it:
   `gh pr view <url> --json state,baseRefName,statusCheckRollup,files` — base
   is `master`, files touched are only inside `crates/firm-cli/src/**` and
   `crates/firm-cli/tests/**` (Boundary honored), the diff actually wires
   `--json` into the export subcommand's arg parser and its print path.
3. `check_refs` contains the named check
   (`cargo test -p firm-cli --test export_json`). I confirm it is *actually*
   green on the PR head commit via `gh pr checks <url>` / the CI run linked in
   the check ref — I do not accept on kiwi's say-so that it passed.
4. The new test genuinely exercises the flag: it parses stdout with
   `serde_json::from_str` and asserts on real fields, not merely that the
   process exits 0.
5. Default (non-`--json`) output path is unchanged — either an existing test
   still covers it, or I read the diff to confirm that branch wasn't touched.
6. `candidate_revision` matches the PR branch's head commit (no drift between
   what was reviewed and what's referenced).
7. No stray formatting churn / unrelated files in the diff.

### If anything fails: request changes, same member/session

```bash
"$HARNESS_BIN" team-run work request-changes \
  --team-run-id tr-123 \
  --work-id w-201 \
  --expected-version <latest> \
  --reason "check_ref cites 'cargo test -p firm-cli --test export_json' but the linked CI run <url> shows it red on <job>; also the diff touches crates/firm-cli/src/legacy_export.rs which is outside this Work's Boundary — please scope to the export --json flag/test only." \
  --idempotency-key "request-changes-w201-1"
```

This delivers a new WorkDelivery to the *same* stable AgentMember / MemberRun
/ native session — kiwi is not reassigned, does not re-claim, and its provider
thread continues where it left off. I go back to step 5 and wait the same way
for resubmission.

### If everything checks out: accept

```bash
"$HARNESS_BIN" team-run work accept \
  --team-run-id tr-123 \
  --work-id w-201 \
  --expected-version <latest> \
  --idempotency-key "accept-w201-1"
```

(The skill doesn't spell this verb out — it's written for Members — but its
own shared invariant §5, "only Host acceptance moves Work to done," requires
this act to exist; I keep it in the same `team-run work <verb>` /
`--expected-version` / `--idempotency-key` grammar the skill uses for every
other Work mutation.)

## 7. Confirm and close the loop

```bash
"$HARNESS_BIN" team-run work show --work-id w-201 --team-run-id tr-123 --json
"$HARNESS_BIN" team-run board-summary --id tr-123
```

Check: Work `w-201` status is `done`, `expected-version` I used matches what
was actually applied (no silent CAS mismatch), kiwi's board line returns to
`idle`. I then report to the user: PR URL, the named check that went green,
and that default `harness export` output is unchanged.

---

## Traps deliberately avoided (mapping back to the skill's invariants)

- **No assignment via chat.** Ownership is established once, via
  `work create --claim-mode host_assign --owner-member-run-id mr-k1`; every
  Message afterward is conversation only (§4), never a re-assignment vector.
- **CAS discipline throughout.** Every mutating call
  (`request-changes`/`resume`/`accept`) carries the Work's current
  `--expected-version`; on a version mismatch I re-`work show` and retry with
  the fresh version rather than guessing.
- **No sleep-loop polling.** I watch via `board-summary` (≤500-char digest)
  and delta cursors (`--since` / `--after-seq`), at safe checkpoints, not a
  busy loop.
- **Provider "completed" ≠ done.** Only a Work that reaches `review` with a
  submitted result_summary and required artifact/check refs is treated as
  finished work; I inspect those refs line by line, independently re-checking
  CI status rather than trusting the claim.
- **`request-changes` keeps the same member/session** — it's a new delivery
  to the existing MemberRun, not a reassignment or a new claim.
- **Only Host acceptance moves Work to `done`** — submission (`review`) is
  never treated as completion by itself.
