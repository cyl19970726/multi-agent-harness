import {
  AlertTriangle,
  CheckCircle2,
  CircleDot,
  ExternalLink,
  FileText,
  GitPullRequest,
  GitBranch,
  Inbox,
  MessageSquare,
  PlayCircle,
  Send,
  ShieldCheck,
  TimerReset,
  Workflow,
} from "lucide-react";
import type { ReactNode } from "react";
import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";
import { memberName, objectShortId, taskTitle } from "../model/readModel";
import type { Task, WorkflowWarning } from "../types";
import { ActionButton, EmptyState, SectionPanel, SegmentedControl, StatusBadge, TimelineRow } from "../ui/primitives";

type SelectionPatch = Partial<SelectionState>;

interface SurfaceProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: SelectionPatch) => void;
}

export function TeamWorkspace({ model, onSelectionChange }: SurfaceProps) {
  return (
    <div className="surfaceStack teamWorkspaceSurface">
      <SurfaceHeader
        kicker="Persistent AgentTeam"
        title={model.selectedTeam?.name ?? "No active team"}
        body="Standing members, current work, messages, decisions, and warnings stay visible together."
        actions={
          <>
            <ActionButton icon={Send} tone="primary">Message member</ActionButton>
            <ActionButton icon={ShieldCheck}>Request review</ActionButton>
          </>
        }
      />

      <section className="goalStrip">
        <div>
          <span>Active Vision / Goal</span>
          <strong>{model.selectedGoal?.title ?? "Missing active goal"}</strong>
          <p>{model.selectedGoal?.objective}</p>
        </div>
        <div className="goalProof">
          <StatusBadge tone="info">{model.goalTasks.length} tasks</StatusBadge>
          <StatusBadge tone={model.warnings.length ? "warn" : "good"}>{model.warnings.length} warnings</StatusBadge>
          <StatusBadge tone={model.decisionQueue.length ? "warn" : "good"}>{model.decisionQueue.length} decision items</StatusBadge>
        </div>
      </section>

      <div className="teamWorkspaceGrid">
        <SectionPanel title="Canonical Activity" kicker="Messages / tasks / evidence / decisions" className="activityPanel">
          <div className="timelineList">
            {model.activity.map((item) => (
              <TimelineRow
                key={item.id}
                kind={item.kind}
                title={item.title}
                meta={item.meta}
                body={item.body}
                severity={item.severity}
                onClick={() => item.objectRef && onSelectionChange({ taskId: item.objectRef, surface: "task" })}
              />
            ))}
          </div>
        </SectionPanel>

        <SectionPanel title="Current Work Pressure" kicker="Task lanes and decision pressure" className="workPressurePanel">
          <LaneBoard tasks={model.goalTasks} compact onSelectTask={(task) => onSelectionChange({ taskId: task.id, surface: "task" })} />
        </SectionPanel>
      </div>

      <div className="queueBand">
        <QueueColumn
          title="Decision Queue"
          items={model.decisionQueue}
          empty="No pending decisions"
          onSelect={(objectRef) => objectRef && onSelectionChange({ taskId: objectRef, surface: "decisions" })}
        />
        <WarningColumn warnings={model.warnings.slice(0, 5)} onSelect={(warning) => onSelectionChange(warning.taskId ? { taskId: warning.taskId, surface: "warnings" } : { surface: "warnings" })} />
      </div>
    </div>
  );
}

