import { useMemo, useState } from "react";
import { Activity, ArrowRight, CheckCircle2, FileCheck2, Inbox, ListFilter, MessageSquare, RadioTower, Search, ShieldCheck } from "lucide-react";

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
  return <section aria-labelledby="team-activity-title" className="pt-2">
    <header className="flex flex-wrap items-end justify-between gap-3"><div><div className="flex items-baseline gap-2"><h2 id="team-activity-title" className="company-editorial-title text-[21px]">Team timeline</h2><span className="text-[10px] tabular-nums text-muted-foreground">{rows.length} records</span></div><p className="mt-0.5 text-[11px] text-muted-foreground">Authored coordination and durable facts. Provider transcripts stay native.</p></div><div className="flex items-center gap-2">{truncated && <Badge tone="warn">latest bounded page</Badge>}<Button size="sm" variant="ghost" aria-expanded={filtersOpen} onClick={() => setFiltersOpen((value) => !value)}><ListFilter className="size-3.5"/>Filter activity</Button></div></header>
    <div className={`${filtersOpen ? "grid" : "hidden"} mt-3 gap-2 border-y border-border py-3 md:grid-cols-[minmax(13rem,1fr)_auto_auto_auto]`} aria-label="Activity filters"><label className="relative min-w-0"><span className="sr-only">Search Activity</span><Search className="pointer-events-none absolute left-3 top-2.5 size-3.5 text-muted-foreground"/><input aria-label="Search Activity" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search activity by keyword…" className="agent-team-control h-9 w-full pl-9 pr-3 text-xs"/></label><FilterSelect label="Source" value={source} onChange={setSource} options={sourceKinds}/><FilterSelect label="Participant" value={participant} onChange={setParticipant} options={members.map((member) => member.agent_member_ref.id)} labels={membersById}/><FilterSelect label="Related Work" value={workId} onChange={setWorkId} options={workIds}/></div>
    {rows.length ? <div className="mt-3 overflow-hidden border-t border-border"><div className="hidden grid-cols-[7rem_minmax(9rem,.72fr)_minmax(15rem,2fr)_minmax(7rem,.7fr)_7rem] gap-4 border-b border-border px-3 py-2.5 text-[9px] font-semibold uppercase tracking-[.12em] text-muted-foreground lg:grid"><span>Record</span><span>Actor / direction</span><span>What changed</span><span>Related Work</span><span className="text-right">Time / state</span></div><ol data-testid="role-view-team-activity">{rows.map((row) => <li key={row.key}>{row.node}</li>)}</ol></div> : <div className="mt-3 border-y border-dashed border-border px-5 py-14 text-center"><Activity className="mx-auto size-7 text-muted-foreground"/><h3 className="mt-3 text-sm font-semibold">{hasCanonicalRows ? "No activity matches these filters" : "No canonical Team activity yet"}</h3><p className="mx-auto mt-1 max-w-md text-xs leading-relaxed text-muted-foreground">{hasCanonicalRows ? "Adjust source, participant, Work or keyword filters." : "Authored Messages, Work events, delivery facts and explicit outcomes will appear here. Native provider transcripts remain in their source session."}</p></div>}
  </section>;
}

function FilterSelect({label,value,onChange,options,labels}:{label:string;value:string;onChange:(value:string)=>void;options:string[];labels?:Map<string,MemberCapacitySummary>}) {
  return <label><span className="sr-only">{label}</span><select aria-label={label} value={value} onChange={(event) => onChange(event.target.value)} className="agent-team-control h-9 min-w-36 px-2 text-xs font-normal"><option value="all">All {label.toLowerCase()}</option>{options.map((option) => <option key={option} value={option}>{labels?.get(option)?.display_name ?? option.replace(/_/g," ")}</option>)}</select></label>;
}

function MessageRow({message,membersById,onOpenWork,onReply}:{message:MessageSummary;membersById:Map<string,MemberCapacitySummary>;onOpenWork:(id:string)=>void;onReply?:(message:MessageSummary)=>void}) {
  const statuses = Array.from(new Set(message.deliveries.map((delivery) => delivery.status)));
  const sender = membersById.get(message.sender.id);
  const senderLabel = sender?.display_name ?? message.sender.id;
  const recipientLabels = message.recipients.map((recipient) => membersById.get(recipient.id)?.display_name ?? recipient.id);
  return <article className="agent-team-record-row grid min-w-0 gap-3 px-3 py-3.5 lg:grid-cols-[7rem_minmax(9rem,.72fr)_minmax(15rem,2fr)_minmax(7rem,.7fr)_7rem] lg:items-start"><RecordType icon={MessageSquare} label="Message" detail={message.kind.replace(/_/g," ")} tone="coral"/><div className="flex min-w-0 items-center gap-2">{sender ? <Avatar name={sender.display_name} identity={`${sender.agent_member_ref.id} ${sender.role}`} size="sm" tone={sender.runtime_state === "running" ? "running" : "idle"}/> : <span className="grid size-8 shrink-0 place-items-center rounded-full bg-primary/10 text-primary"><Inbox className="size-3.5"/></span>}<div className="min-w-0"><p className="truncate text-xs font-semibold">{senderLabel}</p><p className="mt-0.5 flex items-center gap-1 truncate text-[10px] text-muted-foreground"><ArrowRight className="size-3"/>{recipientLabels.join(", ") || "Team"}</p></div></div><div className="min-w-0"><div className="text-[13px] leading-relaxed"><Markdown source={message.body} compact/></div><div className="mt-2 flex flex-wrap items-center gap-2 text-[9px] text-muted-foreground"><span className="font-mono">chain {message.correlation_id}</span>{message.causation_id && <span>reply to {message.causation_id}</span>}<details><summary className="cursor-pointer text-primary">{statuses.length ? statuses.join(" · ") : "no delivery rows"}</summary>{message.deliveries.length > 0 && <ul className="mt-1 space-y-1 bg-secondary p-2">{message.deliveries.map((delivery) => <li key={delivery.id} className="break-all">recipient {delivery.recipient_member_run_id} · {delivery.status} · v{delivery.version}{delivery.provider_receipt_id ? ` · receipt ${delivery.provider_receipt_id}` : ""}</li>)}</ul>}</details>{onReply && message.reply_eligible && <button type="button" className="font-semibold text-primary hover:underline" onClick={() => onReply(message)}>Reply</button>}</div></div><div>{message.work_id ? <button type="button" className="max-w-full truncate text-[10px] text-muted-foreground hover:text-primary" onClick={() => onOpenWork(message.work_id!)}>Work · {message.work_id}</button> : <span className="text-[10px] text-muted-foreground">No Work link</span>}</div><div className="text-left lg:text-right"><time className="block text-[10px] text-muted-foreground" dateTime={message.created_at}>{formatActivityTime(message.created_at)}</time><Badge className="mt-1" tone={statuses.includes("provider_received") ? "good" : statuses.length ? "info" : "muted"}>{statuses.join(" · ") || "authored"}</Badge></div></article>;
}

