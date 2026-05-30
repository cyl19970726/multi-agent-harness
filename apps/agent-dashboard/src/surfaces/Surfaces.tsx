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
import type { Goal, Task, WorkflowWarning } from "../types";
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
  const selectedGoal = model.selectedGoal;
  const nextProposal = model.snapshot.autonomous_proposals?.[0];
  const proposedGoalCount = model.proposedGoals.length;
  const nextProposalCount = model.snapshot.autonomous_proposals?.length ?? 0;
  const goalGroups = [
    { id: "active", title: "Active", goals: model.activeGoals, fallback: selectedGoal ? [selectedGoal] : [] },
    { id: "complete", title: "Completed", goals: model.completeGoals, fallback: [] },
    { id: "blocked", title: "Blocked", goals: model.blockedGoals, fallback: [] },
    { id: "proposed", title: "Proposed", goals: model.proposedGoals, fallback: [] },
  ];

  return (
    <div className="visionPage">
      <section className="visionHero pageHero">
        <div>
          <p className="heroKicker">Vision overview</p>
          <h1>Workbench self-hosting vision</h1>
          <p>Track whether active, completed, blocked, and proposed goals are moving the harness toward a reusable self-hosting workflow.</p>
        </div>
        <div className="heroActions">
          <ActionButton icon={Workflow} onClick={() => onSelectionChange({ surface: "graph" })}>Open graph</ActionButton>
          <ActionButton icon={FileText} onClick={() => selectedGoal && onSelectionChange({ goalId: selectedGoal.id, surface: "goal" })}>Open selected goal</ActionButton>
        </div>
      </section>

      <div className="visionStatusStrip">
        <ProofMetric label="Active" value={String(model.activeGoals.length || (selectedGoal ? 1 : 0))} tone="info" caption="not complete" />
        <ProofMetric label="Completed" value={String(model.completeGoals.length)} tone="good" caption="decision + evaluation" />
        <ProofMetric label="Blocked" value={String(model.blockedGoals.length)} tone={model.blockedGoals.length ? "bad" : "good"} caption="needs lead action" />
        <ProofMetric label="Next items" value={String(proposedGoalCount + nextProposalCount)} tone="warn" caption={`${proposedGoalCount} goals + ${nextProposalCount} proposals`} />
      </div>

      <div className="visionLayout">
        <section className="visionCollection">
          <header className="pageSectionHeader">
            <span>Goal collection</span>
            <strong>Completion is proven by Decision and GoalEvaluation, not task count.</strong>
          </header>
          <div className="visionGroupGrid">
            {goalGroups.map((group) => (
              <section key={group.id} className={`visionGoalGroup ${group.id}`}>
                <header>
                  <span>{group.title}</span>
                  <strong>{group.goals.length || group.fallback.length}</strong>
                </header>
                <div className="visionGoalRows">
                  {(group.goals.length ? group.goals : group.fallback).map((goal) => (
                    <VisionGoalRow key={goal.id} goal={goal} model={model} onSelect={() => onSelectionChange({ goalId: goal.id, surface: "goal" })} />
                  ))}
                  {!group.goals.length && !group.fallback.length && <EmptyState title={`No ${group.title.toLowerCase()} goals`} body="This group is empty in the current snapshot." />}
                </div>
              </section>
            ))}
          </div>
        </section>

        <aside className="visionContextRail">
          <PageSection kicker="Distance-to-vision" title="Next gap to close">
            <div className="contextStack">
              <StatusBadge tone={model.warnings.length ? "warn" : "good"}>{model.warnings.length} workflow gaps</StatusBadge>
              <p>GoalEvaluation must explain what moved the product closer to the Vision before the next Goal is accepted.</p>
            </div>
          </PageSection>
          <PageSection kicker="Next-round proposal" title={nextProposal?.summary ?? "No accepted next proposal yet"}>
            <p>{nextProposal ? `${nextProposal.disposition ?? "pending"} · source ${nextProposal.source_ref ?? nextProposal.source_type ?? "unknown"}` : "Observer proposals will appear here when linked to evidence or evaluation."}</p>
            <div className="inlineBadges">
              <StatusBadge tone={nextProposal?.linked_evidence_ids?.length ? "good" : "warn"}>{nextProposal?.linked_evidence_ids?.length ?? 0} evidence refs</StatusBadge>
              <StatusBadge tone="info">{nextProposal?.follow_up_task_ids?.length ?? 0} follow-up tasks</StatusBadge>
            </div>
          </PageSection>
          <PageSection kicker="Selected goal" title={selectedGoal?.title ?? "No selected goal"}>
            <p>{selectedGoal?.objective ?? "Select a goal from the collection to inspect the work document."}</p>
            <ActionButton icon={FileText} onClick={() => selectedGoal && onSelectionChange({ goalId: selectedGoal.id, surface: "goal" })}>Open goal document</ActionButton>
          </PageSection>
        </aside>
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
  const goalTaskIds = new Set(model.goalTasks.map((task) => task.id));
  const goalEvidence = model.evidence.filter((item) => item.task_id && goalTaskIds.has(item.task_id));
  const goalProposals = model.proposals.filter((item) => goalTaskIds.has(item.task_id));
  const goalDecisions = model.decisions.filter((item) => goalTaskIds.has(item.task_id));
  const goalReviewMessages = model.messages.filter((message) => {
    if (!message.task_id || !goalTaskIds.has(message.task_id) || message.kind !== "report") return false;
    const reviewedTask = model.tasks.find((task) => task.id === message.task_id);
    return reviewedTask?.reviewer_agent_id === message.from_agent_id;
  });
  const goalWarnings = model.warnings.filter((warning) => warning.goalId === goal.id || (warning.taskId && goalTaskIds.has(warning.taskId)));
  const taskCounts = countTasks(model.goalTasks);
  const firstBranch = uniqueValues(model.goalTasks.map((task) => task.branch_ref))[0] ?? "goal branch missing";
  const firstPr = uniqueValues(model.goalTasks.map((task) => task.pr_ref))[0] ?? "PR missing";
  const branchCount = uniqueValues(model.goalTasks.map((task) => task.branch_ref)).length;
  const prCount = uniqueValues(model.goalTasks.map((task) => task.pr_ref)).length;

  return (
    <div className="goalPage">
      <section className="goalHero pageHero">
        <div>
          <p className="heroKicker">Goal work document</p>
          <h1>{goal.title ?? goal.id}</h1>
          <p>{goal.objective ?? "No objective recorded."}</p>
          <div className="heroMeta">
            <StatusBadge tone={goal.status === "complete" ? "good" : goal.status === "blocked" ? "bad" : "info"}>{goal.status ?? "active"}</StatusBadge>
            <StatusBadge tone="info">owner {memberName(model.members, goal.owner_agent_id)}</StatusBadge>
            <StatusBadge tone={goalWarnings.length ? "warn" : "good"}>{goalWarnings.length} warnings</StatusBadge>
          </div>
        </div>
        <div className="heroActions">
          <ActionButton icon={Workflow} onClick={() => onSelectionChange({ surface: "graph" })}>Graph/Kanban</ActionButton>
          <ActionButton icon={ShieldCheck} onClick={() => onSelectionChange({ surface: "decisions" })}>Review proof</ActionButton>
        </div>
      </section>

      <div className="goalStatusStrip">
        <ProofMetric label="GoalDesign" value={(learning?.goal_design?.length ?? 0) > 0 ? "present" : "missing"} tone={(learning?.goal_design?.length ?? 0) > 0 ? "good" : "warn"} caption="design gate" />
        <ProofMetric label="Tasks" value={`${model.goalTasks.length}`} tone="info" caption={`${taskCounts.running + taskCounts.review} active`} />
        <ProofMetric label="Decision" value={`${goalDecisions.length}`} tone={goalDecisions.length ? "good" : "warn"} caption="acceptance proof" />
        <ProofMetric label="Evaluation" value={(learning?.goal_evaluation?.length ?? 0) > 0 ? "present" : "missing"} tone={(learning?.goal_evaluation?.length ?? 0) > 0 ? "good" : "warn"} caption="closeout" />
      </div>

      <section className="goalProofMap" aria-label="Goal proof map">
        <header className="pageSectionHeader">
          <span>Durable goal proof</span>
          <strong>Proof map before the document body</strong>
        </header>
        <div className="proofMiniGrid">
          <ProofLine label={`Branch / PR refs ${branchCount}/${prCount}`} complete={branchCount > 0 && prCount > 0} />
          <ProofLine label={`TaskGraph mapped ${model.goalTasks.length} tasks`} complete={model.goalTasks.length > 0} />
          <ProofLine label={`Evidence refs ${goalEvidence.length}`} complete={goalEvidence.length > 0} />
          <ProofLine label={`Review reports ${goalReviewMessages.length}`} complete={goalReviewMessages.length > 0} />
          <ProofLine label={`Leader decisions ${goalDecisions.length}`} complete={goalDecisions.length > 0} />
          <ProofLine label="GoalEvaluation closeout" complete={(learning?.goal_evaluation?.length ?? 0) > 0} />
        </div>
      </section>

      <div className="goalWorkLayout">
        <PageIndex title="Goal sections" items={["Objective", "Design gate", "Branch / PR", "Execution", "Proof", "Evaluation"]} />

        <main className="goalDocumentFlow">
          <PageSection id={anchorId("Objective", 0)} kicker="Why this goal exists" title="Objective and success criteria">
            <ul className="criteriaList strongList">
              {(goal.success_criteria ?? ["No success criteria recorded."]).map((item) => <li key={item}>{item}</li>)}
            </ul>
          </PageSection>

          <section className="goalDesignBlock" id={anchorId("Design gate", 1)}>
            <header>
              <span>Design gate before implementation</span>
              <strong>GoalDesign and team design</strong>
            </header>
            <div className="designGrid">
              <div><span>Scenario</span><p>Self-hosting the harness frontend through its own Workbench objects.</p></div>
              <div><span>Team</span><p>{model.selectedTeam?.name ?? "team missing"} with {model.members.length} persistent members.</p></div>
              <div><span>Non-goal</span><p>Do not treat completed tasks as Goal completion without Decision and GoalEvaluation.</p></div>
              <div><span>Gate</span><p>{(learning?.goal_design?.length ?? 0) > 0 ? "GoalDesign evidence exists before implementation." : "GoalDesign evidence is missing."}</p></div>
            </div>
          </section>

          <PageSection id={anchorId("Branch / PR", 2)} kicker="Integration policy" title="Branch, PR, worktree, and docs stay near proof">
            <div className="proofGrid strongProofGrid">
              <span><GitBranch size={15} /> {firstBranch}</span>
              <span><GitPullRequest size={15} /> {firstPr}</span>
              <span><FileText size={15} /> page-local layout contracts</span>
            </div>
          </PageSection>

          <section className="goalExecutionBlock" id={anchorId("Execution", 3)}>
            <header>
              <span>TaskGraph execution</span>
              <strong>Kanban lanes plus dependency focus</strong>
            </header>
            <div className="goalExecutionGrid">
              <LaneBoard tasks={model.goalTasks} compact onSelectTask={(task) => onSelectionChange({ taskId: task.id, surface: "task" })} />
              <GraphPreview tasks={model.goalTasks} selectedTaskId={model.selectedTask?.id} onSelectTask={(task) => onSelectionChange({ taskId: task.id, surface: "task" })} />
            </div>
          </section>
        </main>

        <aside className="goalProofRail pageProofRail">
          <PageSection id={anchorId("Proof", 4)} kicker="Acceptance state" title="Goal proof">
            <div className="proofLines">
              <ProofLine label="GoalDesign" complete={(learning?.goal_design?.length ?? 0) > 0} />
              <ProofLine label="Review" complete={goalReviewMessages.length > 0} />
              <ProofLine label="Decision" complete={goalDecisions.length > 0} />
              <ProofLine label="Evidence" complete={goalEvidence.length > 0} />
              <ProofLine label="GoalEvaluation" complete={(learning?.goal_evaluation?.length ?? 0) > 0} />
            </div>
          </PageSection>
          <PageSection kicker="Review packet" title="Evidence, proposals, decisions">
            <div className="contextStack">
              <StatusBadge tone={goalEvidence.length ? "good" : "warn"}>{goalEvidence.length} evidence refs</StatusBadge>
              <StatusBadge tone={goalProposals.length ? "good" : "warn"}>{goalProposals.length} proposals</StatusBadge>
              <StatusBadge tone={goalDecisions.length ? "good" : "warn"}>{goalDecisions.length} decisions</StatusBadge>
            </div>
          </PageSection>
          <PageSection id={anchorId("Evaluation", 5)} kicker="Distance-to-vision" title={(learning?.goal_evaluation?.length ?? 0) > 0 ? "Evaluation recorded" : "Evaluation still missing"}>
            <p>Closeout should state what worked, what failed, remaining distance, and the next Goal or follow-up task.</p>
            <ActionButton icon={FileText} onClick={() => onSelectionChange({ surface: "vision" })}>Back to Vision</ActionButton>
          </PageSection>
        </aside>
      </div>
    </div>
  );
}

