# Governance Engine

`harness governance` is the project-portable home of documentation/skill
governance. The gate logic lives in the `harness-governance` crate (native Rust,
compiled into the `harness` binary), so any project the harness operates on — Go,
Python, mdBook, a repo with no Node toolchain — gets the same closed-loop
governance with zero hosted scripts.

This is the generalization of what used to be `scripts/check-doc-*.mjs` +
`check-skills.mjs` (Node/pnpm only, hardcoded to this repo's `docs/` layout). The
portable methodology stays in the
[bootstrap-project-workflow](../skills/bootstrap-project-workflow/references/governance.md)
skill (the Governance Contract); the harness binary is the enforcer.

## Commands

```text
harness governance check     [--root <path>] [--json]   # run the gates; exit 1 on a blocking failure
harness governance init      [--root <path>]             # write a starter .governance.toml
harness governance describe  [--root <path>]             # print the active config
```

`governance` is store-less: it never resolves or mutates a harness store, so it
runs identically inside a goal worktree, in CI, or in a non-harness repo. The
project root is the cwd by default (`--root` to override).

## Configuration: `.governance.toml`

Per-project config lives at the PROJECT ROOT as `.governance.toml` (committed,
travels with the repo). It is NOT placed under `.harness/`, which is the
gitignored, serve-truncatable store. Absent a config file, a light default runs
(`links` + `size` + `skills`, no registry requirement) so an un-opted-in project
still gets the cheap gates; a project opts into full governance by committing a
`.governance.toml` (scaffold one with `harness governance init`).

Shape (`schema = "agent_harness.governance.v1"`):

```toml
schema = "agent_harness.governance.v1"
doc_roots = ["README.md", "docs", "schemas", ".agents/skills", "examples", "apps"]
skill_roots = ["skills", ".agents/skills"]
max_lines = 500
member_data_root = ".agents/data"   # optional: scanned for *-agent-member.json skill_refs

[registry]                          # optional: omit for a project with no doc registry
path = "docs/registry.json"
schema = "agent_harness.docs_registry.v1"
required_fields = ["path", "ownerRole", "status", "lifecycle", "canonicalFor",
                   "dependsOn", "machineConsumers", "reviewAfter",
                   "lastVerifiedWith", "reorgTrigger"]
allowed_statuses = ["idea", "planned", "stable", "deprecated", "archival"]
allowed_lifecycles = ["volatile", "stable", "archival"]
core_docs = ["README.md", "docs/README.md", "docs/architecture.md"]
```

A project with docs in `book/` sets `doc_roots` accordingly; an mdBook project
drops the `[registry]` block and keeps `links` + `size`.

## Gate kinds

| Kind | Severity | Checks | Ported from |
| --- | --- | --- | --- |
| `links` | blocker | every relative Markdown link resolves to a file | `check-doc-links.mjs` |
| `size` | warning | markdown over `max_lines` (warn, never block) | `check-doc-size.mjs` |
| `skills` | blocker | SKILL.md frontmatter + `agents/openai.yaml` + member `skill_refs` resolve | `check-skills.mjs` |
| `registry` | blocker | required fields, allowed enums, path/dependency existence, no dupes, core docs registered, valid `reviewAfter` | `check-doc-governance.mjs` |

The ports are faithful 1:1 (same roots, rules, and messages), with two
deliberate refinements: directory entries are sorted (deterministic output,
unlike Node's `readdirSync`), and a missing root is skipped rather than throwing.
On a repo where every root exists, the output is byte-identical to the legacy
gates.

## Parity and self-host

`harness governance check` is byte-parity verified against
`pnpm check:links && check:doc-size && check:skills && check:doc-governance` on
this repository (identical stdout and the same warning set on stderr). The
`harness-governance` crate carries a self-host test
(`self_host_repo_is_governance_green`) that runs the engine against this repo via
its committed `.governance.toml` — the permanent regression gate that catches
port drift.

## Relationship to the doc-sync built-in phase

The doc-sync built-in phase (see [goal-phase-loop.md](goal-phase-loop.md)) is the
consumer: after a goal's execution phases pass, it runs the
bootstrap-project-workflow methodology and then the doc gates. As the engine
takes over, that phase's gate command becomes `harness governance check` (one
toolchain-agnostic command) instead of the three Node invocations, so the loop
gates identically on a no-node project. The phase mechanism — auto-append,
blocking/soft, the `verdict` gate, and `declared_doc_updates` as the focused
audit set — is unchanged.

## Status / roadmap

- First cut (done): the crate + the four native gate ports + `harness governance
  check|init|describe` + this repo's `.governance.toml` + byte-parity + the
  self-host test. The legacy `.mjs` gates still run in CI unchanged.
- Full cut (planned): point the doc-sync phase + the `registered_doc` gate at the
  engine, make `package.json` `check:*` delegate to `harness governance check`,
  then retire the four `.mjs` scripts once parity is enforced in CI. The `skills`
  gate gains skill↔doc cross-reference checking (a net add — the legacy doc-sync
  prompt never ran the skills gate).