export function VisionOverview({ model, onSelectionChange }: SurfaceProps) {
  return (
    <div className="surfaceStack">
      <SurfaceHeader
        kicker="Vision overview"
        title="Workbench self-hosting vision"
        body="Goals are grouped by state so completion depends on Decision and GoalEvaluation, not task count."
      />
      <div className="visionGrid">
        <SectionPanel title="Active Goals" kicker="Not complete until evaluated">
          <GoalList goals={model.activeGoals} fallback={model.selectedGoal ? [model.selectedGoal] : []} onSelect={(goalId) => onSelectionChange({ goalId, surface: "goal" })} />
        </SectionPanel>
        <SectionPanel title="Completed Goals" kicker="Decision and evaluation proof">
          <GoalList goals={model.completeGoals} fallback={[]} onSelect={(goalId) => onSelectionChange({ goalId, surface: "goal" })} />
        </SectionPanel>
        <SectionPanel title="Proposed / Next" kicker="Distance-to-vision loop">
          <GoalList goals={model.proposedGoals} fallback={[]} onSelect={(goalId) => onSelectionChange({ goalId, surface: "goal" })} />
          {model.snapshot.autonomous_proposals?.slice(0, 3).map((proposal) => (
            <div key={proposal.id} className="proposalRow">
              <strong>{proposal.summary}</strong>
              <small>{proposal.disposition ?? "pending"} · evidence {proposal.linked_evidence_ids?.join(", ") || "missing"}</small>
            </div>
          ))}
        </SectionPanel>
        <SectionPanel title="Distance To Vision" kicker="Gaps before next goal">
          <div className="distanceStack">
            <StatusBadge tone={model.warnings.length ? "warn" : "good"}>{model.warnings.length} workflow gaps</StatusBadge>
            <p>GoalEvaluation remains the acceptance closeout. Proposed next goals must point back to evidence or evaluation.</p>
            <ActionButton icon={Workflow} onClick={() => onSelectionChange({ surface: "graph" })}>Open graph/Kanban</ActionButton>
          </div>
        </SectionPanel>
      </div>
    </div>
  );
}

export function MemberWorkbench({ model, onSelectionChange }: SurfaceProps) {
  const member = model.selectedMember;
  if (!member) {
    return <EmptyState title="No AgentMember selected" body="Select a durable member from the team rail." />;
  }

  return (
    <div className="memberRoute">
      <section className="memberMeta">
        <span className="avatar large">{initials(member.name ?? member.id)}</span>
        <h1>{member.name ?? member.id}</h1>
        <p>{member.role ?? "Member"} · {member.runtime_status ?? member.status ?? "unknown"}</p>
        <div className="inlineBadges">
          <StatusBadge tone={member.runtime_alive ? "good" : "warn"}>{member.runtime_alive ? "runtime alive" : "runtime unknown"}</StatusBadge>
          <StatusBadge tone="info">{member.provider ?? "provider neutral"}</StatusBadge>
        </div>
        <dl className="metaList">
          <div><dt>Current task</dt><dd>{taskTitle(model.tasks, member.current_task_id)}</dd></div>
          <div><dt>Prompt</dt><dd>{member.prompt_ref ?? "not recorded"}</dd></div>
          <div><dt>Skills</dt><dd>{member.skill_refs?.join(", ") || "none"}</dd></div>
        </dl>
      </section>

      <section className="memberMain">
        <div className="memberActionRow">
          <ActionButton icon={Send} tone="primary">Send message</ActionButton>
          <ActionButton icon={Inbox}>Deliver queued</ActionButton>
          <ActionButton icon={TimerReset}>Retry failed</ActionButton>
          <ActionButton icon={FileText} onClick={() => member.current_task_id && onSelectionChange({ taskId: member.current_task_id, surface: "task" })}>
            Open task
          </ActionButton>
        </div>
        <div className="inboxOutbox">
          <div><strong>{member.inbox_count ?? 0}</strong><span>Inbox</span></div>
          <div><strong>{member.queued_count ?? 0}</strong><span>Queued</span></div>
          <div><strong>{model.selectedMemberMessages.length}</strong><span>Messages</span></div>
        </div>
        <SectionPanel title="Chronological Activity" kicker="Assignment before report and evidence">
          <div className="timelineList">
            {model.selectedMemberTimeline.map((item) => (
              <TimelineRow
                key={item.id}
                kind={item.kind}
                title={item.title}
                meta={item.meta}
                body={item.body}
                severity={item.severity}
                onClick={() => item.objectRef && onSelectionChange({ taskId: item.objectRef, surface: "task" })}
              />
            ))}
          </div>
        </SectionPanel>
      </section>

      <aside className="runtimePanel">
        <SectionPanel title="Runtime Health" kicker="Process / endpoint / protocol / delivery">
          <div className="runtimeStack">
            <RuntimeLine label="Process" value={member.runtime_alive ? "alive" : "unknown"} tone={member.runtime_alive ? "good" : "warn"} />
            <RuntimeLine label="Endpoint" value={member.control_endpoint ?? "not exposed"} tone="info" />
            <RuntimeLine label="Protocol" value={member.provider_thread_id ?? "message-first"} tone="info" />
            <RuntimeLine label="Delivery" value={`${member.queued_count ?? 0} queued`} tone={member.queued_count ? "warn" : "good"} />
          </div>
        </SectionPanel>
        <SectionPanel title="Sessions" kicker="Provider state under member identity">
          <div className="sessionList">
            {model.sessionsByMember.length ? model.sessionsByMember.map((session) => (
              <div key={session.id} className="sessionRow">
                <strong>{session.status ?? "unknown"}</strong>
                <small>{session.prompt_summary ?? session.command ?? session.id}</small>
              </div>
            )) : <EmptyState title="No sessions" body="This member has no provider sessions in the current snapshot." />}
          </div>
        </SectionPanel>
      </aside>
    </div>
  );
}

