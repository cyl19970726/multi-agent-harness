# Host Operating Loop

Read [SKILL.md](../SKILL.md) Part 0 and Part I first — this reference assumes
the shared mental model and only sequences the Host-side procedure. The Host is
the exact `AgentMember` named by `AgentTeam.host_agent_id`, acting through its
one active Host-role TeamMembership. It never sees Member thinking, tool
output, or private session detail beyond the `host_member_public` scope.

## 0. Know which Host you are

| Mode | How you run | What wakes you | What you must do yourself |
| --- | --- | --- | --- |
| `managed` (default for automated Teams) | one MemberRun → AgentSession under the NodeDaemon, same path as every Member | response-required deliveries, blocked/submitted Work, recovery attentions start your next cycle | nothing special — the Supervisor batches ordinary progress into your next cycle |
| `external_interactive` (a Claude Code / Codex / Kimi window bound with `--host-surface … --host-thread-id …`) | your own interactive session; Harness creates no AgentSession, receipt, or timely wake | the plugin hook pushes queued Host mail at SessionStart / UserPromptSubmit / Stop | inside a long turn, block on `team-run wait`; read `host-inbox`; answer with `answer-message` |

Mode changes are explicit Close/Reopen operations with generation fencing;
nothing falls back silently between them.

## 1. Compose the Team before starting anything

Decide, explicitly and durably, before `team-run start`:

- **Roster**: which AgentMembers, which provider mode each runs. The five
  executable Team modes are `codex_app_server`, `claude_agent_sdk`,
  `kimi_acp`, `pi_rpc`, and `deepseek_sdk`; bounded one-shot modes cannot be
  Team members. Every managed coding Host/Member starts with the
  trusted-development `full_access` ceiling; ceilings are frozen at
  AgentSession start and a provider asking for more mid-run fails closed.
- **Disjoint owned paths / worktrees** per member. Two members editing one
  workspace is the single most common way real runs destroy work: uncommitted
  state has no protection. Use `--member-worktree name:path` and
  `--member-owned-path name:path` at create time; a shared cwd is allowed only
  when the Works never touch the same files.
- **Completion standards**: what evidence each kind of Work must carry (PR
  URL, named check command with exit code, artifact path). If you cannot name
  the evidence, the Work is not bounded yet.

Create the durable Team once; it persists across runs:

```bash
firm team create --name <team> --description "<why this Team exists>" \
  --host-agent-id <agent-member-id> --node-id <node> --member <agent-member-id> ...
```

Create a TeamRun per engagement. The Project Binding is frozen on the run and
the returned execution/member roots are the members' worlds:

```bash
firm team-run create --agent-team-id <team-id> --objective "<one-sentence run objective>" \
  --member <name>:<provider>:<execution_mode> ... \
  [--member-worktree <name>:<path>] [--member-owned-path <name>:<path>] \
  [--resume-member <name>:<native-session-id>] \
  [--no-initial-work] \
  [--host-surface <surface> --host-thread-id <id>]   # external_interactive Host only
firm team-run start --id <team-run-id>
```

