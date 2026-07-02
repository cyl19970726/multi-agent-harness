workflow(
    "author-goal-planner-current-audit",
    "Audit author-goal and author-planner with parallel grounded checks against the current harness code, installed skill copies, and workflow runtime semantics before relying on them for future planning.",
    success_criterion = "A structured synthesis and independent critic identify whether the two skills match current code and what must change.",
)

COMMON = """
You are auditing the Multi-Agent Harness repository at /Users/hhh0x/multi-agent-harness.

Current git baseline from the Lead before this run:
- branch: master
- HEAD: 2e88b9c
- origin/master after fetch: 2e88b9c
- the worktree is dirty with in-progress local harness/workflow changes; do not treat dirty files as unrelated noise if they are part of current behavior.

Target skills:
- skills/author-goal/SKILL.md
- skills/author-planner/SKILL.md

Important supporting files to inspect as needed:
- .agents/skills/generic-agent-harness/SKILL.md
- skills/author-workflow/SKILL.md
- crates/harness-cli/src/main.rs
- crates/harness-core/src/lib.rs
- crates/harness-workflow/src/lib.rs
- crates/harness-workflow/src/starlark_front.rs
- docs/goal-learning-loop.md, docs/data-model.md, docs/workflow-runtime.md if useful
- installed skill copies under /Users/hhh0x/.agents/skills and /Users/hhh0x/.codex/skills if relevant

Rules:
- This is a read-only audit. Do not edit files.
- Ground claims in concrete files, commands, or line/function names.
- Distinguish true code drift from content-quality or example gaps.
- Focus on whether these skills are safe and current for Lead Agents using the latest harness workflow.
"""

AUDIT_SCHEMA = {
    "status": "one of: current | mostly_current | drift | blocked",
    "critical_findings": "critical findings, one per line; write NONE if none",
    "important_findings": "important non-critical findings, one per line; write NONE if none",
    "evidence_refs": "file/function/command refs, one per line",
    "recommended_changes": "recommended skill changes, one per line; write NONE if none",
    "confidence": "one of: high | medium | low",
}

audits = parallel([
    {
        "provider": "codex",
        "label": "author-goal-code-drift",
        "phase": "audit",
        "effort": "medium",
        "timeout_s": 420,
        "add_dir": ["/Users/hhh0x/.agents/skills", "/Users/hhh0x/.codex/skills"],
        "schema": AUDIT_SCHEMA,
        "prompt": COMMON + """
Audit author-goal for current-code drift.

Check specifically:
1. Goal lifecycle commands and gates: create, describe-set, design-set, acceptance-set, explore-add, knowledge-add, design-synthesize, stage, learning-status, evaluate, close.
2. Whether the skill's markdown-first model is still true while typed GoalDesign/GoalEvaluation also exist.
3. Whether review/assignment gates now require typed GoalDesign dual-read behavior that the skill should explain.
4. Whether auto-finalize / derived stage / phase behavior described by the skill matches crates/harness-core and crates/harness-cli.
5. Any stale command names, flags, or semantics.

Return only the JSON object requested by the schema.
""",
    },
    {
        "provider": "codex",
        "label": "author-planner-code-drift",
        "phase": "audit",
        "effort": "medium",
        "timeout_s": 420,
        "add_dir": ["/Users/hhh0x/.agents/skills", "/Users/hhh0x/.codex/skills"],
        "schema": AUDIT_SCHEMA,
        "prompt": COMMON + """
Audit author-planner for current-code drift.

Check specifically:
1. Whether GoalPhase, Task.phase_id, Task.design_md, acceptance, owned_paths, depends_on, inputs, outputs, retry match current core structs and CLI parsing.
2. Whether `harness goal plan`, `harness phase compile`, and `harness goal run-phases` behavior in the skill matches compile_planner_script / compile_phase_to_starlark / run-phases code.
3. Whether the skill overstates implemented capabilities around replan, resume, required outputs, registered_doc checks, and per-phase landing commits.
4. Whether it covers current workflow landing semantics enough for developers using phases.
5. Any stale command names, flags, examples, or missing warnings.

Return only the JSON object requested by the schema.
""",
    },
    {
        "provider": "codex",
        "label": "skill-content-quality",
        "phase": "audit",
        "effort": "medium",
        "timeout_s": 420,
        "add_dir": ["/Users/hhh0x/.agents/skills", "/Users/hhh0x/.codex/skills"],
        "schema": AUDIT_SCHEMA,
        "prompt": COMMON + """
Audit the content quality of author-goal and author-planner as operator skills.

Check specifically:
1. Whether each skill has a distinct responsibility and the handoff between them is clear.
2. Whether examples cover the important scenarios users have been asking about: simple manual goals, planned phases, code-development workflow, direct vs patch landing, explicit apply/reject, and review-gated acceptance.
3. Whether the text is actionable enough for an agent to use without hidden chat context.
4. Whether the skills risk confusing GoalDesign typed objects, markdown design_md, goal phases, workflow runtime, agent teams, and subagents.
5. What content is missing even if code is current.

Return only the JSON object requested by the schema.
""",
    },
    {
        "provider": "codex",
        "label": "installed-copy-and-distribution",
        "phase": "audit",
        "effort": "low",
        "timeout_s": 300,
        "add_dir": ["/Users/hhh0x/.agents/skills", "/Users/hhh0x/.codex/skills"],
        "schema": AUDIT_SCHEMA,
        "prompt": COMMON + """
Audit installed-copy and distribution consistency.

Check specifically:
1. Compare repo copies skills/author-goal and skills/author-planner with installed /Users/hhh0x/.agents/skills/author-goal and /Users/hhh0x/.agents/skills/author-planner.
2. Note whether /Users/hhh0x/.codex/skills has copies of these skills and whether that matters for current installation policy.
3. Check if quick validation or metadata shape appears current.
4. Identify any sync-script gap analogous to skills/author-workflow/scripts/sync-installed.sh.

Return only the JSON object requested by the schema.
""",
    },
])