export function GoalDocument({ model, onSelectionChange }: SurfaceProps) {
  const goal = model.selectedGoal;
  if (!goal) return <EmptyState title="No goal selected" body="Load a snapshot or select a goal." />;

  const learning = model.snapshot.goal_learning_status?.find((status) => status.goal_id === goal.id);

  return (
    <DocumentRoute
      sideTitle="Goal document"
      navItems={["Objective", "GoalDesign", "Team", "Branch / PR", "Graph / Board", "Evidence", "Evaluation"]}
      proofTitle="Goal proof"
      proof={
        <>
          <ProofLine label="GoalDesign" complete={(learning?.goal_design?.length ?? 0) > 0} />
          <ProofLine label="Decision" complete={model.decisions.some((decision) => model.goalTasks.some((task) => task.id === decision.task_id))} />
          <ProofLine label="GoalEvaluation" complete={(learning?.goal_evaluation?.length ?? 0) > 0} />
          <ProofLine label="Warnings" complete={!model.warnings.some((warning) => warning.goalId === goal.id)} />
        </>
      }
    >
      <SurfaceHeader kicker="Goal work document" title={goal.title ?? goal.id} body={goal.objective ?? "No objective recorded."} />
      <SectionPanel title="Objective And Success Criteria" kicker="Completion is not inferred from task status">
        <ul className="criteriaList">
          {(goal.success_criteria ?? ["No success criteria recorded."]).map((item) => <li key={item}>{item}</li>)}
        </ul>
      </SectionPanel>
      <SectionPanel title="GoalDesign And Team Design" kicker="Design gate before implementation">
        <p>Scenario: self-hosting the harness frontend through its own Workbench objects.</p>
        <p>Team: {model.selectedTeam?.name ?? "team missing"} with {model.members.length} persistent members.</p>
      </SectionPanel>
      <SectionPanel title="Branch / Worktree / PR Policy" kicker="Integration proof">
        <div className="proofGrid">
          <span><GitBranch size={15} /> goal/dashboard-collaboration-workspace</span>
          <span><GitPullRequest size={15} /> PR #6 reset/rebuild</span>
          <span><FileText size={15} /> page-local layout contracts</span>
        </div>
      </SectionPanel>
      <SectionPanel title="Graph / Kanban Preview" kicker="Dependencies and execution lanes">
        <LaneBoard tasks={model.goalTasks} compact onSelectTask={(task) => onSelectionChange({ taskId: task.id, surface: "task" })} />
      </SectionPanel>
    </DocumentRoute>
  );
}

