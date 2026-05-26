# Operations

## Current Local Check

```bash
npx pnpm@9.15.4 check
```

Current checks:

- JSON parsing for schemas and examples;
- Markdown local link validation;
- document size warning;
- skill frontmatter and UI metadata validation.

## Rust Backend Checks

After Rust is installed and crates exist:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## CI Plan

Phase 0:

```text
docs links
JSON parse
doc size warning
skill metadata check
```

Phase 1:

```text
cargo fmt
cargo clippy
cargo test
```

Phase 2:

```text
schema fixture validation
CLI --help snapshot
Rust type <-> schema coverage
adapter descriptor validation
```

Phase 3:

```text
dashboard lint/typecheck/build
API integration smoke
Docker image build
GitHub release
```

## Code And Docs Consistency

- CLI commands shown in docs must appear in CLI help snapshots.
- JSON schemas referenced in docs must parse.
- Examples referenced in docs must be checked by CI.
- Any doc above roughly 500 lines should produce a warning and include a reason
  if it stays unsplit.
