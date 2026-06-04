# Internal Workflow Catalog — full classification (all 62 records)

Companion to **[internal-workflow-catalog.md](internal-workflow-catalog.md)** (12 deep dives, marked ★ below) and **[external-workflow-gap-analysis.md](external-workflow-gap-analysis.md)** (the gaps A–E).

This is the **complete corpus**, classified by a workflow — dogfood: *a workflow classifying workflows* (`classify-all-internal-workflows`, run `wf_295b8c34-aea`: scout-enumerate → 16 parallel analysis batches → rollup; 18 agents, ~3 min). 62 unique records (53 completed, 2 failed, 7 killed).

## Distribution

| family | count |
| --- | --- |
| Closed-loop build (act→verify→repair) | 12 |
| Plan-first / decision-gated | 1 |
| Adversarial review | 5 |
| Serial gated delivery | 12 |
| Research / map-reduce | 23 |
| Single-agent gated | 9 |

- **planning**: static 55 · hybrid 4 · dynamic-decomposition 3  → planning is *authored*, not agent-generated, in 89% of runs.
- **schema-gated handoffs**: 26/62 (42%) — structured output is how the sophisticated half gates control flow.

## Master index (all 62)

Sorted by family, then duration. ★ = has a deep dive in the companion catalog (with full source).

| family | ★ | runid | name | st | ag | min | plan | schema | gap |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| closed-loop-build | [★1](internal-workflow-catalog.md) | `wf_f6900ec3-4f6` | dynamic-workflow-impl | comp | 5 | 79.3 | S | ✓ | A |
| closed-loop-build |  | `wf_d40102da-d58` | starlark-only-and-observability | comp | 4 | 55.2 | S | ✓ | A |
| closed-loop-build |  | `wf_4bb51aaf-3f9` | workflow-structured-output | comp | 2 | 43.6 | S | ✓ | A |
| closed-loop-build |  | `wf_cc39b98e-771` | ephemeral-worker-refactor | comp | 2 | 27.2 | S | ✓ | A |
| closed-loop-build |  | `wf_366d62b1-93b` | workflow-surface-impl | comp | 3 | 25.1 | S | ✓ | A |
| closed-loop-build |  | `wf_fd2c5e65-ae8` | workflow-observability-backfill | comp | 3 | 23.9 | S | ✓ | A |
| closed-loop-build | [★2](internal-workflow-catalog.md) | `wf_07defa4e-b3f` | resident-daemon | comp | 5 | 20 | S | ✓ | A |
| closed-loop-build |  | `wf_4ead6431-ae7` | starlark-gap-fixes | comp | 3 | 14.4 | S | ✓ | A |
| closed-loop-build |  | `wf_27246c79-af9` | starlark-workflow-frontend | comp | 7 | 11.7 | S | ✓ | A |
| closed-loop-build |  | `wf_f6d4725a-87c` | ephemeral-worker-refactor | kill | 1 | 1.9 | S | ✓ | A |
| closed-loop-build |  | `wf_f056ea75-2ad` | workflow-surface | kill | 1 | 1.4 | H | ✓ | A |
| closed-loop-build |  | `wf_c85fa40f-c78` | ephemeral-worker-refactor | kill | 1 | 1.3 | S | ✓ | A |
| plan-first-decision-gated | [★3](internal-workflow-catalog.md) | `wf_fbda5429-b57` | resident-agent-impl | comp | 6 | 22.2 | H | ✓ | C |
| adversarial-review |  | `wf_12073ee5-627` | live-tui-stageb-review | fail | 15 | 22.9 | D | ✓ | dyn |
| adversarial-review | [★9](internal-workflow-catalog.md) | `wf_961f46bb-0f7` | evaluate-external-workflow | comp | 9 | 6.5 | S | ✓ | contrast |
| adversarial-review |  | `wf_54170927-b26` | chat-redesign-review | comp | 36 | 6.3 | D | ✓ | dyn |
| adversarial-review |  | `wf_a6c24f10-9e2` | align-author-skill | comp | 7 | 6 | S | ✓ | contrast |
| adversarial-review | [★5](internal-workflow-catalog.md) | `wf_f86139b9-018` | multica-layout-review | comp | 26 | 5.7 | D | ✓ | dyn |
| serial-gated-delivery | [★7](internal-workflow-catalog.md) | `wf_6eabf27a-e1b` | generic-harness-object-model-build | comp | 7 | 98.2 | S | · | E |
| serial-gated-delivery |  | `wf_50711139-7db` | member-app-plus-sse | comp | 4 | 79.5 | S | · | E |
| serial-gated-delivery | [★11](internal-workflow-catalog.md) | `wf_b0be908d-0c2` | operator-drives-team | comp | 5 | 61.8 | S | · | E |
| serial-gated-delivery |  | `wf_36d72f9a-77c` | agents-replace-team-member | comp | 3 | 54.3 | S | · | E |
| serial-gated-delivery |  | `wf_b7f05614-030` | exec-stream-finalize-and-close-loop | comp | 5 | 54.2 | S | · | E |
| serial-gated-delivery | [★8](internal-workflow-catalog.md) | `wf_56fc2f22-2d1` | member-lead-claude-build | comp | 8 | 53.4 | S | · | E |
| serial-gated-delivery |  | `wf_fb6c49f9-427` | workflow-runtime-wp2 | comp | 2 | 42.3 | S | · | E |
| serial-gated-delivery |  | `wf_43942bc5-f82` | tier1-session-resume-and-launch-flags | comp | 3 | 30.5 | S | · | E |
| serial-gated-delivery |  | `wf_1265b38e-da9` | exec-stream-substrate-v2 | comp | 4 | 29.3 | S | · | E |
| serial-gated-delivery |  | `wf_290e929e-4af` | workflow-runtime-wp1 | comp | 2 | 23.1 | S | · | E |
| serial-gated-delivery |  | `wf_1e2f1ddf-9b8` | close-the-loop | kill | 2 | 7.8 | H | · | E |
| serial-gated-delivery |  | `wf_ed8e34af-fa6` | exec-stream-substrate | kill | 1 | 1.8 | S | · | E |
| research-mapreduce | [★6](internal-workflow-catalog.md) | `wf_e37ff5d7-20c` | generic-harness-object-model-research | comp | 6 | 61.2 | H | · | D |
| research-mapreduce |  | `wf_22ce8232-535` | task-goal-model-research | comp | 5 | 16.9 | S | · | D |
| research-mapreduce |  | `wf_1dda9269-040` | agent-teams-architecture-diagrams | comp | 3 | 16.3 | S | · | D |
| research-mapreduce |  | `wf_2b1cd5d5-73d` | workbench-page-and-docs-plan | comp | 4 | 13.3 | S | · | D |
| research-mapreduce |  | `wf_ba08ac4e-b94` | agent-teams-architecture-research | comp | 4 | 11.3 | S | · | D |
| research-mapreduce | [★4](internal-workflow-catalog.md) | `wf_f91ff6a5-fda` | workflow-layout-design | comp | 6 | 11.1 | S | ✓ | contrast |
| research-mapreduce |  | `wf_2c42a249-ac9` | mention-and-doc-reference-design | comp | 3 | 10.8 | S | · | D |
| research-mapreduce |  | `wf_0b588f68-81d` | cc-agent-teams-tmux-verification | comp | 3 | 8.5 | S | · | D |
| research-mapreduce |  | `wf_2fda87bc-9b0` | agents-multica-layout-design | comp | 8 | 8.4 | S | ✓ | D |
| research-mapreduce |  | `wf_c8e26cd5-414` | claude-integration-architecture-writeup | comp | 3 | 8.4 | S | · | D |
| research-mapreduce |  | `wf_b302e7ce-0a6` | vision-vs-implementation-gap | comp | 3 | 7.6 | S | · | D |
| research-mapreduce | [★10](internal-workflow-catalog.md) | `wf_a8874e2e-42a` | agent-live-tui-design | comp | 8 | 7.5 | S | ✓ | D |
| research-mapreduce |  | `wf_00be27b8-fbd` | member-lead-claude-integration-review | comp | 5 | 6.3 | S | · | D |
| research-mapreduce |  | `wf_fcbd031c-817` | can-agents-use-workflow-skill | comp | 4 | 6.3 | S | ✓ | D |
| research-mapreduce |  | `wf_e2175003-946` | xhs-wenchuang-report-research | comp | 4 | 5.3 | S | · | — |
| research-mapreduce |  | `wf_d64b7b81-0c7` | operator-interaction-basics | comp | 3 | 5.3 | S | · | D |
| research-mapreduce |  | `wf_bb017c9b-c7f` | exec-mode-architecture-review | comp | 3 | 5.2 | S | · | D |
| research-mapreduce |  | `wf_ca82faa8-2ce` | agent-mention-roadmap-audit | comp | 5 | 4.5 | S | ✓ | D |
| research-mapreduce | [★12](internal-workflow-catalog.md) | `wf_d792276e-f39` | xhs-market-research-routes | comp | 5 | 3.9 | S | · | D |
| research-mapreduce |  | `wf_6954e43f-d27` | orchestration-plugin-design-v2 | comp | 3 | 3.3 | S | ✓ | C |
| research-mapreduce |  | `wf_4b37538f-3d1` | cli-reference-verification | comp | 4 | 2.8 | S | · | D |
| research-mapreduce |  | `wf_c112d3e9-ee6` | workflow-plugin-design | kill | 4 | 0.6 | S | ✓ | D |
| research-mapreduce |  | `wf_f47ddc8f-ed7` | workflow-plugin-design | fail | 0 | 0 | S | ✓ | D |
| single-agent-gated |  | `wf_6a021088-fe2` | workbench-cleanup-and-wp1 | comp | 2 | 22.5 | S | · | E |
| single-agent-gated |  | `wf_edccc0cf-bfe` | fix-clippy-fmt-pr51 | comp | 1 | 17.7 | S | · | E |
| single-agent-gated |  | `wf_a47a7ef9-430` | rebase-pr51-vision-goal-task | comp | 1 | 10 | S | · | E |
| single-agent-gated |  | `wf_a4aafe15-7f2` | member-chat-app-redesign | kill | 1 | 9.7 | S | · | E |
| single-agent-gated |  | `wf_5ced66a7-ea6` | cc-agent-teams-tmux-verification | comp | 3 | 8.3 | S | · | E |
| single-agent-gated |  | `wf_d9299c5b-8d6` | rust-workflow-runtime-design | comp | 4 | 8.2 | S | · | E |
| single-agent-gated |  | `wf_03bdf82f-24b` | agent-integration-model-doc | comp | 1 | 6.5 | S | · | E |
| single-agent-gated |  | `wf_c646e8a3-7ff` | fix-provider-tests-merge-pr51 | comp | 1 | 5.7 | S | · | E |
| single-agent-gated |  | `wf_78ad9884-e42` | member-runtime-observability-doc | comp | 1 | 5 | S | · | E |

