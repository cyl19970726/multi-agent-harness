import { taskColumns } from "../readModel";
import type { DashboardSnapshot } from "../types";

export function KanbanBoard({
  snapshot,
  selectedTaskId,
  onSelectTask,
}: {
  snapshot: Required<DashboardSnapshot>;
  selectedTaskId?: string;
  onSelectTask: (id: string) => void;
}) {
  return (
    <div className="kanban">
      {taskColumns(snapshot).map((column) => (
        <section className="column" key={column.status}>
          <div className="columnHeader">
            <strong>{column.status}</strong>
            <span>{column.tasks.length}</span>
          </div>
          {column.tasks.map((task) => (
            <button
              className={`taskCard ${selectedTaskId === task.id ? "selected" : ""}`}
              type="button"
              key={task.id}
              onClick={() => onSelectTask(task.id)}
            >
              <strong>{task.title || task.id}</strong>
              <span>assignee={task.assignee_agent_id || "-"}</span>
              <span>reviewer={task.reviewer_agent_id || "-"}</span>
            </button>
          ))}
          {!column.tasks.length && <div className="empty">No tasks</div>}
        </section>
      ))}
    </div>
  );
}