export function TaskDocument({ model, onSelectionChange }: SurfaceProps) {
  const task = model.selectedTask;
  if (!task) return <EmptyState title="No task selected" body="Select a task from the lanes." />;

  const assignment = model.messages.find((message) => message.task_id === task.id && message.kind === "task");
  const report = model.messages.find((message) => message.task_id === task.id && message.kind === "report");
  const review = model.messages.find(
    (message) => message.task_id === task.id && message.kind === "report" && message.from_agent_id === task.reviewer_agent_id,
  );
  const evidence = model.evidence.filter((item) => item.task_id === task.id);
  const proposal = model.proposals.find((item) => item.task_id === task.id);
  const decision = model.decisions.find((item) => item.task_id === task.id);
  const taskWarnings = model.warnings.filter((warning) => warning.taskId === task.id);
  const taskSessions = model.snapshot.provider_sessions?.filter((session) => session.task_id === task.id) ?? [];
  const proofSteps = [
    { label: "Assignment", complete: Boolean(assignment), body: assignment?.content ?? "No task message linked yet." },
    { label: "Report", complete: Boolean(report), body: report?.content ?? "No member report linked yet." },
    { label: "Evidence", complete: evidence.length > 0, body: `${evidence.length} evidence refs` },
    { label: "Proposal", complete: Boolean(proposal), body: proposal?.summary ?? "No proposal packet yet." },
    { label: "Review", complete: Boolean(review), body: review?.content ?? "No reviewer report linked yet." },
    { label: "Decision", complete: Boolean(decision), body: decision?.rationale ?? "No Leader decision yet." },
  ];

  return (
    <div className="taskPage">
      <section className="taskHero pageHero">
        <div>
          <p className="heroKicker">Task protocol document</p>
          <h1>{task.title ?? task.id}</h1>
          <p>{task.objective ?? "No objective recorded."}</p>
          <div className="heroMeta">
            <StatusBadge tone={task.status === "done" ? "good" : task.status === "blocked" ? "bad" : "info"}>{task.status}</StatusBadge>
            <StatusBadge tone="info">assignee {memberName(model.members, task.assignee_agent_id)}</StatusBadge>
            <StatusBadge tone="info">reviewer {memberName(model.members, task.reviewer_agent_id)}</StatusBadge>
          </div>
        </div>
        <div className="heroActions">
          <ActionButton icon={Send}>Deliver task</ActionButton>
          <ActionButton icon={ShieldCheck} onClick={() => onSelectionChange({ surface: "decisions" })}>Request review</ActionButton>
        </div>
      </section>

      <div className="taskProtocolStrip">
        {proofSteps.map((step, index) => (
          <TaskProofStep key={step.label} index={index + 1} label={step.label} complete={step.complete} />
        ))}
      </div>

      <div className="taskWorkLayout">
        <PageIndex title="Proof order" items={["Objective", "Protocol order", "Assignment report", "Evidence proposal", "Review", "Decision", "Refs"]} />

        <main className="taskDocumentFlow">
          <PageSection id={anchorId("Objective", 0)} kicker="Assignable reviewable unit" title="Objective and acceptance">
            <ul className="criteriaList strongList">
              {(task.acceptance_criteria ?? ["No acceptance criteria recorded."]).map((item) => <li key={item}>{item}</li>)}
            </ul>
          </PageSection>

          <section className="protocolTimeline" id={anchorId("Protocol order", 1)}>
            <header>
              <span>Canonical order</span>
              <strong>Assignment must precede report, evidence, proposal, review, and decision.</strong>
            </header>
            <div className="protocolSteps">
              {proofSteps.map((step, index) => (
                <article key={step.label} className={`protocolStep ${step.complete ? "complete" : "missing"}`}>
                  <span>{index + 1}</span>
                  <div>
                    <strong>{step.label}</strong>
                    <p>{step.body}</p>
                  </div>
                </article>
              ))}
            </div>
          </section>

          <section className="taskMessageGrid" id={anchorId("Assignment report", 2)}>
            <PageSection kicker="Message(kind=task)" title="Assignment proof">
              {assignment ? <TimelineRow kind="message" title="Task assignment" meta={assignment.delivery_status} body={assignment.content} /> : <EmptyState title="Assignment missing" body="Assignee field alone does not prove assignment." />}
            </PageSection>
            <PageSection kicker="Assignee report" title="Report and runtime">
              {report ? <TimelineRow kind="report" title="Member report" meta={report.delivery_status} body={report.content} /> : <EmptyState title="Report missing" body="No report message is linked to this task yet." />}
            </PageSection>
          </section>

          <PageSection id={anchorId("Evidence proposal", 3)} kicker="PR refs stay near proof" title="Evidence, proposal, checks">
            <div className="proofGrid strongProofGrid">
              <span><FileText size={15} /> {evidence.length} evidence refs</span>
              <span><GitPullRequest size={15} /> {objectShortId(task.pr_ref)}</span>
              <span><GitBranch size={15} /> {objectShortId(task.branch_ref)}</span>
            </div>
            <p>{proposal?.summary ?? "No proposal summary yet."}</p>
          </PageSection>
        </main>

        <aside className="taskContextRail pageProofRail">
          <PageSection kicker="Assignee runtime" title={memberName(model.members, task.assignee_agent_id)}>
            <div className="contextStack">
              <StatusBadge tone={taskSessions.length ? "good" : "warn"}>{taskSessions.length} sessions</StatusBadge>
              <StatusBadge tone={taskWarnings.length ? "warn" : "good"}>{taskWarnings.length} warnings</StatusBadge>
              <ActionButton icon={ExternalLink} onClick={() => onSelectionChange({ memberId: task.assignee_agent_id ?? undefined, surface: "member" })}>Open assignee</ActionButton>
            </div>
          </PageSection>
          <PageSection id={anchorId("Review", 4)} kicker="Reviewer proof" title={review ? "Review report recorded" : "Review report missing"}>
            <p>{review?.content ?? "Task cannot look complete until the reviewer reports on the proposal and evidence."}</p>
            <div className="proofLines">
              <ProofLine label="Review report" complete={Boolean(review)} />
              <ProofLine label="Evidence refs" complete={evidence.length > 0} />
            </div>
          </PageSection>
          <PageSection id={anchorId("Decision", 5)} kicker="Leader decision" title={decision ? `Decision: ${decision.decision ?? "recorded"}` : "Decision missing"}>
            <p>{decision?.rationale ?? "Task cannot look complete until review and Leader decision are recorded."}</p>
            <div className="proofLines">
              <ProofLine label="Review report" complete={Boolean(review)} />
              <ProofLine label="Leader decision" complete={Boolean(decision)} />
            </div>
          </PageSection>
          <PageSection id={anchorId("Refs", 6)} kicker="Owned paths" title="Branch / worktree / PR">
            <div className="contextStack">
              <StatusBadge tone="info">{objectShortId(task.branch_ref)}</StatusBadge>
              <StatusBadge tone={task.pr_ref ? "good" : "warn"}>{objectShortId(task.pr_ref)}</StatusBadge>
              <p>{task.owned_paths?.join(", ") || "No owned paths recorded."}</p>
            </div>
          </PageSection>
        </aside>
      </div>
    </div>
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

function ProofMetric({
  label,
  value,
  tone,
  caption,
}: {
  label: string;
  value: string;
  tone: "good" | "warn" | "bad" | "info" | "muted";
  caption: string;
}) {
  return (
    <div className={`proofMetric ${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{caption}</small>
    </div>
  );
}

function PageSection({ id, kicker, title, children }: { id?: string; kicker: string; title: string; children: ReactNode }) {
  return (
    <section className="pageSection" id={id}>
      <header className="pageSectionHeader">
        <span>{kicker}</span>
        <strong>{title}</strong>
      </header>
      <div className="pageSectionBody">{children}</div>
    </section>
  );
}

function PageIndex({ title, items }: { title: string; items: string[] }) {
  return (
    <aside className="pageIndex">
      <strong>{title}</strong>
      <nav aria-label={title}>
        {items.map((item, index) => (
          <a key={item} href={`#${anchorId(item, index)}`}>{item}</a>
        ))}
      </nav>
    </aside>
  );
}

