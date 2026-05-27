import { GitBranch, Lightbulb } from "lucide-react";
import type { AutonomousProposal } from "../types";
import { Pill } from "./Pill";

export function AutonomousProposalsPanel({
  proposals,
  onSelectTask,
}: {
  proposals: AutonomousProposal[];
  onSelectTask: (id: string) => void;
}) {
  const recent = proposals.slice(-6).reverse();
  return (
    <section className="detailPanel proposalPanel">
      <div className="sectionTitle">
        <h2><Lightbulb size={15} /> Observer Proposals</h2>
        <Pill>{proposals.length}</Pill>
      </div>
      {recent.map((proposal) => (
        <div className="proposalLine" key={proposal.id}>
          <div className="proposalHead">
            <Pill tone={toneForDisposition(proposal.disposition)}>{proposal.disposition || "pending"}</Pill>
            <span>{proposal.kind || proposal.source_type || "proposal"}</span>
            <span>{proposal.from_agent_id || "-"} -&gt; {proposal.to_agent_id || "-"}</span>
          </div>
          <p>{proposal.summary || proposal.id}</p>
          <div className="proposalLinks">
            {proposal.task_id && (
              <button type="button" onClick={() => onSelectTask(proposal.task_id!)}>
                task={proposal.task_id}
              </button>
            )}
            {(proposal.follow_up_task_ids ?? []).map((taskId) => (
              <button type="button" key={taskId} onClick={() => onSelectTask(taskId)}>
                <GitBranch size={12} /> follow-up={taskId}
              </button>
            ))}
            {proposal.decision_id && <span>decision={proposal.decision_id}</span>}
          </div>
        </div>
      ))}
      {!recent.length && <p className="muted">No observer proposals for this goal</p>}
    </section>
  );
}

function toneForDisposition(disposition?: string): "good" | "warn" | "bad" | undefined {
  if (disposition === "accepted") return "good";
  if (disposition === "rejected") return "bad";
  if (disposition === "deferred" || disposition === "request_evidence") return "warn";
  return undefined;
}