## By family — every record with its transferable idiom

### Closed-loop build (act→verify→repair) (12)

> Serial build stages each return a typed STAGE_RESULT.ok (or VERIFY) schema as the gate; on !ok exactly one repair pass runs with prior blockers injected, and a still-red stage stops-on-fail for human review—often wrapping an adversarial review and a bounded verify->fix loop.

- **dynamic-workflow-impl** ★1 · `wf_f6900ec3-4f6` · 5 agents · 79.3m · completed · plan=static · gap=A
  - *what:* 5 sequential stages (extract harness-workflow crate + JSON-IR/CLI -> streaming pipeline -> author skill -> dashboard drill-in -> e2e acceptance), each emitting a STAGE_RESULT.ok schema gate with a single repair pass and stop-on-fail.
  - *idiom:* Drive serial stages through a for-loop where each agent returns a typed STAGE_RESULT{ok, verification, blockers}; on ok=false run exactly one repair pass, and break the chain (stop-on-fail) if it still can't go green so a human can intervene.
  - *patterns:* verify-loop, STAGE_RESULT.ok-gate, schema-gated-handoff, GATE_FAILED-cascade
- **starlark-only-and-observability** · `wf_d40102da-d58` · 4 agents · 55.2m · completed · plan=static · gap=A
  - *what:* Three serial build stages (Starlark-only, obs capture, obs UI) each gated by STAGE_RESULT.ok with a one-shot repair pass and stop-on-fail.
  - *idiom:* Each serial stage returns a STAGE_RESULT.ok schema; on ok=false a single repair pass runs, and a still-failing stage stops the whole run for human review.
  - *patterns:* verify-loop, STAGE_RESULT-gate, schema-gated-handoff, GATE_FAILED-cascade
- **workflow-structured-output** · `wf_4bb51aaf-3f9` · 2 agents · 43.6m · completed · plan=static · gap=A
  - *what:* Two serial stages (runtime + skill) each returning a STAGE_RESULT schema; ok=false triggers one repair pass then stop, CI commands as the gate.
  - *idiom:* Drive serial stages through a typed STAGE_RESULT schema whose ok boolean is the gate: on !ok run exactly one repair pass with the prior blockers injected, then break (stop-on-fail) so a red stage never advances.
  - *patterns:* verify-loop, schema-gated-handoff, GATE_FAILED-cascade, real-acceptance
- **ephemeral-worker-refactor** · `wf_cc39b98e-771` · 2 agents · 27.2m · completed · plan=static · gap=A
  - *what:* 2 serial stages (contract refactor then real ephemeral spawn driver) each emit a STAGE_RESULT.ok schema; on !ok one repair pass runs, and a failed stage breaks the loop for human review.
  - *idiom:* Drive serial implementation stages through a STAGE_RESULT.ok schema gate with exactly one automatic repair pass per stage and a hard stop-on-fail break, so broken work never propagates downstream.
  - *patterns:* verify-loop, schema-gated-handoff, GATE_FAILED-cascade
