import { useMemo, useState } from "react";
import { Activity, ArrowRight, FileCheck2, ListFilter, MessageSquare, RadioTower } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import type { MemberCapacitySummary, MessageSummary, TeamActivitySummary } from "../../../model/roleViews";

export function TeamConversationStream({ activity, messages, members=[], truncated, onOpenWork, onReply }: {
  activity:TeamActivitySummary[];
  messages:MessageSummary[];
  members?:MemberCapacitySummary[];
  truncated:boolean;
  onOpenWork:(workId:string)=>void;
  onReply?:(message:MessageSummary)=>void;
}) {
  const [filtersOpen,setFiltersOpen] = useState(false);
  const [query,setQuery] = useState("");
  const [source,setSource] = useState("all");
  const [participant,setParticipant] = useState("all");
  const [workId,setWorkId] = useState("all");
  const membersById = useMemo(() => new Map(members.map((member) => [member.agent_member_ref.id,member])),[members]);
  const workIds = Array.from(new Set([...messages.map((message) => message.work_id),...activity.map((item) => item.work_id)].filter(Boolean) as string[]));
  const sourceKinds = Array.from(new Set(["message",...activity.map((item) => item.source)]));
  const matches = (row:{source:string;participants:string[];workId?:string|null;search:string}) =>
    (source === "all" || row.source === source) &&
    (participant === "all" || row.participants.includes(participant)) &&
    (workId === "all" || row.workId === workId) &&
    (!query.trim() || row.search.toLowerCase().includes(query.trim().toLowerCase()));
  const visibleMessages = messages.filter((message) => matches({source:"message",participants:[message.sender.id,...message.recipients.map((recipient) => recipient.id)],workId:message.work_id,search:[message.body,message.kind,message.sender.id,...message.recipients.map((recipient) => recipient.id)].join(" ")}));
  const visibleActivity = activity.filter((item) => matches({source:item.source,participants:item.actor_ref ? [item.actor_ref.id] : [],workId:item.work_id,search:[item.summary,item.status,item.source,item.actor_ref?.id,item.id].filter(Boolean).join(" ")}));
  const messageIds = new Set(visibleMessages.map((message) => message.message_id));
  const nonMessageActivity = visibleActivity.filter((item) => item.source !== "message" || !messageIds.has(item.id));
  const rows = [
    ...visibleMessages.map((message) => ({key:`message:${message.message_id}`,at:message.created_at,node:<MessageRow message={message} membersById={membersById} onOpenWork={onOpenWork} onReply={onReply}/>})),
    ...nonMessageActivity.map((item) => ({key:`${item.source}:${item.id}`,at:item.created_at,node:<ActivityRow item={item} membersById={membersById} onOpenWork={onOpenWork}/>})),
  ].sort((left,right) => right.at.localeCompare(left.at));
  const hasCanonicalRows = messages.length > 0 || activity.length > 0;
  return <section aria-labelledby="team-activity-title"><header className="flex flex-wrap items-end justify-between gap-3"><div><h2 id="team-activity-title" className="text-base font-semibold">Activity and messages</h2><p className="text-xs text-muted-foreground">Source-labelled coordination facts. Provider transcripts are never mirrored here.</p></div><div className="flex items-center gap-2">{truncated && <Badge tone="warn">latest 100</Badge>}<Button size="sm" variant="secondary" aria-expanded={filtersOpen} onClick={() => setFiltersOpen((value) => !value)}><ListFilter className="size-3.5"/>Filters</Button></div></header>
    {filtersOpen && <div className="mt-3 grid gap-2 rounded-xl border border-border bg-muted/20 p-3 md:grid-cols-4" aria-label="Activity filters"><label className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Search<input aria-label="Search Activity" value={query} onChange={(event) => setQuery(event.target.value)} className="mt-1 h-10 w-full rounded-md border border-border bg-background px-3 text-xs font-normal normal-case tracking-normal"/></label><FilterSelect label="Source" value={source} onChange={setSource} options={sourceKinds}/><FilterSelect label="Participant" value={participant} onChange={setParticipant} options={members.map((member) => member.agent_member_ref.id)} labels={membersById}/><FilterSelect label="Related Work" value={workId} onChange={setWorkId} options={workIds}/></div>}
    {rows.length ? <ol className="mt-3 space-y-2" data-testid="role-view-team-activity">{rows.map((row) => <li key={row.key}>{row.node}</li>)}</ol> : <div className="mt-3 rounded-xl border border-dashed border-border px-5 py-12 text-center"><Activity className="mx-auto size-6 text-muted-foreground"/><h3 className="mt-3 text-sm font-medium">{hasCanonicalRows ? "No activity matches these filters" : "No canonical Team activity yet"}</h3><p className="mt-1 text-xs text-muted-foreground">{hasCanonicalRows ? "Adjust the source, participant, Work, or text filters." : "Work events, authored messages, delivery facts and outcomes appear here when recorded."}</p></div>}
  </section>;
}

