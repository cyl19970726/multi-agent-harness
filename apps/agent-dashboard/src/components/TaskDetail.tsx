import { FileText, GitPullRequest, Inbox, ShieldCheck } from "lucide-react";
import {
  assignmentProofForTask,
  decisionsForTask,
  evidenceForTask,
  messagesForTask,
  proposalsForTask,
  reportsForTask,
  reviewEvidenceForTask,
  sessionsForTask,
} from "../readModel";
import type { DashboardSnapshot, Evidence, Message, ProviderSession, Task, WorkflowWarning } from "../types";
import { Pill } from "./Pill";

export function TaskDetail({
  snapshot,
  task,
  warnings,
  onSelectMember,
}: {
  snapshot: Required<DashboardSnapshot>;
  task?: Task;
  warnings: WorkflowWarning[];
  onSelectMember: (id: string) => void;
}) {
  if (!task) {
    return <section className="detailPanel"><h2>Task Detail</h2><p className="muted">No task selected</p></section>;
  }

  const taskMessages = messagesForTask(snapshot, task.id);
  const assignments = assignmentProofForTask(snapshot, task);
  const reports = reportsForTask(snapshot, task.id);
  const evidence = evidenceForTask(snapshot, task.id);
  const reviewEvidence = reviewEvidenceForTask(snapshot, task.id);
  const proposals = proposalsForTask(snapshot, task.id);
  const decisions = decisionsForTask(snapshot, task.id);
  const sessions = sessionsForTask(snapshot, task.id);
  const taskWarnings = warnings.filter((warning) => warning.taskId === task.id);

  return (
    <section className="detailPanel">
      <div className="sectionTitle">
        <h2>Task Detail</h2>
        <Pill>{task.status}</Pill>
      </div>
      <h3>{task.title || task.id}</h3>
      <p>{task.objective || "-"}</p>
      <div className="detailGrid">
        <span>owner={task.owner_agent_id || "-"}</span>
        <button type="button" className="inlineLink" onClick={() => task.assignee_agent_id && onSelectMember(task.assignee_agent_id)}>
          assignee={task.assignee_agent_id || "-"}
        </button>
        <span>reviewer={task.reviewer_agent_id || "-"}</span>
        <span>branch={task.branch_ref || "-"}</span>
        <span>pr={task.pr_ref || "-"}</span>
        <span>workspace={task.workspace_ref || "-"}</span>
      </div>
      <AssignmentProof assignments={assignments} assignee={task.assignee_agent_id || undefined} />
      <TaskMessages messages={taskMessages} reports={reports} />
      <EvidenceBlock evidence={evidence} reviewEvidence={reviewEvidence} sessions={sessions} />
      <ProposalDecision proposals={proposals} decisions={decisions} evidence={evidence} />
      {taskWarnings.length > 0 && (
        <div className="warningBox">
          {taskWarnings.map((warning) => <span key={warning.id}>{warning.summary}</span>)}
        </div>
      )}
    </section>
  );
}

function AssignmentProof({ assignments, assignee }: { assignments: Message[]; assignee?: string }) {
  return (
    <div className="detailList">
      <h4><ShieldCheck size={14} /> Assignment Proof</h4>
      {assignments.map((message) => (
        <div className="listLine" key={message.id}>
          <Pill tone={message.delivery_status === "failed" ? "bad" : message.delivery_status === "queued" ? "warn" : "good"}>
            {message.delivery_status}
          </Pill>
          <span>{message.from_agent_id || "-"} -&gt; {message.to_agent_id || assignee || "-"}</span>
        </div>
      ))}
      {!assignments.length && <p className="muted">No delivered task assignment for {assignee || "assignee"}</p>}
    </div>
  );
}

function TaskMessages({ messages, reports }: { messages: Message[]; reports: Message[] }) {
  return (
    <div className="detailList">
      <h4><Inbox size={14} /> Messages</h4>
      {messages.slice(-5).map((message) => (
        <div className="listLine" key={message.id}>
          <Pill tone={message.delivery_status === "failed" ? "bad" : message.delivery_status === "queued" ? "warn" : "good"}>
            {message.kind}:{message.delivery_status}
          </Pill>
          <span>{message.from_agent_id || "-"} -&gt; {message.to_agent_id || "-"}</span>
        </div>
      ))}
      {!messages.length && <p className="muted">No task messages</p>}
      {reports.length > 0 && (
        <div className="subList">
          <strong>Reports</strong>
          {reports.slice(-3).map((message) => (
            <span key={message.id}>{message.from_agent_id || "-"} · {message.content || message.id}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function EvidenceBlock({
  evidence,
  reviewEvidence,
  sessions,
}: {
  evidence: Evidence[];
  reviewEvidence: Evidence[];
  sessions: ProviderSession[];
}) {
  return (
    <div className="detailList">
      <h4><FileText size={14} /> Evidence / Runtime</h4>
      {evidence.slice(0, 5).map((item) => (
        <div className="listLine" key={item.id}>
          <Pill>{item.source_type || "evidence"}</Pill>
          <span>{item.summary || item.source_ref || item.id}</span>
        </div>
      ))}
      {sessions.slice(-3).map((session) => (
        <div className="listLine" key={session.id}>
          <Pill tone={session.status === "failed" || session.status === "stale" ? "bad" : session.status === "succeeded" ? "good" : "warn"}>
            {session.status || "session"}
          </Pill>
          <span>thread={session.provider_thread_id || "-"} turn={session.provider_turn_id || "-"} terminal={session.terminal_source || "-"}</span>
        </div>
      ))}
      <span className="muted">review evidence: {reviewEvidence.length}</span>
      {!evidence.length && !sessions.length && <p className="muted">No evidence or provider sessions</p>}
    </div>
  );
}

function ProposalDecision({
  proposals,
  decisions,
  evidence,
}: {
  proposals: ReturnType<typeof proposalsForTask>;
  decisions: ReturnType<typeof decisionsForTask>;
  evidence: Evidence[];
}) {
  const evidenceIds = new Set(evidence.map((item) => item.id));
  return (
    <div className="detailList">
      <h4><GitPullRequest size={14} /> Proposal / Decision</h4>
      {proposals.map((proposal) => (
        <div className="listLine" key={proposal.id}>
          <Pill>{proposal.status || "draft"}</Pill>
          <span>{proposal.title || proposal.id} · evidence={(proposal.evidence_ids ?? []).filter((id) => evidenceIds.has(id)).length}/{(proposal.evidence_ids ?? []).length}</span>
        </div>
      ))}
      {decisions.map((decision) => (
        <div className="listLine" key={decision.id}>
          <Pill tone="good">decision</Pill>
          <span>{decision.decision || decision.id} · evidence={(decision.evidence_ids ?? []).length}</span>
        </div>
      ))}
      {!proposals.length && !decisions.length && <p className="muted">No proposals or decisions</p>}
    </div>
  );
}
