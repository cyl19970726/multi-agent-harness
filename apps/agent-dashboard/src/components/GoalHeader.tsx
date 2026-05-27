import type { Goal } from "../types";
import { Pill } from "./Pill";

export function GoalHeader({
  goal,
  taskCount,
  warningCount,
}: {
  goal?: Goal;
  taskCount: number;
  warningCount: number;
}) {
  return (
    <div className="goalHeader">
      <div>
        <h2>{goal?.title || goal?.id || "No goal"}</h2>
        <p>{goal?.objective || "Load a snapshot to inspect a harness workflow."}</p>
      </div>
      <div className="goalMeta">
        <Pill tone="good">{taskCount} tasks</Pill>
        <Pill tone={warningCount ? "warn" : "good"}>{warningCount} warnings</Pill>
      </div>
    </div>
  );
}
