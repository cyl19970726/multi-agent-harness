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
