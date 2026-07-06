workflow(
    "frontend-ui-audit",
    "Audit real Agent Workbench screenshots from product-flow and visual-design lenses, then synthesize concrete UI fixes for the next iteration.",
    success_criterion="The audit names concrete visible problems and prioritized next UI changes, based on screenshots rather than code only.",
)

phase("independent-review")
reviews = parallel([
    {
        "label": "goal-phase-ux",
        "provider": "codex",
        "image": [args["goal_image"]],
        "schema": {
            "ok": "bool",
            "findings": "visible UX/product issues, one per line",
            "next_actions": "recommended fixes, one per line",
        },
        "prompt": """You are reviewing the Goal phase screen of a multi-agent workbench.

Judge the screenshot only. The intended product model is:
Goal phase -> phase spec -> compiled workflow -> live execution -> readable workflow steps.

Hard constraints:
- The phase must not read as a Task Graph.
- Do not accept visible Graph/Kanban/task dependency concepts inside the phase.
- The phase content should be readable by an operator who wants to know what will run, what is running, and what passed.
- Flag visual density, weak hierarchy, confusing labels, raw debug text, overflow, or card-dump layout.

Return JSON only. Findings and next_actions should be newline-separated strings.""",
    },
    {
        "label": "workflow-detail-ux",
        "provider": "codex",
        "image": [args["workflow_image"]],
        "schema": {
            "ok": "bool",
            "findings": "visible UX/product issues, one per line",
            "next_actions": "recommended fixes, one per line",
        },
        "prompt": """You are reviewing the Workflow run detail screen of a multi-agent workbench.

Judge the screenshot only. The intended reading order is:
workflow spec / compiled plan first, then live execution summary, verdict, execution timeline, and drill-in.

Hard constraints:
- The screen should not start like a raw log report.
- Spec should be discoverable near the top.
- Phase/step content should render as readable workflow execution, not raw provider JSON.
- Flag visual density, weak hierarchy, raw debug text, overflow, missing context, or poor first-viewport composition.

Return JSON only. Findings and next_actions should be newline-separated strings.""",
    },
])

phase("synthesis")
summary = agent(
    """Synthesize these independent UI reviews into one prioritized action list.

Reviews JSON:
{reviews}

Return:
- ok=false if any P0/P1 visible issue remains.
- findings: concrete visible issues, one per line.
- next_actions: prioritized implementation actions, one per line.
Do not invent issues outside the screenshots. Keep it concise and actionable.""".format(reviews=json.encode(reviews)),
    provider="codex",
    label="synthesize-ui-audit",
    schema={
        "ok": "bool",
        "findings": "visible issues, one per line",
        "next_actions": "prioritized fixes, one per line",
    },
)

if summary == None:
    output({"ok": False, "findings": "Audit synthesis failed to return JSON.", "next_actions": "Re-run frontend-ui-audit with valid screenshots."})
    verdict(False, "frontend UI audit did not produce a structured synthesis")
else:
    output(summary)
    verdict(summary["ok"], summary["findings"])