export function TaskDocument({ model, onSelectionChange }: SurfaceProps) {
  const task = model.selectedTask;
  if (!task) return <EmptyState title="No task selected" body="Select a task from the lanes." />;

  const assignment = model.messages.find((message) => message.task_id === task.id && message.kind === "task");
  const report = model.messages.find((message) => message.task_id === task.id && message.kind === "report");
  const evidence = model.evidence.filter((item) => item.task_id === task.id);
  const proposal = model.proposals.find((item) => item.task_id === task.id);
  const decision = model.decisions.find((item) => item.task_id === task.id);

  return (
    <DocumentRoute
      sideTitle="Task proof"
      navItems={["Objective", "Assignment", "Report", "Evidence", "Proposal", "Review", "Decision"]}
      proofTitle="Protocol order"
      proof={
        <>
          <ProofLine label="Assignment message" complete={Boolean(assignment)} />
          <ProofLine label="Report message" complete={Boolean(report)} />
          <ProofLine label="Evidence" complete={evidence.length > 0} />
          <ProofLine label="Proposal" complete={Boolean(proposal)} />
          <ProofLine label="Decision" complete={Boolean(decision)} />
        </>
      }
    >
      <SurfaceHeader kicker="Task work document" title={task.title ?? task.id} body={task.objective ?? "No objective recorded."} />
      <SectionPanel title="Objective And Acceptance" kicker="Assignable reviewable unit">
        <ul className="criteriaList">
          {(task.acceptance_criteria ?? ["No acceptance criteria recorded."]).map((item) => <li key={item}>{item}</li>)}
        </ul>
      </SectionPanel>
      <SectionPanel title="Assignment Proof" kicker="Message(kind=task) before report">
        {assignment ? <TimelineRow kind="message" title="Task assignment" meta={assignment.delivery_status} body={assignment.content} /> : <EmptyState title="Assignment missing" body="Assignee field alone does not prove assignment." />}
      </SectionPanel>
      <SectionPanel title="Report And Runtime" kicker="Assignee report connected to member runtime">
        {report ? <TimelineRow kind="report" title="Member report" meta={report.delivery_status} body={report.content} /> : <EmptyState title="Report missing" body="No report message is linked to this task yet." />}
      </SectionPanel>
      <SectionPanel title="Evidence / Proposal / Checks" kicker="PR refs stay near proof">
        <div className="proofGrid">
          <span><FileText size={15} /> {evidence.length} evidence refs</span>
          <span><GitPullRequest size={15} /> {objectShortId(task.pr_ref)}</span>
          <span><GitBranch size={15} /> {objectShortId(task.branch_ref)}</span>
        </div>
        <p>{proposal?.summary ?? "No proposal summary yet."}</p>
        <ActionButton icon={ExternalLink} onClick={() => onSelectionChange({ memberId: task.assignee_agent_id ?? undefined, surface: "member" })}>Open assignee</ActionButton>
      </SectionPanel>
    </DocumentRoute>
  );
}

export function GraphKanban({ model, mode, onSelectionChange }: SurfaceProps & { mode: "kanban" | "graph" | "split" }) {
  return (
    <div className="surfaceStack graphSurface">
      <SurfaceHeader
        kicker="Graph / Kanban"
        title="TaskGraph execution and relationship view"
        body="Kanban is the operational default. Graph focus explains dependencies and blockers without taking over Team."
        actions={
          <SegmentedControl
            label="Graph mode"
            value={mode}
            onChange={(value) => onSelectionChange({ mode: value })}
            options={[
              { value: "kanban", label: "Kanban" },
              { value: "graph", label: "Graph" },
              { value: "split", label: "Split" },
            ]}
          />
        }
      />
      <div className={`graphGrid ${mode}`}>
        <SectionPanel title="Kanban Lanes" kicker="Operational state">
          <LaneBoard tasks={model.goalTasks} onSelectTask={(task) => onSelectionChange({ taskId: task.id, surface: "task" })} />
        </SectionPanel>
        <SectionPanel title="Graph Focus" kicker="Dependencies, blockers, follow-ups">
          <GraphPreview tasks={model.goalTasks} selectedTaskId={model.selectedTask?.id} onSelectTask={(task) => onSelectionChange({ taskId: task.id, surface: "task" })} />
        </SectionPanel>
        <SectionPanel title="Selected Object" kicker="Synchronized card/node">
          <p>{model.selectedTask?.title ?? "No task selected"}</p>
          <p>{model.selectedTask?.objective}</p>
          <div className="inlineBadges">
            <StatusBadge tone="info">{model.selectedTask?.status ?? "none"}</StatusBadge>
            <StatusBadge>{model.selectedTask?.depends_on_task_ids?.length ?? 0} deps</StatusBadge>
          </div>
        </SectionPanel>
      </div>
    </div>
  );
}

