import { childThreadsForMember, inboxForMember, outboxForMember, sessionsForMember } from "../readModel";
import type { AgentMember, DashboardSnapshot, Message } from "../types";
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

  const inbox = inboxForMember(snapshot, member.id);
  const outbox = outboxForMember(snapshot, member.id);
  const sessions = sessionsForMember(snapshot, member.id);
  const childThreads = childThreadsForMember(snapshot, member.id);
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
        <span>runtime={member.runtime_id || "-"}</span>
        <span>pid={member.runtime_pid ?? "-"}</span>
        <span>queue={member.queued_count ?? 0}</span>
        <span>thread={member.provider_thread_id || "-"}</span>
        <span>child threads={member.provider_child_thread_count ?? childThreads.length}</span>
      </div>
      <RuntimeHealth member={member} />
      {member.current_task_id && (
        <button className="inlineAction" type="button" onClick={() => onSelectTask(member.current_task_id!)}>
          current task: {member.current_task_id}
        </button>
      )}
      <MessageStack title="Inbox" messages={inbox} onSelectTask={onSelectTask} />
      <MessageStack title="Outbox" messages={outbox} onSelectTask={onSelectTask} />
      <h4>Provider Sessions</h4>
      {sessions.slice(-5).map((session) => (
        <div className="listLine" key={session.id}>
          <Pill tone={session.status === "failed" ? "bad" : session.status === "succeeded" ? "good" : "warn"}>{session.status || "-"}</Pill>
          <span>{session.task_id || session.id} · thread={session.provider_thread_id || "-"} turn={session.provider_turn_id || "-"}</span>
        </div>
      ))}
      {!sessions.length && <p className="muted">No provider sessions</p>}
      <h4>Child Threads</h4>
      {childThreads.slice(-4).map((thread) => (
        <div className="listLine" key={thread.id}>
          <Pill>{thread.status || "thread"}</Pill>
          <span>{thread.provider_agent_nickname || thread.provider_thread_id || thread.id}</span>
        </div>
      ))}
      {!childThreads.length && <p className="muted">No child threads</p>}
    </section>
  );
}

function RuntimeHealth({ member }: { member: AgentMember }) {
  const health = member.runtime_health ?? {};
  return (
    <div className="healthGrid">
      <span>process={String(health.process_alive ?? member.runtime_alive ?? "-")}</span>
      <span>socket={String(health.socket_exists ?? "-")}</span>
      <span>protocol={String(health.protocol_probe ?? "-")}</span>
      <span>delivery={String(health.delivery_probe ?? "-")}</span>
    </div>
  );
}

function MessageStack({
  title,
  messages,
  onSelectTask,
}: {
  title: string;
  messages: Message[];
  onSelectTask: (id: string) => void;
}) {
  return (
    <>
      <h4>{title}</h4>
      {messages.slice(-4).map((message) => (
        <button className="listLine clickable" type="button" key={message.id} onClick={() => message.task_id && onSelectTask(message.task_id)}>
          <Pill tone={message.delivery_status === "queued" ? "warn" : message.delivery_status === "failed" ? "bad" : "good"}>
            {message.delivery_status}
          </Pill>
          <span>{message.kind} · {message.task_id || "no task"}</span>
        </button>
      ))}
      {!messages.length && <p className="muted">No {title.toLowerCase()}</p>}
    </>
  );
}
