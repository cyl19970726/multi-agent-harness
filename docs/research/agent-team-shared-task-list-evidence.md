# Agent Team Shared Task List: Failure Reconstruction

```text
status: active research evidence
owner_role: execution-foundation
authority_class: research
parent_study: docs/research/agent-team-shared-task-list.md
evidence_snapshot: 2026-08-02
```

> This is the evidence companion to the shared task-list study. Counts are a
> point-in-time snapshot of one important dogfood lifecycle, not a permanent
> product metric or an implemented contract.

## Evidence method

The reconstruction uses three sources:

1. Harness-native TeamRun, MemberRun, TeamMessage, correlation, Handoff, and
   delivery records from `team-run-1785417589241-p28630-0`;
2. the corresponding provider-native Session histories for execution claims;
   and
3. official Claude Code Agent Teams documentation for the comparison model.

## The failure lifecycle

### 1. A focused two-lane audit began correctly

`AgentOS Governance Dogfood Team` started with two bounded read-only lanes:

- Docs Governor audited Docs information architecture.
- Org Governor audited Organization, permission, binding, and navigation truth.

Each Member was expected to return a Markdown Handoff for Host review. At this
size, Assignment messages were understandable because the Host could hold the
entire active work set in immediate context.

### 2. The same TeamRun became a long-lived operating system

Over approximately 25.82 hours the run grew to:

| Fact | Snapshot |
| --- | ---: |
| MemberRuns | 23 |
| Distinct TeamMessages | 1,103 |
| Assignment messages | 94 |
| Ordinary messages | 649 |
| Handoffs | 348 |
| Control messages | 12 |
| Correlation chains | 106 |
| Providers | 18 Codex, 4 Kimi, 1 Claude |

At the snapshot, one Member was running, eight were idle, twelve stopped, and
two disconnected. The TeamRun still reported `running`.

The system retained all coordination, but it no longer had a bounded view of
work. A Host reading mail could reconstruct task state; the Team could not
query it directly.

### 3. More Members did not create more throughput

During the lifecycle, the user observed that the Supervisor still appeared to
do nearly all meaningful work. The recorded diagnosis was revealing: seventeen
Members existed, but only the Platform lane was implementing and the Lead lane
was reviewing; many Docs, Work, and Organization agents were idle.

The attempted repair was another large Assignment message to the Lead. It asked
the Lead to inspect all current Org/Work/Docs state, activate lanes, allocate
worktrees, collect Handoffs, and keep the whole system moving. The repair added
capacity, but it did not add a shared representation of demand.

So the operating loop stayed Host-centric:

```text
Host reads many messages
  -> reconstructs current work in private context
  -> writes another Assignment
  -> Member reconstructs repository and WorkItem state
  -> Member sends progress or Handoff
  -> Host reconstructs the global picture again
```

Idle Members could not simply ask for “ready unassigned work”. The Host had to
notice idleness, remember outstanding work, choose a Member, recreate enough
context, and send another message.

### 4. One capacity Assignment became an implicit project database

The capacity-expansion correlation
`dogfood-cto-capacity-expansion-20260731-v1` began with Assignment message
`tmsg-1785501835803-p19605-0`.

It combined several responsibilities:

- establish a CTO hierarchy;
- create UI, Core, and Runtime Members;
- create or link WorkItems;
- allocate clean worktrees;
- establish reviewers and merge gates;
- update Docs and Work projections;
- validate Skills; and
- produce a capacity report.

Within that one correlation, the Lead received five inputs but emitted 57
outputs over about 1.89 hours:

| Output kind | Count |
| --- | ---: |
| Handoffs | 38 |
| Ordinary messages | 19 |
| Outputs directly tied to the root Assignment | 18 |

The outputs repeatedly corrected roster, causation ids, worktree facts, gates,
and acceptance status for UI, Core, Runtime, and Skill lanes. The first Handoff
reported one child Member created and two authorized but unfilled. Later
messages changed that picture and required new overall Handoffs.

This was not excessive communication by itself. The problem was that the
correlation chain was the only place where the evolving task set existed. Every
new reader had to replay prose to know current truth.

