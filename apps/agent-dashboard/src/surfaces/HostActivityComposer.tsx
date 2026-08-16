import { useEffect, useState } from "react";
import { ArrowRight, Inbox } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import { TeamMessageComposer } from "@/components/workbench/team/TeamMessageComposer";
import {
  fetchRoleView,
  type HostConsoleData,
  type MessageSummary,
  type RoleActionExecutor,
  type RoleView,
} from "../model/roleViews";

/** Keeps Activity conversational without weakening TeamWorkspace read authority. */
export function HostActivityComposer({apiUrl,space,project,routeIdentity,teamRunId,replyTo,refreshKey,onAction,onClearReply,fixedRecipient,variant="panel",collapsibleOnMobile=true}:{
  apiUrl:string;
  space:string;
  project:string;
  routeIdentity:string;
  teamRunId?:string;
  replyTo:MessageSummary|null;
  refreshKey?:string;
  onAction:RoleActionExecutor;
  onClearReply:()=>void;
  fixedRecipient?:{id:string;label:string};
  variant?:"panel"|"conversation";
  collapsibleOnMobile?:boolean;
}) {
  const [view,setView] = useState<RoleView<HostConsoleData>|null>(null);
  const [error,setError] = useState<string|null>(null);
  const [refresh,setRefresh] = useState(0);
  useEffect(() => {
    let live=true;
    fetchRoleView<HostConsoleData>(apiUrl,`/v1/views/host-console/${encodeURIComponent(routeIdentity)}`,{space,project})
      .then((value) => { if(live){setView(value);setError(null);} })
      .catch((reason) => { if(live)setError(String(reason)); });
    return () => { live=false; };
  },[apiUrl,space,project,routeIdentity,refreshKey,refresh]);
  if (error) return <p role="alert" className="rounded-lg border border-status-warn/35 bg-status-warn/10 p-3 text-xs">Activity composer unavailable: {error}</p>;
  if (!view) return <div role="status" className="h-28 animate-pulse rounded-xl border border-border bg-muted/25" aria-label="Loading authenticated Activity composer"/>;
  const resolvedRunId = teamRunId ?? view.data.team_supervisor?.team_run_id;
  const scopeMismatch = Boolean(resolvedRunId && view.allowed_actions.some((action) => action.target_ref.kind === "team_run" && action.target_ref.id !== resolvedRunId));
  if (scopeMismatch) return <p role="alert" className="rounded-lg border border-destructive/35 bg-destructive/5 p-3 text-xs">Message actions do not match the selected TeamRun. Activity writes are disabled.</p>;
  return <TeamMessageComposer collapsibleOnMobile={collapsibleOnMobile && variant !== "conversation"} variant={variant} fixedRecipient={fixedRecipient} actions={view.allowed_actions} members={view.data.member_capacity} works={view.data.all_works} replyTo={replyTo} teamId={view.data.team_ref} teamRunId={resolvedRunId} actionsCurrent={view.freshness === "current"} onAction={onAction} onClearReply={onClearReply} onCompleted={() => setRefresh((value) => value+1)}/>;
}

/** Host-only response pressure from the authenticated HostConsole projection.
 * Shared Messages are never reclassified in the browser. */
export function HostActivityLeadInbox({apiUrl,space,project,routeIdentity,refreshKey,onReply,onOpenWork}:{
  apiUrl:string; space:string; project:string; routeIdentity:string; refreshKey?:string;
  onReply:(message:MessageSummary)=>void; onOpenWork:(workId:string)=>void;
}) {
  const [view,setView] = useState<RoleView<HostConsoleData>|null>(null);
  useEffect(() => {
    let live=true;
    fetchRoleView<HostConsoleData>(apiUrl,`/v1/views/host-console/${encodeURIComponent(routeIdentity)}`,{space,project})
      .then((value) => { if(live)setView(value); })
      .catch(() => { if(live)setView(null); });
    return () => { live=false; };
  },[apiUrl,space,project,routeIdentity,refreshKey]);
  if (!view?.data.host_inbox.length) return null;
  const membersById=new Map(view.data.member_capacity.map((member) => [member.agent_member_ref.id,member]));
  return <section aria-labelledby="lead-inbox-title" className="agent-team-lead-inbox mt-4 border-y border-border">
    <header className="flex items-center gap-2 px-1 py-2.5"><Inbox className="size-3.5 text-primary"/><h2 id="lead-inbox-title" className="text-[11px] font-semibold uppercase tracking-[.12em]">Lead Inbox</h2><span className="hidden text-[10px] text-muted-foreground sm:inline">server-projected response pressure</span><span className="ml-auto text-xs font-semibold tabular-nums text-primary">{view.data.host_inbox.length}</span></header>
    <ol>{view.data.host_inbox.slice(0,3).map((message) => { const sender=membersById.get(message.sender.id); return <li key={message.message_id} className="grid gap-2 border-t border-border/75 px-1 py-3 sm:grid-cols-[minmax(10rem,.65fr)_minmax(16rem,1.8fr)_auto] sm:items-center"><div className="flex min-w-0 items-center gap-2">{sender ? <Avatar name={sender.display_name} identity={`${sender.agent_member_ref.id} ${sender.role}`} size="sm" tone={sender.runtime_state === "running" ? "running" : "idle"}/> : <span className="grid size-8 shrink-0 place-items-center rounded-full bg-accent text-primary"><Inbox className="size-3.5"/></span>}<div className="min-w-0"><p className="truncate text-xs font-semibold">{sender?.display_name ?? message.sender.id}</p><p className="truncate text-[10px] text-muted-foreground">{message.kind.replace(/_/g," ")} · response required</p></div></div><p className="min-w-0 text-[13px] leading-relaxed text-foreground/85">{message.body}</p><div className="flex items-center gap-2 sm:justify-end">{message.work_id && <button type="button" className="max-w-32 truncate text-[10px] text-muted-foreground hover:text-primary" onClick={() => onOpenWork(message.work_id!)}>{message.work_id}</button>}<Button size="sm" variant="ghost" aria-label="Respond from Lead Inbox" onClick={() => onReply(message)}>Respond <ArrowRight className="size-3.5"/></Button></div></li>;})}</ol>
  </section>;
}