export function DocsContext({ model }: SurfaceProps) {
  return (
    <div className="surfaceStack">
      <SurfaceHeader
        kicker="Mounted docs"
        title="Docs connected to active work"
        body="Docs stay source-linked and explain why they matter to the selected Goal, Task, Member, or Decision."
      />
      <div className="docsGrid">
        <SectionPanel title="Related Docs" kicker="Source-linked context">
          <div className="docRows">
            {model.docs.map((doc) => (
              <a key={doc.path} className="docRow" href={`../../${doc.path}`}>
                <strong>{doc.title}</strong>
                <span>{doc.reason}</span>
                <small>{doc.path} · {doc.lifecycle}</small>
              </a>
            ))}
          </div>
        </SectionPanel>
        <SectionPanel title="Missing Context Warnings" kicker="Knowledge routing">
          <WarningColumn warnings={model.warnings.filter((warning) => warning.kind.includes("goal") || warning.kind.includes("evidence"))} />
        </SectionPanel>
      </div>
    </div>
  );
}

export function DecisionCenter({ model, onSelectionChange }: SurfaceProps) {
  const selectedTask = model.selectedTask;
  return (
    <div className="surfaceStack">
      <SurfaceHeader
        kicker="Evidence / Proposal / Review / Decision"
        title="Acceptance proof center"
        body="Acceptance stages stay visually distinct so missing proof cannot look complete."
      />
      <div className="proofStrip">
        <ProofStage title="Evidence" complete={model.evidence.some((item) => item.task_id === selectedTask?.id)} body={`${model.evidence.length} refs`} />
        <ProofStage title="Proposal" complete={model.proposals.some((item) => item.task_id === selectedTask?.id)} body={`${model.proposals.length} proposals`} />
        <ProofStage title="Review" complete={model.messages.some((item) => item.kind === "report" && item.task_id === selectedTask?.id)} body="critic/report rows" />
        <ProofStage title="Decision" complete={model.decisions.some((item) => item.task_id === selectedTask?.id)} body={`${model.decisions.length} decisions`} />
      </div>
      <QueueColumn
        title="Global Decision Queue"
        items={model.decisionQueue}
        empty="No pending acceptance work"
        onSelect={(objectRef) => objectRef && onSelectionChange({ taskId: objectRef, surface: "task" })}
      />
    </div>
  );
}

export function WarningsRepair({ model, onSelectionChange }: SurfaceProps) {
  return (
    <div className="surfaceStack">
      <SurfaceHeader
        kicker="Warnings / repair"
        title="Workflow risk queue"
        body="Each warning names what is wrong, where it is, why it matters, and the safe path."
      />
      <div className="warningGrid">
        <SectionPanel title="Severity Groups" kicker="Not color-only">
          <WarningColumn warnings={model.warnings} onSelect={(warning) => warning.taskId && onSelectionChange({ taskId: warning.taskId, surface: "task" })} />
        </SectionPanel>
        <SectionPanel title="Repair Panel" kicker="Safe action or disabled reason">
          {model.warnings[0] ? (
            <div className="repairPanel">
              <AlertTriangle size={28} aria-hidden="true" />
              <h2>{model.warnings[0].kind}</h2>
              <p>{model.warnings[0].summary}</p>
              <p>Affected object: {model.warnings[0].taskId ?? model.warnings[0].goalId ?? model.warnings[0].memberId ?? "unknown"}</p>
              <ActionButton icon={PlayCircle} disabled>Repair API not wired yet</ActionButton>
            </div>
          ) : (
            <EmptyState title="No warnings" body="The current snapshot has no advisory warning rows." />
          )}
        </SectionPanel>
      </div>
    </div>
  );
}

