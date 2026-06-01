# WP-6: MCP Consumed + Skill-Ref Resolution Contract + Provider Capability Declaration

## Overview

This work package closes three PROPOSED gaps from docs/agent-integration-model.md:

1. **MCP consumed end-to-end** (Pillar 2) ✓ COMPLETE
   - Neutral `LaunchMcp` block reaches providers (codex: `--config mcp_servers.*`; claude: `--mcp-config`)
   - Added optional `mcp` field to `AgentProviderConfig` so members can declare MCP servers
   - `build_launch_spec` carries it to providers

2. **Skill contract** (Pillar 1) ✓ COMPLETE
   - Skills live at `.agents/skills/<id>/SKILL.md`
   - Implemented resolver maps `skill_refs` -> SKILL.md path/content
   - Check: extended `check:skills` to validate referenced skill_refs resolve (fail fast on dangling refs)
   - Documented the contract in agent-integration-model.md

3. **Provider capability declaration** (Pillar 3) ✓ COMPLETE
   - Neutral capability descriptor: streaming, resume, mid_turn_approval, subagents, mcp, hooks
   - Static methods for codex_capabilities() and claude_capabilities()
   - Can be queried from snapshot so UI/dashboard shows honest support
   - Added ProviderCapabilities type to harness-core

---

## Stage 1: Add Skill Resolver (Neutral)

**Goal**: Implement skill reference resolution from `.agents/skills/<id>/SKILL.md`

**Success Criteria**: ✓ COMPLETE
- Skill resolver reads SKILL.md files (via `skill_resolver::resolve_skill`)
- Maps skill_refs -> file path + content
- Fail fast on dangling refs with clear error messages
- Unit tests for happy path + missing skill error

**Tests**: ✓ PASSING
- Resolve valid skill_ref
- Error on missing skill

**Status**: Complete

---

## Stage 2: Add Optional MCP Field to Member + build_launch_spec

**Goal**: Make MCP attachment declarative on members; carry it through launch spec

**Success Criteria**: ✓ COMPLETE
- AgentProviderConfig.mcp: Option<LaunchMcp> added (additive)
- build_launch_spec populates LaunchSpec.mcp from member
- Existing data (mcp: None) validates unchanged
- Unit tests for spec composition

**Tests**: ✓ PASSING
- Member with MCP -> LaunchSpec.mcp populated
- Member without MCP -> LaunchSpec.mcp = None
- MCP servers in spec match member declaration

**Status**: Complete

---

## Stage 3: Provider Capability Declaration Type + Table

**Goal**: Add ProviderCapabilities type and static tables for codex/claude

**Success Criteria**: ✓ COMPLETE
- ProviderCapabilities type in harness-core (streaming, resume, mid_turn_approval, subagents, mcp, hooks)
- Static fn for codex_capabilities() -> ProviderCapabilities
- Static fn for claude_capabilities() -> ProviderCapabilities
- Can be queried from snapshot / included in member view
- Unit tests for shape + dispatch

**Tests**: ✓ PASSING
- Codex capabilities match doc table (streaming=yes, resume=yes, mid_turn_approval=no, ...)
- Claude capabilities match doc table
- Serde round-trip
- Display format shows enabled features
- supports_streaming_exec() check works

**Status**: Complete

---

## Stage 4: Skill Injection Contract + Check

**Goal**: Document where resolved skills are injected; add check for dangling refs

**Success Criteria**: ✓ COMPLETE
- Extended scripts/check-skills.mjs to verify skill_refs in member JSON resolve to existing skills
- Documented injection method in agent-integration-model.md (codex: skill input; claude: system prompt)
- Check runs green with no dangling refs
- Errors include clear path to skill fix

**Tests**: ✓ PASSING
- Agent member with valid skill_refs passes check
- check:skills reports "checked 4 skills and validated all skill_refs in member records"

**Status**: Complete

---

## Stage 5: Update Documentation

**Goal**: Move Pillar 1/2/3 sections from PROPOSED to Specified

**Success Criteria**: ✓ COMPLETE
- agent-integration-model.md updated: skill contract, MCP shape, provider capabilities now real/specified
- Shapes match implemented types
- Links pass check:links (92 files checked)
- Registry updated if needed

**Status**: Complete

---

## Gate Results

```
cargo test --lib ✓ 34 tests passed
npx pnpm@9.15.4 check ✓ all checks green
- validate:json ✓ 77 files
- check:schema-fixtures ✓ 24 valid, 20 invalid
- check:tool-descriptors ✓ 11 descriptors
- check:links ✓ 92 markdown files
- check:doc-size ✓ docs checked
- check:skills ✓ 4 skills + all skill_refs validated
- check:doc-governance ✓ registry valid
- check:dashboard ✓ built successfully
```

All tests pass WITHOUT a live provider binary (unit tests use representative fixtures).

---

## Additive Schema Policy (ADR 0017) - VERIFIED

- NO breaking changes to existing data/fixtures ✓
- Existing records (mcp: None, capability declarations absent) validate unchanged ✓
- Tests include round-trip of both old and new data shapes ✓
- MCP field added to AgentProviderConfig with #[serde(default)] ✓

---

## Files Changed

1. `crates/harness-core/src/lib.rs`:
   - Added `mcp: Option<LaunchMcp>` field to `AgentProviderConfig`
   - Updated `build_launch_spec` to carry mcp from provider_config
   - Added `skill_resolver` module (ResolvedSkill, SkillResolutionError, resolve_skill, resolve_skills)
   - Added `ProviderCapabilities` struct with codex_exec() and claude_exec() implementations
   - Added 10 unit tests for MCP + skill resolver + provider capabilities

2. `crates/harness-cli/src/main.rs`:
   - Updated two AgentProviderConfig constructors to include `mcp: None` field

3. `scripts/check-skills.mjs`:
   - Extended to validate skill_refs in member JSON files
   - Fail fast on dangling skill_refs with clear error messages

4. `docs/agent-integration-model.md`:
   - Updated Pillar 1 "Skills" section: marked as WP-6 implemented
   - Updated Pillar 2 "MCP integration" section: marked as WP-6 implemented
   - Updated Pillar 3 "Provider capability declaration" section: marked as WP-6 implemented
   - Updated "Open Gaps Flagged by This Model" table to reflect closed gaps

---

## Implementation Summary

**Skill Resolver**:
- Synchronous function to resolve skill_refs to SKILL.md content
- Used by providers to inject skills into launch spec
- Error type with Display impl for clear error messages
- No IO errors on missing skills, only descriptive SkillResolutionError

**MCP Neutral Block**:
- LaunchMcp and LaunchMcpServer types already existed in launch spec
- Now populated from AgentProviderConfig.mcp field (additive)
- build_launch_spec carries it to neutral spec
- Providers can map it to their own MCP config format (Codex --config, Claude --mcp-config)

**Provider Capabilities**:
- Struct with 6 boolean capabilities per capability declaration table
- Static methods per provider: codex_exec(), claude_exec()
- Display impl to show enabled features
- Helper method supports_streaming_exec() for validation

All changes maintain provider-neutral semantics (ADR 0011) and additive-optional policy (ADR 0017).
