# Star Harness Plugin

This is the unified, provider-neutral distribution package for Codex, Claude
Code, and Kimi.
It installs experience adapters only:

- generated mirrors of the canonical Host and Member skills;
- optional Harness MCP registration over the same application services as CLI;
- Mission/Team shortcuts and historical Kimi command aliases;
- fail-open lifecycle telemetry, exact native Host binding, bounded
  SessionStart/UserPromptSubmit Inbox context, and one-shot provider-reviewed
  Stop continuation for Codex, Claude, and Kimi mail that arrived while the
  Host was busy; and
- Dashboard deep-link guidance.

Product architecture, messages, Mission/Wave state, TeamRun lifecycle, and
provider capability review remain in Harness. Provider transcripts, tool
activity, and subagent history remain in provider-native sessions.

Bound Member hooks pass an explicit provider identity, so Claude/Kimi events
cannot be mislabeled as Codex. Hooks never ACK mail, impersonate a Host or
Member, or persist provider transcript/thinking.

The optional unbound MCP surface authors only as the Host, an Operator, or a
Service. It cannot select `member_run` or `agent_member` by id; Member mail
originates from that Member's bound persistent Provider runtime.

The skill directories under `plugins/star-harness/skills/` are generated. Edit
their canonical sources under `skills/`, then run:

```bash
node scripts/sync-star-harness-plugin-skills.mjs
node scripts/sync-star-harness-plugin-skills.mjs --check
```

The repository marketplace publishes this directory as `star-harness`. Install
it after building and placing `harness` on `PATH`:

```bash
# Codex CLI / Codex Desktop
codex plugin marketplace add cyl19970726/multi-agent-harness
codex plugin add star-harness@multi-agent-harness

# Claude Code
claude plugin marketplace add cyl19970726/multi-agent-harness --scope user
claude plugin install star-harness@multi-agent-harness --scope user
```

Start a new Codex task or Claude Code session after installation. Plugin
installation does not upgrade Codex, Claude Code, Kimi, or another Provider.
Provider maintenance remains a separate, staged operation governed by ADR
0031: one Provider at a time, no active-session hot replacement,
`review_required` until deterministic and live acceptance, and rollback on
failure.

Repository maintainers can validate or publish one local canonical installation
with:

```bash
pnpm star-harness:install:check
pnpm star-harness:install
```

The apply command builds a versioned Harness binary, points the stable
`~/.local/bin/harness` link at it, removes the duplicate personal Codex copy,
refreshes the Git marketplace, updates Codex and Claude installations, and
writes a rollback/audit record under `~/.local/state/star-harness/`. Existing
sessions keep the Plugin and Provider runtime they already loaded.

The reviewed Kimi CLI does not currently expose a generic plugin-management
command. Kimi Agent Team members use `kimi_acp`, the Harness collaboration
envelope, and skills discovered from their explicit cwd or `--skills-dir`.
`kimi.plugin.json` remains the unified package descriptor for a future native
Kimi plugin installer; do not claim it is globally installed today.

On clients that support command manifests, the command basenames are:

```text
/star-harness:mission-new
/star-harness:team-start
/star-harness:team-status
/star-harness:new-run
/star-harness:status
/star-harness:dashboard
```

The last three preserve the historical command basenames. The retired
`agent-team` namespace cannot remain registered by the new `star-harness`
plugin id without installing a second compatibility plugin, which this package
intentionally avoids so Skills and hooks have one owner.
