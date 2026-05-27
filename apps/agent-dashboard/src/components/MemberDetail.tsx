import { messagesForMember, sessionsForMember } from "../readModel";
import type { AgentMember, DashboardSnapshot } from "../types";
import { Pill } from "./Pill";

export function MemberDetail({
  snapshot,
  member,
  onSelectTask,
}: {
  snapshot: Required<DashboardSnapshot>;
  member?: AgentMember;
  onSelectTask: (id: string) => void;
}) {
  if (!member) {
    return <section className="detailPanel"><h2>Member</h2><p className="muted">No member selected</p></section>;
  }

  const messages = messagesForMember(snapshot, member.id);
  const sessions = sessionsForMember(snapshot, member.id);
  const tone = member.runtime_status === "running" && member.runtime_alive ? "good" : "warn";

  return (
    <section className="detailPanel compact">
      <div className="sectionTitle">
        <h2>Member</h2>
        <Pill tone={tone}>{member.runtime_status || member.status || "offline"}</Pill>
      </div>
      <h3>{member.name || member.id}</h3>
      <p>{member.description || member.role || "-"}</p>
      <div className="metaBlock">
        <span>provider={member.provider || "-"}</span>
        <span>pid={member.runtime_pid ?? "-"}</span>
        <span>queue={member.queued_count ?? 0}</span>
        <span>thread={member.provider_thread_id || "-"}</span>
      </div>
      {member.current_task_id && (
        <button className="inlineAction" type="button" onClick={() => onSelectTask(member.current_task_id!)}>
          current task: {member.current_task_id}
        </button>
      )}
      <h4>Inbox / Outbox</h4>
      {messages.slice(-6).map((message) => (
        <div className="listLine" key={message.id}>
          <Pill tone={message.delivery_status === "queued" ? "warn" : message.delivery_status === "failed" ? "bad" : "good"}>
            {message.delivery_status}
          </Pill>
          <span>{message.kind} · {message.task_id || "no task"}</span>
        </div>
      ))}
      <h4>Provider Sessions</h4>
      {sessions.slice(-5).map((session) => (
        <div className="listLine" key={session.id}>
          <Pill tone={session.status === "failed" ? "bad" : session.status === "succeeded" ? "good" : "warn"}>{session.status || "-"}</Pill>
          <span>{session.task_id || session.id}</span>
        </div>
      ))}
      {!sessions.length && <p className="muted">No provider sessions</p>}
    </section>
  );
}
