import type { DashboardSnapshot } from "../types";
import { Pill } from "./Pill";

interface RawViewsProps {
  snapshot: Required<DashboardSnapshot>;
}

export function RawViews({ snapshot }: RawViewsProps) {
  return (
    <section className="rawViews">
      <RawSection title="Messages">
        {snapshot.messages.slice(-10).reverse().map((message) => (
          <RawItem key={message.id} label={`${message.kind}:${message.delivery_status}`} title={message.id}>
            from={message.from_agent_id || "-"} to={message.to_agent_id || "-"} task={message.task_id || "-"}
          </RawItem>
        ))}
      </RawSection>
      <RawSection title="Provider Sessions">
        {snapshot.provider_sessions.slice(-10).reverse().map((session) => (
          <RawItem key={session.id} label={session.status || "session"} title={session.id}>
            agent={session.agent_member_id || "-"} task={session.task_id || "-"}
          </RawItem>
        ))}
      </RawSection>
      <RawSection title="Evidence / Decisions">
        {snapshot.evidence.slice(-5).reverse().map((evidence) => (
          <RawItem key={evidence.id} label={evidence.source_type || "evidence"} title={evidence.summary || evidence.id}>
            {evidence.source_ref || "-"}
          </RawItem>
        ))}
        {snapshot.decisions.slice(-5).reverse().map((decision) => (
          <RawItem key={decision.id} label="decision" title={decision.decision || decision.id}>
            {decision.rationale || "-"}
          </RawItem>
        ))}
      </RawSection>
    </section>
  );
}

function RawSection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h2>{title}</h2>
      <div className="denseList">{children}</div>
    </div>
  );
}

function RawItem({ label, title, children }: { label: string; title: string; children: React.ReactNode }) {
  return (
    <article className="item">
      <h3>{title}</h3>
      <div className="pills"><Pill>{label}</Pill></div>
      <div className="meta">{children}</div>
    </article>
  );
}
