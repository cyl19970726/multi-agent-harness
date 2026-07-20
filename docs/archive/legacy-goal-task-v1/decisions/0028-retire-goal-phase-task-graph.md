# ADR 0028: Retire Goal, GoalPhase, and the legacy Task Graph

## Status

Accepted. Retirement is staged because active stores contain historical data;
the end state is removal from active product context and code.

## Context

The repository still contains an older Goal/GoalPhase/Task Graph product model.
Keeping it beside Mission/Wave and Company OS objects repeatedly causes new
designs and Agents to inherit the wrong hierarchy. Compatibility has therefore
become a context and maintenance liability rather than a product capability.

The current repository store also contains real historical records and links.
Deleting types and ledgers before exporting their provenance would be an
unacceptable destructive migration.

## Decision

Retire the old model completely from active operation:

- no new writes, CLI/API/MCP commands, Dashboard navigation, default snapshot,
  Skill, example, prompt, or self-hosting flow may create or depend on it;
- Mission/Wave remains the native execution hierarchy;
- WorkItem is the native company responsibility record and is not a renamed
  legacy engineering task;
- historical ledgers are exported byte-for-byte with hashes, a relation edge
  manifest, schemas, and interpretation material;
- verified archives are offline history and are never dual-read as live state;
- after every configured project is exported and verified, old types, ledgers,
  routes, schemas, fixtures, tests, workflows, and active documentation are
  deleted in dependency order.

Active Company OS documents should not repeatedly teach the retired vocabulary.
The temporary removal plan is the single migration entry until it moves into
the verified archive.

## Consequences

- This supersedes the compatibility policy in ADR 0026 and ADR 0027. It does
  not change ADR 0026's Mission/Wave, executor, retry, gate, or transient-live
  thinking semantics.
- A migration exporter must land before destructive deletion.
- Legacy commands are frozen with an explicit migration notice before removal;
  they are never silently redirected to Mission or WorkItem.
- Historical product ADRs and designs ultimately leave active navigation and
  move into the versioned archive with a concise active tombstone.
- Acceptance requires zero retired-model references outside the archive and
  zero legacy ledgers in each migrated active store.

## Validation

The executable R0–R5 sequence and exact repository inventory are retained in
the adjacent archived removal plan. The final archive preserves that plan and
its evidence.
