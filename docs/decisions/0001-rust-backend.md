# 0001: Rust Backend

## Decision

Use Rust for the backend.

## Rationale

The core is an event system, state machine, and audit ledger. Rust is a good
fit for append-only storage, concurrent agent writes, permission gates, and
typed lifecycle transitions.

## Consequences

Core object validation, store logic, CLI/API behavior, and provider runtime
supervision should be implemented in Rust-first crates.
