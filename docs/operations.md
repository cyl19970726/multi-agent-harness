# Operations

## Current Gates

```bash
npx pnpm@9.15.4 check
```

Current checks:

- JSON parsing for schemas, docs, and examples;
- schema fixture validation;
- Markdown local link validation;
- document size warning;
- skill frontmatter and UI metadata validation;
- docs governance registry validation.

Rust checks are also active in CI:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Planned Gates

These are design commitments, not current blockers until scripts and CI jobs
exist.

```text
CLI --help snapshot
Rust type <-> schema coverage
adapter descriptor validation
Mermaid render/lint
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
