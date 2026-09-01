import * as ScrollArea from "@radix-ui/react-scroll-area";
import { ArrowDownLeft, ArrowUpRight, CheckCheck, Link2, MessageSquareText } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Markdown } from "@/components/workbench/Markdown";
import type { AgentWorkspaceRosterItem, MessageSummary, WorkSummary } from "@/model/roleViews";
import { DockModuleState } from "./DockModuleState";
import type { DockModuleStatus } from "./types";

type MessageLens = "priority" | "all" | "inbox" | "outbox";
interface MessageConversation { id:string; messages:MessageSummary[]; latest:MessageSummary; actionRequired:boolean; unread:boolean; }

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
  const [lens, setLens] = useState<MessageLens>("priority");
  const [selectedMessageId, setSelectedMessageId] = useState(initialMessageId ?? messages[0]?.message_id ?? "");
  const listViewportRef = useRef<HTMLDivElement>(null);
  const detailViewportRef = useRef<HTMLDivElement>(null);
  const incoming = (message: MessageSummary) => message.sender.id !== selectedAgentId;
  const unread = (message: MessageSummary) => incoming(message) && message.deliveries.some((delivery) => ["queued", "delivered"].includes(delivery.status));
  const conversations=useMemo(()=>groupConversations(messages,incoming,unread),[messages,selectedAgentId]);
  const visible = useMemo(() => {
    const priority=conversations.filter((conversation)=>conversation.actionRequired||conversation.unread);
    if(lens==="priority")return priority.length?priority:conversations;
    return conversations.filter((conversation) => lens === "all" || lens === "inbox" && conversation.messages.some(incoming) || lens === "outbox" && conversation.messages.some((message)=>!incoming(message)));
  }, [conversations, lens, selectedAgentId]);
  useEffect(() => {
    if (messages.some((message) => message.message_id === selectedMessageId)) return;
    setSelectedMessageId(messages[0]?.message_id ?? "");
  }, [messages, selectedMessageId]);
  const selectedConversation=conversations.find((conversation)=>conversation.messages.some((message)=>message.message_id===selectedMessageId))??visible[0];
  const selected=selectedConversation?.messages.find((message)=>message.message_id===selectedMessageId)??selectedConversation?.latest;
  const select = (message: MessageSummary) => { setSelectedMessageId(message.message_id); onSelectMessage?.(message); if (!expanded) detailViewportRef.current?.scrollTo({ top: 0 }); };

  if (status && status.kind !== "ready" && status.kind !== "stale") return <DockModuleState status={status}/>;
  return <div className="agent-messages-dock" data-expanded={expanded || undefined}>
    {status?.kind === "stale" && (
      <DockModuleState status={status}/>
    )}
    <div className="agent-dock-filterbar" aria-label="Message filters">
      {(["priority", "all", "inbox", "outbox"] as const).map((item) => <button key={item} type="button" aria-pressed={lens === item} onClick={() => setLens(item)}>{item}</button>)}
      <span>{visible.length}</span>
    </div>
    <div className="agent-dock-split">
      <ScrollArea.Root className="agent-dock-list" data-testid="messages-dock-list"><ScrollArea.Viewport ref={listViewportRef} className="size-full">
        {visible.length ? <ol>{visible.map((conversation) => {
          const message=conversation.latest;
          const linkedWorkIds=[...new Set(conversation.messages.flatMap((item)=>item.work_id?[item.work_id]:[]))];
          const linkedWork=linkedWorkIds.length===1?works.find((work)=>work.work_id===linkedWorkIds[0]):undefined;
          return <li key={conversation.id}><button type="button" aria-current={selectedConversation?.id === conversation.id || undefined} onClick={() => select(message)}>
            <span className="agent-dock-row-title">{conversationTitle(conversation,roster,selectedAgentId)}</span>
            <span className="agent-dock-row-preview">{message.body}</span>
            <span className="agent-dock-row-meta">{conversation.messages.some(incoming) ? <><ArrowDownLeft/>Incoming</> : <><ArrowUpRight/>Sent</>}<span>{conversation.messages.length} {conversation.messages.length===1?"message":"messages"}</span><time>{formatTime(message.created_at)}</time></span>
            {linkedWorkIds.length>0 && <span className="agent-dock-work-link"><Link2/>{linkedWorkIds.length===1?(linkedWork?.title||`Work · ${shortId(linkedWorkIds[0]!)}`):`${linkedWorkIds.length} Work contexts`}</span>}
            {(conversation.actionRequired||conversation.unread) && <span className="agent-dock-unread">{conversation.actionRequired?"Response required":"Unread"}</span>}
          </button></li>;
        })}</ol> : (
          <DockModuleState emptyTitle="No Messages in this view" emptyDetail={lens === "priority" ? "No incoming response is required and no unsettled delivery is unread." : "Change the filter to inspect another canonical Message set."}/>
        )}
      </ScrollArea.Viewport></ScrollArea.Root>
      <ScrollArea.Root className="agent-dock-detail" data-testid="messages-dock-detail"><ScrollArea.Viewport ref={detailViewportRef} className="size-full">
        {selected&&selectedConversation ? <MessageDetail message={selected} conversation={selectedConversation} works={works} roster={roster} selectedAgentId={selectedAgentId} onSelect={select}/> : <DockModuleState emptyTitle="Select a Message" emptyDetail="Choose a Message to read its authored content and delivery evidence."/>}
      </ScrollArea.Viewport></ScrollArea.Root>
    </div>
  </div>;
}

