# CI/CD

## Phase 0: Docs And Schema CI

Current first gate:

```text
node scripts/validate-json.mjs
node scripts/check-doc-links.mjs
```

This keeps the documentation and schema skeleton coherent while the Rust
backend is being introduced.

## Phase 1: Rust Backend CI

After the first Rust crates land:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Phase 2: Contract CI

Add contract checks:

- Rust type to JSON schema coverage;
- JSON schema validates example artifacts;
- CLI `--help` snapshot matches docs;
- `.task` example fixtures load correctly;
- adapter tool descriptors validate.

## Phase 3: Dashboard CI

After the dashboard app exists:

```text
pnpm lint
pnpm typecheck
pnpm build
```

## CD Plan

Keep CD simple until the product stabilizes:

1. GitHub release draft from tags.
2. Publish Rust crates when APIs stabilize.
3. Build Docker image for `harness-api`.
4. Publish dashboard artifact or image.

Do not add deployment automation before the core object model is stable.