### 5. One Company WorkItem was repeatedly rediscovered

The company WorkItem `work-agentos-github-source-binding-v0` appeared in 13
distinct Assignment correlations sent to four Member recipients over 7.64
hours.

Those assignments included Work provenance, Docs Skill governance, Lead
judgment, PR review, post-PR audit, continuation planning, a delivery canary,
bridge repair, fresh R1 assignments, and renewed Docs/Work audits. These were
not thirteen identical implementation jobs. They were thirteen moments when a
participant had to rediscover the same WorkItem's current execution state and
invent a new conversation chain.

Across all 94 Assignment messages:

- 14 explicitly mention an “existing WorkItem”;
- 7 explicitly warn “do not create a duplicate”; and
- 40 ask the recipient to inspect or re-read current state.

Other repeated references show the same pattern:

| Company WorkItem | Correlations | Recipients |
| --- | ---: | ---: |
| `work-agentos-org-role-permission-closure-v1` | 11 | 7 |
| `work-agentos-runtime-upgrade-reconciliation-v1` | 10 | 6 |

The repeated warning against duplicates is itself evidence that the task model
does not make uniqueness and current ownership obvious enough.

## Root cause, not symptoms

The failure was not primarily too few Agents, weak models, insufficient
permissions, or too little messaging. It was a missing shared state object.

| Symptom | Immediate workaround | Why the workaround did not scale |
| --- | --- | --- |
| Idle Members while work remained | Host sends more Assignments | Host must first reconstruct all demand |
| Repeated repository and WorkItem inspection | Put more context in each message | Context grows and becomes stale immediately |
| Duplicate-looking correlations | Warn Members not to duplicate work | Warning is not atomic ownership |
| Unclear dependency order | Explain ordering in Markdown | No queryable “ready now” projection |
| Repeated Handoffs | Summarize the whole lane again | Conversation history remains the task database |
| Org hierarchy feels slow | Add more management layers | Each layer repeats the same private reconstruction |

The lifecycle can be summarized as:

```text
no shared task list
  -> Assignment message becomes task identity
  -> task state lives in Host/Member private context
  -> idle Members cannot discover ready work
  -> Host becomes the scheduler and database
  -> more Members produce more coordination mail
  -> repeated inspection and duplicate-risk warnings increase
  -> Organization layers amplify the same weakness
```

The parent study turns this evidence into a bounded design recommendation:
[Agent Team Shared Task List](agent-team-shared-task-list.md).

## Reproducibility appendix

Snapshot locator:

```text
Execution Space: firm-dogfood
Store root: ~/.firm/execution-spaces/firm-dogfood
TeamRun: team-run-1785417589241-p28630-0
Snapshot date: 2026-08-02 Asia/Shanghai
```

Snapshot hashes at reconstruction time:

| File | SHA-256 |
| --- | --- |
| `team_runs.jsonl` | `4e39dcedb96862e067e1f2b38f2e3372a5f419e23b9439db17df39a47b813cfa` |
| `member_runs.jsonl` | `62ad800eeb516fe45d5a623ac558ca2cb4165710378f52eb9c0db138a952e4f2` |
| `team_messages.jsonl` | `66727e04cd75a878d94a73f65ccb4d9c42c73a4b33e18b686329831d5f323023` |

The reconstruction first applies latest-wins by record id, then filters every
MemberRun and TeamMessage by the TeamRun id above. Provider execution claims
resolve each selected MemberRun's `native_session` locator; the report does not
derive tool/turn claims from TeamMessage prose.

The 14/7/40 content counts use case-insensitive body matching on the 94
latest-wins Assignment messages:

```text
14: body contains the phrase "existing WorkItem" or its Chinese equivalent
 7: body contains an explicit "do not create a duplicate" instruction
40: body directs the recipient to inspect, read, re-read, query, or reconcile
    current repository/store/WorkItem state before acting
```

These categories overlap and are classification evidence, not additive totals.
To reproduce them, export the filtered Assignment rows, preserve ids/body, run
the literal phrase queries, then manually review the matching set for meaning.
Future refreshed evidence must record a new snapshot date and hashes rather
than silently replacing these counts.