function ActivityRow({item,membersById,onOpenWork}:{item:TeamActivitySummary;membersById:Map<string,MemberCapacitySummary>;onOpenWork:(id:string)=>void}) {
  const actor = item.actor_ref ? membersById.get(item.actor_ref.id) : undefined;
  const source=sourcePresentation(item.source,item.status);
  const SourceIcon=source.icon;
  return <article className="agent-team-record-row grid min-w-0 gap-3 px-3 py-3.5 lg:grid-cols-[7rem_minmax(9rem,.72fr)_minmax(15rem,2fr)_minmax(7rem,.7fr)_7rem] lg:items-start"><RecordType icon={source.icon} label={source.label} detail={item.source.replace(/_/g," ")} tone={source.tone}/><div className="flex min-w-0 items-center gap-2">{actor ? <Avatar name={actor.display_name} identity={`${actor.agent_member_ref.id} ${actor.role}`} size="sm" tone={actor.runtime_state === "running" ? "running" : "idle"}/> : <span className="grid size-8 shrink-0 place-items-center rounded-full bg-secondary text-muted-foreground"><SourceIcon className="size-3.5"/></span>}<div className="min-w-0"><p className="truncate text-xs font-semibold">{actor?.display_name ?? item.actor_ref?.id ?? "System"}</p><p className="truncate text-[10px] text-muted-foreground">{actor?.role ?? "canonical record"}</p></div></div><div className="min-w-0"><p className="text-[13px] font-medium leading-relaxed">{item.summary || source.label}</p><p className="mt-1 truncate font-mono text-[9px] text-muted-foreground">{item.id}</p></div><div>{item.work_id ? <button type="button" className="max-w-full truncate text-[10px] text-muted-foreground hover:text-primary" onClick={() => onOpenWork(item.work_id!)}>Work · {item.work_id}</button> : <span className="text-[10px] text-muted-foreground">No Work link</span>}</div><div className="text-left lg:text-right"><time className="block text-[10px] text-muted-foreground" dateTime={item.created_at}>{formatActivityTime(item.created_at)}</time><Badge className="mt-1" tone={source.badgeTone}>{item.status?.replace(/_/g," ") || "recorded"}</Badge></div></article>;
}

function RecordType({icon:Icon,label,detail,tone}:{icon:typeof Activity;label:string;detail:string;tone:"coral"|"blue"|"green"|"amber"|"violet"}) { const colors={coral:"text-primary bg-primary/10",blue:"text-status-running bg-status-running/10",green:"text-status-good bg-status-good/10",amber:"text-status-warn bg-status-warn/10",violet:"text-status-decision bg-status-decision/10"}; return <div className="flex min-w-0 items-center gap-2"><span className={`grid size-8 shrink-0 place-items-center rounded-lg ${colors[tone]}`}><Icon className="size-4"/></span><div className="min-w-0"><p className="truncate text-[10px] font-semibold">{label}</p><p className="truncate text-[9px] text-muted-foreground">{detail}</p></div></div>; }
function sourcePresentation(source:string,status:string|null) { const value=source.toLowerCase(); if(value.includes("delivery"))return {icon:RadioTower,label:"Delivery",tone:"green" as const,badgeTone:"good" as const}; if(value.includes("gate"))return {icon:ShieldCheck,label:"Gate evaluation",tone:"green" as const,badgeTone:status === "failed" ? "bad" as const : "good" as const}; if(value.includes("report")||value.includes("finding")||value.includes("evidence"))return {icon:FileCheck2,label:"Work evidence",tone:"amber" as const,badgeTone:"warn" as const}; if(value.includes("runtime")||value.includes("command"))return {icon:RadioTower,label:"Runtime",tone:"violet" as const,badgeTone:"info" as const}; if(value.includes("work"))return {icon:CheckCircle2,label:"Work event",tone:"blue" as const,badgeTone:"info" as const}; return {icon:Activity,label:"Activity",tone:"blue" as const,badgeTone:"muted" as const}; }
function formatActivityTime(value:string) { const date=new Date(value); return Number.isNaN(date.getTime()) ? value : date.toLocaleString([], {month:"short",day:"numeric",hour:"numeric",minute:"2-digit"}); }