function FilterSelect({label,value,onChange,options,labels}:{label:string;value:string;onChange:(value:string)=>void;options:string[];labels?:Map<string,MemberCapacitySummary>}) {
  return <label className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{label}<select value={value} onChange={(event) => onChange(event.target.value)} className="mt-1 h-10 w-full rounded-md border border-border bg-background px-2 text-xs font-normal normal-case tracking-normal"><option value="all">All</option>{options.map((option) => <option key={option} value={option}>{labels?.get(option)?.display_name ?? option.replace(/_/g," ")}</option>)}</select></label>;
}

function MessageRow({message,membersById,onOpenWork,onReply}:{message:MessageSummary;membersById:Map<string,MemberCapacitySummary>;onOpenWork:(id:string)=>void;onReply?:(message:MessageSummary)=>void}) {
  const statuses = Array.from(new Set(message.deliveries.map((delivery) => delivery.status)));
  const sender = membersById.get(message.sender.id);
  const senderLabel = sender?.display_name ?? (message.sender.kind === "agent_member" ? "Team Lead or external member" : message.sender.id);
  const recipientLabels = message.recipients.map((recipient) => membersById.get(recipient.id)?.display_name ?? (recipient.kind === "agent_member" ? "Team Lead" : recipient.id));
  return <article className="rounded-xl border border-border bg-card p-3"><div className="flex min-w-0 items-start gap-3">{sender ? <Avatar name={sender.display_name} identity={`${sender.agent_member_ref.id} ${sender.role}`} size="sm" tone={sender.runtime_state === "running" ? "running" : "idle"}/> : <span className="grid size-8 shrink-0 place-items-center rounded-full bg-primary/10 text-primary"><MessageSquare className="size-3.5"/></span>}<div className="min-w-0 flex-1"><div className="flex min-w-0 flex-wrap items-center gap-2 text-[10px]"><span className="font-semibold text-foreground">{senderLabel}</span><ArrowRight className="size-3"/><span className="truncate text-muted-foreground">{recipientLabels.join(", ") || "Team"}</span><Badge>{message.kind.replace(/_/g," ")}</Badge><time className="ml-auto text-muted-foreground" dateTime={message.created_at}>{new Date(message.created_at).toLocaleString()}</time></div><div className="mt-2 text-sm"><Markdown source={message.body} compact/></div></div></div><div className="mt-3 flex min-w-0 flex-wrap items-center gap-2 border-t border-border pt-2 text-[10px] text-muted-foreground">{message.work_id && <Button size="sm" variant="secondary" className="h-7 px-2 text-[10px]" onClick={() => onOpenWork(message.work_id!)}>Work · {message.work_id}</Button>}<span>chain {message.correlation_id}</span>{message.causation_id && <span>reply to {message.causation_id}</span>}<span>{statuses.length ? statuses.join(" · ") : "no delivery rows"}</span>{onReply && message.reply_eligible && <Button size="sm" className="ml-auto h-7 px-2 text-[10px]" onClick={() => onReply(message)}>Reply</Button>}</div></article>;
}

function ActivityRow({item,membersById,onOpenWork}:{item:TeamActivitySummary;membersById:Map<string,MemberCapacitySummary>;onOpenWork:(id:string)=>void}) {
  const Icon = item.source.includes("delivery") ? RadioTower : item.source.includes("report") || item.source.includes("finding") ? FileCheck2 : Activity;
  const actor = item.actor_ref ? membersById.get(item.actor_ref.id) : undefined;
  return <article className="grid min-w-0 grid-cols-[2rem_minmax(0,1fr)] gap-2 rounded-xl border border-border bg-card p-3">{actor ? <Avatar name={actor.display_name} identity={`${actor.agent_member_ref.id} ${actor.role}`} size="sm" tone={actor.runtime_state === "running" ? "running" : "idle"}/> : <span className="grid size-8 place-items-center rounded-lg bg-muted text-muted-foreground"><Icon className="size-3.5"/></span>}<div className="min-w-0"><div className="flex min-w-0 flex-wrap items-center gap-2 text-[10px]"><span className="font-semibold uppercase tracking-[.11em] text-muted-foreground">{item.source.replace(/_/g," ")}</span>{item.status && <Badge>{item.status.replace(/_/g," ")}</Badge>}<span className="truncate">{actor?.display_name ?? item.actor_ref?.id ?? "system record"}</span><time className="ml-auto text-muted-foreground" dateTime={item.created_at}>{new Date(item.created_at).toLocaleString()}</time></div>{item.summary && <p className="mt-1 break-words text-xs leading-relaxed text-foreground/85">{item.summary}</p>}<div className="mt-2 flex items-center gap-2 text-[10px] text-muted-foreground"><span className="font-mono">{item.id}</span>{item.work_id && <button className="ml-auto rounded px-2 py-1 text-primary hover:bg-primary/5" onClick={() => onOpenWork(item.work_id!)}>Open Work</button>}</div></div></article>;
}
