# Architecture

The harness has two layers:

```text
Generic Multi-Agent Product
  core objects
  message store
  task/report materialization
  claim/blocker/decision ledger
  permission gates
  provider sessions
  Agent Dashboard

Project Adapter
  tool descriptors
  skill prompts
  artifact readers
  project Dashboard deep links
  domain evidence policy
  permission policy
```

## Data Flow

```text
Lead / Router
  -> AgentMessage(type=task)
  -> AgentMember
  -> Provider adapter or project tool
  -> AgentMessage(type=report)
  -> materializer
  -> AgentReport / Claim / Blocker / Decision
  -> Agent Dashboard
```

## Source Of Truth

Provider chats are not source of truth. A provider result becomes harness truth
only after an Agent Member writes it into one of these objects:

- `AgentMessage`
- `AgentReport`
- `Claim`
- `Blocker`
- `Decision`
- project artifact referenced by `EvidenceRef`

## Trust Order

```text
provider chat < message < materialized report < claim/blocker < lead decision
```

Only materialized artifacts can enter release, promotion, live, or scale gates.