- **workflow-surface-impl** · `wf_366d62b1-93b` · 3 agents · 25.1m · completed · plan=static · gap=A
  - *what:* Implement read-only Workflow surface, then adversarial review (severity-filtered findings drive a fix), then up to 6 verify->fix iterations gated on a backend/frontend/assets/read-only VERIFY schema until green.
  - *idiom:* Sandwich one implement agent between a severity-filtered adversarial REVIEW schema (critical/high gate a fix) and a bounded verify->fix loop (<=6 attempts on a multi-bool VERIFY schema) so the build only finishes when the CI-equivalent gate is genuinely green.
  - *patterns:* verify-loop, adversarial-verify, schema-gated-handoff, judge-synthesis
- **workflow-observability-backfill** · `wf_fd2c5e65-ae8` · 3 agents · 23.9m · completed · plan=static · gap=A
  - *what:* 3 serial stages (persist initiated_by+spec / expose persisted turn events / dashboard backfill) each gated on STAGE_RESULT.ok with one repair pass and a stop-on-fail break.
  - *idiom:* Run a sequential Backend->Api->Frontend chain where each stage self-gates on a STAGE_RESULT.ok schema, gets one repair pass, and stop-on-fail halts the pipeline so a green commit is required before the next stage builds on it.
  - *patterns:* verify-loop, schema-gated-handoff, GATE_FAILED-cascade
- **resident-daemon** ★2 · `wf_07defa4e-b3f` · 5 agents · 20m · completed · plan=static · gap=A
  - *what:* Builds a Unix-socket resident daemon hosting the ResidentPool: typed Design -> Implement, adversarial typed-findings review, conditional Fix on blocking findings, then a 6-attempt build/test/clippy verify-fix loop.
  - *idiom:* A typed Design handoff into Implement, then an adversarial typed-findings review whose critical/high subset conditionally triggers a Fix agent, all closed by a schema-gated verify->fix loop.
  - *patterns:* schema-gated-handoff, adversarial-verify, verify-loop, GATE_FAILED-cascade
- **starlark-gap-fixes** · `wf_4ead6431-ae7` · 3 agents · 14.4m · completed · plan=static · gap=A
  - *what:* 3 serial agents fix 8 Starlark front-end/driver gaps (core crate, then CLI, then full-workspace gate), each emitting a schema'd gate_passed result and gating on cargo fmt+clippy+test+pnpm.
  - *idiom:* Serial fix stages each return a FIX schema with a gate_passed boolean and run an unpiped per-crate fmt+clippy+test gate before the next stage, with a final full-workspace gate agent confirming the whole repo is green.
  - *patterns:* verify-loop, schema-gated-handoff, real-gate, pipeline-no-barrier
- **starlark-workflow-frontend** · `wf_27246c79-af9` · 7 agents · 11.7m · completed · plan=static · gap=A
  - *what:* Serial Core->Integrate stages (each schema-gated on cargo fmt+clippy+test) then parallel artifacts then a full-repo acceptance gate.
  - *idiom:* Each serial implementation stage must self-prove against an UNPIPED fmt+clippy+test gate and report a STAGE schema (compiled/tests_passed/gate_passed) before the next stage proceeds, with a final full-repo gate as real acceptance.
  - *patterns:* verify-loop, schema-gated-handoff, parallel-barrier, real-acceptance
- **ephemeral-worker-refactor** · `wf_f6d4725a-87c` · 1 agents · 1.9m · killed · plan=static · gap=A
  - *what:* Two serial implementation stages (provider-contract refactor then real ephemeral spawn driver), each schema-gated on STAGE_RESULT.ok with a single repair pass and stop-on-fail.
  - *idiom:* Each serial stage returns a typed STAGE_RESULT{ok,...}; ok=false triggers exactly one repair pass carrying the prior blockers, and a still-failing stage breaks the loop to stop for human review.
  - *patterns:* verify-loop, schema-gated-handoff, GATE_FAILED-cascade, STAGE_RESULT.ok-gate, stop-on-fail
- **workflow-surface** · `wf_f056ea75-2ad` · 1 agents · 1.4m · killed · plan=hybrid · gap=A
  - *what:* Typed design -> implement -> adversarial review -> blocking fix -> up-to-6-attempt verify/fix loop gated on a VERIFY schema, building a read-only dashboard Workflow surface.
  - *idiom:* A typed DESIGN schema is injected verbatim into the implementer as ground truth, an adversarial REVIEW (typed findings, blocking-only fix) precedes a bounded verify->fix loop (<=6 attempts) gated on a typed VERIFY{backend_ok,frontend_ok,assets_regenerated} until green.
  - *patterns:* verify-loop, schema-gated-handoff, C-leading-plan, adversarial-verify, stop-on-green
- **ephemeral-worker-refactor** · `wf_c85fa40f-c78` · 1 agents · 1.3m · killed · plan=static · gap=A
  - *what:* Two-stage Rust refactor (provider node contract, then real ephemeral spawn driver) with per-stage verify gate, one repair retry, and stop-on-fail.
  - *idiom:* A serial STAGES loop where each stage emits a typed STAGE_RESULT{ok}, gets exactly one repair pass on ok=false, and a failed gate breaks the loop to stop downstream stages.
  - *patterns:* verify-loop, schema-gated-handoff, GATE_FAILED-cascade, real-acceptance

### Plan-first / decision-gated (1)

> Parallel typed probes feed a typed Decide agent emitting a winner enum that, with a code-map, is injected as ground truth into the implementer, then closed by a schema-gated verify->fix loop.

- **resident-agent-impl** ★3 · `wf_fbda5429-b57` · 6 agents · 22.2m · completed · plan=hybrid · gap=C
  - *what:* Three parallel schema-typed probes (codex exec-server vs app-server vs claude code-map) feed a typed winner-enum decision injected into a resident-claude implementation, closed by a 5-attempt VERIFY-schema build/test/clippy fix loop.
  - *idiom:* Parallel typed probes feed a typed Decide agent emitting a winner enum, which plus a typed code-map is injected as ground truth into the implement prompt, then a schema-gated verify->fix loop closes the build.
  - *patterns:* parallel-barrier, schema-gated-handoff, verify-loop, GATE_FAILED-cascade

### Adversarial review (5)

> Review dimensions emit typed findings, then each finding is adversarially refuted/verified (default real=false, one verifier per finding) before a judge synthesizes only confirmed findings into a prioritized must/should/optional fix list.

