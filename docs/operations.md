# Operations

This document owns local operation, troubleshooting, release, and maintenance
rules.

## Current Local Check

```bash
npx pnpm@9.15.4 check
```

Current checks:

- JSON parsing for schemas and examples;
- Markdown local link validation.

## Future Rust Checks

After the Rust crates land:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Operating Rule

Project-specific commands belong to adapters. The generic harness should expose
stable task/message/evidence/decision operations.
