# Candidate Gate Contracts

## Canonical model

Verification is not embedded in Work. A Work stays a simple responsibility atom;
verification is expressed by candidate-scoped records:

```text
WorkModuleBinding
  -> GateRequirement (exact Work revision + Candidate + requirement-set fingerprint)
      -> GateEvaluation | GateWaiver
          -> accept_work
```

`WorkModuleBinding` freezes a reusable Module version and configuration.
`GateRequirement` freezes a typed evaluator `ActorRef`, evaluator version and
their canonical fingerprint together with the evidence contract and dependency
set. `GateEvaluation.performed_by` must be the transport-authenticated actor and
must exactly equal that frozen evaluator identity; matching only a free-form
name or evaluator version is never sufficient. `GateEvaluation` records that
evaluator's exact Candidate verdict and evidence.
`GateWaiver` is explicit, scoped, authorized, justified, expiring and
revocable.

## Acceptance invariant

A Result `WorkReport` atomically submits the exact active Work revision into
Review. Canonical `accept_work` then acquires the Store writer lock and rereads
the Work, Result report, Candidate fingerprint, evidence, frozen Module
bindings, requirements, dependency closure, evaluations and waivers.

Acceptance fails closed when any version, fingerprint, evaluator identity,
evidence reference, dependency or authority is stale or mismatched. Rejection
must append no accepted Work, decision, event, delivery or provider command.
Idempotent replay returns the prior canonical operation only for the exact same
authenticated actor, command and payload.

## Direct and Module-derived requirements

Direct requirements are allowed for small one-off verification. Reusable
verification belongs in a versioned Module. Binding an integration-plan Module
creates the frozen requirement set for that Work revision; later Module edits
cannot change the Candidate's contract.

Requirements form an acyclic dependency graph. A dependent requirement may pass
only after every dependency has an exact passing evaluation or an active
authorized waiver. Changing the requirement set revises its fingerprint and
invalidates stale evaluations.

## Host review

A reviewer reports findings and a verdict; only Host authority may accept Work.
Provider completion, CI green status, a PR comment or an unbound historical
Review is evidence, never acceptance authority. The owner cannot satisfy an
independent-review requirement by self-attribution.

## Transport parity

CLI, HTTP and MCP resolve authenticated transport identity and its authorized
authority set from server-side credential/session state, overwrite any payload
actor claim, and invoke the same trust application service. HTTP request
headers and bodies cannot select or expand either identity. The
canonical operation ledger is the sole mutation journal for Module, requirement,
evaluation, waiver, Result and acceptance changes.