export function DebugSurface({ model, sourceLabel }: { model: WorkbenchModel; sourceLabel: string }) {
  return (
    <div className="surfaceStack debugSurface">
      <SurfaceHeader
        kicker="Debug"
        title="Raw snapshot is secondary"
        body="Debug is explicit and source-labeled. It is never the primary Workbench route."
      />
      <div className="debugGrid">
        <SectionPanel title="Source" kicker="Live/offline state">
          <p>{sourceLabel}</p>
          <p>{model.snapshot.generated_at ?? "No generated_at in snapshot."}</p>
        </SectionPanel>
        <SectionPanel title="Raw Snapshot" kicker="Diagnosis only">
          <pre>{JSON.stringify(model.snapshot, null, 2)}</pre>
        </SectionPanel>
      </div>
    </div>
  );
}

function SurfaceHeader({ kicker, title, body, actions }: { kicker: string; title: string; body: string; actions?: ReactNode }) {
  return (
    <header className="surfaceHeader">
      <div>
        <p>{kicker}</p>
        <h1>{title}</h1>
        <span>{body}</span>
      </div>
      {actions && <div className="surfaceActions">{actions}</div>}
    </header>
  );
}

function DocumentRoute({
  sideTitle,
  navItems,
  proofTitle,
  proof,
  children,
}: {
  sideTitle: string;
  navItems: string[];
  proofTitle: string;
  proof: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="documentRoute">
      <aside className="docRail">
        <strong>{sideTitle}</strong>
        {navItems.map((item, index) => (
          <a key={item} href={`#section-${index}`}>{item}</a>
        ))}
      </aside>
      <div className="documentBody">{children}</div>
      <aside className="proofPanel">
        <h2>{proofTitle}</h2>
        <div className="proofLines">{proof}</div>
      </aside>
    </div>
  );
}