SYNTH_SCHEMA = {
    "author_goal_status": "one of: current | mostly_current | drift | blocked",
    "author_planner_status": "one of: current | mostly_current | drift | blocked",
    "outdated_or_wrong": "true code/CLI drift items, one per line; write NONE if none",
    "missing_current_main_concepts": "missing concepts or underexplained semantics, one per line; write NONE if none",
    "recommended_patches": "concrete patches to make, one per line; write NONE if none",
    "must_fix_now": "bool",
    "evidence_refs": "supporting refs, one per line",
    "summary": "short conclusion in Chinese",
}

synthesis = agent(
    COMMON + """
Synthesize these parallel audits into one decision-quality result.

Audits JSON:
""" + json.encode(audits) + """

You must answer:
- Are skills/author-goal and skills/author-planner consistent with the current code state?
- Are there true outdated/wrong statements?
- What important gaps remain even if they are not code drift?
- Should we patch now, or create follow-up work?

Return only the JSON object requested by the schema.
""",
    provider = "codex",
    label = "synthesis",
    phase = "synthesis",
    effort = "high",
    timeout_s = 420,
    add_dir = ["/Users/hhh0x/.agents/skills", "/Users/hhh0x/.codex/skills"],
    schema = SYNTH_SCHEMA,
)

CRITIC_SCHEMA = {
    "actionable": "bool",
    "verdict": "one of: accept | revise | blocked",
    "must_fix_before_use": "must-fix gaps before relying on the skills, one per line; write NONE if none",
    "defer": "valid follow-up gaps, one per line; write NONE if none",
    "reason": "short reason",
}

critic = agent(
    COMMON + """
Criticize the synthesis for overclaiming and missing edge cases.

Audits JSON:
""" + json.encode(audits) + """

Synthesis JSON:
""" + json.encode(synthesis) + """

Judge whether the synthesis is actionable enough for the Lead to accept this audit.
If evidence is weak, say revise and name what is missing. If the skills are unsafe to use before edits, list must-fix items.

Return only the JSON object requested by the schema.
""",
    provider = "codex",
    label = "critic",
    phase = "critic",
    effort = "medium",
    timeout_s = 420,
    add_dir = ["/Users/hhh0x/.agents/skills", "/Users/hhh0x/.codex/skills"],
    schema = CRITIC_SCHEMA,
)

ok = type(synthesis) == "dict" and type(critic) == "dict" and critic["verdict"] != "blocked"
output({
    "audits": audits,
    "synthesis": synthesis,
    "critic": critic,
})
verdict(ok, reason = "structured synthesis and critic completed" if ok else "missing or blocked synthesis/critic")
