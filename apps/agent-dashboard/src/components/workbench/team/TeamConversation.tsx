import { Activity, ArrowRight, FileCheck2, MessageSquare, RadioTower } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Markdown } from "@/components/workbench/Markdown";
import type { MessageSummary, TeamActivitySummary } from "../../../model/roleViews";

export function TeamConversationStream({ activity, messages, truncated, onOpenWork, onReply }: {
  activity:TeamActivitySummary[];
  messages:MessageSummary[];
  truncated:boolean;
  onOpenWork:(workId:string)=>void;
  onReply?:(message:MessageSummary)=>void;
}) {
  const messageIds = new Set(messages.map((message) => message.message_id));
  const nonMessageActivity = activity.filter((item) => item.source !== "message" || !messageIds.has(item.id));
  const rows = [
    ...messages.map((message) => ({key:`message:${message.message_id}`,at:message.created_at,node:<MessageRow message={message} onOpenWork={onOpenWork} onReply={onReply}/>})),
    ...nonMessageActivity.map((item) => ({key:`${item.source}:${item.id}`,at:item.created_at,node:<ActivityRow item={item} onOpenWork={onOpenWork}/>})),
  ].sort((left,right) => right.at.localeCompare(left.at));
  if (!rows.length) return <div className="rounded-xl border border-dashed border-border px-5 py-12 text-center"><Activity className="mx-auto size-6 text-muted-foreground"/><h3 className="mt-3 text-sm font-medium">No canonical Team activity yet</h3><p className="mt-1 text-xs text-muted-foreground">Work events, authored messages, delivery facts and outcomes appear here when recorded.</p></div>;
  return <section aria-labelledby="team-activity-title"><header className="flex items-end justify-between gap-3"><div><h2 id="team-activity-title" className="text-base font-semibold">Activity and messages</h2><p className="text-xs text-muted-foreground">Source-labelled coordination and execution facts. Provider transcripts are never mirrored here.</p></div>{truncated && <Badge tone="warn">latest 100</Badge>}</header><ol className="mt-3 space-y-2" data-testid="role-view-team-activity">{rows.map((row) => <li key={row.key}>{row.node}</li>)}</ol></section>;
}

function MessageRow({message,onOpenWork,onReply}:{message:MessageSummary;onOpenWork:(id:string)=>void;onReply?:(message:MessageSummary)=>void}) {
  const statuses = Array.from(new Set(message.deliveries.map((delivery) => delivery.status)));
  return <article className="rounded-xl border border-border bg-card p-3"><div className="flex min-w-0 flex-wrap items-center gap-2 text-[10px]"><span className="inline-flex items-center gap-1 font-semibold uppercase tracking-[.11em] text-primary"><MessageSquare className="size-3.5"/>Authored message</span><Badge>{message.kind}</Badge><span className="truncate text-muted-foreground">{message.sender.id} <ArrowRight className="inline size-3"/> {message.recipients.map((recipient) => recipient.id).join(", ") || "Team"}</span><time className="ml-auto text-muted-foreground" dateTime={message.created_at}>{new Date(message.created_at).toLocaleString()}</time></div><div className="mt-2 text-sm"><Markdown source={message.body} compact/></div><div className="mt-3 flex min-w-0 flex-wrap items-center gap-2 border-t border-border pt-2 text-[10px] text-muted-foreground">{message.work_id && <Button size="sm" variant="secondary" className="h-7 px-2 text-[10px]" onClick={() => onOpenWork(message.work_id!)}>Work · {message.work_id}</Button>}<span>chain {message.correlation_id}</span>{message.causation_id && <span>reply to {message.causation_id}</span>}<span>{statuses.length ? statuses.join(" · ") : "no delivery rows"}</span>{onReply && message.reply_eligible && <Button size="sm" className="ml-auto h-7 px-2 text-[10px]" onClick={() => onReply(message)}>Reply</Button>}</div></article>;
}

function ActivityRow({item,onOpenWork}:{item:TeamActivitySummary;onOpenWork:(id:string)=>void}) {
  const Icon = item.source.includes("delivery") ? RadioTower : item.source.includes("report") || item.source.includes("finding") ? FileCheck2 : Activity;
  return <article className="grid min-w-0 grid-cols-[2rem_minmax(0,1fr)] gap-2 rounded-xl border border-border bg-card p-3"><span className="grid size-8 place-items-center rounded-lg bg-muted text-muted-foreground"><Icon className="size-3.5"/></span><div className="min-w-0"><div className="flex min-w-0 flex-wrap items-center gap-2 text-[10px]"><span className="font-semibold uppercase tracking-[.11em] text-muted-foreground">{item.source.replace(/_/g," ")}</span>{item.status && <Badge>{item.status}</Badge>}<span className="truncate">{item.actor_ref?.id ?? "system record"}</span><time className="ml-auto text-muted-foreground" dateTime={item.created_at}>{new Date(item.created_at).toLocaleString()}</time></div>{item.summary && <p className="mt-1 break-words text-xs leading-relaxed text-foreground/85">{item.summary}</p>}<div className="mt-2 flex items-center gap-2 text-[10px] text-muted-foreground"><span className="font-mono">{item.id}</span>{item.work_id && <button className="ml-auto rounded px-2 py-1 text-primary hover:bg-primary/5" onClick={() => onOpenWork(item.work_id!)}>Open Work</button>}</div></div></article>;
}
