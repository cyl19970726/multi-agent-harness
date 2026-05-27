import { taskColumnsForTasks } from "../readModel";
import type { Task } from "../types";

export function KanbanBoard({
  tasks,
  selectedTaskId,
  onSelectTask,
}: {
  tasks: Task[];
  selectedTaskId?: string;
  onSelectTask: (id: string) => void;
}) {
  return (
    <div className="kanban">
      {taskColumnsForTasks(tasks).map((column) => (
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