- **live-tui-stageb-review** · `wf_12073ee5-627` · 15 agents · 22.9m · failed · plan=dynamic-decomposition · gap=dyn
  - *what:* Parallel backend+frontend adversarial review of a Stage-B SSE diff -> one verifier per finding -> prioritized fix plan; failed when schema-forced reviewers never called StructuredOutput.
  - *idiom:* Parallel schema-typed review dimensions emit findings, then spawn exactly one adversarial verifier per finding (fan-out width = runtime finding count, default real=false) before a synthesizer triages must/should/optional.
  - *patterns:* parallel-barrier, adversarial-verify, GATE_FAILED-cascade, schema-gated-handoff, judge-synthesis
- **evaluate-external-workflow** ★9 · `wf_961f46bb-0f7` · 9 agents · 6.5m · completed · plan=static · gap=contrast
  - *what:* 4 evaluation dimensions each assessed (typed) then adversarially refuted (typed), then synthesized into a verified production-readiness scorecard + roadmap.
  - *idiom:* Pipeline each review dimension through assess(schema)->adversarial refute(schema)->attach verdict, then synthesize honoring holds=false corrections into a prioritized roadmap.
  - *patterns:* pipeline-no-barrier, adversarial-verify, schema-gated-handoff, judge-synthesis, real-acceptance
- **chat-redesign-review** · `wf_54170927-b26` · 36 agents · 6.3m · completed · plan=dynamic-decomposition · gap=dyn
  - *what:* Parallel multi-dimension review of an uncommitted chat-redesign diff, each finding adversarially verified then synthesized into a prioritized fix list.
  - *idiom:* Fan out fixed review dimensions, then spawn exactly one adversarial verifier per emitted finding (count set by runtime output) before a judge triages into must/should/optional.
  - *patterns:* parallel-barrier, adversarial-verify, one-verifier-per-finding, judge-synthesis, schema-gated-handoff
- **align-author-skill** · `wf_a6c24f10-9e2` · 7 agents · 6m · completed · plan=static · gap=contrast
  - *what:* Three diverse lenses critique the author-workflow skill for naivety, a verifier rejects overstated gaps, and confirmed gaps become a prioritized edit list.
  - *idiom:* Run each critique lens through a paired critique->adversarial-verify pipeline (verifier rejects overstated gaps), then synthesize only verifier-CONFIRMED gaps into concrete edits.
  - *patterns:* pipeline-no-barrier, adversarial-verify, perspective-diverse-verify, judge-synthesis, schema-gated-handoff
- **multica-layout-review** ★5 · `wf_f86139b9-018` · 26 agents · 5.7m · completed · plan=dynamic-decomposition · gap=dyn
  - *what:* Parallel multi-dimension review of a committed Multica-layout branch diff, each finding adversarially verified then triaged into a prioritized fix list with a mergeable verdict.
  - *idiom:* Parallel review dimensions emit schema'd findings, then one adversarial verifier per finding (default real=false) filters before a judge produces a mergeability verdict.
  - *patterns:* parallel-barrier, adversarial-verify, one-verifier-per-finding, judge-synthesis, schema-gated-handoff

### Serial gated delivery (12)

> A linear chain of work-packages where each WP implements in an isolated worktree, self-gates on a real build/test wall (cargo+pnpm/CI), auto-merges to master on green, and emits a 'GATE_FAILED:' first line that early-aborts the cascade—typically capped by a from-zero real-binary acceptance agent proving observed behavior.

- **generic-harness-object-model-build** ★7 · `wf_6eabf27a-e1b` · 7 agents · 98.2m · completed · plan=static · gap=E
  - *what:* 7 sequential gated+auto-merged work-packages (schema spine -> Rust core -> Review/Gap/learning objects -> closeout gate -> docs) migrating to a generic object model, each gated on cargo test + pnpm check with GATE_FAILED early-abort.
  - *idiom:* Chain WPs where each implements, runs a hard build/test gate, auto-merges to master on green, and emits a 'GATE_FAILED:' first line that the orchestrator checks to abort the whole cascade before the next WP builds on broken work.
  - *patterns:* pipeline-no-barrier, GATE_FAILED-cascade, auto-merge, schema-gated-handoff
- **member-app-plus-sse** · `wf_50711139-7db` · 4 agents · 79.5m · completed · plan=static · gap=E
  - *what:* BE SSE stream -> FE member chat re-skin -> FE SSE consumption, each gated+worktree+auto-merged with GATE_FAILED cascade, capped by a real interactive browser acceptance agent verifying live SSE push and composer against the merged stack.
  - *idiom:* End a gated serial delivery chain with a real interactive browser acceptance agent that drives the live full stack, reads its own screenshots as evidence, and returns ACCEPT_PASS/ACCEPT_FAIL with fix-forward instead of trusting per-WP self-reports.
  - *patterns:* GATE_FAILED-cascade, auto-merge, worktree-isolation, real-acceptance, pipeline-no-barrier
- **operator-drives-team** ★11 · `wf_b0be908d-0c2` · 5 agents · 61.8m · completed · plan=static · gap=E
  - *what:* 4 gated+worktree+auto-merged WPs (operator sender_kind -> HTTP create routes -> dashboard affordances -> real delivery) enabling an external operator to drive the team, with GATE_FAILED cascade and a final real browser+CLI acceptance agent.
  - *idiom:* Sequence WPs that each demand a REAL-RUN proof (an actual CLI/HTTP call whose effect shows in the snapshot, never a hand-written fixture), gate+auto-merge each, and close with a real browser+CLI acceptance agent that returns ACCEPT_PASS/FAIL on observed snapshot deltas.
  - *patterns:* GATE_FAILED-cascade, auto-merge, worktree-isolation, real-acceptance
- **agents-replace-team-member** · `wf_36d72f9a-77c` · 3 agents · 54.3m · completed · plan=static · gap=E
  - *what:* Backend de-centers Team, frontend builds Agents area, then a from-zero real-binary acceptance agent verifies; each gates+CI-merges or aborts.
  - *idiom:* A BE->FE->acceptance chain where each step gates (fmt+clippy+test+pnpm), opens a PR, polls CI green and auto-merges, with a GATE_FAILED first-line abort propagating the cascade.
  - *patterns:* GATE_FAILED-cascade, pipeline-no-barrier, auto-merge, real-acceptance
- **exec-stream-finalize-and-close-loop** · `wf_b7f05614-030` · 5 agents · 54.2m · completed · plan=static · gap=E
  - *what:* Four sequential work packages (retire app-server, store/SSE, MCP+skill+capability, close-loop) each gated+auto-merged, then real-binary from-zero acceptance.
  - *idiom:* A four-WP chain (WP5->WP4->WP6->WP7) each gates(cargo test+pnpm check), worktree-merges, and a GATE_FAILED first line short-circuits the rest, capped by a from-zero real-binary acceptance closing the autonomy loop.
  - *patterns:* GATE_FAILED-cascade, pipeline-no-barrier, auto-merge, real-acceptance