`--no-initial-work` (#728) creates the MemberRuns without the per-member
bootstrap Work so your explicit assignment can follow. Assign Works **after**
`team-run start`: a Work bound before the first provider Open is frozen against
runtime facts the member's first round invalidates. `team-run add-member --id
<run> --member <name>:<provider>:<mode> [--initial-work "<brief>"]` joins a
member to a running run and provisions its AgentSession (#749).

## 2. Decompose into bounded Works

Write each Work so a Member can execute it without asking what "done" means:

- **What** — the problem in one sentence.
- **Mental model** — states, invariants, data flow the member must hold.
- **Boundary** — paths to touch and paths never to touch.
- **Gates/Evidence** — what the reviewer will check, verbatim.

Choose the claim mode deliberately: `host_assign` followed by canonical
`work assign --membership-id` when one stable TeamMembership must own it;
`team_claim` when any eligible member may atomically claim. Prefer several
bounded Works over one epic — TeamRun completion atomically rejects
non-terminal Works, so unbounded Works block teardown.

Works are flat peer nodes. Add a hard dependency only when the successor truly
cannot execute before the prerequisite is accepted:

```bash
firm team-run work create ... --prerequisite-work-id <work-id>      # at creation
firm team-run work replace-dependencies --team-id <accountable-team-id> \
  --work-id <work-id> --expected-version <n> --prerequisite-work-id <work-id> ... \
  --idempotency-key <stable-command-key>                             # Host-only edit
```

Fan-out independent lanes; fan-in integration/review after all required
inputs. Never encode decomposition as Work containment. The kernel rejects
self-edges, duplicates, missing prerequisites, stale revisions, and cycles.
Treat failed or cancelled prerequisites as Host replan attention, not automatic
downstream resolution. A Member's dependency proposal arrives as a Message;
only your `replace-dependencies` changes the graph.

## 3. Start, then wait without polling

`team-run start` reserves the run and returns immediately; members run in the
background under the machine NodeDaemon's Supervisor generation. Give the
user the returned dashboard URL at once.

The run has a sequenced event stream and a cursor-based board. **Block on the
stream; never sleep-and-status:**

```bash
# 1. wait for the next event(s); default timeout 600 s, poll interval 500 ms
firm team-run wait --id <team-run-id> --after-seq <last-seq> --timeout-secs 600 --json
#    → { timed_out, after_seq, next_after_seq, events[] }
# 2. read only what changed
firm team-run work list --team-run-id <team-run-id> --since <next_since>
firm team-run board-summary --id <team-run-id>
# 3. external_interactive Host: read your mail explicitly
firm team-run host-inbox --surface <surface> --thread-id <id> --json
# 4. non-blocking replay of the stream when you need history
firm team-run events --id <team-run-id> --after-seq <seq>
```

Rules:

- Chain cursors: pass `next_after_seq` back to `--after-seq`, and the JSON
  `list` response's `next_since` back to `--since`. Omitting `--after-seq`
  means "wait for what happens next", not "replay this run's history".
- A `timed_out` return is information ("nothing happened for 10 minutes"),
  not an error; decide whether to keep waiting, message the member, or
  interrupt.
- `board-summary` is a ≤500-character digest: `open= active= blocked= review=
  accepted= cancelled=`, `assigned= unassigned= ready=`, one
  `idle|working|awaiting-review` line per active member, and the supervisor
  generation/heartbeat line. Use it to decide, not to wait.
- A background watcher is acceptable only when its body is `wait` (or a
  bounded chain of `wait` → `work list --since`); a watcher whose body is
  `sleep N` + `status` is the polling anti-pattern moved out of sight.
- If `wait` cannot express what you need (a Work-scoped condition, a delivery
  state, a member-scoped filter), **file the gap as a repository Issue before
  scripting around it**. The bypass is a product finding, not a private
  convenience.

## 4. Converse with exact correlation

Your inbox is a projection of per-recipient deliveries addressed to the Host.
A managed Host reads it at safe boundaries inside its cycle; an
external_interactive Host reads `host-inbox`. Mail from members is durable
immediately but does not interrupt your current reasoning.

- A member's decision-shaped question arrives as a correlated Message with
  exact ids. Answer **on the same correlation**; a fresh uncorrelated reply
  strands the member's pause. A provider-native question
  (`ProviderInteractionRequest`) is answered with:

  ```bash
  firm team-run answer-message --id <team-run-id> --message-id <message-id> \
    (--option-id <exact-option-id> | --response-text "<text>")
  ```

- To author ordinary mail as an external_interactive Host:

  ```bash
  firm team-run message send --team-run-id <run> --to-membership <membership-id> \
    --body "<markdown>" --surface <surface> --thread-id <id> \
    [--work-id <id>] [--response-required] [--idempotency-key <key>]
  ```

  A managed Host uses the same `member message send|reply|request-decision`
  Role Actions as any member.
- Use `informational` intent for anything that does not need a member
  provider round; `response-required` mail is what wakes an idle managed
  member cycle.
- Steer changes a member's **current** turn only when the provider
  acknowledges it; a queued Message affects the **next** safe boundary.
  `team-run interrupt-member --id <run> --member-run-id <id> --reason <text>`
  stops one turn without closing the member.
- Never order work in chat. If conversation produces durable follow-up,
  create a peer Work and, when ordering is real, mutate the dependency graph
  through `replace-dependencies`.

## 5. Review evidence, then decide

Submission moves Work to `Review` — that is a request for judgment, not a
result. Review means: open the artifact refs (the PR diff, the file), rerun
or read the named check refs, and walk the completion criteria line by line.

Before trusting a submission's report at all, apply the submission report
contract (SKILL.md):

- **Verify the SHA, never trust it.** `git cat-file -t <reported-sha>` must
  answer `commit` and the object must equal the submitted candidate revision
  — a report once carried a SHA whose tail did not exist (#787).
- **Review the commit, not the report.** The report is a pointer; the diff
  is the object under review.
- **A missing Verbatim evidence section is Changes Required on its face.** If
  the `--result-summary` does not start with the exact SHA, the three-dot
  `git diff --stat <base>...<sha>`, the literal `git status --porcelain`
  output, and every named gate's command with verbatim result line(s) and
  exit code, do not reconstruct the evidence yourself — request changes and
  name the missing section.

```bash
firm team-run work show --work-id <work-id> --json        # report, artifact/check refs, deliveries
firm team-run work accept --work-id <work-id> --expected-version <n>      # → Closed / Accepted
firm team-run work request-changes --work-id <work-id> --expected-version <n> \
  --reason "<what and why>"                                               # → Open
```

- **Accept** → Work closes with resolution `Accepted`. Declared Work gates are
  a Store invariant; there is no bypass flag, so unmet gates mean
  request-changes, not a workaround.
- **Request changes** → Work returns Review → Open with your reasons recorded
  in WorkEvent history. Stable AgentMember/TeamMembership responsibility
  remains; the scheduler must create the next exact `WorkExecutionBinding`
  and delivery generation before Start. Reuse a compatible Workspace/native
  session when its runtime fences pass; do not treat MemberRun continuity as
  ownership.
- Never accept on a provider completion status, a delivery receipt, or a
  green fixture alone.

Ordinary Member-owned Work is accepted by you. For Work owned by the Host you
must not self-accept: submit the Host Work, send one response-required
Work-linked Message to an exact active non-owner peer in the same TeamRun, and
wait for that peer's explicit `firm member work accept`. If the peer finds a
problem, it reports the requested revision through the linked conversation;
revise and resubmit the same Work. A solo Host leaves its Work in `Review`
until an exact active peer is available; it must never fabricate acceptance
through a generic Human/Service control-plane credential.

Delivery repair belongs to you as well:

```bash
# an Open, never-started Work frozen on a member generation that no longer runs
firm team-run work redeliver --work-id <work-id> --expected-version <n> [--reason <text>]
# release a binding you are abandoning; move a Work to a successor run
firm team-run work release --work-id <work-id> --expected-version <n>
firm team-run work retarget --work-id <work-id> --expected-version <n> --successor-team-run-id <run>
```

`redeliver` refuses honestly: `WORK_TERMINAL_NOT_REDELIVERABLE`,
`WORK_ALREADY_STARTED`, `WORK_NOT_ASSIGNED`, `WORK_HAS_NO_UNSTARTED_DELIVERY`,
`WORK_DELIVERY_LIVE`, `EXECUTION_SPACE_SCOPE_MISMATCH`. A `Claimed` delivery
that never settled is uncertain: reconcile it, never replay it.

Cross-Team needs are an explicit `WorkDelegation` from a source Work you own
to a target Work in the other flat Team; target completion never
auto-completes your source Work.

## 6. Recover instead of restarting

A dead member runtime is not lost work. The lifecycle controls:

```bash
firm team-run interrupt-member --id <run> --member-run-id <id> --reason <text>   # stop one turn
firm team-run close-member     --id <run> --member-run-id <id> --reason <text>   # release runtime, keep session
firm team-run reopen-member    --id <run> --member-run-id <id>                   # resume exact native session, new generation
firm team-run deactivate-member --id <run> --member-run-id <id> --reason <text>  # permanent; reassign/cancel Work first
firm team-run recover --id <run>                                                 # claim/receipt diagnostics after a crash
```

- `close-member` releases the provider runtime, retains the MemberRun and its
  native session binding; mail queued for it is frozen.
- `reopen-member` resumes the **exact native session** under a new runtime
  generation after delivery reconciliation — the member returns with its
  memory intact. Prefer this over any fresh spawn. A Work that was delivered
  but never started before the Close needs `work redeliver` afterwards.
- `deactivate-member` retires the coordination identity permanently;
  unfinished Work must be reassigned or cancelled first.
- After a service restart, the new Supervisor generation fences the old one;
  queued deliveries reconcile rather than replay. If a provider cannot prove
  an interrupt/close acknowledgement, the state stays `RecoveryRequired` —
  resolve it explicitly; never report completion you cannot prove.
- To continue a member from an earlier native session on a new run, pass
  `--resume-member <name>:<native-session-id>` at `team-run create`; resume is
  never inferred from the newest local session.

## 7. Tear down honestly

```bash
firm team-run complete --id <run>
firm team-run cancel   --id <run> --reason <text> [--cancelled-by <actor>] [--confirm-provider-stopped]
```

TeamRun completion atomically rejects every non-terminal Work — close,
reassign, or cancel them first. `cancel` on a run that is still executing
requires `--confirm-provider-stopped`: it routes through interrupted-run
recovery instead of the ordinary planning/waiting/reviewing → cancelled
transition, and you are asserting that the provider processes are gone. A completed TeamRun does not close members;
the durable Team, its members, and their sessions outlive the run and carry
into the next one.
