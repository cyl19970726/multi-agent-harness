# ADR 0056: Correlated Message And Session Permission Cutover

- Status: accepted; breaking clean cutover
- Date: 2026-08-14
- Supersedes: ADR 0030 interaction-object and permission-routing contract
- Product authority: AgentFirm CompanyOS DOC-87
- Implementation issue: [#462](https://github.com/cyl19970726/multi-agent-harness/issues/462)

## Context

The provider bridge had two competing coordination models. Ordinary questions
could be represented as correlated Messages, while a second durable interaction
ledger, schema, API, SSE stream, and Dashboard projection described the same
pause. Tool permission callbacks were also promoted into that second lifecycle
even though every AgentSession already has an effective permission ceiling.

## Decision

1. A provider question is one correlated request `Message`; its answer is one
   correlated reply `Message` bound to the exact request, AgentSession, member,
   runtime generation, and provider option or text.
2. There is no separate provider-interaction object, store, schema, endpoint,
   SSE frame, Dashboard list, or compatibility reader.
3. One machine-scoped NodeDaemon owns local AgentSessions and provider effects.
   Team membership and subscriptions are routing overlays; they do not own the
   provider thread.
4. The AgentSession freezes its effective permission ceiling before provider
   start. In-ceiling operations proceed through the provider's native sandbox.
   Out-of-ceiling or unexpected approval callbacks fail closed and cannot widen
   the session or create a second permission workflow.
5. Protected Company effects remain governed by their domain approval policy;
   that policy is separate from provider tool permission.
6. Historical JSONL files are not migrated, read, projected, dual-written, or
   mutated after this cutover.

## Consequences

- CLI/MCP use `answer-message` / `team_run_answer_message`; HTTP uses the
  message-scoped `/messages/{id}/answer` route.
- Provider-native session records remain transcript/tool/turn truth.
- `MessageDelivery` and provider receipts remain transport evidence.
- ADR 0030 and matching references in older ADRs are historical evidence only.
- The change is intentionally breaking because compatibility would preserve
  the ambiguity this decision removes.

## Acceptance

- active source and current docs contain no operational reference to the
  retired object, ledger, route, or projection;
- question/reply idempotency and authority tests pass on exact Messages;
- Codex launches with its mapped sandbox and `approvalPolicy=never`;
- unexpected Codex approval callbacks fail closed with no question Message;
- schema, Rust, Dashboard, governance, and plugin checks pass.