- **member-lead-claude-build** ★8 · `wf_56fc2f22-2d1` · 8 agents · 53.4m · completed · plan=static · gap=E
  - *what:* Two parallel autonomous tracks (FE WP1-5, BE WP6-8), each WP gated + branch/PR/squash-merged off latest master with a GATE_FAILED cascade.
  - *idiom:* Run two independent serial work-package chains in parallel (Promise.all over async IIFEs), each WP gating-then-auto-merging onto master so the next WP builds on merged work, with a GATE_FAILED first-line early-abort per chain.
  - *patterns:* GATE_FAILED-cascade, auto-merge, pipeline-no-barrier, parallel-barrier, schema-gated-handoff
- **workflow-runtime-wp2** · `wf_fb6c49f9-427` · 2 agents · 42.3m · completed · plan=static · gap=E
  - *what:* One gated WP2 (scheduler/pipeline/SSE, CI-poll-then-merge, GATE_FAILED abort) followed by a from-zero real-binary acceptance agent proving live SSE + concurrency overlap.
  - *idiom:* End a gated implement-then-merge work package with a from-zero real-binary ACCEPTANCE agent that rebuilds an empty store, drives the live system (SSE frames during the run), and emits ACCEPT_PASS/ACCEPT_FAIL only on observed real behavior — never fixtures.
  - *patterns:* GATE_FAILED-cascade, auto-merge, real-acceptance, schema-gated-handoff
- **tier1-session-resume-and-launch-flags** · `wf_43942bc5-f82` · 3 agents · 30.5m · completed · plan=static · gap=E
  - *what:* Serial WP1 (launch flags) -> WP2 (session-resume + poisoned filter), each gated/auto-merged with GATE_FAILED abort, then a from-zero acceptance proving memory across deliveries.
  - *idiom:* Chain WP1->WP2 work packages where each gates+squash-merges onto master before the next, then cap with a from-zero real-binary acceptance that proves the end-to-end behavioral claim (an agent recalling a planted code word across two deliveries via resume args).
  - *patterns:* GATE_FAILED-cascade, auto-merge, real-acceptance, pipeline-no-barrier
- **exec-stream-substrate-v2** · `wf_1265b38e-da9` · 4 agents · 29.3m · completed · plan=static · gap=E
  - *what:* 4 sequential exec-stream WPs (LaunchSpec -> codex exec -> claude exec -> from-zero acceptance), each gated on cargo+pnpm and auto-merged, GATE_FAILED aborts the chain.
  - *idiom:* Chain sequential work-packages where each gates on real build+check, auto-merges its own PR, and a GATE_FAILED string early-aborts the cascade, capped by a from-empty-store acceptance agent that proves the feature on real binaries.
  - *patterns:* pipeline-no-barrier, GATE_FAILED-cascade, auto-merge, real-acceptance, worktree-isolation, verify-loop
- **workflow-runtime-wp1** · `wf_290e929e-4af` · 2 agents · 23.1m · completed · plan=static · gap=E
  - *what:* Builds the minimal Rust workflow runtime (serial+parallel agent steps), gates on cargo+pnpm with GATE_FAILED abort, then a from-zero acceptance agent proves real codex+claude deliveries and parallel overlap.
  - *idiom:* A single implement work-package whose GATE_FAILED early-return precedes a from-zero real-binary acceptance agent turns a build into a verifiable delivery.
  - *patterns:* GATE_FAILED-cascade, real-acceptance, worktree-isolation, real-gate
- **close-the-loop** · `wf_1e2f1ddf-9b8` · 2 agents · 7.8m · killed · plan=hybrid · gap=E
  - *what:* Pre-check probe then WP1-WP3 gated+auto-merged work packages closing one real goal through the harness learning loop, proven by real dashboard snapshot counts.
  - *idiom:* A leading read-only probe scopes a serial WP chain whose each step gates on cargo+pnpm, auto-merges via squash, aborts on GATE_FAILED, and a final acceptance agent proves the loop closed by real store-count deltas (0->1), not fixtures.
  - *patterns:* scout-enumerate, GATE_FAILED-cascade, auto-merge, worktree-isolation, real-acceptance, real-gate
- **exec-stream-substrate** · `wf_ed8e34af-fa6` · 1 agents · 1.8m · killed · plan=static · gap=E
  - *what:* WP1 neutral LaunchSpec then WP2 codex-exec and WP3 claude-exec delivery adapters, each gated+worktree+squash-merged, ending in real dual-provider exec-stream acceptance against live binaries.
  - *idiom:* A linear WP chain (neutral spec -> codex adapter -> claude adapter) where each WP works in an isolated worktree, gates on cargo+pnpm without needing a live binary, squash-merges, and a GATE_FAILED first-line aborts the cascade before a real-binary acceptance.
  - *patterns:* GATE_FAILED-cascade, auto-merge, worktree-isolation, real-acceptance, real-gate, pipeline-no-barrier

### Research / map-reduce (23)

> Read-only fan-out where N parallel scouts/auditors gather grounded evidence (often under a shared brief or schema), barrier-join, and one synthesizer fuses their outputs—injected as labeled INPUT blocks or stringified JSON—into a single decision-ready plan/doc, usually with no programmatic gate.

- **generic-harness-object-model-research** ★6 · `wf_e37ff5d7-20c` · 6 agents · 61.2m · completed · plan=hybrid · gap=D
  - *what:* Enumerate let-me-try skills, gather 4 sources in parallel (lmt object model + our schema/docs/Rust/frontend), synthesize a migration plan.
  - *idiom:* An enumerate agent lists the corpus, its output is interpolated into N parallel gather prompts, and a single synthesizer fuses all four into one decision-ready plan.
  - *patterns:* scout-enumerate, research-mapreduce, parallel-barrier, judge-synthesis
- **task-goal-model-research** · `wf_22ce8232-535` · 5 agents · 16.9m · completed · plan=static · gap=D
  - *what:* 4 parallel agents gather let-me-try task/goal skills+PRD and our schema+frontend, then a synthesizer produces a 7-section gap analysis and Task/Goal display proposal.
  - *idiom:* Four parallel readers (two reference-repo, two local model+frontend) fan into one synthesizer that receives all four outputs inlined as labeled INPUT A-D blocks, turning gathered context into a single grounded gap-analysis prose deliverable.
  - *patterns:* parallel-barrier, scout-enumerate, judge-synthesis
- **agent-teams-architecture-diagrams** · `wf_1dda9269-040` · 3 agents · 16.3m · completed · plan=static · gap=D
  - *what:* 2 parallel agents re-read Claude Code + multica source for diagram-grade structural facts, then one writer authors two ASCII-diagram docs in a worktree, gates on pnpm check, and merges a PR.
  - *idiom:* Two parallel source-readers re-extract structural facts that fan into one writer agent which composes the diagram docs in an isolated worktree, gating on pnpm check before opening and squash-merging the PR.
  - *patterns:* parallel-barrier, judge-synthesis, worktree-isolation, real-gate
