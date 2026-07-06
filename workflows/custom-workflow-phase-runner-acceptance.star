workflow(
    "custom-workflow-phase-runner-acceptance",
    "Acceptance fixture proving a user-authored workflow-mode GoalPhase can run directly through goal run-phases.",
)

log("custom workflow-mode GoalPhase fixture started")
output({
    "accepted": True,
    "surface": "goal-phase-workflow",
    "contract": "GoalPhase.execution_mode=workflow loads workflow_ref directly",
})
verdict(True, "custom workflow-mode phase direct run accepted")
