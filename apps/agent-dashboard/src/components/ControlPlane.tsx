import { tasksForGoal } from "../readModel";
import type { AgentMember, DashboardSnapshot, Task, WorkflowWarning } from "../types";
import { GoalHeader } from "./GoalHeader";
import { KanbanBoard } from "./KanbanBoard";
import { MemberDetail } from "./MemberDetail";
import { TaskDetail } from "./TaskDetail";
import { TeamRoster } from "./TeamRoster";
import { WarningsPanel } from "./WarningsPanel";

interface ControlPlaneProps {
  snapshot: Required<DashboardSnapshot>;
  warnings: WorkflowWarning[];
  selectedGoalId?: string;
  selectedTaskId?: string;
  selectedMemberId?: string;
  onSelectGoal: (id: string) => void;
  onSelectTask: (id: string) => void;
  onSelectMember: (id: string) => void;
}

export function ControlPlane(props: ControlPlaneProps) {
  const { snapshot, warnings, selectedGoalId, selectedTaskId, selectedMemberId } = props;
  const activeGoal = snapshot.goals.find((goal) => goal.id === selectedGoalId) ?? snapshot.goals[0];
  const tasks = tasksForGoal(snapshot, activeGoal?.id);
  const selectedTask = snapshot.tasks.find((task) => task.id === selectedTaskId) ?? tasks[0];
  const selectedMember =
    snapshot.members.find((member) => member.id === selectedMemberId) ??
    memberForTask(snapshot.members, selectedTask);

  return (
    <section className="controlPlane">
      <aside className="rail">
        <h2>Goals</h2>
        <div className="railList">
          {snapshot.goals.map((goal) => (
            <button
              className={`railItem ${activeGoal?.id === goal.id ? "active" : ""}`}
              type="button"
              key={goal.id}
              onClick={() => props.onSelectGoal(goal.id)}
            >
              <strong>{goal.title || goal.id}</strong>
              <span>{goal.status || "active"}</span>
            </button>
          ))}
        </div>
        <h2>Teams</h2>
        <TeamRoster snapshot={snapshot} onSelectMember={props.onSelectMember} />
      </aside>

      <main className="workbench">
        <GoalHeader goal={activeGoal} taskCount={tasks.length} warningCount={warnings.length} />
        <KanbanBoard snapshot={snapshot} selectedTaskId={selectedTask?.id} onSelectTask={props.onSelectTask} />
        <TaskDetail
          snapshot={snapshot}
          task={selectedTask}
          warnings={warnings}
          onSelectMember={props.onSelectMember}
        />
      </main>

      <aside className="rightRail">
        <MemberDetail snapshot={snapshot} member={selectedMember} onSelectTask={props.onSelectTask} />
        <WarningsPanel
          warnings={warnings}
          onSelectTask={props.onSelectTask}
          onSelectMember={props.onSelectMember}
        />
      </aside>
    </section>
  );
}

function memberForTask(members: AgentMember[], task?: Task): AgentMember | undefined {
  if (!task?.assignee_agent_id) return members[0];
  return members.find((member) => member.id === task.assignee_agent_id) ?? members[0];
}