- **workbench-page-and-docs-plan** · `wf_2b1cd5d5-73d` · 4 agents · 13.3m · completed · plan=static · gap=D
  - *what:* 3 parallel repo/doc auditors -> 1 synthesizer emits a page-soundness scorecard + docs-cleanup roadmap (prose, no gate).
  - *idiom:* Three read-only auditors (doc inventory, surface soundness, intended-requirements doctrine) fan out in parallel, then a single synthesizer fuses all three transcripts into one prioritized scorecard + cleanup plan.
  - *patterns:* parallel-barrier, judge-synthesis
- **agent-teams-architecture-research** · `wf_ba08ac4e-b94` · 4 agents · 11.3m · completed · plan=static · gap=D
  - *what:* 3 parallel codebase studies -> 1 architect writes 4 research/decision docs, gates on pnpm check, and auto-merges the PR.
  - *idiom:* Three parallel source-reading studies (Claude Code source, multica, our harness) fan into one architect who writes the docs in an isolated worktree, gates on pnpm check, and auto-opens+merges the PR — a research map-reduce with a real-delivery tail.
  - *patterns:* parallel-barrier, judge-synthesis, worktree-isolation, real-gate, auto-merge
- **workflow-layout-design** ★4 · `wf_f91ff6a5-fda` · 6 agents · 11.1m · completed · plan=static · gap=contrast
  - *what:* 2 typed understanders -> 3 parallel layout proposals (fixed tournament) -> 1 judge synthesizes ONE winning layout spec doc.
  - *idiom:* Two schema-typed understanding agents (domain semantics + design system) become ground-truth context injected into a fixed 3-way layout tournament, whose typed proposals a single design-lead judges and synthesizes into one winning layout doc.
  - *patterns:* parallel-barrier, schema-gated-handoff, judge-synthesis
- **mention-and-doc-reference-design** · `wf_2c42a249-ac9` · 3 agents · 10.8m · completed · plan=static · gap=D
  - *what:* Two parallel doc/code readers feed a single isolated-worktree design-doc writer that gates on pnpm check and opens+merges a PR.
  - *idiom:* Two parallel read-only agents (existing-doctrine + current-implementation-with-file:line) are fanned-in as labeled INPUT A / INPUT B ground-truth context into a single design-writer agent that then gates on a real build check and auto-merges a PR.
  - *patterns:* parallel-barrier, shared-context, schema-gated-handoff, real-gate, auto-merge
- **cc-agent-teams-tmux-verification** · `wf_0b588f68-81d` · 3 agents · 8.5m · completed · plan=static · gap=D
  - *what:* Parallel official-docs and source-rescan agents feed one reconcile-and-correct writer that updates the research record, gates on pnpm check, and merges a PR.
  - *idiom:* Two parallel evidence-gatherers (official docs via WebFetch/WebSearch + local-source rescan with file:line) are reconciled by a single writer that injects both as INPUT A/B, honestly confirms-or-corrects the prior record, gates on a real check, and auto-merges.
  - *patterns:* parallel-barrier, shared-context, real-gate, auto-merge
- **agents-multica-layout-design** · `wf_2fda87bc-9b0` · 8 agents · 8.4m · completed · plan=static · gap=D
  - *what:* Four schema-typed audits fan into three schema-typed layout variants which fan into one synthesizer producing a single implementable design doc.
  - *idiom:* A three-stage schema-gated fan-out/fan-in: 4 parallel AUDIT-schema scouts -> JSON-injected into 3 parallel VARIANT-schema designers (distinct fixed angles) -> one FINAL-schema synthesizer that grafts the best of all variants, every handoff carried as serialized structured output.
  - *patterns:* parallel-barrier, schema-gated-handoff, shared-context, judge-synthesis, pipeline-no-barrier
- **claude-integration-architecture-writeup** · `wf_c8e26cd5-414` · 3 agents · 8.4m · completed · plan=static · gap=D
  - *what:* Two parallel read-only code tracers (claude path + neutral backbone) feed one synthesizer that writes an end-to-end architecture explainer.
  - *idiom:* Two read-only file:line tracers run under a parallel() barrier, then their text outputs are injected verbatim as labeled INPUT A/INPUT B into one synthesizer that writes the prose deliverable.
  - *patterns:* parallel-barrier, judge-synthesis, scout-enumerate
- **vision-vs-implementation-gap** · `wf_b302e7ce-0a6` · 3 agents · 7.6m · completed · plan=static · gap=D
  - *what:* Parallel vision-docs reader and skeptical impl-reality reader feed one architect that produces a distance-to-vision gap analysis and prioritized next step.
  - *idiom:* Two parallel read-only gatherers (where-we-are-going vs where-we-actually-are, one running a live snapshot) fan into one synthesizer that produces a distance-to-vision scorecard and recommendation; no programmatic gate.
  - *patterns:* parallel-barrier, judge-synthesis, scout-enumerate
- **agent-live-tui-design** ★10 · `wf_a8874e2e-42a` · 8 agents · 7.5m · completed · plan=static · gap=D
  - *what:* 4 parallel typed repo audits feed 3 parallel design angles, then one synthesizer emits the implementable TUI design + WP plan.
  - *idiom:* Fan out typed audits, then a second parallel barrier of design angles, then one synthesizer fed all prior JSON as injected context — a two-stage map then reduce.
  - *patterns:* parallel-barrier, judge-synthesis, schema-gated-handoff, scout-enumerate
- **member-lead-claude-integration-review** · `wf_00be27b8-fbd` · 5 agents · 6.3m · completed · plan=static · gap=D
  - *what:* 4 parallel grounded readers (docs/codex/frontend/claude-seam) feed one architect synthesis producing the AgentMember/Lead refactor + Claude-provider plan.
  - *idiom:* Four parallel scoped readers (one using a specialized agentType) hand prose findings into a single architect synthesizer that injects all four as labeled INPUT blocks.
  - *patterns:* parallel-barrier, judge-synthesis, mixed-model
- **can-agents-use-workflow-skill** · `wf_fcbd031c-817` · 4 agents · 6.3m · completed · plan=static · gap=D
  - *what:* Three parallel investigators probe whether spawned agents can use the author-workflow skill+CLI, synthesized into a yes/no/partial answer with the exact gap.
  - *idiom:* Three parallel read-only investigators each answer a distinct sub-question with typed evidence, then a single synthesizer merges them into one honest yes/no/partial verdict — no build, no gate.
  - *patterns:* parallel-barrier, research-mapreduce, judge-synthesis, schema-gated-handoff
- **xhs-wenchuang-report-research** · `wf_e2175003-946` · 4 agents · 5.3m · completed · plan=static · gap=—
  - *what:* 4 parallel WebSearch agents research XHS cultural-creative product forms / hot IPs / design angles / a design gallery, each returning cited markdown; results returned as an array with no merge.
  - *idiom:* Four fixed parallel researchers each own a distinct facet with a shared COMMON instruction block (cite real links, no fabricated numbers); returns the raw array with no synthesizer — the human compiles.
  - *patterns:* scout-enumerate, pipeline-no-barrier