function LaneBoard({
  tasks,
  compact,
  onSelectTask,
}: {
  tasks: Task[];
  compact?: boolean;
  onSelectTask: (task: Task) => void;
}) {
  const statuses = compact ? ["planned", "running", "review", "done"] : ["planned", "assigned", "running", "blocked", "review", "done"];
  return (
    <div className={`laneBoard${compact ? " compact" : ""}`}>
      {statuses.map((status) => {
        const laneTasks = tasks.filter((task) => task.status === status);
        return (
          <section key={status} className="lane">
            <header>
              <strong>{status}</strong>
              <span>{laneTasks.length}</span>
            </header>
            <div className="laneTasks">
              {laneTasks.length ? laneTasks.map((task) => (
                <button key={task.id} type="button" className="taskRow" onClick={() => onSelectTask(task)}>
                  <strong>{task.title ?? task.id}</strong>
                  <small>{memberName([], task.assignee_agent_id)} · {objectShortId(task.branch_ref)}</small>
                </button>
              )) : <span className="laneEmpty">No work</span>}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function GraphPreview({ tasks, selectedTaskId, onSelectTask }: { tasks: Task[]; selectedTaskId?: string; onSelectTask: (task: Task) => void }) {
  return (
    <div className="graphPreview">
      <svg viewBox="0 0 520 220" role="img" aria-label="Task dependency graph">
        <path d="M92 70 C160 70 160 122 228 122" />
        <path d="M228 122 C302 122 302 70 382 70" />
        <path d="M228 122 C302 122 302 170 382 170" />
      </svg>
      <div className="graphNodes">
        {tasks.slice(0, 5).map((task, index) => (
          <button
            key={task.id}
            type="button"
            className={`graphNode node${index}${task.id === selectedTaskId ? " active" : ""}`}
            onClick={() => onSelectTask(task)}
          >
            <CircleDot size={14} aria-hidden="true" />
            <span>{task.title ?? task.id}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

function GoalList({ goals, fallback, onSelect }: { goals: { id: string; title?: string; status?: string; objective?: string }[]; fallback: { id: string; title?: string; status?: string; objective?: string }[]; onSelect: (goalId: string) => void }) {
  const rows = goals.length ? goals : fallback;
  if (!rows.length) return <EmptyState title="No goals" body="This group has no goals in the current snapshot." />;
  return (
    <div className="goalRows">
      {rows.map((goal) => (
        <button key={goal.id} type="button" className="goalRow" onClick={() => onSelect(goal.id)}>
          <strong>{goal.title ?? goal.id}</strong>
          <span>{goal.objective}</span>
          <StatusBadge tone={goal.status === "complete" ? "good" : goal.status === "blocked" ? "bad" : "info"}>{goal.status ?? "active"}</StatusBadge>
        </button>
      ))}
    </div>
  );
}

function QueueColumn({ title, items, empty, onSelect }: { title: string; items: { id: string; kind: string; title: string; meta: string; body?: string; severity?: WorkflowWarning["severity"]; objectRef?: string }[]; empty: string; onSelect?: (objectRef?: string) => void }) {
  return (
    <SectionPanel title={title} kicker="Review pressure">
      <div className="timelineList compact">
        {items.length ? items.map((item) => (
          <TimelineRow key={item.id} kind={item.kind} title={item.title} meta={item.meta} body={item.body} severity={item.severity} onClick={() => onSelect?.(item.objectRef)} />
        )) : <EmptyState title={empty} body="No queue items in the current snapshot." />}
      </div>
    </SectionPanel>
  );
}

function WarningColumn({ warnings, onSelect }: { warnings: WorkflowWarning[]; onSelect?: (warning: WorkflowWarning) => void }) {
  return (
    <div className="warningRows">
      {warnings.length ? warnings.map((warning) => (
        <button key={warning.id} type="button" className={`warningRow ${warning.severity}`} onClick={() => onSelect?.(warning)}>
          <AlertTriangle size={16} aria-hidden="true" />
          <span>
            <strong>{warning.kind}</strong>
            <small>{warning.summary}</small>
          </span>
          <StatusBadge tone={warning.severity === "high" ? "bad" : warning.severity === "medium" ? "warn" : "info"}>{warning.severity}</StatusBadge>
        </button>
      )) : <EmptyState title="No warnings" body="No workflow risks are projected for this object." />}
    </div>
  );
}

function RuntimeLine({ label, value, tone }: { label: string; value: string; tone: "good" | "warn" | "info" | "bad" | "muted" }) {
  return (
    <div className="runtimeLine">
      <span>{label}</span>
      <StatusBadge tone={tone}>{value}</StatusBadge>
    </div>
  );
}

function ProofLine({ label, complete }: { label: string; complete: boolean }) {
  return (
    <div className="proofLine">
      {complete ? <CheckCircle2 size={16} aria-hidden="true" /> : <AlertTriangle size={16} aria-hidden="true" />}
      <span>{label}</span>
      <StatusBadge tone={complete ? "good" : "warn"}>{complete ? "present" : "missing"}</StatusBadge>
    </div>
  );
}

function ProofStage({ title, complete, body }: { title: string; complete: boolean; body: string }) {
  return (
    <section className={`proofStage ${complete ? "complete" : "missing"}`}>
      {complete ? <CheckCircle2 size={18} aria-hidden="true" /> : <AlertTriangle size={18} aria-hidden="true" />}
      <h2>{title}</h2>
      <p>{body}</p>
    </section>
  );
}

function initials(value: string): string {
  return value
    .split(/[-_\s]/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}
