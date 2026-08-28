# Claude Code entry

@AGENTS.md

All operating rules for this repository live in `AGENTS.md` (imported above);
this file adds nothing and must stay a thin import so Claude and Codex read
one constitution.

Skills: `.claude/skills` is a symlink to `.agents/skills` — the single source
for repository skills. Edit skills only under `.agents/skills/`; never create
real skill directories under `.claude/`.

Enforced by `node scripts/check-agent-config-sync.mjs` (part of `pnpm check`).
