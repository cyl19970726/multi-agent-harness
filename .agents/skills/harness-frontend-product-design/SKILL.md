---
name: harness-frontend-product-design
description: "Orchestrate frontend product design, implementation, and visual acceptance through independent subagents or multiple sessions. Use when a frontend task needs product-source research, page discovery, divergent UX/visual concepts, critical review, responsive layout contracts, screenshot-guided implementation, or independent browser acceptance."
---

# Frontend Product Design Harness

Run frontend product work as a multi-session design process. Keep project
meaning in the project's own sources; keep this skill limited to collaboration,
artifact, handoff, and acceptance methods.

## Hard Boundary

Do not embed or infer a specific project's:

- product objects, lifecycle, organization model, or vocabulary;
- canonical pages, routes, modules, or navigation;
- storage schema, command line, task system, provider, or framework;
- repository paths or document-system hierarchy.

Load those facts at runtime from user-provided references, authoritative product
sources, the current repository, and the working application. Cite the sources
inside the design artifacts. If sources conflict, report the conflict instead of
creating a new product definition in the skill.

## Operating Model

Use a Host session to coordinate independent working contexts. Prefer fresh
subagents for bounded parallel work and fresh persistent sessions when a role
needs multiple rounds or browser interaction.

Minimum roles for material design work:

- Product Researcher: reconstructs user outcome, workflows, constraints, and
  authority from raw sources.
- UX/IA Designer: discovers pages and interaction structure from workflows.
- Visual Designer: proposes concrete responsive compositions.
- Critic: challenges assumptions, missing states, usability, and feasibility.
- Implementer: changes the product and produces exact-revision evidence.
- Independent Reviewer: judges the frozen reference against the working UI.

Combine research roles for small work. Keep Implementer and Independent
Reviewer in different execution contexts for any acceptance claim. A renamed
role inside the same context is not independent.

For high-risk work, split Independent Reviewer into two fresh contexts:

- Product Reviewer: judges workflow, comprehension, and product correctness.
- Usability/Visual Reviewer: operates the UI and judges interaction, responsive
  behavior, and reference fidelity.

Have them submit independently before either reads the other's conclusion.

## Context Isolation

Give each role only what it needs:

- Researchers receive raw product sources and the user request.
- Designers receive the research brief, references, constraints, and relevant
  existing UI—not the Host's preferred answer.
- Critics receive candidate artifacts and source refs—not a recommendation.
- Implementers receive the selected design and acceptance contract.
- Reviewers receive the frozen reference, exact-revision screenshots or live
  URL, declared deviations, and acceptance contract—not implementation effort,
  test success, or completion persuasion.

Record a context id or session/thread reference for every material contribution.
Do not present parallel outputs as independent if they share one reasoning
history.

## Artifact Envelope

Keep the protocol storage-neutral. Each material artifact should identify:

```text
artifact type and round:
author role and context id:
status: draft | submitted | accepted | rejected | superseded | blocked
input refs:
scope and non-goals:
known facts:
inferences:
unknowns:
claims and evidence refs:
findings and open questions:
requested next action:
supersedes:
```

The Host may store these artifacts wherever the project records design work.
Do not require a particular database, task system, file path, or id format.

## Workflow

### 1. Build the source packet

The Host gathers:

- user request and success criteria;
- authoritative product sources and revision markers;
- approved visual references, if any;
- relevant implementation routes and representative data;
- technical, accessibility, responsive, and delivery constraints;
- known unknowns and conflicting sources.

Do not ask designers to reverse-engineer product truth from the current
component tree when better authority exists.

### 2. Run independent discovery

Dispatch Product Researcher and UX/IA Designer independently when scope is
unclear or structural. Their outputs must state source refs, assumptions, user
workflows, primary questions, failure modes, and proposed page/surface
boundaries.

The Host reconciles them into a discovery brief without silently resolving
material product conflicts. Escalate a missing authority decision when it would
change the product.

### 3. Generate divergent concepts

Ask two or three Visual Designer contexts for materially different solutions.
Vary the design premise, information hierarchy, interaction model, density, or
responsive strategy—not merely color, radius, or spacing.

Each concept must include:

- design premise and target workflow;
- page/surface map derived from the source packet;
- desktop, tablet, and mobile composition;
- first-viewport hierarchy and primary action;
- loading, empty, warning, error, and long-content behavior;
- accessibility and keyboard implications;
- data/read-model needs and implementation risks;
- explicit departures from the supplied reference.

For a localized change, one concept plus one independent challenge can be
enough. Do not force three concepts when the decision is already constrained.