- **operator-interaction-basics** · `wf_d64b7b81-0c7` · 3 agents · 5.3m · completed · plan=static · gap=D
  - *what:* 2 parallel code-readers probe the harness message/identity model and the operator interaction surface; an architect synthesizer answers 5 operator questions and outputs a prioritized plan — read-only, prose deliverable, no gate.
  - *idiom:* Two parallel read-only investigators (message/identity model + interaction capability matrix) barrier-join, then a single synthesizer is handed both raw outputs as labeled INPUT A/B to answer fixed Q1-Q5 and emit a sequenced WP plan.
  - *patterns:* parallel-barrier, judge-synthesis, shared-context
- **exec-mode-architecture-review** · `wf_bb017c9b-c7f` · 3 agents · 5.2m · completed · plan=static · gap=D
  - *what:* 2 parallel investigators (codex/claude headless-exec docs research + our runtime/serve/SSE arch read) feed an architect synthesizer that judges the dashboard-as-UI hypothesis and recommends a target architecture; read-only, no gate.
  - *idiom:* One web-research agent + one local-code-reader run in parallel, barrier-join, then a single architect synthesizer consumes both as INPUT A/B to deliver a decision-ready verdict — fan-out-then-judge with no programmatic gate.
  - *patterns:* parallel-barrier, judge-synthesis, shared-context
- **agent-mention-roadmap-audit** · `wf_ca82faa8-2ce` · 5 agents · 4.5m · completed · plan=static · gap=D
  - *what:* Four parallel read-only code audits (each schema-forced, Explore type) over fixed WP/gate dimensions, barriered, then their JSON findings are injected verbatim into one synthesizer that emits a typed sequenced implementation plan + gate-fix recommendation.
  - *idiom:* Fan out N fixed read-only audits each emitting a typed FINDINGS_SCHEMA, then JSON.stringify the collected findings into a single synthesizer that emits a typed sequenced plan — schema-gated map then reduce with no build.
  - *patterns:* parallel-barrier, scout-enumerate, schema-gated-handoff, judge-synthesis
- **xhs-market-research-routes** ★12 · `wf_d792276e-f39` · 5 agents · 3.9m · completed · plan=static · gap=D
  - *what:* Four parallel market-research dimensions (categories/IP/benchmark-spots/viral-patterns), each grounded in a shared real XHS data brief + WebSearch, barriered, then concatenated into one synthesizer that emits a multi-route research plan; pure prose, no gate.
  - *idiom:* Pin a shared real-data brief into the prompt of every parallel researcher so each WebSearch dimension is grounded in the same evidence, then concatenate their prose into one synthesizer that produces the actionable plan.
  - *patterns:* parallel-barrier, judge-synthesis
- **orchestration-plugin-design-v2** · `wf_6954e43f-d27` · 3 agents · 3.3m · completed · plan=static · gap=C
  - *what:* Three parallel bounded probes (runtime/CLI seam, plugin exposure surface, dashboard drill-in), each read-only with a fixed file list, ~12-call budget, FINDINGS_SCHEMA and per-probe error catch, returning structured whatExists/gaps/recommendation for a plugin design; no synthesizer, no build.
  - *idiom:* Three bounded read-only probes, each constrained to a fixed file list + tool-call budget and forced through a whatExists/gaps/recommendation schema, run in parallel with per-probe catch and returned raw — a structured design-probe map with no reduce step.
  - *patterns:* parallel-barrier, schema-gated-handoff, scout-enumerate
- **cli-reference-verification** · `wf_4b37538f-3d1` · 4 agents · 2.8m · completed · plan=static · gap=D
  - *what:* 3 parallel CLI-reference/codebase probes feed one synthesizer that reconciles claude -p vs codex exec against what the harness actually invokes.
  - *idiom:* Fan out N read-only fact-gathering agents (two doc-fetch + one codebase-read), then inject all three raw outputs verbatim into one synthesizer prompt that produces the reconciled prose answer.
  - *patterns:* parallel-barrier, research-mapreduce, judge-synthesis
- **workflow-plugin-design** · `wf_c112d3e9-ee6` · 4 agents · 0.6m · killed · plan=static · gap=D
  - *what:* Four parallel read-only investigation probes (Claude trace observability, Codex extension surface, harness runtime, trace schema/dashboard) each returning structured findings, collected at a barrier.
  - *idiom:* Fan out N fixed read-only Explore probes in parallel under one shared FINDINGS_SCHEMA (claim+evidence facts, reusePoints, risks, recommendation), then barrier-collect into one design corpus.
  - *patterns:* parallel-barrier, scout-enumerate, schema-gated-handoff
- **workflow-plugin-design** · `wf_f47ddc8f-ed7` · 0 agents · 0m · failed · plan=static · gap=D
  - *what:* 4 parallel schema-typed read-only design probes (Claude trace observability, Codex extension surface, harness runtime integration, trace schema) for a workflow-plugin design; aborted at startup (0 agents, 5ms).
  - *idiom:* A single Investigate phase fans out 4 fixed schema-typed read-only probes (each returns summary/facts/reusePoints/risks/recommendation) to map a design space before any build.
  - *patterns:* parallel-barrier, scout-enumerate, schema-gated-handoff

### Single-agent gated (9)

> One agent self-gates an end-to-end deliverable (doc, lint cleanup, rebase, or PR) on a real unpiped EXIT-0 check in an isolated worktree, polls CI/gh to green, and auto-merges—returning GATE_FAILED instead of merging on any red.

- **workbench-cleanup-and-wp1** · `wf_6a021088-fe2` · 2 agents · 22.5m · completed · plan=static · gap=E
  - *what:* Runs two independent worktree-isolated agents in parallel: one rewrites/deletes dashboard docs gated on pnpm check, one wires honest actions + collapses the rail gated on tsc+vite build, each opening its own PR.
  - *idiom:* Two unrelated self-gated PRs (docs cleanup + frontend WP) run in isolated worktrees concurrently, each iterating its own hard gate to green and opening (not merging) a PR — parallelism for independence, not decomposition.
  - *patterns:* parallel-barrier, worktree-isolation, real-gate, pipeline-no-barrier
- **fix-clippy-fmt-pr51** · `wf_edccc0cf-bfe` · 1 agents · 17.7m · completed · plan=static · gap=E
  - *what:* One agent clears ~62 clippy/fmt violations on PR #51, gates on fmt+clippy+test+pnpm, polls gh pr checks until rust passes, then squash-merges and syncs master.
  - *idiom:* A single agent can self-gate a lint cleanup on an unpiped fmt+clippy+test EXIT-0 wall, poll CI to green, then perform the owner-approved merge — emitting GATE_FAILED instead of merging on failure.
  - *patterns:* real-gate, GATE_FAILED-cascade, real-acceptance
