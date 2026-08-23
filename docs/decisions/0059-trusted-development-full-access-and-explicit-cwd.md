# ADR 0059: Trusted-development FullAccess and explicit cwd

Status: accepted; implementation tracked by DEV-65 / Issue #535

## Context

Real managed Codex dogfood showed three different concepts had been coupled:
provider capability, trusted local development permission, and workspace
isolation. A managed Host was silently narrowed to ReadOnly, the Dashboard did
not show the effective AgentSession truth, and Harness MCP mutation tools became
an accidental coordination fallback. Earlier Host-only workspace reservation
also treated a worktree policy as runtime identity authority.

## Decision

- Managed Codex, Claude, Kimi, Pi, and DeepSeek Harness Hosts and Members use
  the provider-reviewed FullAccess/bypass mapping in trusted development.
- The permission ceiling and exact canonical cwd are frozen when AgentSession
  is created. Existing Sessions cannot be widened in place.
- Every managed MemberRun resolves an explicit cwd. Host and multiple Members
  may share the same cwd concurrently. Harness does not own a cwd-exclusive
  writer lease.
- A worktree is an optional task-level isolation choice. It is appropriate for
  overlapping edits or independent Git history, but it does not create another
  agent identity, Work authority, Session truth, or coordination store.
- Agent-side Harness mutations use `firm` CLI and its exact Supervisor-issued
  capability. The managed launch profile removes Harness/AgentFirm mutation MCP
  servers while preserving unrelated provider MCP configuration.
- Dashboard private RoleViews display the exact current AgentSession provider,
  effective permission, cwd, lifecycle, generation, native-session reference,
  and reviewed Desktop open target. Provider-native transcript remains the
  execution truth and is never mirrored into Harness coordination state.

## Consequences

- Sharing one cwd can produce ordinary file conflicts. Work boundaries,
  Messages, version control, and Review coordinate those conflicts; Harness
  does not claim to prevent them.
- FullAccess remains local development execution access. It does not authorize
  payment, publishing, account or organization changes, secrets, or any other
  protected external effect.
- Store, Supervisor, NodeDaemon, AgentSession generation, RuntimeCommand,
  provider receipt, and Host acceptance fences remain unchanged.
- `external_interactive` remains an explicit weaker exception with no managed
  AgentSession or fabricated provider receipt.

## Acceptance

- Fresh managed Host and Member Sessions for all five providers report
  FullAccess and the exact frozen cwd.
- Two managed Sessions can bind the same canonical cwd; a separate worktree can
  be selected when the operator wants isolation.
- Managed agents have no Harness mutation MCP tools and can execute Work,
  Message, accept, Close, and Reopen through authenticated CLI paths only.
- Dashboard and Codex Desktop open the same native Session identity without a
  transcript mirror or a false `No provider bound` state.
- A fresh post-merge Team dogfood proves the contract with one NodeDaemon.