### 4. Run blind critique

Give the Critic raw candidate artifacts, discovery brief, and sources. Ask for:

- a rubric defined before scoring;
- unsupported assumptions and missing workflows;
- hierarchy, comprehension, navigation, responsive, accessibility, and
  feasibility findings;
- P0/P1/P2 risks with evidence;
- useful parts worth preserving from rejected concepts;
- a recommendation or a request for another concept round.

The Critic must not know which option the Host or user prefers.

### 5. Select and contract

The Host records:

```text
selected concept:
why it won:
parts borrowed from other concepts:
rejected concepts and reasons:
remaining weaknesses:
authority and reference refs:
open product decisions:
```

Then create a page-local implementation contract for each changed surface:

```text
surface and route:
target user workflow:
responsive diagrams:
first viewport:
regions and dimensions:
scroll ownership:
interaction and keyboard order:
state matrix:
data density and wrapping rules:
component and read-model needs:
approved deviations:
screenshot viewports and scenarios:
observable pass/fail conditions:
```

Use ASCII box diagrams when the contract must travel reliably through Markdown,
terminals, and agent transcripts. A list of components is not a layout contract.

### 6. Decide implementation architecture

Have an Architect or the Implementer inspect the actual repository and propose
the narrowest viable architecture. Record routing, state/read-model boundary,
component ownership, styling approach, accessibility strategy, dependencies,
and old-code disposition. Do not freeze a technology stack in this skill.

### 7. Implement through screenshot checkpoints

The Implementer works in slices:

```text
shell or high-risk slice
  -> engineering checks
  -> representative browser state
  -> exact-revision screenshot
  -> compare with contract/reference
  -> repair or return to design
  -> next slice
```

Capture the first structural screenshot early. Stop patching and return to
design when the result reveals a systemic composition failure, inaccessible
interaction, uncontrolled overflow, a raw/debug-first experience, or a contract
too vague to judge.

### 8. Self-review before handoff

The Implementer completes the declared scope and freezes one evidence bundle:

- exact implementation revision;
- reference and contract revisions;
- route, viewport, and representative-state screenshots;
- comparison findings;
- console, overflow, keyboard, accessibility, and relevant engineering checks;
- declared deviations and unresolved findings.

Resolve all P0/P1 self-review findings before requesting independent review.
Self-review grants eligibility to review; it is not acceptance.

### 9. Run independent visual and product review

Create a fresh Reviewer context. Require one observation for every frozen
screenshot or tested scenario. Each defect must include:

```text
severity:
observed result:
screenshot or browser evidence:
reference/contract comparison:
likely causal layer:
specific repair:
observable recheck condition:
```

Require a page-level systemic diagnosis in addition to local defects. Reject
verdict-only reviews, generic polish requests, and acceptance justified only by
passing tests or component presence.

Run separate product/usability and visual-fidelity reviewers when either axis is
high risk. Both reconstruct their checklist from the source packet; neither uses
a product checklist stored in this skill.

### 10. Iterate and close

Route review findings back to the appropriate context:

- source or workflow ambiguity -> Product Researcher / user;
- page boundary or interaction failure -> UX/IA Designer;
- composition or visual-system failure -> Visual Designer;
- implementation mismatch -> Implementer;
- weak acceptance contract -> Host and Critic.

Freeze a new evidence bundle after changes. Do not reuse a previous PASS across
a changed revision or reference.

Close with the selected/rejected decisions, final evidence, remaining waivers,
and reusable process learning. Store them where the project keeps design and
delivery records; this skill does not prescribe a repository path.

## Stop Conditions

Stop and ask for authority or user direction when:

- product sources conflict on a decision that changes the UX;
- required reference assets are missing;
- the target route or representative state cannot be run;
- a destructive architecture change exceeds the authorized scope.

Stop implementation and return to design when:

- screenshots cannot be judged against an explicit contract;
- local fixes repeat without correcting a page-level composition problem;
- mobile/tablet behavior becomes an unstructured stack;
- implementation structure, rather than user workflow, dictates the design.

Do not claim independent acceptance when no separate execution context was
available. Report implementer self-review instead.

## Quality Standard

Judge screenshots across:

1. product legibility and action confidence;
2. composition and dominant work surface;
3. density, spacing, typography, and alignment rhythm;
4. surface hierarchy, color restraint, and control proportions;
5. responsive transformation, overflow, focus, and state behavior;
6. fidelity to the approved reference and declared deviations.

Rendered data, a clean console, passing tests, accessible primitives, or a
completed checklist cannot alone establish frontend product quality.
