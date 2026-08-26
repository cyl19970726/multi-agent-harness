# Host Operating Loop

Read [SKILL.md](../SKILL.md) Part I first — this reference assumes the shared
mental model and only sequences the Host-side procedure. The Host is Team
Lead through a Host-role TeamMembership; it is not an implicit MemberRun and
it never sees Member thinking, tool output, or private session detail beyond
the `host_member_public` scope.

## 1. Compose the Team before starting anything

Decide, explicitly and durably, before `team-run start`:

- **Roster**: which AgentMembers, which provider mode each runs
  (`codex_app_server`, `claude_agent_sdk`, `kimi_acp` are the executable Team
  modes — bounded one-shot modes cannot be Team members), and each member's
  permission ceiling. Ceilings are frozen at AgentSession start; a provider
  asking for more mid-run fails closed.
- **Disjoint owned paths / worktrees** per member. Two members editing one
  workspace is the single most common way real runs destroy work: uncommitted
  state has no protection. Use `--member-worktree name:path` and
  `--member-owned-path name:path` at create time.
- **Completion standards**: what evidence each kind of Work must carry (PR
  URL, named check command with exit code, artifact path). If you cannot name
  the evidence, the Work is not bounded yet.

Create the durable Team once (`firm team create --name … --host-agent-id …
--node-id … --member …`); it persists across runs. Create a TeamRun per
engagement; the Project Binding is frozen on the run and the returned
execution/member roots are the members' worlds.

## 2. Decompose into bounded Works

Write each Work so a Member can execute it without asking what "done" means:

- **What** — the problem in one sentence.
- **Mental model** — states, invariants, data flow the member must hold.
- **Boundary** — paths to touch and paths never to touch.
- **Gates/Evidence** — what the reviewer will check, verbatim.

Choose the claim mode deliberately: `host_assign` (with
`--owner-member-run-id`) when one member must own it; `team_claim` when any
eligible member may atomically claim. Prefer several bounded Works over one
epic — TeamRun completion atomically rejects non-terminal Works, so unbounded
Works block teardown.

Works are flat peer nodes. Add a hard dependency only when the successor truly
cannot execute before the prerequisite is accepted. Fan-out independent lanes;
fan-in integration/review after all required inputs. Never encode decomposition
as Work containment. The kernel must reject self-edges, duplicates, missing
prerequisites, stale revisions, and cycles. Treat failed or cancelled
prerequisites as Host replan attention, not automatic downstream resolution.

## 3. Start, then watch without polling

`team-run start` reserves the run and returns immediately; members run in the
background under the machine NodeDaemon's Supervisor generation. Give the
user the returned dashboard URL at once.

Do not poll in a loop. The board is cursor-based and events are sequenced:

```bash
firm team-run board-summary --id <team-run-id>
firm team-run work list --team-run-id <team-run-id> --brief
firm team-run work list --team-run-id <team-run-id> --since <cursor>
```

`board-summary` is a ≤500-character overview (work counts + each member's
idle/working/awaiting-review state). Use `--since` cursors or event
notifications between checks; a sleep-and-status loop burns your own context
and the budget without adding information.

## 4. Converse with exact correlation

Your inbox is a projection of per-recipient deliveries addressed to the Host.
Read it at your safe boundaries; mail from members is durable immediately but
does not interrupt your current reasoning.

- A member's decision-shaped question arrives as a correlated Message with
  exact ids. Answer **on the same correlation** with the exact option/message
  ids; a fresh uncorrelated reply strands the member's pause.
- Use `informational` intent for anything that does not need a member
  provider round; `response-required` mail is what wakes a member cycle.
- Steer changes a member's **current** turn only when the provider
  acknowledges it; a queued Message affects the **next** safe boundary.
  Interrupt stops one turn without closing the member.
- Never order work in chat. If conversation produces durable follow-up,
  create a peer Work and, when ordering is real, mutate the dependency graph
  through the authenticated Work action.

## 5. Review evidence, then decide

Submission moves Work to `review` — that is a request for judgment, not a
result. Review means: open the artifact refs (the PR diff, the file), rerun
or read the named check refs, and walk the completion criteria line by line.

- **Accept** → Work closes with resolution `Accepted`.
- **Request changes** → Work returns to `Active` with your reasons recorded
  in the WorkEvent history; the member continues in the same MemberRun,
  workspace, and native session. Do not spawn a replacement agent for a
  revision — the existing member holds all the context.
- Never accept on a provider completion status, a delivery receipt, or a
  green fixture alone.

These actions apply to ordinary Member-owned Work. For Work owned by the Host,
the Host must not self-accept. Submit the Host Work, send one response-required
Work-linked Message to an exact active non-owner peer in the same TeamRun, and
wait for that peer's explicit `firm member work accept`. If the peer finds a
problem, it reports the requested revision through the linked conversation;
revise and resubmit the same Work. A solo Host leaves its Work in `review` until
an exact active peer is available; it must never fabricate acceptance through a
generic Human/Service control-plane credential.

Cross-Team needs are an explicit `WorkDelegation` from a source Work you own
to a target Work in the other flat Team; target completion never
auto-completes your source Work.

## 6. Recover instead of restarting

A dead member runtime is not lost work. The lifecycle controls:

- `close-member` releases the provider runtime, retains the MemberRun and its
  session binding.
- `reopen-member` resumes the **exact native session** under a new adapter
  generation after delivery reconciliation — the member returns with its
  memory intact. Prefer this over any fresh spawn.
- `deactivate-member` retires the coordination identity permanently;
  unfinished Work must be reassigned or cancelled first.
- After a service restart, the new Supervisor generation fences the old one;
  queued deliveries reconcile rather than replay. If a provider cannot prove
  an interrupt/close acknowledgement, the state stays `RecoveryRequired` —
  resolve it explicitly; never report completion you cannot prove.

## 7. Tear down honestly

TeamRun completion atomically rejects every non-terminal Work — close,
reassign, or cancel them first. A completed TeamRun does not close members;
the durable Team, its members, and their sessions outlive the run and carry
into the next one.