function anchorId(label: string, index: number): string {
  return `page-section-${index}-${label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")}`;
}

function VisionGoalRow({ goal, model, onSelect }: { goal: Goal; model: WorkbenchModel; onSelect: () => void }) {
  const tasks = model.tasks.filter((task) => task.goal_id === goal.id);
  const taskIds = new Set(tasks.map((task) => task.id));
  const learning = model.snapshot.goal_learning_status?.find((status) => status.goal_id === goal.id);
  const decisionCount = model.decisions.filter((decision) => taskIds.has(decision.task_id)).length;
  const hasEvaluation = (learning?.goal_evaluation?.length ?? 0) > 0;

  return (
    <button type="button" className="visionGoalRow" onClick={onSelect}>
      <span>
        <strong>{goal.title ?? goal.id}</strong>
        <small>{goal.objective ?? "No objective recorded."}</small>
      </span>
      <span className="visionGoalProof">
        <StatusBadge tone={goal.status === "complete" ? "good" : goal.status === "blocked" ? "bad" : "info"}>{goal.status ?? "active"}</StatusBadge>
        <StatusBadge tone={decisionCount ? "good" : "warn"}>{decisionCount} decisions</StatusBadge>
        <StatusBadge tone={hasEvaluation ? "good" : "warn"}>{hasEvaluation ? "evaluation" : "evaluation missing"}</StatusBadge>
      </span>
    </button>
  );
}

function TaskProofStep({ index, label, complete }: { index: number; label: string; complete: boolean }) {
  return (
    <div className={`taskProofStep ${complete ? "complete" : "missing"}`}>
      <span>{index}</span>
      <strong>{label}</strong>
      <small>{complete ? "present" : "missing"}</small>
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
    <div className={`proofLine ${complete ? "complete" : "missing"}`}>
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

function countTasks(tasks: Task[]) {
  return {
    planned: tasks.filter((task) => task.status === "planned").length,
    assigned: tasks.filter((task) => task.status === "assigned").length,
    running: tasks.filter((task) => task.status === "running").length,
    blocked: tasks.filter((task) => task.status === "blocked").length,
    review: tasks.filter((task) => task.status === "review").length,
    done: tasks.filter((task) => task.status === "done").length,
    archived: tasks.filter((task) => task.status === "archived").length,
  };
}

function uniqueValues(values: (string | null | undefined)[]): string[] {
  return [...new Set(values.filter((value): value is string => Boolean(value)))];
}

function initials(value: string): string {
  return value
    .split(/[-_\s]/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}
