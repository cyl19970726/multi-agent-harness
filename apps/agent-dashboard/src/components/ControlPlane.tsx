import { activeGoal, membersForTasks, tasksForGoal, teamsForMembers, warningsForScope } from "../readModel";
import type { AgentMember, DashboardAction, DashboardSnapshot, Task, WorkflowWarning } from "../types";
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
  onAction: DashboardAction;
}

export function ControlPlane(props: ControlPlaneProps) {
  const { snapshot, warnings, selectedGoalId, selectedTaskId, selectedMemberId } = props;
  const goal = activeGoal(snapshot, selectedGoalId);
  const tasks = tasksForGoal(snapshot, goal?.id);
  const members = membersForTasks(snapshot, tasks);
  const teams = teamsForMembers(snapshot.teams, members);
  const scopedWarnings = warningsForScope(warnings, goal?.id, tasks, members);
  const selectedTask = tasks.find((task) => task.id === selectedTaskId) ?? tasks[0];
  const selectedMember =
    members.find((member) => member.id === selectedMemberId) ??
    memberForTask(members, selectedTask) ??
    memberForTask(snapshot.members, selectedTask);

  return (
    <section className="controlPlane">
      <aside className="rail">
        <h2>Goals</h2>
        <div className="railList">
          {snapshot.goals.map((item) => (
            <button
              className={`railItem ${goal?.id === item.id ? "active" : ""}`}
              type="button"
              key={item.id}
              onClick={() => props.onSelectGoal(item.id)}
            >
              <strong>{item.title || item.id}</strong>
              <span>{item.status || "active"}</span>
            </button>
          ))}
        </div>
        <h2>Teams</h2>
        <TeamRoster teams={teams} members={members} onSelectMember={props.onSelectMember} />
      </aside>

      <main className="workbench">
        <GoalHeader goal={goal} taskCount={tasks.length} warningCount={scopedWarnings.length} />
        <KanbanBoard tasks={tasks} selectedTaskId={selectedTask?.id} onSelectTask={props.onSelectTask} />
        <TaskDetail
          snapshot={snapshot}
          task={selectedTask}
          warnings={scopedWarnings}
          onSelectMember={props.onSelectMember}
          onAction={props.onAction}
        />
      </main>

      <aside className="rightRail">
        <MemberDetail
          snapshot={snapshot}
          member={selectedMember}
          onSelectTask={props.onSelectTask}
          onAction={props.onAction}
        />
        <WarningsPanel
          warnings={scopedWarnings}
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
