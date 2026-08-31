import * as ScrollArea from "@radix-ui/react-scroll-area";
import { ArrowDownLeft, ArrowUpRight, CheckCheck, Link2, MessageSquareText } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Markdown } from "@/components/workbench/Markdown";
import type { AgentWorkspaceRosterItem, MessageSummary, WorkSummary } from "@/model/roleViews";
import { DockModuleState } from "./DockModuleState";
import type { DockModuleStatus } from "./types";

type MessageLens = "inbox" | "outbox" | "all" | "unread";

export function MessagesDock({ messages, works, roster, selectedAgentId, expanded, status, initialMessageId, onSelectMessage }: {
  messages: MessageSummary[];
  works: WorkSummary[];
  roster: AgentWorkspaceRosterItem[];
  selectedAgentId: string;
  expanded: boolean;
  status?: DockModuleStatus;
  initialMessageId?: string;
  onSelectMessage?: (message: MessageSummary) => void;
}) {
  const [lens, setLens] = useState<MessageLens>("inbox");
  const [selectedMessageId, setSelectedMessageId] = useState(initialMessageId ?? messages[0]?.message_id ?? "");
  const listViewportRef = useRef<HTMLDivElement>(null);
  const detailViewportRef = useRef<HTMLDivElement>(null);
  const incoming = (message: MessageSummary) => message.sender.id !== selectedAgentId;
  const unread = (message: MessageSummary) => incoming(message) && message.deliveries.some((delivery) => ["queued", "delivered"].includes(delivery.status));
  const visible = useMemo(() => messages.filter((message) => lens === "all" || lens === "inbox" && incoming(message) || lens === "outbox" && !incoming(message) || lens === "unread" && unread(message)).sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at)), [messages, lens, selectedAgentId]);
  useEffect(() => {
    if (messages.some((message) => message.message_id === selectedMessageId)) return;
    setSelectedMessageId(messages[0]?.message_id ?? "");
  }, [messages, selectedMessageId]);
  const selected = messages.find((message) => message.message_id === selectedMessageId) ?? visible[0];
  const select = (message: MessageSummary) => { setSelectedMessageId(message.message_id); onSelectMessage?.(message); if (!expanded) detailViewportRef.current?.scrollTo({ top: 0 }); };

  if (status && status.kind !== "ready" && status.kind !== "stale") return <DockModuleState status={status}/>;
  return <div className="agent-messages-dock" data-expanded={expanded || undefined}>
    {status?.kind === "stale" && (
      <DockModuleState status={status}/>
    )}
    <div className="agent-dock-filterbar" aria-label="Message filters">
      {(["inbox", "outbox", "all", "unread"] as const).map((item) => <button key={item} type="button" aria-pressed={lens === item} onClick={() => setLens(item)}>{item}</button>)}
      <span>{visible.length}</span>
    </div>
    <div className="agent-dock-split">
      <ScrollArea.Root className="agent-dock-list" data-testid="messages-dock-list"><ScrollArea.Viewport ref={listViewportRef} className="size-full">
        {visible.length ? <ol>{visible.map((message) => {
          const isIncoming = incoming(message);
          const actor = roster.find((item) => item.agent_member_ref.id === message.sender.id);
          const linkedWork = works.find((work) => work.work_id === message.work_id);
          return <li key={message.message_id}><button type="button" aria-current={selected?.message_id === message.message_id || undefined} onClick={() => select(message)}>
            <span className="agent-dock-row-title">{actor?.display_name ?? message.sender.display_name ?? message.sender.id}</span>
            <span className="agent-dock-row-preview">{message.body}</span>
            <span className="agent-dock-row-meta">{isIncoming ? <><ArrowDownLeft/>Inbox</> : <><ArrowUpRight/>Outbox</>}<time>{formatTime(message.created_at)}</time></span>
            {message.work_id && <span className="agent-dock-work-link"><Link2/>{linkedWork?.title ?? "Linked Work"}</span>}
            {unread(message) && <span className="agent-dock-unread">Unread</span>}
          </button></li>;
        })}</ol> : (
          <DockModuleState emptyTitle="No Messages in this view" emptyDetail={lens === "unread" ? "There are no unsettled incoming deliveries." : "Change the filter to inspect another canonical Message set."}/>
        )}
      </ScrollArea.Viewport></ScrollArea.Root>
      <ScrollArea.Root className="agent-dock-detail" data-testid="messages-dock-detail"><ScrollArea.Viewport ref={detailViewportRef} className="size-full">
        {selected ? <MessageDetail message={selected} works={works} roster={roster} selectedAgentId={selectedAgentId}/> : <DockModuleState emptyTitle="Select a Message" emptyDetail="Choose a Message to read its authored content and delivery evidence."/>}
      </ScrollArea.Viewport></ScrollArea.Root>
    </div>
  </div>;
}

function MessageDetail({ message, works, roster, selectedAgentId }: { message: MessageSummary; works: WorkSummary[]; roster: AgentWorkspaceRosterItem[]; selectedAgentId: string }) {
  const actor = roster.find((item) => item.agent_member_ref.id === message.sender.id);
  const recipients = message.recipients.map((recipient) => recipient.display_name ?? roster.find((item) => item.agent_member_ref.id === recipient.id)?.display_name ?? recipient.id);
  const linkedWork = works.find((work) => work.work_id === message.work_id);
  const isIncoming = message.sender.id !== selectedAgentId;
  const receipts = message.deliveries.filter((delivery) => delivery.provider_receipt_id).length;
  return <article className="agent-message-detail" aria-label="Message details">
    <header><p>{isIncoming ? "Inbox" : "Outbox"}</p><h2>{actor?.display_name ?? message.sender.display_name ?? message.sender.id}</h2><span>To {recipients.join(", ") || "No projected recipient"} · {formatTime(message.created_at)}</span></header>
    <section className="agent-message-body"><MessageSquareText aria-hidden="true"/><Markdown source={message.body}/></section>
    {message.work_id && <section className="agent-message-work-context"><h3><Link2/>Linked Work</h3><strong>{linkedWork?.title ?? "Work title unavailable"}</strong><p>{message.work_id}</p><small>Context only. This Message does not mutate Work.</small></section>}
    <section><h3><CheckCheck/>Delivery</h3><p>{deliveryLabel(message)}</p><p>{receipts ? `${receipts} provider receipt${receipts === 1 ? "" : "s"} recorded.` : "No provider receipt recorded."}</p>{message.deliveries.length > 0 && <ul>{message.deliveries.map((delivery) => <li key={delivery.id}><span>{delivery.recipient_display_name ?? delivery.recipient_identity_id ?? delivery.recipient_member_run_id}</span><strong>{humanize(delivery.status)}</strong></li>)}</ul>}</section>
    <details><summary>Technical details</summary><dl><div><dt>Message</dt><dd>{message.message_id}</dd></div><div><dt>Correlation</dt><dd>{message.correlation_id}</dd></div>{message.causation_id && <div><dt>Causation</dt><dd>{message.causation_id}</dd></div>}</dl></details>
  </article>;
}

function deliveryLabel(message: MessageSummary) { return humanize(message.delivery_state ?? (message.deliveries.length ? message.deliveries.map((delivery) => delivery.status).join(" / ") : "No recipient delivery")); }
function humanize(value: string) { return value.split(/[_-]+/).filter(Boolean).map((part) => part[0]?.toUpperCase() + part.slice(1)).join(" "); }
function formatTime(value: string) { const parsed = Date.parse(value); return Number.isFinite(parsed) ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(parsed) : "Time unavailable"; }
