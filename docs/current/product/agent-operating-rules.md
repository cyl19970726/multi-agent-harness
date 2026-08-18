# Agent Operating Rules — Detailed Companion

```text
status: canonical operating detail
owner_role: lead-operations
canonical_for: implementation-bound remainder of the AGENTS.md relocation (CLI commands, gate-grepped phrases, execution-space selectors)
```

## Authority

Product doctrine for this topic — agent operating rules, execution-object
semantics, lifecycle policy, self-hosting method, and the AGENTS.md
relocation map — is canonical in Notion; see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location.
This repository file survives only as the implementation-bound remainder
below. Root [AGENTS.md](../../../AGENTS.md) still states the hard invariants
and wins any conflict.

## Implementation-bound invariants

- The phrases `provider's native`, `streams into Harness ledgers`, and
  `Resume must use the provider-native session id` must stay in root
  `AGENTS.md` — `scripts/check-native-session-boundary.mjs` greps for them.
- Useful local commands:

  ```bash
  target/debug/firm init
  target/debug/firm node init
  target/debug/firm team create --name <team> --description <purpose> \
    --host-agent-id <agent-member-id> \
    --node-id <node-uuid> --member <agent-member-id>
  target/debug/firm team-run create --agent-team-id <team> --objective <objective>
  target/debug/firm team-run work create --team-run-id <team-run> \
    --title <title> --context <markdown> \
    --completion-criteria <criteria> --owner-member-run-id <member-run>
  target/debug/firm team-run work list --team-run-id <team-run>
  target/debug/firm dashboard snapshot
  target/debug/firm serve --addr 127.0.0.1:8787
  npx pnpm@9.15.4 acceptance:legacy-retirement
  ```

- Execution Space / Project Binding selectors: `--space <id>` /
  `HARNESS_SPACE` / `firm space switch`; `--project <id|path>` /
  `HARNESS_PROJECT` / `firm project switch`; `--store` / `HARNESS_ROOT` are
  deprecation-warned back-compat overrides. `AgentTeamRun.project_binding_id`
  and `WorkflowRun.project_binding_id` pin the execution resource once set
  and later selector changes must not retarget them. The reserved GLOBAL
  `_global` (`~/`) project is non-git and rejects
  `writable`/`isolation="worktree"` nodes with an actionable message.
