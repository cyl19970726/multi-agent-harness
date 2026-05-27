import { GitPullRequest, Inbox } from "lucide-react";
import { decisionsForTask, messagesForTask, proposalsForTask } from "../readModel";
import type { DashboardSnapshot, Task, WorkflowWarning } from "../types";
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
  const proposals = proposalsForTask(snapshot, task.id);
  const decisions = decisionsForTask(snapshot, task.id);
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
      </div>
      <TaskMessages messages={taskMessages} />
      <ProposalDecision proposals={proposals} decisions={decisions} />
      {taskWarnings.length > 0 && (
        <div className="warningBox">
          {taskWarnings.map((warning) => <span key={warning.id}>{warning.summary}</span>)}
        </div>
      )}
    </section>
  );
}

function TaskMessages({ messages }: { messages: ReturnType<typeof messagesForTask> }) {
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
    </div>
  );
}

function ProposalDecision({
  proposals,
  decisions,
}: {
  proposals: ReturnType<typeof proposalsForTask>;
  decisions: ReturnType<typeof decisionsForTask>;
}) {
  return (
    <div className="detailList">
      <h4><GitPullRequest size={14} /> Proposal / Decision</h4>
      {proposals.map((proposal) => (
        <div className="listLine" key={proposal.id}><Pill>{proposal.status || "draft"}</Pill><span>{proposal.title || proposal.id}</span></div>
      ))}
      {decisions.map((decision) => (
        <div className="listLine" key={decision.id}><Pill tone="good">decision</Pill><span>{decision.decision || decision.id}</span></div>
      ))}
      {!proposals.length && !decisions.length && <p className="muted">No proposals or decisions</p>}
    </div>
  );
}