- **rebase-pr51-vision-goal-task** · `wf_a47a7ef9-430` · 1 agents · 10m · completed · plan=static · gap=E
  - *what:* One agent rebases an open PR onto master in a worktree, combines conflicts, and only force-pushes if both cargo test and pnpm check pass.
  - *idiom:* A single agent does a delicate worktree rebase that must keep both sides' code, then self-gates on TWO real gates (cargo test >=141 AND pnpm check EXIT 0) before force-push, emitting 'GATE_FAILED:' and refusing to push if either gate or a side is lost.
  - *patterns:* worktree-isolation, real-gate, GATE_FAILED-cascade
- **member-chat-app-redesign** · `wf_a4aafe15-7f2` · 1 agents · 9.7m · killed · plan=static · gap=E
  - *what:* One isolated worktree agent redesigns MemberWorkbench into a two-pane chat app, self-gating on tsc/build/screenshot then auto-merging its PR; killed mid-run.
  - *idiom:* A single worktree-isolated implementer must pass a hard tsc+build+screenshot gate BEFORE committing, auto-merges its own squash PR on green, and returns a 'GATE_FAILED:'-prefixed report instead of merging on failure.
  - *patterns:* worktree-isolation, GATE_FAILED-cascade, real-acceptance, auto-merge
- **cc-agent-teams-tmux-verification** · `wf_5ced66a7-ea6` · 3 agents · 8.3m · completed · plan=static · gap=E
  - *what:* Parallel docs-vs-source verification of a tmux claim, then one agent reconciles, corrects the research doc behind a real pnpm-check gate, and merges a PR.
  - *idiom:* Two parallel evidence-gatherers (official docs vs local source) fan into a single reconcile-and-write agent that self-gates on `pnpm check` EXIT 0 in an isolated worktree and auto-merges a PR, emitting GATE_FAILED on a red gate.
  - *patterns:* parallel-barrier, judge-synthesis, real-acceptance, GATE_FAILED-cascade, worktree-isolation, auto-merge
- **rust-workflow-runtime-design** · `wf_d9299c5b-8d6` · 4 agents · 8.2m · completed · plan=static · gap=E
  - *what:* Three parallel probes feed one agent that writes a Rust workflow-runtime design doc behind a real pnpm-check gate and merges a PR.
  - *idiom:* Three parallel probes (external spec, prior report, our building blocks) fan into one design-doc writer that self-gates on `pnpm check` EXIT 0 in a worktree, registers the doc, and auto-merges a PR with a GATE_FAILED early-abort.
  - *patterns:* parallel-barrier, judge-synthesis, scout-enumerate, real-acceptance, GATE_FAILED-cascade, worktree-isolation, auto-merge
- **agent-integration-model-doc** · `wf_03bdf82f-24b` · 1 agents · 6.5m · completed · plan=static · gap=E
  - *what:* One isolated-worktree agent writes the canonical integration-model doc + ADR, gates on pnpm check, then auto-merges the PR.
  - *idiom:* A single doc-only worktree agent self-gates on `pnpm check` EXIT 0 with a GATE_FAILED early-abort, then commits/pushes/opens-PR/auto-merges in one turn.
  - *patterns:* worktree-isolation, real-gate, GATE_FAILED-cascade, auto-merge
- **fix-provider-tests-merge-pr51** · `wf_c646e8a3-7ff` · 1 agents · 5.7m · completed · plan=static · gap=E
  - *what:* Single agent makes provider tests env-independent, runs full rust CI gate (incl. binary-absent PATH), polls PR #51 checks, and squash-merges only when rust+docs pass.
  - *idiom:* One agent self-gates an end-to-end delivery: reproduce the CI condition locally, poll real gh pr checks until green, then merge — emit GATE_FAILED instead of merging on any red.
  - *patterns:* CI-poll-then-merge, real-acceptance, GATE_FAILED-cascade, worktree-isolation
- **member-runtime-observability-doc** · `wf_78ad9884-e42` · 1 agents · 5m · completed · plan=static · gap=E
  - *what:* One agent writes a canonical observability contract doc in an isolated worktree, gates on `pnpm check` (validate:json/check:links/check:doc-governance), then commits, PRs and squash-merges; aborts with GATE_FAILED: if the gate cannot go green.
  - *idiom:* A single worktree-isolated agent can self-gate a doc deliverable on a real `pnpm check` EXIT-0 plus doc-governance registry, then auto-PR/squash-merge, returning GATE_FAILED on the first line if green is unreachable.
  - *patterns:* worktree-isolation, real-gate, GATE_FAILED-cascade, auto-merge

## Cross-corpus takeaways

- Gating is the spine, and it is almost always a REAL gate: build/test/lint/CI EXIT-0 walls (cargo+pnpm, gh pr checks) or screenshot/browser acceptance, never self-reported success. Delivery families end with a from-zero real-binary acceptance agent that proves observed behavior (store-count deltas, live SSE frames) rather than fixtures—our external workflow+skill should make a real, unpiped gate and an independent acceptance step first-class primitives.
- A 'GATE_FAILED:' first-line convention is the universal abort signal: each step checks the prior step's first line and short-circuits the whole cascade so broken work never compounds. This cheap string protocol is worth adopting as the standard cross-step failure contract.
- Two distinct repair idioms recur: closed-loop builds use a typed STAGE_RESULT.ok gate with exactly ONE repair pass then stop-on-fail, while verify->fix loops bound iterations (<=6 attempts) on a multi-bool VERIFY schema until green. Bounding repair attempts and forcing a hard stop for human intervention is a deliberate, repeated safety choice.
- Planning is overwhelmingly static (55/62); only 4 hybrid (a leading read-only probe scopes an otherwise-fixed chain) and 3 dynamic (fan-out width = runtime finding count, i.e. one-verifier-per-finding). Dynamic decomposition appears ONLY in adversarial-review, and it is also where failures cluster—so reserve runtime-sized fan-out for review/verification, and keep build/delivery chains statically planned.
- Schemas (26/62) are used as enforced handoff contracts, not decoration: they gate stage transitions (STAGE_RESULT.ok), carry verified findings between adversarial steps, and serialize map-reduce handoffs as injected JSON. But schema-forcing is a live failure mode—two runs failed/aborted when schema-forced agents never called StructuredOutput—so the external design must treat 'agent didn't emit the schema' as a first-class error path with a fallback, not assume the structured output always arrives.
- Context-sharing in research map-reduce is done by explicit injection: enumerate/scout outputs are interpolated verbatim into downstream prompts as labeled INPUT A/B blocks or stringified findings, and parallel researchers are grounded by a shared brief pinned into every prompt. There is no implicit shared memory—our skill should make explicit prompt-injection of upstream outputs (and a shared grounding brief) the supported mechanism for fan-in.

---

> Dataset: `/tmp/wf-classified.json` (62 structured entries + rollup), produced by run `wf_295b8c34-aea`. Family assignment is the workflow's own pass; the planning axis (static/hybrid/dynamic) captures the dynamic-decomposition seam even where a record's primary family is research or review.