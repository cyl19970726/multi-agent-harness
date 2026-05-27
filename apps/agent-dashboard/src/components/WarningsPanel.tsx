import { AlertTriangle } from "lucide-react";
import type { WorkflowWarning } from "../types";
import { Pill } from "./Pill";

export function WarningsPanel({
  warnings,
  onSelectTask,
  onSelectMember,
}: {
  warnings: WorkflowWarning[];
  onSelectTask: (id: string) => void;
  onSelectMember: (id: string) => void;
}) {
  return (
    <section className="detailPanel compact">
      <div className="sectionTitle">
        <h2>Warnings</h2>
        <Pill tone={warnings.length ? "warn" : "good"}>{warnings.length}</Pill>
      </div>
      {warnings.slice(0, 12).map((warning) => (
        <article className={`warningItem ${warning.severity}`} key={warning.id}>
          <AlertTriangle size={15} />
          <div>
            <strong>{warning.kind}</strong>
            <p>{warning.summary}</p>
            <div className="warningLinks">
              {warning.taskId && <button type="button" onClick={() => onSelectTask(warning.taskId!)}>task {warning.taskId}</button>}
              {warning.memberId && <button type="button" onClick={() => onSelectMember(warning.memberId!)}>member {warning.memberId}</button>}
            </div>
          </div>
        </article>
      ))}
      {!warnings.length && <p className="muted">No workflow warnings</p>}
    </section>
  );
}
