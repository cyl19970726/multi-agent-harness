import { useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { Activity, ArrowLeft, ArrowRight, BriefcaseBusiness, ExternalLink, MessageSquare, RadioTower, RefreshCw, Search, ShieldCheck, Users, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import { TeamMessageComposer } from "@/components/workbench/team/TeamMessageComposer";
import { fetchNativeMemberActivity } from "../api";
import type { NativeActivityProjection } from "../types";
import {
  fetchRoleView,
  type HostConsoleData,
  type MemberCapacitySummary,
  type MessageSummary,
  type RoleActionExecutor,
  type RoleView,
  type TeamActivitySummary,
  type TeamWorkspaceData,
  type WorkSummary,
} from "../model/roleViews";
import type { SelectionState } from "../app/selection";
import { RoleActionPanel } from "./RoleActionPanel";
import { ViewProvenance } from "./RoleViewPrimitives";

type TimelineRow =
  | {kind:"message";at:string;message:MessageSummary}
  | {kind:"activity";at:string;activity:TeamActivitySummary}
  | {kind:"native";at:string;native:NativeActivityProjection["items"][number]};

export function AgentConversationWorkspace({apiUrl,space,project,routeIdentity,view,selection,refreshKey,onAction,actionsCurrent,onSelectionChange}:{
  apiUrl:string;
  space:string;
  project:string;
  routeIdentity:string;
  view:RoleView<TeamWorkspaceData>;
  selection:SelectionState;
  refreshKey?:string;
  onAction:RoleActionExecutor;
  actionsCurrent:boolean;
  onSelectionChange:(next:Partial<SelectionState>)=>void;
}) {
  const team=view.data.team;
  const selectedMember=view.data.members.find((member) => member.current_member_run_ref === selection.memberRunId)
    ?? view.data.members.find((member) => member.agent_member_ref.id === selection.teamConversation);
  const targetId=selection.teamConversation === "host" ? team.host_agent_id : selectedMember?.agent_member_ref.id;
  const targetLabel=selection.teamConversation === "host" ? "Host Agent" : selectedMember?.display_name ?? targetId ?? "Agent";
  const isHostTarget=selection.teamConversation === "host";
  const [replyTo,setReplyTo]=useState<MessageSummary|null>(null);
  const [nativeActivity,setNativeActivity]=useState<NativeActivityProjection|null>(null);
  const [nativeError,setNativeError]=useState<string|null>(null);
  const [nativeLoading,setNativeLoading]=useState(false);
  const [hostView,setHostView]=useState<RoleView<HostConsoleData>|null>(null);
  const [hostViewError,setHostViewError]=useState<string|null>(null);
  const [hostRefresh,setHostRefresh]=useState(0);
  const [mobileAgentsOpen,setMobileAgentsOpen]=useState(false);
  const [mobileContextOpen,setMobileContextOpen]=useState(false);
  const agentsButtonRef=useRef<HTMLButtonElement>(null);
  const contextButtonRef=useRef<HTMLButtonElement>(null);
  const selectedMemberRunId=selectedMember?.current_member_run_ref ?? selection.memberRunId;

  useEffect(() => {
    setReplyTo(null);
  },[targetId]);

  useEffect(() => {
    let live=true;
    setNativeActivity(null);
    setNativeError(null);
    if (!selectedMemberRunId || isHostTarget) return () => { live=false; };
    setNativeLoading(true);
    fetchNativeMemberActivity(apiUrl,selectedMemberRunId,project,space)
      .then((value) => { if(live)setNativeActivity(value); })
      .catch((reason) => { if(live)setNativeError(String(reason)); })
      .finally(() => { if(live)setNativeLoading(false); });
    return () => { live=false; };
  },[apiUrl,space,project,selectedMemberRunId,isHostTarget,refreshKey]);

  useEffect(() => {
    let live=true;
    setHostView(null);
    setHostViewError(null);
    if (team.viewer_role !== "host") return () => { live=false; };
    fetchRoleView<HostConsoleData>(apiUrl,`/v1/views/host-console/${encodeURIComponent(routeIdentity)}`,{space,project})
      .then((value) => { if(live)setHostView(value); })
      .catch((reason) => { if(live)setHostViewError(String(reason)); });
    return () => { live=false; };
  },[apiUrl,space,project,routeIdentity,team.viewer_role,refreshKey,hostRefresh]);

  const memberWork=useMemo(() => view.data.works
    .filter((work) => targetId && work.owner_actor_ref?.id === targetId)
    .sort((left,right) => phaseRank(left.phase)-phaseRank(right.phase) || right.updated_at.localeCompare(left.updated_at)),[view.data.works,targetId]);
  const boundWork=selectedMemberRunId
    ? memberWork.filter((work) => work.current_member_run_ref === selectedMemberRunId)
    : [];
  const relatedWorkIds=new Set(memberWork.map((work) => work.work_id));
  const messages=view.data.messages.filter((message) => targetId && (message.sender.id === targetId || message.recipients.some((recipient) => recipient.id === targetId)));
  const activity=view.data.activity.filter((item) => item.source !== "message" && targetId && (item.actor_ref?.id === targetId || Boolean(item.work_id && relatedWorkIds.has(item.work_id))));
  const timeline:TimelineRow[]=[
    ...messages.map((message):TimelineRow => ({kind:"message",at:message.created_at,message})),
    ...activity.map((item):TimelineRow => ({kind:"activity",at:item.created_at,activity:item})),
    ...(nativeActivity?.items ?? []).map((item,index):TimelineRow => ({kind:"native",at:item.occurred_at ?? `0000-${String(index).padStart(6,"0")}`,native:item})),
  ].sort((left,right) => left.at.localeCompare(right.at));
  const contextualActions=(hostView?.allowed_actions ?? []).filter((action) =>
    Boolean(selectedMemberRunId && action.target_ref.kind === "member_run" && action.target_ref.id === selectedMemberRunId));
  const otherOwnedWork=memberWork.filter((work) => work.current_member_run_ref !== selectedMemberRunId);
  const showContext=Boolean(boundWork.length || selectedMemberRunId || otherOwnedWork.length || contextualActions.length);
  const canCompose=team.viewer_role === "host" && !isHostTarget && Boolean(hostView);

  const openMember=(member:MemberCapacitySummary) => onSelectionChange({
    teamMode:"workspace",
    teamConversation:member.agent_member_ref.id,
    memberRunId:member.current_member_run_ref ?? undefined,
    teamTab:"members",
  });
  const openHost=() => onSelectionChange({teamMode:"workspace",teamConversation:"host",memberRunId:undefined,teamTab:"members"});
  const closeConversation=() => onSelectionChange({teamConversation:undefined,memberRunId:undefined,teamMode:"workspace",teamTab:"members"});

  return <main className="agent-team-surface h-full min-h-0 flex-1 overflow-hidden bg-background" data-testid="agent-conversation-workspace">
    <div className={`grid h-full min-h-0 ${showContext ? "lg:grid-cols-[18.5rem_minmax(0,1fr)_19.5rem]" : "lg:grid-cols-[18.5rem_minmax(0,1fr)]"}`}>
      <aside className="hidden min-h-0 overflow-y-auto border-r border-border bg-[#fbf8f3] lg:block" aria-label="Agent conversations">
        <ConversationNavigation team={team} members={view.data.members} selectedTargetId={targetId} onBack={closeConversation} onOpenHost={openHost} onOpenMember={openMember}/>
      </aside>

      <section className="flex min-h-0 min-w-0 flex-col">
        <header className="shrink-0 border-b border-border bg-card px-3 py-3.5 sm:px-6">
          <div className="flex min-w-0 items-center gap-3">
            <Button size="sm" variant="secondary" className="lg:hidden" onClick={closeConversation} aria-label="Back to Team Workspace"><ArrowLeft className="size-4"/></Button>
            {selectedMember ? <Avatar name={selectedMember.display_name} identity={`${selectedMember.agent_member_ref.id} ${selectedMember.role}`} size="lg" tone={selectedMember.runtime_state === "running" ? "running" : selectedMember.capacity === "available" ? "good" : "idle"}/> : <span className="grid size-10 shrink-0 place-items-center rounded-full bg-primary/10 text-primary"><ShieldCheck className="size-4"/></span>}
            <div className="min-w-0 flex-1"><div className="flex min-w-0 flex-wrap items-center gap-2"><h1 className="truncate text-lg font-semibold tracking-[-0.02em] sm:text-xl">{targetLabel}</h1><Badge>{isHostTarget ? "Host Agent" : selectedMember?.role ?? "Agent"}</Badge></div><p className="mt-0.5 truncate text-[11px] text-muted-foreground">{isHostTarget ? `Host Agent · ${team.host_agent_id}` : `${selectedMember?.provider ?? "provider not projected"} · ${selectedMember?.model ?? "model not projected"} · ${selectedMemberRunId ?? "no current MemberRun"}`}</p></div>
            <Button ref={agentsButtonRef} size="sm" variant="secondary" className="lg:hidden" onClick={() => setMobileAgentsOpen(true)} aria-label="Open Agent list" aria-expanded={mobileAgentsOpen}><Users className="size-4"/></Button>
            {showContext && <Button ref={contextButtonRef} size="sm" variant="secondary" className="lg:hidden" onClick={() => setMobileContextOpen(true)} aria-label="Open conversation context" aria-expanded={mobileContextOpen}><BriefcaseBusiness className="size-4"/></Button>}
            <div className="hidden md:block"><ViewProvenance view={view}/></div>
          </div>
        </header>

        {boundWork[0] && <button type="button" onClick={() => setMobileContextOpen(true)} className="flex min-h-11 items-center gap-2 border-b border-border bg-accent/25 px-3 text-left text-[10px] lg:hidden"><BriefcaseBusiness className="size-3.5 text-primary"/><span className="min-w-0 flex-1 truncate"><b>Current Work</b> · {boundWork[0].title || boundWork[0].work_id}</span><Badge tone={boundWork[0].condition === "blocked" ? "bad" : boundWork[0].phase === "review" ? "warn" : "muted"}>{boundWork[0].condition !== "normal" ? boundWork[0].condition : boundWork[0].phase}</Badge><ArrowRight className="size-3.5"/></button>}

        <div className="agent-team-chat-canvas min-h-0 flex-1 overflow-y-auto px-3 py-4 sm:px-6" tabIndex={0} aria-label={`Conversation with ${targetLabel}`}>
          <div className="mx-auto max-w-[58rem] space-y-1">
            {timeline.map((row,index) => <ConversationRow key={`${row.kind}:${row.at}:${index}`} row={row} members={view.data.members} hostId={team.host_agent_id} onOpenWork={(workId) => onSelectionChange({teamConversation:undefined,memberRunId:undefined,teamTab:"works",teamWorkId:workId})} onReply={canCompose ? setReplyTo : undefined}/>) }
            {nativeLoading && <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/20 p-3 text-xs text-muted-foreground"><RefreshCw className="size-3.5 animate-spin"/>Reading provider-native activity on demand…</div>}
            {nativeError && <div className="rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground"><b>Provider-native activity unavailable.</b> Durable coordination remains visible. {nativeError}</div>}
            {isHostTarget && <div className="rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground">Host conversation is addressable through canonical Messages. Host execution is not fabricated as a MemberRun or native-session timeline.</div>}
            {!timeline.length && !nativeLoading && !nativeError && !isHostTarget && <div className="grid min-h-64 place-items-center rounded-xl border border-dashed border-border p-8 text-center"><div><MessageSquare className="mx-auto size-6 text-muted-foreground"/><h2 className="mt-3 text-sm font-medium">No conversation yet</h2><p className="mt-1 text-xs text-muted-foreground">Send the first coordination message. Work ownership changes only through Work actions.</p></div></div>}
          </div>
        </div>

        <div className="shrink-0 bg-background">
          {canCompose && hostView ? <TeamMessageComposer variant="conversation" actions={hostView.allowed_actions} members={hostView.data.member_capacity} works={hostView.data.all_works} replyTo={replyTo} fixedRecipient={{id:targetId!,label:targetLabel}} teamId={hostView.data.team_ref} teamRunId={team.latest_run?.id} actionsCurrent={actionsCurrent && hostView.freshness === "current"} onAction={onAction} onClearReply={() => setReplyTo(null)} onCompleted={() => setHostRefresh((value) => value+1)}/> : <div className="border-t border-border px-4 py-3 text-xs text-muted-foreground">{hostViewError ? `Message controls unavailable: ${hostViewError}` : isHostTarget && team.viewer_role === "host" ? "This is the authenticated Host identity. An Operator-to-Host authoring action is not projected, so the browser will not fabricate one." : "This RoleView does not authorize conversation writes for the selected actor."}</div>}
        </div>
      </section>

      {showContext && <aside className="hidden min-h-0 overflow-y-auto border-l border-border bg-[#fffaf4] p-4 lg:block" aria-label="Conversation context"><ConversationContext boundWork={boundWork} otherOwnedWork={otherOwnedWork} member={selectedMember} actions={contextualActions} onAction={onAction} actionsCurrent={actionsCurrent && hostView?.freshness === "current"} teamId={team.team_id} teamRunId={team.latest_run?.id} onCompleted={() => setHostRefresh((value) => value+1)} onOpenProfile={() => selectedMember && onSelectionChange({surface:"agents",memberId:selectedMember.agent_member_ref.id,teamConversation:undefined,memberRunId:undefined})}/></aside>}
    </div>
    {mobileAgentsOpen && <MobileSheet title="Team agents" onClose={() => setMobileAgentsOpen(false)} triggerRef={agentsButtonRef}><ConversationNavigation team={team} members={view.data.members} selectedTargetId={targetId} onBack={() => { setMobileAgentsOpen(false); closeConversation(); }} onOpenHost={() => { setMobileAgentsOpen(false); openHost(); }} onOpenMember={(member) => { setMobileAgentsOpen(false); openMember(member); }}/></MobileSheet>}
    {mobileContextOpen && showContext && <MobileSheet title="Conversation context" onClose={() => setMobileContextOpen(false)} triggerRef={contextButtonRef}><div className="p-4"><ConversationContext boundWork={boundWork} otherOwnedWork={otherOwnedWork} member={selectedMember} actions={contextualActions} onAction={onAction} actionsCurrent={actionsCurrent && hostView?.freshness === "current"} teamId={team.team_id} teamRunId={team.latest_run?.id} onCompleted={() => setHostRefresh((value) => value+1)} onOpenProfile={() => selectedMember && onSelectionChange({surface:"agents",memberId:selectedMember.agent_member_ref.id,teamConversation:undefined,memberRunId:undefined})}/></div></MobileSheet>}
  </main>;
}

function MobileSheet({title,onClose,triggerRef,children}:{title:string;onClose:()=>void;triggerRef:RefObject<HTMLButtonElement|null>;children:ReactNode}) {
  const sheetRef=useRef<HTMLDivElement>(null);
  const closeRef=useRef(onClose);
  closeRef.current=onClose;
  useEffect(() => {
    const sheet=sheetRef.current;
    sheet?.querySelector<HTMLElement>("button, [href], input, select, textarea, [tabindex]")?.focus();
    const onKeyDown=(event:KeyboardEvent) => {
      if(event.key === "Escape") { event.preventDefault(); closeRef.current(); return; }
      if(event.key !== "Tab") return;
      const focusable=[...(sheet?.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])') ?? [])];
      if(!focusable.length) return;
      const first=focusable[0],last=focusable[focusable.length-1];
      if(event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if(!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown",onKeyDown);
    return () => { document.removeEventListener("keydown",onKeyDown); window.requestAnimationFrame(() => triggerRef.current?.focus()); };
  },[triggerRef]);
  return <div className="fixed inset-0 z-50 bg-black/30 lg:hidden" role="presentation" onMouseDown={(event) => { if(event.target === event.currentTarget)onClose(); }}><div ref={sheetRef} role="dialog" aria-modal="true" aria-label={title} className="agent-team-sheet-enter absolute inset-y-0 right-0 w-[min(92vw,24rem)] overflow-y-auto border-l border-border bg-background shadow-2xl"><header className="sticky top-0 z-10 flex min-h-12 items-center justify-between border-b border-border bg-background px-3"><h2 className="text-sm font-semibold">{title}</h2><Button size="sm" variant="secondary" onClick={onClose}><X className="size-4"/>Close</Button></header>{children}</div></div>;
}

function ConversationNavigation({team,members,selectedTargetId,onBack,onOpenHost,onOpenMember}:{team:TeamWorkspaceData["team"];members:MemberCapacitySummary[];selectedTargetId?:string;onBack:()=>void;onOpenHost:()=>void;onOpenMember:(member:MemberCapacitySummary)=>void}) {
  const [query,setQuery]=useState("");
  const visible=members.filter((member) => [member.display_name,member.role,member.agent_member_ref.id].join(" ").toLowerCase().includes(query.trim().toLowerCase()));
  return <div className="p-3"><div className="flex items-start justify-between gap-2 px-1"><div className="min-w-0"><p className="truncate text-base font-semibold tracking-[-0.01em]">{team.display_name || team.team_id}</p><button type="button" onClick={onBack} className="mt-1 flex items-center gap-1 text-[10px] text-muted-foreground hover:text-primary"><ArrowLeft className="size-3"/>Back to Team Workspace</button></div><Button size="icon" variant="secondary" onClick={onBack} aria-label="Back to Team Workspace"><X className="size-4"/></Button></div><label className="relative mt-4 block"><span className="sr-only">Search Agent conversations</span><Search className="pointer-events-none absolute left-3 top-2.5 size-3.5 text-muted-foreground"/><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search conversations…" className="h-9 w-full rounded-md border border-border bg-card pl-9 pr-3 text-xs"/></label><div className="mt-4 space-y-1"><p className="px-2 text-[9px] font-semibold uppercase tracking-[.14em] text-muted-foreground">Host Agent</p><ConversationTarget selected={selectedTargetId === team.host_agent_id} label="Host Agent" detail={team.host_agent_id} tone="host" onClick={onOpenHost}/><p className="px-2 pt-4 text-[9px] font-semibold uppercase tracking-[.14em] text-muted-foreground">Team Members</p>{visible.map((member) => <button key={member.agent_member_ref.id} type="button" onClick={() => onOpenMember(member)} className={`flex min-h-[4.5rem] w-full items-center gap-3 rounded-xl px-2.5 py-2.5 text-left transition ${selectedTargetId === member.agent_member_ref.id ? "agent-team-selected" : "border border-transparent hover:bg-card/75"}`}><Avatar name={member.display_name} identity={`${member.agent_member_ref.id} ${member.role}`} size="lg" tone={member.runtime_state === "running" ? "running" : member.capacity === "available" ? "good" : "idle"}/><span className="min-w-0 flex-1"><span className="block truncate text-sm font-semibold">{member.display_name}</span><span className="mt-1 block truncate text-[10px] text-muted-foreground">{member.role}</span><span className="mt-0.5 block truncate text-[9px] text-muted-foreground">{member.runtime_state ?? member.capacity} · {member.current_member_run_ref ?? "no current run"}</span></span>{member.blocked_work_count + member.review_work_count > 0 && <span className="rounded-full bg-primary/10 px-2 py-0.5 text-[9px] font-semibold text-primary">{member.blocked_work_count + member.review_work_count}</span>}<ArrowRight className="size-3 text-muted-foreground"/></button>)}{!visible.length && <div className="rounded-lg border border-dashed border-border p-4 text-center text-[10px] text-muted-foreground">No Agent matches this search.</div>}</div></div>;
}

function ConversationTarget({selected,label,detail,tone,onClick}:{selected:boolean;label:string;detail:string;tone:"host";onClick:()=>void}) { return <button type="button" onClick={onClick} className={`flex min-h-[4.5rem] w-full items-center gap-3 rounded-xl px-2.5 py-2.5 text-left transition ${selected ? "agent-team-selected" : "border border-transparent hover:bg-card/75"}`}><span className="grid size-12 shrink-0 place-items-center rounded-full bg-primary/10 text-primary ring-1 ring-primary/15"><ShieldCheck className="size-5"/></span><span className="min-w-0 flex-1"><span className="flex items-center gap-1.5 text-sm font-semibold">{label}<Badge tone={tone === "host" ? "decision" : "muted"}>Host</Badge></span><span className="mt-1 block truncate text-[10px] text-muted-foreground">{detail}</span><span className="mt-0.5 block text-[9px] text-status-good">Addressable Team identity</span></span><ArrowRight className="size-3 text-muted-foreground"/></button>; }

function ConversationRow({row,members,hostId,onOpenWork,onReply}:{row:TimelineRow;members:MemberCapacitySummary[];hostId:string;onOpenWork:(id:string)=>void;onReply?:(message:MessageSummary)=>void}) {
  const byId=new Map(members.map((member) => [member.agent_member_ref.id,member]));
  if(row.kind === "native") return <article className="flex gap-3 border-y border-border/70 px-1 py-3"><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-muted text-muted-foreground"><RadioTower className="size-3.5"/></span><div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-2 text-[10px] text-muted-foreground"><b className="uppercase tracking-[.1em]">Provider-native</b><span>read on demand · not mirrored</span><Badge>{row.native.kind}</Badge><time className="ml-auto">{formatTime(row.native.occurred_at)}</time></div><h3 className="mt-1 text-xs font-semibold">{row.native.title}</h3>{row.native.summary && <p className="mt-1 whitespace-pre-wrap text-xs leading-relaxed text-muted-foreground">{row.native.summary}</p>}</div></article>;
  if(row.kind === "activity") { const actor=row.activity.actor_ref ? byId.get(row.activity.actor_ref.id) : undefined; return <article className="flex gap-3 border-b border-border/70 px-1 py-3"><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-muted text-muted-foreground"><Activity className="size-3.5"/></span><div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-2 text-[10px]"><b className="uppercase tracking-[.1em] text-muted-foreground">{row.activity.source.replace(/_/g," ")}</b>{row.activity.status && <Badge>{row.activity.status.replace(/_/g," ")}</Badge>}<span>{actor?.display_name ?? row.activity.actor_ref?.id ?? "System"}</span><time className="ml-auto text-muted-foreground">{formatTime(row.activity.created_at)}</time></div>{row.activity.summary && <p className="mt-1 text-xs leading-relaxed">{row.activity.summary}</p>}<div className="mt-2 flex items-center gap-2 text-[10px] text-muted-foreground"><span className="font-mono">{row.activity.id}</span>{row.activity.work_id && <button type="button" className="ml-auto text-primary hover:underline" onClick={() => onOpenWork(row.activity.work_id!)}>Open Work</button>}</div></div></article>; }
  const sender=byId.get(row.message.sender.id);
  const senderLabel=sender?.display_name ?? (row.message.sender.id === hostId ? "Host Agent" : row.message.sender.id);
  const recipientLabels=row.message.recipients.map((recipient) => byId.get(recipient.id)?.display_name ?? (recipient.id === hostId ? "Host Agent" : recipient.id));
  return <article className="flex gap-3 px-1 py-3"><div className="shrink-0">{sender ? <Avatar name={sender.display_name} identity={`${sender.agent_member_ref.id} ${sender.role}`} size="sm" tone={sender.runtime_state === "running" ? "running" : "idle"}/> : <span className="grid size-8 place-items-center rounded-full bg-primary/10 text-primary"><ShieldCheck className="size-3.5"/></span>}</div><div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-2 text-[10px]"><b className="text-foreground">{senderLabel}</b><ArrowRight className="size-3 text-muted-foreground"/><span className="text-muted-foreground">{recipientLabels.join(", ") || "Team"}</span><Badge>Harness message</Badge><time className="ml-auto text-muted-foreground">{formatTime(row.message.created_at)}</time></div><div className="mt-2 text-sm leading-relaxed"><Markdown source={row.message.body} compact/></div><div className="mt-2 flex flex-wrap items-center gap-2 text-[10px] text-muted-foreground">{row.message.work_id && <Button size="sm" variant="secondary" className="h-7 px-2 text-[10px]" onClick={() => onOpenWork(row.message.work_id!)}>Work · {row.message.work_id}</Button>}<span>chain {row.message.correlation_id}</span><details><summary className="cursor-pointer">{Array.from(new Set(row.message.deliveries.map((delivery) => delivery.status))).join(" · ") || "no delivery rows"}</summary>{row.message.deliveries.length > 0 && <ul className="mt-1 space-y-1 rounded-md bg-muted/35 p-2">{row.message.deliveries.map((delivery) => <li key={delivery.id} className="min-w-0 break-all">MemberRun recipient {delivery.recipient_member_run_id} · {delivery.status} · v{delivery.version}{delivery.provider_receipt_id ? " · receipt " + delivery.provider_receipt_id : " · no provider receipt"}</li>)}</ul>}</details>{onReply && row.message.reply_eligible && <button type="button" className="ml-auto text-primary hover:underline" onClick={() => onReply(row.message)}>Reply</button>}</div></div></article>;
}

function ConversationContext({boundWork,otherOwnedWork,member,actions,onAction,actionsCurrent,teamId,teamRunId,onCompleted,onOpenProfile}:{boundWork:WorkSummary[];otherOwnedWork:WorkSummary[];member?:MemberCapacitySummary;actions:RoleView<unknown>["allowed_actions"];onAction:RoleActionExecutor;actionsCurrent:boolean;teamId:string;teamRunId?:string;onCompleted:()=>void;onOpenProfile:()=>void}) {
  return <div className="space-y-4 text-xs"><header className="border-b border-border pb-3"><p className="agent-team-eyebrow">Decision context</p><h2 className="company-editorial-title mt-1 text-lg">Current Work and execution</h2></header>{boundWork.length > 0 ? <section><div className="flex items-center gap-2"><BriefcaseBusiness className="size-3.5 text-primary"/><h3 className="font-semibold">Works bound to this MemberRun</h3></div><div className="mt-2 divide-y divide-border">{boundWork.map((item) => <div key={item.work_id} className="py-2"><p className="font-medium">{item.title || item.work_id}</p><p className="mt-1 break-all font-mono text-[9px] text-muted-foreground">Exact MemberRun binding · {item.work_id} · v{item.work_revision}</p><p className="mt-1 text-[9px] text-muted-foreground">{item.phase} · {item.condition}{item.resolution ? " · " + item.resolution : ""}</p></div>)}</div></section> : member && <p className="border-y border-dashed border-border py-3 text-[10px] text-muted-foreground">No Work is bound to this exact MemberRun. Owned Work is not treated as current execution.</p>}{otherOwnedWork.length > 0 && <section className="border-t border-border pt-4"><h3 className="font-semibold">Other owned Work</h3><div className="mt-2 divide-y divide-border">{otherOwnedWork.map((item) => <div key={item.work_id} className="py-2"><p className="font-medium">{item.title || item.work_id}</p><p className="mt-1 text-[9px] text-muted-foreground">{item.phase} · {item.condition} · not the selected MemberRun binding</p></div>)}</div></section>}{member && <section className="border-t border-border pt-4"><div className="flex items-center gap-2"><Activity className="size-3.5 text-primary"/><h3 className="font-semibold">Current execution</h3></div><dl className="mt-3 space-y-1.5"><ContextFact label="MemberRun" value={member.current_member_run_ref ?? "none"}/><ContextFact label="Runtime" value={`${member.runtime_state ?? "unknown"}${member.runtime_generation != null ? ` · g${member.runtime_generation}` : ""}`}/><ContextFact label="Session" value={member.native_session_health ?? "unknown"}/><ContextFact label="Capacity" value={member.capacity}/></dl><Button className="mt-3 w-full" size="sm" variant="secondary" onClick={onOpenProfile}><ExternalLink className="size-3.5"/>Full Member profile</Button></section>}{actions.length > 0 && <section className="border-t border-border pt-4"><div className="mb-2 flex items-center gap-2"><ShieldCheck className="size-3.5 text-primary"/><h3 className="font-semibold">Available controls</h3></div><RoleActionPanel compact actions={actions} onAction={onAction} actionsCurrent={actionsCurrent} context={{teamId,teamRunId}} onCompleted={onCompleted}/></section>}<p className="border-t border-border pt-3 text-[9px] leading-relaxed text-muted-foreground">Messages, runtime controls and Work transitions remain separate authenticated operations.</p></div>;
}

function ContextFact({label,value}:{label:string;value:string}) { return <div className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2"><dt className="text-muted-foreground">{label}</dt><dd className="truncate text-right font-medium" title={value}>{value}</dd></div>; }
function phaseRank(phase:string) { return phase === "active" ? 0 : phase === "review" ? 1 : phase === "open" ? 2 : 3; }
function formatTime(value?:string|null) { if(!value)return "time unavailable"; const date=new Date(value); return Number.isNaN(date.getTime()) ? value : date.toLocaleString(); }
