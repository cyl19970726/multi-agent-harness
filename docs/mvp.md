# MVP

## Purpose

The active execution MVP proves:

```text
Mission -> ordered Host-plan Wave
Mission <-> independent AgentTeam -> Mission-scoped TeamRun -> MemberRun
```

Mission/Wave gives the Host durable external memory while leaving real
execution with Agent Teams, Dynamic Workflows, Host work, and provider-native
sessions. The retired Goal/GoalPhase/task-graph stack is not an active
dependency.

## MVP Slice

### 1. Mission And Wave

- Mission stores durable Markdown context, linked `agent_team_ids`, status, and
  closeout.
- Wave stores ordered Markdown context, revision, updated actor, outcome,
  artifacts, and explicit Host advance.
- Wave is not an executor container, task graph, barrier, or session boundary.
- Append-only history reconstructs how Host judgment changed.

### 2. Independent Agent Team

- Stable AgentTeam definition can exist without a Mission and can link to more
  than one Mission over time.
- A Mission may link multiple teams.
- Primary TeamRun creation uses `mission_id + agent_team_id` and no Wave id.
- MemberRuns and provider-native sessions may continue across Wave advance.
- Assignment ownership uses `TeamMessage(kind=assignment)` and
  `correlation_id`; optional `origin_wave_id` is navigation metadata.
- Host can add a repair member while the run is active without erasing prior
  assignments or attempts.

### 3. Other Execution

- Dynamic Workflow owns its run/step/result/artifact truth.
- Host work records observable outcomes/artifacts without invented children.
- A Wave may explain either capability but does not absorb its runtime model.

### 4. Provider Truth

- Harness persists coordination, session locators, messages, pending
  interactions, controls, outcomes, and artifact/check references.
- Provider-native storage remains sole transcript, tool/command/file/turn, and
  resume truth.
- Thinking is sanitized transient live state only and never evidence.
- Provider questions and plan reviews become PendingInteractions; a provider
  `completed` frame is not a semantic answer or approval.

### 5. Host And Dashboard

- CLI is the complete control surface and shares application logic with HTTP.
- MCP is an optional thin adapter with no independent lifecycle or storage.
- The thin `orchestrate-mission-waves` skill teaches Host procedure, not schema.
- Mission Canvas renders long Markdown context, linked teams, ordered Wave
  history, responsibility tables, carry-over, advance, and closeout.
- Team and Member pages provide honest activity, navigation, chat, pending
  interaction, steer, interrupt, and resume according to adapter capability.

## Deterministic Acceptance Journey

1. Create a Mission with Markdown context.
2. Create/link an independent AgentTeam with at least three members.
3. Create Wave 1 with a responsibility table in Markdown.
4. Start a Mission-scoped TeamRun without `wave_id`.
5. Assign correlated lanes and bind provider-native sessions.
6. Advance Wave 1 while one member remains active.
7. Create Wave 2, continue the same MemberRun/session, and add a repair member.
8. Verify Mission, Wave history, linked team, messages, origin metadata,
   pending interactions, artifacts, and native-session resolution in CLI/API
   and Dashboard.
9. Close the Mission without deleting, archiving, or silently completing the
   team.
10. Prove the optional MCP adapter delegates to the same behavior.

Run:

```bash
npx pnpm@9.15.4 acceptance:mission-wave
```

Deterministic acceptance proves contracts, not a live-provider claim. A live
claim additionally needs resolvable provider-native records.

## Compatibility Boundary

Existing direct-Wave-executor fields and rows remain readable as legacy data.
New authoring, fixtures, tests, Dashboard copy, and Host examples do not create
them. Retry history is preserved; migration never mutates an old attempt into
the new model.

## Explicit Non-Goals

- task graph or universal executor object;
- Harness transcript/tool/thinking ledger;
- Wave-owned TeamRun lifecycle;
- automatic team deletion at Mission closeout;
- mandatory MCP or plugin installation;
- treating Company OS WorkItem approval as equivalent to Wave advance.