function MessageDetail({ message, conversation, works, roster, selectedAgentId, onSelect }: { message: MessageSummary; conversation:MessageConversation; works: WorkSummary[]; roster: AgentWorkspaceRosterItem[]; selectedAgentId: string; onSelect:(message:MessageSummary)=>void }) {
  const actor = roster.find((item) => item.agent_member_ref.id === message.sender.id);
  const recipients = message.recipients.map((recipient) => recipient.display_name ?? roster.find((item) => item.agent_member_ref.id === recipient.id)?.display_name ?? recipient.id);
  const linkedWork = works.find((work) => work.work_id === message.work_id);
  const isIncoming = message.sender.id !== selectedAgentId;
  const receipts = message.deliveries.filter((delivery) => delivery.provider_receipt_id).length;
  return <article className="agent-message-detail" aria-label="Message details">
    <header><p>{isIncoming ? "Inbox" : "Outbox"}</p><h2>{actor?.display_name ?? message.sender.display_name ?? message.sender.id}</h2><span>To {recipients.join(", ") || "No projected recipient"} · {formatTime(message.created_at)}</span></header>
    {conversation.messages.length>1&&<section className="agent-message-conversation"><h3>Conversation</h3>{conversation.messages.map((item)=><button key={item.message_id} type="button" aria-current={item.message_id===message.message_id||undefined} onClick={()=>onSelect(item)}><strong>{displayName(item.sender.id,item.sender.display_name,roster)}</strong><span>{item.body}</span><time>{formatTime(item.created_at)}</time></button>)}</section>}
    <section className="agent-message-body"><MessageSquareText aria-hidden="true"/><Markdown source={message.body}/></section>
    {message.work_id && <section className="agent-message-work-context"><h3><Link2/>Work context</h3><strong>{linkedWork?.title ?? `Work · ${shortId(message.work_id)}`}</strong><p>{linkedWork?`${humanize(linkedWork.phase)} · ${humanize(linkedWork.condition)} · ${linkedWork.assignee_ref?.display_name??"Unassigned"}`:message.work_id}</p><small>Reading context only. This Message does not mutate Work, prove a Result, or grant acceptance.</small></section>}
    <section><h3><CheckCheck/>Delivery</h3><p>{deliveryLabel(message)}</p><p>{receipts ? `${receipts} provider receipt${receipts === 1 ? "" : "s"} recorded.` : "No provider receipt recorded."}</p>{message.deliveries.length > 0 && <ul>{message.deliveries.map((delivery) => <li key={delivery.id}><span>{delivery.recipient_display_name ?? delivery.recipient_identity_id ?? delivery.recipient_member_run_id}</span><strong>{humanize(delivery.status)}</strong></li>)}</ul>}</section>
    <details><summary>Technical details</summary><dl><div><dt>Message</dt><dd>{message.message_id}</dd></div><div><dt>Correlation</dt><dd>{message.correlation_id}</dd></div>{message.causation_id && <div><dt>Causation</dt><dd>{message.causation_id}</dd></div>}</dl></details>
  </article>;
}

function deliveryLabel(message: MessageSummary) { return humanize(message.delivery_state ?? (message.deliveries.length ? message.deliveries.map((delivery) => delivery.status).join(" / ") : "No recipient delivery")); }
function groupConversations(messages:MessageSummary[],incoming:(message:MessageSummary)=>boolean,unread:(message:MessageSummary)=>boolean):MessageConversation[]{
  const groups=new Map<string,MessageSummary[]>();
  for(const message of messages){const id=message.correlation_id?.trim()?`correlation:${message.correlation_id}`:`message:${message.message_id}`;const group=groups.get(id);if(group)group.push(message);else groups.set(id,[message]);}
  return [...groups].map(([id,items])=>{const ordered=[...items].sort((left,right)=>Date.parse(left.created_at)-Date.parse(right.created_at));return{id,messages:ordered,latest:ordered[ordered.length-1]!,actionRequired:ordered.some((message)=>incoming(message)&&message.response_intent==="response_required"),unread:ordered.some(unread)};}).sort((left,right)=>Date.parse(right.latest.created_at)-Date.parse(left.latest.created_at));
}
function conversationTitle(conversation:MessageConversation,roster:AgentWorkspaceRosterItem[],selectedAgentId:string){const others=[...new Set(conversation.messages.flatMap((message)=>[message.sender,...message.recipients]).filter((actor)=>actor.id!==selectedAgentId).map((actor)=>displayName(actor.id,actor.display_name,roster)))];return others.length?others.join(" · "):"Team coordination";}
function displayName(id:string,projected:string|undefined|null,roster:AgentWorkspaceRosterItem[]){return projected??roster.find((item)=>item.agent_member_ref.id===id)?.display_name??shortId(id);}
function shortId(value:string){return value.length>20?`${value.slice(0,10)}…${value.slice(-6)}`:value;}
function humanize(value: string) { return value.split(/[_-]+/).filter(Boolean).map((part) => part[0]?.toUpperCase() + part.slice(1)).join(" "); }
function formatTime(value: string) { const parsed = Date.parse(value); return Number.isFinite(parsed) ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(parsed) : "Time unavailable"; }
