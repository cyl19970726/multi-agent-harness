# ADR 0053: Finance contract-layer retirement

**Date:** 2026-08-05
**Status:** accepted
**Issue:** [#323](https://github.com/cyl19970726/multi-agent-harness-company-skills/issues/323)

## Decision

Retire the Finance operator skill and governance role from what was then the
active contract layer of the legacy Company OS (that whole layer is now retired
by DOC-108). The append-only Commitment/Payment code remains dormant; the
smoke script is preserved as historical evidence.

## Context

The Finance slice (`company-finance-operator` skill, Finance Governance Agent,
Finance CLI smoke) was implemented as a baseline operator path in the
retired Company OS contract layer. It was not needed by any dogfood scenario,
and maintaining it in the contract layer created documentation and acceptance
drag without proportional value.

## Consequences

- `company-finance-operator` skill parked at `skills/parked-company-finance-operator-20260805/`
- Finance removed from the eight-Skill legacy Company OS operator suite
- Finance Governance Agent row marked as parked in governance.md and
  governance-agent-workspaces.md
- Finance CLI smoke script preserved with a retirement header
- `.governance.toml` vocab note defers finance vocabulary retirement until code
  decommission
- Finance vocabulary is not added to `retired_vocabulary.terms` while append-only
  code still exists

## Reversibility

Reversing this decision requires:
1. Un-parking the skill directory
2. Restoring the skill to install-skill.sh, acceptance-skill-install.sh, and
   skill-contracts.md
3. Restoring the Finance Governance row in governance-agent-workspaces.md to
   active status
4. Reverting the `.governance.toml` comment
