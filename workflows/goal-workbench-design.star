# Read-only Goal Workbench contract workflow.
#
# This is intentionally narrower than a broad design tournament: prior
# interrupted runs already showed the right direction (phase spine + operator
# flow). This workflow produces one implementation-ready page contract and then
# runs a separate gate whose job is to reject any contract that does not require
# real browser screenshots.

workflow(
    "goal-workbench-v1-design",
    "Produce and gate an implementation-ready Goal page contract from first " +
    "principles, with a hard screenshot acceptance requirement before any UI " +
    "implementation can be accepted.",
    budget_usd = 4.0,
    success_criterion = "Goal page contract includes phase-first layout, proof chain, implementation slices, and real screenshot gate",
)

CONTEXT = """
Product truth:
- Multi-Agent Harness is a goal-task-agent workflow system.
- The UI must prove: Goal -> GoalDesign -> phases -> task graph -> Message assignment
  -> AgentMember work -> Evidence -> Proposal/Review -> Decision -> GoalEvaluation.
- Goal is not a task list. A Goal is an auditable outcome.
- Phase is the sequential execution/gate model. Tasks join phases via Task.phase_id.
- The legacy stage bar draft > exploring > explored > working > done > verifying > verified
  is only a derived lifecycle projection for phase-driven goals.

Current user pain:
- The user is on surface=tasks&goal=goal-content-model-v1 and cannot tell the actual
  goal, goal spec, phase plan, or acceptance state.
- They want the Goal page itself to show phases and their work without jumping to
  a separate task board.
- They explicitly emphasized actual screenshots for verification.

Design direction to synthesize:
- Make Goal detail the primary workbench.
- First viewport: goal identity, spec/acceptance summary, derived lifecycle summary,
  phase progress, current blocker/next action, proof chain health.
- Main body: vertical phase spine. Each phase shows intent, gate acceptance,
  status, phase tasks, graph/kanban projection, evidence/review/decision state,
  outputs, and next action.
- Separate phase tasks from unphased/follow-up tasks.
- Task detail opens inline/drawer/inspector without losing goal context.
- Work page remains goal collection/index.
- Actual acceptance requires browser screenshots for desktop, tablet, and mobile,
  plus console and horizontal overflow checks.
"""

CONTRACT = {"content": "long Chinese markdown contract"}
GATE = {
    "ok": "bool",
    "findings": "findings, one per line",
    "content": "improved or accepted contract notes",
}

def text_of(item):
    if type(item) == "dict" and type(item["content"]) == "string":
        return item["content"]
    return ""

def long_enough(text, min_len):
    return type(text) == "string" and len(text.strip()) >= min_len

phase("draft")
draft = agent(
    CONTEXT + """
你是 Multi-Agent Harness 的 Goal Workbench 产品设计负责人。

请从第一性原理产出一个可以直接写入 docs/dashboard/pages/goal.md 的页面契约。
必须是中文 markdown，至少 1200 字符，必须包含这些标题：

## Selected Direction
## Why
## First Viewport
## Desktop ASCII
## Tablet ASCII
## Mobile ASCII
## Phase Spine
## Task Grouping
## Proof Chain
## Stage Bar Treatment
## Implementation Slices
## Screenshot Gate

硬要求：
- Desktop/Tablet/Mobile ASCII 必须是真实 box diagram，不是描述。
- Screenshot Gate 必须明确要求实际桌面、平板、手机截图。
- Screenshot Gate 必须明确 console 检查和 horizontal overflow 检查。
- 不能说“只要 check:dashboard 通过就可以”。
- 不要输出过程说明，不要说“我会先读取”，只输出 contract。

Return JSON only: {"content": "..."}.
""",
    label = "draft:goal-page-contract",
    provider = "codex",
    effort = "medium",
    timeout_s = 360,
    schema = CONTRACT,
)

draft_text = text_of(draft)

phase("gate")
gate = agent(
    CONTEXT + """
你是独立 gate reviewer。检查下面的 Goal page contract 是否可进入实现。

拒绝条件：
- 没有真实 Desktop/Tablet/Mobile ASCII box diagram。
- 没有明确要求实际浏览器截图。
- 没有明确 console 检查。
- 没有明确 horizontal overflow 检查。
- 仍然把 legacy stage bar 当主视觉。
- Phase/tasks/evidence/review/decision/GoalEvaluation 证明链不清楚。
- 没有区分 phase tasks 和 unphased/follow-up tasks。

如果通过，ok=true，并在 content 中给出简短 gate notes。
如果不通过，ok=false，并给出 findings。不要宽松。

CONTRACT:
""" + draft_text,
    label = "gate:screenshot-contract",
    provider = "codex",
    effort = "medium",
    timeout_s = 300,
    schema = GATE,
)

gate_ok = type(gate) == "dict" and gate["ok"] == True
final_text = draft_text

if not gate_ok:
    phase("repair")
    findings = gate["findings"] if type(gate) == "dict" else "gate returned no structured findings"
    repair = agent(
        CONTEXT + """
修复下面的 Goal page contract，使它满足 gate findings。
输出完整 contract，不要只输出差异。必须保留所有 required headings 和真实 ASCII。

GATE FINDINGS:
""" + findings + """

CURRENT CONTRACT:
""" + draft_text,
        label = "repair:goal-page-contract",
        provider = "codex",
        effort = "medium",
        timeout_s = 360,
        schema = CONTRACT,
    )
    final_text = text_of(repair)

musts = ["Desktop ASCII", "Tablet ASCII", "Mobile ASCII", "实际", "截图", "console", "overflow"]
missing = []
for item in musts:
    if item not in final_text:
        missing.append(item)

ok = long_enough(final_text, 1000) and len(missing) == 0
output({
    "contract": final_text,
    "gate_ok": gate_ok,
    "gate_findings": gate["findings"] if type(gate) == "dict" else "",
    "missing_required_terms": "\n".join(missing),
})
verdict(ok, reason = "Goal page contract passed screenshot gate" if ok else "Goal page contract missing required screenshot-gate terms")
