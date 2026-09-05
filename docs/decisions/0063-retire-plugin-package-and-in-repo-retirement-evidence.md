# ADR 0063: Retire the Plugin Package and the In-Repo Retirement Evidence

```text
status: accepted; implemented by the 2026-09-05 repository cleanup
date: 2026-09-05
amends: ADR 0028, ADR 0053, ADR 0061
canonical_for: where retired repository surfaces live; how the collaboration skill is distributed; what the Harness installer publishes
```

## Context

Five top-level directories carried only ceremony or already-proven history:

- `plugins/star-harness` (with `.claude-plugin/marketplace.json`) was a
  byte-identical mirror of `skills/`, four thin slash commands, and hooks that
  still called the retired `harness hook record` and taught the retired
  `team-run ack`. Keeping the mirror needed a sync script and three gates. The
  dogfood evidence (#766, #767) showed that the harness injects no skill into
  members and that stale user-scope copies shadowed the plugin copy anyway, so
  the marketplace path never delivered the current contract.
- `specs/retirement` and `archive/dynamic-workflow` were the Dynamic Workflow
  retirement proof (ADR 0028, DEV-56): the bound register (189 rows) and the
  archived tree. The first `pnpm check` gate re-hashed that tree on every run.
  The export-and-verify obligation was met when DEV-56 closed; re-proving it
  per commit only cost every agent context.
- `archive/skills` held the Company OS operator skills archived by DOC-108
  (ADR 0053). Since DEV-181 the `[retired_skills]` gate excludes those names
  by configuration, so an archived copy no longer serves any check.
- `specs/*` (nested-agent-team-organization, organization-company-work,
  audit-work-description-quality-w5, supervisor-daemonization) were superseded
  design evidence; accepted Notion docs own current intent.
- `collab-skill-workspace` was the frozen evaluation workspace of one skill
  iteration.

## Decision

1. **Delete all five directories. Git history is the archive.** The last
   master tree that contains them is `918e9002`.

   | Path | Files | Tree SHA at `918e9002` |
   | --- | --- | --- |
   | `plugins/star-harness` | 16 | `7a98db63a81e4432dce0d36ed4ccc5d19d9ab6aa` |
   | `archive/dynamic-workflow` | 48 | `77384400aacc6f50c7bb910945c224909ef94fd9` |
   | `archive/skills` | 34 | `aa01917588b9f26df0f5c5b65c8751a39b61a1b8` |
   | `specs` (incl. `retirement/*.v1.json`) | 10 | see manifests below |
   | `collab-skill-workspace` | 27 | frozen evaluation output |

   The retirement manifests remain the proof of ADR 0028's export step:
   `dynamic-workflow-completion.v1.json` (task DEV-56, retirement start
   revision `8f4fa38a0d0486a7d8cf9ec0882b8584b10033bb`, 192 source candidates,
   189 register rows) and `dynamic-workflow-bound-register.v1.json`
   (AF-RET-001-v4 bound to `921d0a47d98ae4f7d7eee04518a9e6ca55024c06`, payload
   sha256 `ca53ee4eef284435b84a4058ac032fa4499f8c19833102d46a2b90164ff12e3a`).
   Recover any of it with
   `git show 918e9002:specs/retirement/dynamic-workflow-completion.v1.json` or
   `git archive 918e9002 archive/dynamic-workflow`.
2. **One retired-path gate replaces four path-policing gates.**
   `scripts/check-retired-paths.mjs` runs first in `pnpm check` and
   `pnpm check:fast` and fails when `plugins/`, `.claude-plugin/`, `specs/`,
   `archive/`, `collab-skill-workspace/`, or the retired scripts
   (`check-dynamic-workflow-retirement-manifest`, `check-star-harness-plugin`,
   `check-star-harness-hook`, `sync-star-harness-plugin-skills`) reappear.
3. **The collaboration skill has two distribution paths and no plugin.**
   Canonical sources stay in `skills/`; agents inside this repository read the
   `.agents/skills` symlinks (`.claude/skills` → `.agents/skills`); other
   projects take snapshots with `scripts/install-skill.sh`. Kimi loads the
   same directories from cwd or `--skills-dir`.
4. **The installer publishes the binary only.**
   `scripts/manage-star-harness-install.sh` keeps the locked, atomic
   publication of `~/.local/bin/harness` and `firm` with rollback, plus the
   Claude and DeepSeek member runners. Its version is now the firm-cli crate
   version plus the source revision (`<crate>+g<sha>[.dirty]`) instead of the
   plugin manifest version; it no longer requires `codex` or `claude` on PATH,
   and its installation state is `schema_version: 2` without plugin fields.
   Operators remove pre-cutover plugin installs once with the commands in
   [operations.md](../current/operations/operations.md).

## Consequences

- AGENTS.md invariant 3 still holds: the historical stores were exported and
  verified before this deletion (DEV-56). No current surface may re-create the
  retired paths or restore the plugin as a distribution channel.
- The plugin's SessionStart hook that printed the Host thread binding and
  pushed the Host inbox is gone with it. A Host passes
  `--host-surface`/`--host-thread-id` itself and reads
  `firm team-run host-inbox`, as `references/host-loop.md` already describes.
- `~/.local/lib/star-harness/<version>` directories created before this change
  keep their plugin-semver names; the operator removes them when convenient.
- Historical ADRs (0028, 0053, 0061) keep their references to the archived
  paths as history; they are not rewritten.
