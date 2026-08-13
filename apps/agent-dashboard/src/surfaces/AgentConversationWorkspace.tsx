import * as ScrollArea from "@radix-ui/react-scroll-area";
import * as Tabs from "@radix-ui/react-tabs";
import * as Tooltip from "@radix-ui/react-tooltip";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  Activity, ArrowLeft, BriefcaseBusiness, ChevronRight, Circle, Clock3,
  History, Inbox, KeyRound, MessageSquare,
  PanelRight, Search, ShieldCheck, SlidersHorizontal, Sparkles, TerminalSquare,
  UserRound, Users, Wrench, X,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import {
  WorkspaceActionIndex,
  WorkspaceCanvasIntro,
  WorkspaceFact,
  WorkspaceSection,
  WorkspaceState,
} from "@/components/workbench/agent/AgentWorkspacePrimitives";
import { AgentMessageCommandComposer } from "@/components/workbench/agent/AgentMessageCommandComposer";
import { eventPresentation, OperationalFactRow } from "@/components/workbench/agent/AgentStreamPrimitives";
import type { SelectionState } from "../app/selection";
import {
  fetchRoleView,
  type AgentWorkspaceActivityItem,
  type AgentWorkspaceData,
  type AgentWorkspaceRosterItem,
  type AllowedAction,
  type MessageSummary,
  type RoleActionExecutor,
  type RoleView,
  type WorkSummary,
} from "../model/roleViews";
import { RoleActionPanel } from "./RoleActionPanel";
import { ViewProvenance, ViewState } from "./RoleViewPrimitives";
import "./agent-workspace.css";

type WorkspaceMode = "session" | "messages" | "work";
/**
 * Client-only provider activity. The shell may inject the latest exact-run
 * preview from its in-memory SSE registry; this surface never fetches,
 * persists, replays or manufactures one.
 */
export interface AgentWorkspaceLiveActivity {
  team_run_id:string;
  member_run_id:string;
  provider:string;
  kind:string;
  title?:string;
  summary:string;
  status?:string;
  revision:number;
  emitted_at:string;
  expires_at:string;
  provider_event_ref?:string|null;
  agent_session_id?:string|null;
  runtime_generation?:number|null;
  stream_seq?:number|null;
}
type ContextSelection =
  | {kind:"event"; event:AgentWorkspaceActivityItem}
  | {kind:"message"; message:MessageSummary}
  | {kind:"work"; work:WorkSummary}
  | null;

export function AgentConversationWorkspace({
  apiUrl,space,project,company,routeIdentity,selection,refreshKey,liveActivity,onAction,actionsEnabled,onSelectionChange,
}:{
  apiUrl:string; space:string; project:string; company?:string; routeIdentity:string; selection:SelectionState;
  liveActivity?:AgentWorkspaceLiveActivity|null;
  refreshKey?:string; onAction:RoleActionExecutor; actionsEnabled:boolean;
  onSelectionChange:(next:Partial<SelectionState>)=>void;
}) {
  const [view,setView]=useState<RoleView<AgentWorkspaceData>|null>(null);
  const [viewRequestPath,setViewRequestPath]=useState<string|null>(null);
  const [error,setError]=useState<string|null>(null);
  const [loading,setLoading]=useState(true);
  const [refresh,setRefresh]=useState(0);
  const [contextSelection,setContextSelection]=useState<ContextSelection>(null);
  const [profileOpen,setProfileOpen]=useState(false);
  const [rosterOpen,setRosterOpen]=useState(false);
  const [contextOpen,setContextOpen]=useState(false);
  const profileTriggerRef=useRef<HTMLButtonElement>(null);
  const profileCloseRef=useRef<HTMLButtonElement>(null);
  const workspaceRef=useRef<HTMLElement>(null);
  const mode:WorkspaceMode=selection.agentWorkspaceMode ?? "session";
  const agentId=selection.teamConversation && selection.teamConversation !== "host" ? selection.teamConversation : undefined;
  const requestQuery=new URLSearchParams();
  if(agentId)requestQuery.set("agent_id",agentId);
  if(selection.agentSessionId)requestQuery.set("session_id",selection.agentSessionId);
  const requestPath=`/v1/views/agent-workspace/${encodeURIComponent(routeIdentity)}${requestQuery.size?`?${requestQuery.toString()}`:""}`;

  useEffect(()=>{
    let live=true;
    setLoading(true);
    setError(null);
    fetchRoleView<AgentWorkspaceData>(apiUrl,requestPath,{space,project,company})
      .then((next)=>{if(live){setView(next);setViewRequestPath(requestPath);setError(null);setContextSelection(null);}})
      .catch((reason)=>{if(live)setError(String(reason));})
      .finally(()=>{if(live)setLoading(false);});
    return()=>{live=false;};
  },[apiUrl,space,project,company,requestPath,refreshKey,refresh]);
  useEffect(()=>{
    const frame=window.requestAnimationFrame(()=>{
      const root=workspaceRef.current;
      root?.querySelector<HTMLElement>('[role="tabpanel"][data-state="active"] [data-radix-scroll-area-viewport]')?.scrollTo({top:0,left:0,behavior:"auto"});
    });
    return()=>window.cancelAnimationFrame(frame);
  },[mode,selection.teamConversation]);

  const currentView=viewRequestPath===requestPath?view:null;
  if(!currentView)return <main className="agent-team-surface h-full min-h-0 flex-1"><ViewState loading={loading} error={error} identityLabel={`Agent Workspace · ${routeIdentity}`} onRetry={()=>setRefresh(value=>value+1)}>{null}</ViewState></main>;
  const data=currentView.data;
  // This surface owns an independently authenticated RoleView. Its write
  // freshness must therefore follow that exact projection, not the ambient
  // snapshot domains used by the surrounding dashboard shell.
  const actionsCurrent=actionsEnabled && currentView.freshness==="current" && !loading && !error;
  const selected=data.selected_agent;
  const publicProjection=data.projection_scope==="host_member_public";
  const selectedRunId=selected.current_member_run_ref;
  const currentLiveActivity=!publicProjection
    && liveActivity
    && selectedRunId
    && liveActivity.member_run_id===selectedRunId
    && liveActivity.team_run_id===data.team.latest_run_id
    && isUnexpiredActivity(liveActivity)
      ? liveActivity
      : null;
  const selectedRoster=data.roster.find(item=>item.agent_member_ref.id===selected.agent_member_ref.id);
  const currentWork=data.works.find(work=>work.work_id===(contextSelection?.kind==="work"?contextSelection.work.work_id:data.context_summary.current_work_id));
  const selectAgent=(agent:AgentWorkspaceRosterItem)=>{
    onSelectionChange({
      teamConversation:agent.is_host ? "host" : agent.agent_member_ref.id,
      memberRunId:agent.is_host ? undefined : agent.current_member_run_ref ?? undefined,
      agentWorkspaceMode:"session",
      agentSessionId:undefined,
      teamWorkId:undefined,
    });
    setRosterOpen(false);
  };
  const closeWorkspace=()=>onSelectionChange({teamConversation:undefined,memberRunId:undefined,agentWorkspaceMode:undefined,agentSessionId:undefined,teamWorkId:undefined});
  const context=<AgentContextRail view={currentView} data={data} selected={contextSelection} currentWork={currentWork} actions={currentView.allowed_actions}/>;

  return <Tooltip.Provider delayDuration={350}>
    <main ref={workspaceRef} className="agent-team-surface agent-workspace h-full min-h-0 flex-1 overflow-hidden" data-testid="agent-workspace">
      <div className="agent-workspace-layout grid h-full min-h-0 grid-cols-1 lg:grid-cols-[17.5rem_minmax(0,1fr)_18.125rem]" data-host={selected.is_host||undefined}>
        <aside className="agent-workspace-roster hidden min-h-0 border-r border-border lg:flex lg:flex-col" aria-label="Agent roster">
          <AgentRoster data={data} selectedId={selected.agent_member_ref.id} onBack={closeWorkspace} onSelect={selectAgent}/>
        </aside>

        <section className="agent-workspace-center flex min-h-0 min-w-0 flex-col">
          <header data-testid="agent-workspace-identity" className="agent-workspace-header flex min-h-[5.5rem] shrink-0 items-center gap-3 border-b border-border px-4 sm:px-7">
            <Button size="icon" variant="secondary" className="lg:hidden" onClick={closeWorkspace} aria-label="Back to Team Workspace"><ArrowLeft className="size-4"/></Button>
            <button ref={profileTriggerRef} type="button" className="group flex min-w-0 flex-1 items-center gap-3 text-left" onClick={()=>setProfileOpen(true)} aria-label={`Open ${selected.display_name} configuration`}>
              <Avatar name={selected.display_name} identity={`${selected.agent_member_ref.id} ${selected.role}`} size="lg" tone={selected.runtime_status==="running"?"running":selected.runtime_status==="idle"?"good":"idle"}/>
              <span className="min-w-0">
                <span className="flex min-w-0 items-center gap-2"><span className="truncate text-[1.28rem] font-semibold leading-tight tracking-[-0.025em] text-foreground">{selected.display_name}</span><ChevronRight className="size-3.5 text-muted-foreground transition-transform group-hover:translate-x-0.5"/></span>
                <span className="mt-1 flex min-w-0 items-center gap-2 text-[11px] text-muted-foreground"><span className="truncate">{humanizeToken(selected.role)}</span><span>·</span>{publicProjection?<span>Public coordination view</span>:<><span>{selected.provider ? humanizeToken(selected.provider) : "No provider bound"}</span><span>·</span><span className="truncate">{data.selected_session_id ? shortId(data.selected_session_id) : "No native Session"}</span></>}</span>
              </span>
            </button>
            {!publicProjection&&<div className="hidden items-center gap-2 md:flex"><RuntimePill state={selected.runtime_status}/></div>}
            <Button size="icon" variant="secondary" className="lg:hidden" onClick={()=>setRosterOpen(true)} aria-label="Open Agent roster"><Users className="size-4"/></Button>
            <Button size="icon" variant="secondary" className="lg:hidden" onClick={()=>setContextOpen(true)} aria-label="Open Agent context"><PanelRight className="size-4"/></Button>
          </header>

          {error&&<div role="alert" className="border-b border-status-warn/25 bg-status-warn/5 px-6 py-2 text-xs">Refresh failed; writes are disabled until the authoritative view returns. {error}</div>}
          <Tabs.Root value={mode} onValueChange={value=>{setContextSelection(null);onSelectionChange({agentWorkspaceMode:value as WorkspaceMode});}} className="flex min-h-0 flex-1 flex-col">
            <div data-testid="agent-workspace-modebar" className="aw-modebar flex min-h-12 shrink-0 items-end border-b border-border px-4 sm:px-7">
              <Tabs.List aria-label="Agent Workspace modes" className="agent-workspace-tabs flex h-full items-end gap-7">
                <WorkspaceTab value="session" icon={Sparkles} label="Session" count={data.session_activity.items.length}/>
                <WorkspaceTab value="messages" icon={MessageSquare} label="Messages" count={data.context_summary.unread_count}/>
                <WorkspaceTab value="work" icon={BriefcaseBusiness} label="Work" count={data.works.length}/>
              </Tabs.List>
              <div className="ml-auto flex h-full items-center gap-3 text-[10px] text-muted-foreground">{mode==="session"&&data.sessions.length>1&&<SessionSelect data={data} onChange={sessionId=>onSelectionChange({agentSessionId:sessionId||undefined})}/>}<span className="flex items-center gap-1.5"><ShieldCheck className="size-3.5"/>{data.projection_scope==="host_member_public"?"Public coordination only":selected.is_host?"Host-owned Session":data.session_activity.availability==="available"?"Owner-bound Session":"Native Session unavailable"}</span></div>
            </div>
            <Tabs.Content value="session" className="min-h-0 flex-1 outline-none"><SessionCanvas data={data} liveActivity={currentLiveActivity} onSelect={setContextSelection}/></Tabs.Content>
            <Tabs.Content value="messages" className="min-h-0 flex-1 outline-none"><MessagesCanvas data={data} onSelect={setContextSelection}/></Tabs.Content>
            <Tabs.Content value="work" className="min-h-0 flex-1 outline-none"><WorkCanvas data={data} onSelect={(work)=>{setContextSelection({kind:"work",work});onSelectionChange({teamWorkId:work.work_id});}}/></Tabs.Content>
          </Tabs.Root>

          <AgentComposer data={data} actions={currentView.allowed_actions} actionsCurrent={!error&&actionsCurrent&&currentView.freshness==="current"} selectedRunId={selectedRunId} onAction={onAction} onCompleted={()=>setRefresh(value=>value+1)}/>
        </section>

        <aside className="agent-workspace-context hidden min-h-0 min-w-0 overflow-hidden border-l border-border lg:block" aria-label="Agent context">{context}</aside>
      </div>

      {rosterOpen&&<MobileSheet title="Agent roster" onClose={()=>setRosterOpen(false)}><AgentRoster data={data} selectedId={selected.agent_member_ref.id} onBack={closeWorkspace} onSelect={selectAgent}/></MobileSheet>}
      {contextOpen&&<MobileSheet title="Agent context" onClose={()=>setContextOpen(false)}>{context}</MobileSheet>}
      {profileOpen&&<ProfileDialog data={data} closeRef={profileCloseRef} openerRef={profileTriggerRef} onClose={()=>setProfileOpen(false)}/>}
    </main>
  </Tooltip.Provider>;
}

function WorkspaceTab({value,label,count,icon:Icon}:{value:WorkspaceMode;label:string;count:number;icon:typeof Sparkles}){
  return <Tabs.Trigger value={value} className="relative flex h-11 items-center gap-2 text-[12px] font-semibold text-muted-foreground outline-none data-[state=active]:text-foreground"><Icon className="size-3.5"/>{label}{count>0&&<span className="text-[9px] font-medium text-muted-foreground">{count}</span>}<span className="absolute inset-x-0 bottom-0 h-0.5 origin-center scale-x-0 bg-primary transition-transform data-[state=active]:scale-x-100"/></Tabs.Trigger>;
}

function AgentRoster({data,selectedId,onBack,onSelect}:{data:AgentWorkspaceData;selectedId:string;onBack:()=>void;onSelect:(agent:AgentWorkspaceRosterItem)=>void}){
  const [query,setQuery]=useState("");
  const visible=data.roster.filter(agent=>`${agent.display_name} ${agent.role} ${agent.agent_member_ref.id}`.toLowerCase().includes(query.toLowerCase().trim()));
  return <div className="flex min-h-0 flex-1 flex-col bg-transparent">
    <header className="shrink-0 px-4 pb-3 pt-5"><p className="agent-team-eyebrow">Agent Team</p><div className="mt-1 flex items-start gap-1"><div className="min-w-0 flex-1"><h2 className="line-clamp-2 max-h-[2.65rem] text-[18px] font-semibold leading-[1.15] tracking-[-0.025em]">{data.team.display_name||data.team.team_id}</h2><button type="button" onClick={onBack} className="mt-1 text-[10px] text-muted-foreground hover:text-foreground">← Back to Team Workspace</button></div><Tooltip.Root><Tooltip.Trigger asChild><Button size="icon" variant="ghost" onClick={onBack} aria-label="Back to Team Workspace"><X className="size-4"/></Button></Tooltip.Trigger><Tooltip.Portal><Tooltip.Content side="right" className="rounded-md bg-foreground px-2 py-1 text-[10px] text-background">Back to Team Workspace</Tooltip.Content></Tooltip.Portal></Tooltip.Root></div>
      <label className="relative mt-4 block"><Search className="pointer-events-none absolute left-3 top-2.5 size-3.5 text-muted-foreground"/><span className="sr-only">Search Agents</span><input value={query} onChange={event=>setQuery(event.target.value)} placeholder="Search agents" className="h-9 w-full rounded-lg border border-border/80 bg-background/75 pl-9 pr-3 text-xs outline-none focus:border-primary/55"/></label>
    </header>
    <ScrollArea.Root className="min-h-0 flex-1 overflow-hidden"><ScrollArea.Viewport className="size-full"><div className="px-2 pb-5">
      {visible.map((agent,index)=><div key={agent.agent_member_ref.id}>{index===0&&<p className="px-3 pb-1 pt-2 text-[9px] font-semibold uppercase tracking-[.15em] text-muted-foreground">Host Agent</p>}{index===1&&<p className="px-3 pb-1 pt-5 text-[9px] font-semibold uppercase tracking-[.15em] text-muted-foreground">Team Members</p>}<button type="button" onClick={()=>onSelect(agent)} data-selected={agent.agent_member_ref.id===selectedId} className="agent-roster-row group flex w-full items-center gap-3 px-3 py-3 text-left">
        <Avatar name={agent.display_name} identity={`${agent.agent_member_ref.id} ${agent.role}`} size="md" tone={agent.runtime_state==="running"?"running":agent.capacity==="available"?"good":"idle"}/>
        <span className="min-w-0 flex-1"><span className="flex items-start gap-2"><span className="agent-roster-name min-w-0 break-words text-[13px] font-semibold leading-4">{agent.display_name}</span>{agent.is_host&&<span className="shrink-0 text-[9px] font-semibold text-primary">Host</span>}</span><span className="agent-roster-meta mt-0.5 block break-words text-[10px] leading-4 text-muted-foreground">{humanizeToken(agent.role)} · {humanizeToken(agent.runtime_state??agent.capacity??"unknown")}</span></span>
        <span className="aw-roster-tail"><Circle className={`size-2 fill-current ${agent.runtime_state==="running"?"text-status-running":agent.capacity==="available"?"text-status-good":"text-muted-foreground/35"}`}/>{(agent.queued_work_count??0)>0&&<span aria-label={`${agent.queued_work_count} queued Work`}>{agent.queued_work_count}</span>}</span>
      </button></div>)}
    </div></ScrollArea.Viewport><ScrollArea.Scrollbar orientation="vertical" className="flex w-2 p-0.5"><ScrollArea.Thumb className="flex-1 rounded-full bg-border"/></ScrollArea.Scrollbar></ScrollArea.Root>
  </div>;
}

function SessionCanvas({data,liveActivity,onSelect}:{data:AgentWorkspaceData;liveActivity:AgentWorkspaceLiveActivity|null;onSelect:(next:ContextSelection)=>void}){
  const [selectedEventId,setSelectedEventId]=useState<string|null>(null);
  const rows=useMemo(()=>[
    ...data.messages.map(message=>({kind:"message" as const,at:message.created_at,message})),
    ...data.session_activity.items.map(event=>({kind:"event" as const,at:event.occurred_at??"",event})),
  ].sort((left,right)=>timestampKey(left.at)-timestampKey(right.at)),[data.messages,data.session_activity.items]);
  const publicProjection=data.projection_scope==="host_member_public";
  const currentWork=data.works.find(work=>work.work_id===data.context_summary.current_work_id);
  const seenConversations=new Set<string>();
  return <ScrollArea.Root className="h-full overflow-hidden"><ScrollArea.Viewport className="size-full"><div className="agent-session-stream w-full px-5 pb-8 sm:px-7">
    <div className="aw-session-context-strip">
      <span className="aw-session-context-strip__label">{publicProjection?<ShieldCheck aria-hidden="true"/>:<Sparkles aria-hidden="true"/>}{publicProjection?"Public coordination":"Messages + owner-bound Session"}</span>
      <strong>{currentWork?.title??(publicProjection?"Authored Messages and Work facts":"Harness Messages and native Session activity")}</strong>
      <span>{currentWork?`${humanizeToken(currentWork.phase)} Work · `:""}{data.messages.length} messages{publicProjection?"":` · ${data.session_activity.items.length} native facts`}</span>
    </div>
    {liveActivity&&<CurrentExecutionSlot activity={liveActivity}/>}
    {rows.length
      ? <div className="aw-session-chronology" aria-label="Harness Messages and owner-bound native Session facts ordered by recorded time">{rows.map(row=>{
        if(row.kind==="message"){
          const continuation=seenConversations.has(row.message.correlation_id);
          seenConversations.add(row.message.correlation_id);
          return <AuthoredTurn key={`message:${row.message.message_id}`} data={data} message={row.message} selectedAgentId={data.selected_agent.agent_member_ref.id} continuation={continuation} onSelect={()=>onSelect({kind:"message",message:row.message})}/>;
        }
        return row.event.kind==="message"
          ? <NativeAuthoredRecord key={`native:${row.event.event_id}`} event={row.event} selected={selectedEventId===row.event.event_id} onSelect={()=>{setSelectedEventId(row.event.event_id);onSelect({kind:"event",event:row.event});}}/>
          : <div key={`native:${row.event.event_id}`} className="aw-execution-fact"><ExpandableEvent event={row.event} selected={selectedEventId===row.event.event_id} onSelect={()=>{setSelectedEventId(row.event.event_id);onSelect({kind:"event",event:row.event});}}/></div>;
      })}</div>
      : <EmptyCanvas compact title={data.selected_agent.is_host?"No Host-owned Session events or public Messages yet":"No Session activity yet"} detail={data.session_activity.disabled_reason??"Display-safe provider events and public authored Messages will appear here when recorded."}/>
    }
    {data.session_activity.truncated&&<p className="mt-4 border-t border-border pt-3 text-[10px] text-muted-foreground">Showing the latest bounded provider-native events.</p>}
  </div></ScrollArea.Viewport><ScrollArea.Scrollbar orientation="vertical" className="flex w-2 p-0.5"><ScrollArea.Thumb className="rounded-full bg-border"/></ScrollArea.Scrollbar></ScrollArea.Root>;
}

function CurrentExecutionSlot({activity}:{activity:AgentWorkspaceLiveActivity}){
  const presentation=eventPresentation(activity.kind,activity.status);
  const Icon=presentation.icon;
  return <section className="aw-current-execution" data-family={presentation.family} data-status={activity.status??"running"} aria-label="Current provider execution" aria-live="polite">
    <span className="aw-current-execution__pulse" aria-hidden="true"><Icon/></span>
    <span className="aw-current-execution__copy"><span className="aw-current-execution__eyebrow">Live · transient</span><strong>{activity.title??presentation.label}</strong><span>{activity.summary}</span></span>
    <span className="aw-current-execution__source"><b>{humanizeToken(activity.provider)}</b><small>{humanizeToken(activity.status??"running")}</small></span>
  </section>;
}

function NativeAuthoredRecord({event,selected,onSelect}:{event:AgentWorkspaceActivityItem;selected:boolean;onSelect:()=>void}){
  return <article role="button" tabIndex={0} aria-label={`Open native Session message ${event.title}`} data-selected={selected||undefined} className="aw-native-authored-record" onClick={onSelect} onKeyDown={eventKey=>activateOnKeyDown(eventKey,onSelect)} onKeyUp={eventKey=>activateOnKeyUp(eventKey,onSelect)}>
    <span className="aw-native-authored-record__mark"><MessageSquare aria-hidden="true"/></span>
    <div className="min-w-0 flex-1"><header><strong>{event.title}</strong><span>Native Session message</span><time>{formatTime(event.occurred_at)}</time></header><p>{event.summary??"Display-safe content recorded by the provider-native Session."}</p><footer>Provider-native record · {humanizeToken(event.status)}</footer></div>
  </article>;
}

function SessionSelect({data,onChange}:{data:AgentWorkspaceData;onChange:(sessionId:string)=>void}){
  return <label className="flex shrink-0 items-center gap-2 text-[10px] text-muted-foreground"><Clock3 className="size-3.5"/><span className="sr-only">Selected Session</span><select aria-label="Selected Session" value={data.selected_session_id??""} onChange={event=>onChange(event.target.value)} className="h-8 max-w-52 rounded-md border border-border bg-background px-2 text-[10px] text-foreground">{data.sessions.length?data.sessions.map(session=><option key={session.session_id??session.member_run_id??session.team_run_id} value={session.session_id??session.member_run_id??""}>{session.runtime_status} · {shortId(session.session_id??session.member_run_id)}</option>):<option value="">No Session bound</option>}</select></label>;
}

function AuthoredTurn({data,message,selectedAgentId,continuation,onSelect}:{data:AgentWorkspaceData;message:MessageSummary;selectedAgentId:string;continuation:boolean;onSelect:()=>void}){
  const fromSelected=message.sender.id===selectedAgentId;
  const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);
  const name=actor?.display_name??(fromSelected?data.selected_agent.display_name:message.sender.id);
  return <article role="button" tabIndex={0} aria-label={`Open authored Message from ${name}`} data-from-selected={fromSelected||undefined} data-thread-continuation={continuation||undefined} className="agent-authored-turn" onClick={onSelect} onKeyDown={event=>activateOnKeyDown(event,onSelect)} onKeyUp={event=>activateOnKeyUp(event,onSelect)}>
    <Avatar name={name} identity={`${message.sender.id} ${actor?.role??""}`} size="md" tone={actor?.runtime_state==="running"?"running":"idle"}/><div className="min-w-0 flex-1"><header className="mb-1 flex items-baseline gap-2"><p className="truncate text-[12px] font-semibold">{name}</p><span className="aw-record-kind">Message</span><time className="ml-auto text-[10px] text-muted-foreground">{formatTime(message.created_at)}</time></header>
    <div className="aw-authored-body"><Markdown source={message.body}/></div>
    <div className="aw-record-meta">{message.work_id&&<span>Work · {shortId(message.work_id)}</span>}<span title={`Correlation ${message.correlation_id}`}>{message.causation_id?"Reply in conversation":"Authored conversation"}</span>{message.deliveries.length>0&&<span>Delivery · {message.deliveries.map(delivery=>humanizeToken(delivery.status)).join(" · ")}</span>}</div></div>
  </article>;
}

function ExpandableEvent({event,selected,onSelect}:{event:AgentWorkspaceActivityItem;selected:boolean;onSelect:()=>void}){
  return <div data-boundary-aligned={selected||undefined}><OperationalFactRow kind={event.kind} status={event.status} title={event.title} summary={event.summary} timestamp={formatTime(event.occurred_at)} expanded={selected} selected={selected} onToggle={onSelect}/></div>;
}

function MessagesCanvas({data,onSelect}:{data:AgentWorkspaceData;onSelect:(next:ContextSelection)=>void}){
  const [lens,setLens]=useState<"all"|"inbox"|"outbox"|"unread">("all");
  const [query,setQuery]=useState("");
  const selectedId=data.selected_agent.agent_member_ref.id;
  const visible=data.messages.filter(message=>{
    const incoming=message.sender.id!==selectedId;
    const unread=message.deliveries.some(delivery=>["queued","delivered"].includes(delivery.status));
    return (lens==="all"||(lens==="inbox"&&incoming)||(lens==="outbox"&&!incoming)||(lens==="unread"&&unread))&&`${message.body} ${message.sender.id} ${message.work_id??""}`.toLowerCase().includes(query.toLowerCase());
  });
  return <ScrollArea.Root className="h-full overflow-hidden"><ScrollArea.Viewport className="size-full"><div className="agent-messages-canvas mx-auto max-w-[58rem] px-5 pb-10 sm:px-7" data-empty={!visible.length||undefined}>
    <WorkspaceCanvasIntro compact eyebrow="Harness messages" title={`${data.selected_agent.display_name} · Inbox and Outbox`} detail="Authored coordination stays linked to its sender, recipient, delivery and Work." facts={[`${data.messages.length} total`,`${data.context_summary.unread_count} unread`]}/>
    <div className="agent-message-toolbar aw-filter-strip flex flex-wrap items-center gap-2 border-y border-border"><div className="flex">{(["all","inbox","outbox","unread"] as const).map(item=><button key={item} type="button" data-active={lens===item} onClick={()=>setLens(item)} className="px-3 text-[10px] font-semibold capitalize text-muted-foreground">{item}</button>)}</div><label className="relative ml-auto min-w-[14rem] flex-1 sm:max-w-[20rem]"><Search className="pointer-events-none absolute left-3 top-2.5 size-3.5 text-muted-foreground"/><span className="sr-only">Search messages</span><input value={query} onChange={event=>setQuery(event.target.value)} placeholder="Search messages" className="h-9 w-full border-0 bg-transparent pl-9 pr-3 text-xs outline-none"/></label></div>
    {visible.length
      ? <MessageThreads data={data} messages={visible} selectedId={selectedId} onSelect={onSelect}/>
      : <EmptyCanvas title={query||lens!=="all"?"No messages match this view":"No authored messages yet"} detail={query||lens!=="all"?"Clear the current filter to return to the full authored record.":"Use the command surface below to start a durable, Work-linked conversation."}/>
    }
  </div></ScrollArea.Viewport></ScrollArea.Root>;
}

function MessageThreads({data,messages,selectedId,onSelect}:{data:AgentWorkspaceData;messages:MessageSummary[];selectedId:string;onSelect:(next:ContextSelection)=>void}){
  const seen=new Set<string>();
  return <div className="agent-message-stream">{messages.map(message=>{
    const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);
    const actorName=actor?.display_name??message.sender.id;
    const recipients=message.recipients.map(recipient=>data.roster.find(item=>item.agent_member_ref.id===recipient.id)?.display_name??recipient.id).join(", ");
    const continuation=seen.has(message.correlation_id);seen.add(message.correlation_id);
    return <button key={message.message_id} type="button" data-thread-continuation={continuation||undefined} className="agent-message-row flex w-full gap-3 text-left" onClick={()=>onSelect({kind:"message",message})}><Avatar name={actorName} identity={`${message.sender.id} ${actor?.role??""}`} size="md" tone={actor?.runtime_state==="running"?"running":"idle"}/><span className="min-w-0 flex-1"><span className="flex items-baseline gap-2"><b className="truncate text-[12.5px]">{actorName}</b><span className="aw-record-kind">{message.sender.id===selectedId?"Outbox":"Inbox"} → {recipients}</span><time className="ml-auto text-[10.5px] text-muted-foreground">{formatTime(message.created_at)}</time></span><span className="mt-1.5 block max-w-[42rem] whitespace-pre-wrap text-[13.5px] leading-[1.58] text-foreground/90">{message.body}</span><span className="aw-record-meta">{message.work_id&&<span>Work · {shortId(message.work_id)}</span>}<span title={`Correlation ${message.correlation_id}`}>{message.causation_id?"Reply":"Conversation"}</span><span>{message.deliveries.map(delivery=>humanizeToken(delivery.status)).join(", ")||"Recorded"}</span></span></span></button>;
  })}</div>;
}

function WorkCanvas({data,onSelect}:{data:AgentWorkspaceData;onSelect:(work:WorkSummary)=>void}){
  const [lens,setLens]=useState<"current"|"open"|"active"|"review"|"closed"|"eligible">("current");
  const memberId=data.selected_agent.agent_member_ref.id;
  const owns=(work:WorkSummary)=>work.owner_actor_ref?.id===memberId;
  const visible=data.works.filter(work=>lens==="current"?owns(work)&&work.phase!=="closed":lens==="eligible"?!owns(work)&&work.eligible_member_ids.includes(memberId):work.phase===lens);
  const ordered=[...visible].sort((left,right)=>workVisualRank(left,data.context_summary.current_work_id)-workVisualRank(right,data.context_summary.current_work_id)||timestampKey(right.updated_at)-timestampKey(left.updated_at));
  const ownedCount=data.works.filter(owns).length;
  const reviewCount=data.works.filter(work=>work.phase==="review").length;
  return <ScrollArea.Root className="h-full overflow-hidden"><ScrollArea.Viewport className="size-full"><div className="agent-work-canvas mx-auto max-w-[60rem] px-5 pb-10 sm:px-7">
    <WorkspaceCanvasIntro compact eyebrow="Responsibility" title={`${data.selected_agent.display_name} · Work`} detail="Ownership, execution phase, condition and gate progress stay distinct." facts={[`${ownedCount} owned`,`${reviewCount} in review`,`${data.works.length-ownedCount} eligible or shared`]}/>
    <div className="aw-filter-strip flex flex-wrap items-center gap-5 border-y border-border">{(["current","open","active","review","closed","eligible"] as const).map(item=><button key={item} type="button" data-active={lens===item} onClick={()=>setLens(item)} className="agent-work-lens relative text-[10px] font-semibold capitalize text-muted-foreground data-[active=true]:text-foreground">{item}</button>)}<span className="ml-auto text-[10px] text-muted-foreground">{visible.length} {visible.length===1?"record":"records"}</span></div>
    {ordered.length?<div className="agent-work-stream">{ordered.map((work,index)=>{const current=work.work_id===data.context_summary.current_work_id;const group=workGroupLabel(work,current,lens);const prior=index>0?workGroupLabel(ordered[index-1],ordered[index-1].work_id===data.context_summary.current_work_id,lens):null;return <div key={work.work_id}>{group!==prior&&<p className="aw-work-group-label">{group}</p>}<button type="button" data-current={current||undefined} data-phase={work.phase} data-condition={work.condition} className="agent-work-row grid w-full grid-cols-[minmax(0,1fr)_6.5rem_7.5rem] gap-5 text-left" onClick={()=>onSelect(work)}><span className="min-w-0">{current&&<span className="aw-work-kicker">Current Work</span>}<span className={current?"mt-1 flex items-center gap-2":"flex items-center gap-2"}><span className="break-words text-[13.5px] font-semibold leading-[1.35]">{work.title||work.work_id}</span>{work.condition!=="normal"&&<WorkspaceState label={humanizeToken(String(work.condition))} tone="bad"/>}</span>{work.completion_criteria_markdown&&<span className="mt-1 block max-w-[42rem] line-clamp-1 text-[12.5px] leading-[1.5] text-foreground/75">{work.completion_criteria_markdown}</span>}<span className="aw-record-meta"><span>{work.owner_actor_ref?.id===memberId?"Owned responsibility":"Eligible responsibility"}</span><span>{shortId(work.work_id)} · revision {work.work_revision}</span><span>{humanizeToken(String(work.priority))} priority</span></span></span><span className="aw-work-state"><WorkspaceState label={humanizeToken(work.phase)} tone={work.phase==="active"?"running":work.phase==="review"?"warn":work.phase==="closed"?"good":"muted"}/>{meaningfulRecovery(work)&&<span>{humanizeToken(String(work.delivery_summary.recovery_class))}</span>}</span><span className="aw-work-gates text-right"><strong>{work.gate_summary.passed}/{work.gate_summary.required}</strong><span>gates passed</span><time>{formatTime(work.updated_at)}</time></span></button></div>})}</div>:<EmptyCanvas title="No Work in this view" detail="Eligibility is not ownership. Work remains authoritative in the Team Work kernel."/>}
  </div></ScrollArea.Viewport></ScrollArea.Root>;
}

function AgentContextRail({view,data,selected,currentWork,actions}:{view:RoleView<AgentWorkspaceData>;data:AgentWorkspaceData;selected:ContextSelection;currentWork?:WorkSummary;actions:AllowedAction[]}){
  const selfPrivate=data.projection_scope!=="host_member_public";
  const isHost=data.selected_agent.is_host;
  const publicProjection=data.projection_scope==="host_member_public";
  const ownedWorks=data.works.filter(work=>work.owner_actor_ref?.id===data.selected_agent.agent_member_ref.id);
  const eligibleWorks=data.works.filter(work=>work.owner_actor_ref?.id!==data.selected_agent.agent_member_ref.id&&work.eligible_member_ids.includes(data.selected_agent.agent_member_ref.id));
  const activeWorks=data.works.filter(work=>work.phase==="active"||work.phase==="review");
  const attentionWorks=activeWorks.filter(work=>work.phase==="review"||work.condition==="blocked");
  const evidenceWorks=data.works.filter(work=>work.latest_report_ref||work.latest_finding_refs.length||work.latest_failure_ref||work.artifact_refs.length||work.check_refs.length);
  const incoming=data.messages.filter(message=>message.sender.id!==data.selected_agent.agent_member_ref.id);
  const unreadIncoming=incoming.filter(message=>message.deliveries.some(delivery=>["queued","delivered"].includes(delivery.status)));
  const hostInbox=(unreadIncoming.length?unreadIncoming:incoming).slice(-2).reverse();
  const latestExchange=[...data.messages].sort((left,right)=>timestampKey(right.created_at)-timestampKey(left.created_at))[0];
  const actionIndex=[...actions.reduce((index,action)=>{
    const existing=index.get(action.kind);
    if(!existing||existing.disabled_reason&& !action.disabled_reason)index.set(action.kind,action);
    return index;
  },new Map<string,AllowedAction>()).values()];
  const anchoredWork=selected?.kind==="work"?selected.work:currentWork;
  const anchoredNeedsJudgment=Boolean(anchoredWork&&(anchoredWork.phase==="review"||anchoredWork.condition==="blocked"));
  const responsibility={open:ownedWorks.filter(work=>work.phase==="open").length,active:ownedWorks.filter(work=>work.phase==="active").length,review:ownedWorks.filter(work=>work.phase==="review").length};
  const otherOwnedWorks=ownedWorks.filter(work=>work.work_id!==anchoredWork?.work_id&&!attentionWorks.some(attention=>attention.work_id===work.work_id));
  const prioritizedActions=[...actionIndex].sort((left,right)=>decisionActionRank(left.kind,anchoredWork)-decisionActionRank(right.kind,anchoredWork));
  return <ScrollArea.Root className="h-full min-w-0 overflow-hidden"><ScrollArea.Viewport className="size-full min-w-0 [&>div]:!block [&>div]:!min-w-0"><div className="aw-context-story min-w-0 overflow-hidden px-5 pb-8 pt-5">
    <p className="aw-context-story__eyebrow">{isHost?"Host operations":publicProjection?"Public Agent context":"Agent operations"}</p>
    <WorkContext work={anchoredWork} title={isHost&&anchoredNeedsJudgment?"Current decision":"Current Work"}/>
    {(selected?.kind==="message"||selected?.kind==="event")&&<div className="aw-context-selection-inset" aria-label="Selected context">{selected.kind==="message"?<MessageContext data={data} message={selected.message}/>:<EventContext event={selected.event}/>}</div>}
    {!isHost&&<ContextSection title="Responsibility" hint={eligibleWorks.length?`${eligibleWorks.length} eligible Work`:undefined}><ResponsibilityStrip values={responsibility}/>{latestExchange&&<div className="mt-3"><ContextMessageRow data={data} message={latestExchange}/></div>}</ContextSection>}
    {isHost&&attentionWorks.length>0&&<ContextSection title="Needs Host" hint={`${attentionWorks.length} ${attentionWorks.length===1?"responsibility":"responsibilities"}`}>{attentionWorks.filter(work=>work.work_id!==anchoredWork?.work_id).slice(0,2).map(work=><ContextWorkRow key={work.work_id} data={data} work={work}/>)}</ContextSection>}
    {isHost&&hostInbox.length>0&&<ContextSection title="Team Inbox" hint={unreadIncoming.length?`${unreadIncoming.length} unsettled`:`${hostInbox.length} recent`}>{hostInbox.map(message=><ContextMessageRow key={message.message_id} data={data} message={message}/>)}</ContextSection>}
    {isHost&&otherOwnedWorks.length>0&&<ContextSection title="Assigned Work" hint={`${ownedWorks.length} total`}>{otherOwnedWorks.slice(0,2).map(work=><ContextWorkRow key={work.work_id} data={data} work={work}/>)}</ContextSection>}
    {evidenceWorks.some(work=>work.work_id===anchoredWork?.work_id)&&<ContextSection title="Evidence"><ContextWorkRow data={data} work={evidenceWorks.find(work=>work.work_id===anchoredWork?.work_id)!} evidence/></ContextSection>}
    {selfPrivate?<ContextSection title="Current Session" hint={humanizeToken(data.selected_agent.runtime_status??"unknown")}><ContextFact label="Provider" value={data.selected_agent.provider?humanizeToken(data.selected_agent.provider):"Not bound"}/><ContextFact label="Session" value={shortId(data.selected_session_id)}/><ContextFact label="Last activity" value={formatTime(data.context_summary.last_activity_at)}/></ContextSection>:<ContextSection title="Privacy"><div className="aw-privacy-notice"><ShieldCheck aria-hidden="true"/><p>This view includes public Messages and Work only. The selected Agent's private Session, tools and runtime are structurally absent.</p></div></ContextSection>}
    {prioritizedActions.some(action=>!action.disabled_reason)&&<ContextSection title={isHost&&anchoredNeedsJudgment?"Decision actions":"Next"}><WorkspaceActionIndex label={isHost&&anchoredNeedsJudgment?"Resolve in composer":isHost?"Available Host Controls":"Available Controls"} actions={prioritizedActions.filter(action=>!action.disabled_reason).slice(0,6).map(action=>({key:action.kind,label:actionLabel(action.kind)}))}/></ContextSection>}
    <details className="mt-5 border-t border-border pt-4"><summary className="cursor-pointer text-[10px] font-semibold text-muted-foreground">Projection · {view.freshness} · seq {view.as_of_event_sequence}</summary><div className="mt-3"><ViewProvenance view={view}/></div></details>
  </div></ScrollArea.Viewport><ScrollArea.Scrollbar orientation="vertical" className="flex w-2 p-0.5"><ScrollArea.Thumb className="rounded-full bg-border"/></ScrollArea.Scrollbar></ScrollArea.Root>;
}

function WorkContext({work,title}:{work?:WorkSummary;title:string}){return <ContextSection title={title} primary>{work?<><h3 className="aw-context-work-title text-[14px] font-semibold leading-5">{work.title||work.work_id}</h3><p className="aw-context-work-narrative">{workNarrative(work)}</p><div className="mt-2 flex items-center gap-2"><WorkspaceState label={humanizeToken(work.phase)} tone={work.phase==="active"?"running":work.phase==="review"?"warn":work.phase==="closed"?"good":"muted"}/><span className="text-[10px] text-muted-foreground">{shortId(work.work_id)} · v{work.work_revision}</span></div><div className="mt-3"><ContextFact label="Condition" value={humanizeToken(String(work.condition))}/><ContextFact label="Priority" value={humanizeToken(String(work.priority))}/>{meaningfulRecovery(work)&&<ContextFact label="Delivery recovery" value={humanizeToken(String(work.delivery_summary.recovery_class))}/>}<ContextFact label="Gates" value={`${work.gate_summary.passed}/${work.gate_summary.required}`}/></div></>:<p className="text-[11px] leading-5 text-muted-foreground">No current Work is projected for this Agent.</p>}</ContextSection>}
function MessageContext({data,message}:{data:AgentWorkspaceData;message:MessageSummary}){const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);return <ContextSection title="Message in focus" hint={formatTime(message.created_at)}><p className="aw-context-focus-title">{actor?.display_name??message.sender.id}</p><p className="aw-context-focus-copy">{message.body}</p><div className="mt-3"><ContextFact label="Delivery" value={message.deliveries.map(item=>humanizeToken(item.status)).join(", ")||"Recorded"}/>{message.work_id&&<ContextFact label="Linked Work" value={shortId(message.work_id)}/>}</div></ContextSection>}
function EventContext({event}:{event:AgentWorkspaceActivityItem}){return <ContextSection title="Native fact in focus" hint={formatTime(event.occurred_at)}><p className="aw-context-focus-title">{event.title}</p><p className="aw-context-focus-copy">{event.summary??"Display-safe provider-native fact."}</p><div className="mt-3"><ContextFact label="Source" value="Provider native"/><ContextFact label="Status" value={humanizeToken(event.status)}/></div></ContextSection>}

function AgentComposer({data,actions,actionsCurrent,selectedRunId,onAction,onCompleted}:{data:AgentWorkspaceData;actions:AllowedAction[];actionsCurrent:boolean;selectedRunId:string|null;onAction:RoleActionExecutor;onCompleted:()=>void}){
  const sendAction=actions.find(action=>action.kind==="send_message");
  const hostAgent=data.roster.find(item=>item.is_host);
  const usable=actions.filter(action=>action.kind!=="reply_message");
  const [selectedKey,setSelectedKey]=useState(sendAction?keyForAction(sendAction):usable[0]?keyForAction(usable[0]):"");
  const selected=usable.find(action=>keyForAction(action)===selectedKey)??sendAction;
  useEffect(()=>{if(selectedKey&&!usable.some(action=>keyForAction(action)===selectedKey))setSelectedKey(sendAction?keyForAction(sendAction):usable[0]?keyForAction(usable[0]):"");},[selectedKey,sendAction,usable]);
  if(!actionsCurrent)return <div className="agent-workspace-composer shrink-0 border-t border-border bg-background/95 px-4 py-3 text-xs text-muted-foreground" role="status">Authoritative Agent Workspace refresh is pending or failed. Composer and action writes are unavailable.</div>;
  const actionControl=<label className="flex min-w-0 items-center gap-2 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground"><SlidersHorizontal className="size-3 shrink-0 text-primary"/><span className="sr-only">Action</span><select aria-label="Composer action" value={selected?keyForAction(selected):""} onChange={event=>setSelectedKey(event.target.value)} title={selected?.disabled_reason??actionLabel(selected?.kind??"")} className="h-8 min-w-0 w-full rounded-md border border-border bg-background px-2 text-[10px] font-medium normal-case tracking-normal text-foreground"><option value="" disabled>No action authorized</option>{usable.map(action=><option key={keyForAction(action)} value={keyForAction(action)} disabled={Boolean(action.disabled_reason)}>{actionLabel(action.kind)}</option>)}</select></label>;
  const fixedRecipient=data.team.viewer_role==="host"?(data.selected_agent.is_host?undefined:{id:data.selected_agent.agent_member_ref.id,label:data.selected_agent.display_name}):{id:data.team.host_agent_id,label:hostAgent?.display_name??"Host Agent"};
  const recipients=data.roster.filter(item=>!item.is_host).map(item=>({id:item.agent_member_ref.id,label:item.display_name}));
  return <div data-testid="agent-workspace-composer" data-composer-kind={selected?.kind==="send_message"?"message":"action"} className="agent-workspace-composer shrink-0 border-t border-border bg-background/95">
      {selected?.kind==="send_message"?<AgentMessageCommandComposer action={selected} actionControl={actionControl} recipient={fixedRecipient} recipients={recipients} works={data.works} teamId={data.team.team_id} teamRunId={data.team.latest_run_id??undefined} actionsCurrent={actionsCurrent} onAction={onAction} onCompleted={onCompleted}/>:selected?<div className="mx-auto max-w-4xl px-4 py-3"><div className="mb-2">{actionControl}</div><RoleActionPanel compact actions={[selected]} onAction={onAction} context={{teamId:data.team.team_id,teamRunId:data.team.latest_run_id??undefined}} actionsCurrent={actionsCurrent} onCompleted={onCompleted}/>{selected.target_ref.kind==="member_run"&&selectedRunId&&selected.target_ref.id!==selectedRunId&&<p className="mt-2 text-[10px] text-status-warn">This action targets a different MemberRun and is not executed from this selected Agent context.</p>}</div>:<div className="mx-auto max-w-4xl px-4 py-4">{actionControl}<p className="mt-2 text-xs text-muted-foreground">No canonical action is authorized for this identity and state.</p></div>}
  </div>;
}

function ProfileDialog({data,onClose,closeRef,openerRef}:{data:AgentWorkspaceData;onClose:()=>void;closeRef:React.RefObject<HTMLButtonElement>;openerRef:React.RefObject<HTMLButtonElement>}){
  const selected=data.selected_agent,c=data.configuration;
  const hasProviderConfiguration=Boolean(selected.provider||selected.execution_mode||c.provider_profile_ref||c.permission_ceiling||c.workspace_policy);
  const hasRestrictions=Boolean(c.prompt_ref||c.forbidden_actions.length);
  const currentSession=data.sessions.find(session=>session.session_id===data.selected_session_id)??data.sessions[0];
  const priorSessions=data.sessions.filter(session=>session!==currentSession);
  const dialogRef=useRef<HTMLElement>(null);
  const onCloseRef=useRef(onClose);
  onCloseRef.current=onClose;
  useEffect(()=>{
    const dialog=dialogRef.current;
    const focusFrame=window.requestAnimationFrame(()=>closeRef.current?.focus());
    const onKeyDown=(event:KeyboardEvent)=>{
      if(event.key==="Escape"){event.preventDefault();onCloseRef.current();return;}
      if(event.key!=="Tab")return;
      const focusable=[...(dialog?.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])')??[])];
      if(!focusable.length){event.preventDefault();dialog?.focus();return;}
      const first=focusable[0],last=focusable[focusable.length-1];
      if(event.shiftKey&&(document.activeElement===first||!dialog?.contains(document.activeElement))){event.preventDefault();last.focus();}
      else if(!event.shiftKey&&(document.activeElement===last||!dialog?.contains(document.activeElement))){event.preventDefault();first.focus();}
    };
    document.addEventListener("keydown",onKeyDown);
    return()=>{
      window.cancelAnimationFrame(focusFrame);
      document.removeEventListener("keydown",onKeyDown);
      openerRef.current?.focus();
    };
  },[closeRef,openerRef]);
  return <div className="fixed inset-0 z-50 bg-[#3b2f27]/12" role="presentation" onMouseDown={event=>{if(event.target===event.currentTarget)onClose();}}><section ref={dialogRef} role="dialog" aria-modal="true" aria-label={`${selected.display_name} configuration`} tabIndex={-1} className="agent-profile-drawer absolute inset-y-0 right-0 w-[min(92vw,29rem)] overflow-y-auto border-l border-border bg-background shadow-[0_0_28px_rgb(55_43_35_/_0.04)]">
    <header className="sticky top-0 z-10 flex min-h-14 items-center gap-3 border-b border-border bg-background/95 px-5 py-2"><Avatar name={selected.display_name} identity={`${selected.agent_member_ref.id} ${selected.role}`} size="lg" tone="running"/><div className="min-w-0 flex-1"><h2 className="truncate text-xl font-semibold tracking-[-0.02em]">{selected.display_name}</h2><p className="mt-0.5 text-[10px] text-muted-foreground">{humanizeToken(selected.role)} · durable AgentMember</p></div><Button ref={closeRef} size="icon" variant="ghost" onClick={onClose} aria-label="Close Agent configuration"><X className="size-4"/></Button></header>
    <div className="space-y-7 px-6 py-6"><ProfileSection title="Who"><ContextFact label="Role" value={humanizeToken(selected.role)} canonical={selected.role}/><ContextFact label="Organization" value={humanizeToken(selected.organization_status)} canonical={selected.organization_status}/>{c.description&&<p className="aw-profile-description">{c.description}</p>}<p className="aw-profile-canonical" title={selected.agent_member_ref.id}>Durable AgentMember · {shortId(selected.agent_member_ref.id)}</p></ProfileSection>
      <ProfileSection title="Authority">{hasProviderConfiguration?<><ContextFact label="Permission ceiling" value={c.permission_ceiling?humanizeToken(c.permission_ceiling):"Not projected"} canonical={c.permission_ceiling??undefined}/><ContextFact label="Workspace policy" value={c.workspace_policy?humanizeToken(c.workspace_policy):"Not projected"} canonical={c.workspace_policy??undefined}/>{c.prompt_ref&&<ContextFact label="Prompt reference" value={c.prompt_ref}/>} {c.forbidden_actions.length>0&&<ProfileList label="Forbidden actions" values={c.forbidden_actions} humanize empty=""/>}</>:<p className="aw-profile-empty">No execution authority is projected for this Agent.</p>}</ProfileSection>
      {(c.skill_refs.length>0||c.tool_refs.length>0||c.capabilities.length>0)&&<ProfileSection title="Capabilities">{c.skill_refs.length>0&&<ProfileList label="Skills" values={c.skill_refs} empty=""/>}{c.capabilities.length>0&&<ProfileList label="Capabilities" values={c.capabilities} humanize empty=""/>}{c.tool_refs.length>0&&<ProfileList label="Configured tools" values={c.tool_refs} humanize empty=""/>}</ProfileSection>}
      <ProfileSection title="Runtime">{hasProviderConfiguration?<><ContextFact label="Provider" value={selected.provider?humanizeToken(selected.provider):"Not bound"} canonical={selected.provider??undefined}/><ContextFact label="Execution mode" value={selected.execution_mode?humanizeToken(selected.execution_mode):"Not bound"} canonical={selected.execution_mode??undefined}/>{c.provider_profile_ref&&<ContextFact label="Provider profile" value={humanizeToken(c.provider_profile_ref)} canonical={c.provider_profile_ref}/>}</>:<p className="aw-profile-empty">No provider runtime is projected for this Agent.</p>}{currentSession&&<div className="aw-profile-current-session"><div><span>Current Session</span><Badge tone={currentSession.runtime_status==="running"?"good":"muted"}>{humanizeToken(currentSession.runtime_status)}</Badge></div><strong>{currentSession.provider?humanizeToken(currentSession.provider):"Provider not bound"}</strong><p>{formatTime(currentSession.last_active_at)} · <span title={currentSession.session_id??currentSession.member_run_id??undefined}>{shortId(currentSession.session_id??currentSession.member_run_id)}</span></p></div>}{c.workspace_binding&&<div className="mt-3"><ContextFact label="Workspace" value={c.workspace_binding.status?humanizeToken(c.workspace_binding.status):"Bound"} canonical={c.workspace_binding.id}/>{c.workspace_binding.locator&&<ContextFact label="Path" value={c.workspace_binding.locator}/>}</div>}</ProfileSection>
      <ProfileSection title="History">{priorSessions.length?priorSessions.map((session,index)=><div key={session.session_id??session.member_run_id??session.team_run_id} className="aw-session-history-row"><div className="flex items-center justify-between gap-3"><b>Previous Session {index+1}</b><Badge tone={session.runtime_status==="running"?"good":"muted"}>{humanizeToken(session.runtime_status)}</Badge></div><p>{session.provider?humanizeToken(session.provider):"Provider not bound"} · {formatTime(session.last_active_at)}</p><small title={session.session_id??session.member_run_id??undefined}>{shortId(session.session_id??session.member_run_id)}</small></div>):<p className="aw-profile-empty">No previous native Session is projected.</p>}</ProfileSection>
    </div>
  </section></div>;
}

function MobileSheet({title,onClose,children}:{title:string;onClose:()=>void;children:React.ReactNode}){return <div className="aw-sheet-backdrop fixed inset-0 z-40 lg:hidden" onMouseDown={event=>{if(event.target===event.currentTarget)onClose();}}><section role="dialog" aria-modal="true" aria-label={title} className="aw-mobile-sheet absolute inset-y-0 right-0 w-[min(92vw,25rem)] overflow-y-auto border-l"><header className="sticky top-0 z-10 flex min-h-12 items-center justify-between border-b px-4"><h2 className="text-sm font-semibold">{title}</h2><Button size="icon" variant="secondary" onClick={onClose} aria-label={`Close ${title}`}><X className="size-4"/></Button></header>{children}</section></div>}
function RuntimePill({state}:{state:string|null}){return <span className="flex items-center gap-1.5 rounded-full border border-border px-2.5 py-1 text-[10.5px] font-semibold text-muted-foreground"><Circle className={`size-2 fill-current ${state==="running"?"text-status-running":state==="idle"?"text-status-good":"text-muted-foreground/40"}`}/>{state??"unknown"}</span>}
function ContextSection({title,hint,primary=false,children}:{title:string;hint?:string;primary?:boolean;children:React.ReactNode}){return <WorkspaceSection title={title} hint={hint} primary={primary}>{children}</WorkspaceSection>}
function ProfileSection({title,children}:{title:string;children:React.ReactNode}){const Icon=title==="Who"?UserRound:title==="Authority"?KeyRound:title==="Capabilities"?Wrench:title==="Runtime"?Activity:History;return <section className="aw-profile-section"><h3><Icon aria-hidden="true"/>{title}</h3><div>{children}</div></section>}
function ContextFact({label,value,canonical}:{label:string;value:string;canonical?:string}){return <WorkspaceFact label={label} value={value} canonicalValue={canonical}/>}
function MiniMetric({label,value}:{label:string;value:number}){return <div><p className="text-[10.5px] text-muted-foreground">{label}</p><p className="mt-1 text-[15px] font-semibold">{value}</p></div>}
function ResponsibilityStrip({values}:{values:{open:number;active:number;review:number}}){return <div className="aw-responsibility-strip" aria-label={`${values.open} open, ${values.active} active, ${values.review} in review`}>{Object.entries(values).map(([label,value])=><div key={label}><strong>{value}</strong><span>{label}</span></div>)}</div>}
function ContextWorkRow({data,work,evidence=false}:{data:AgentWorkspaceData;work:WorkSummary;evidence?:boolean}){const owner=data.roster.find(agent=>agent.agent_member_ref.id===work.owner_actor_ref?.id);return <div className="border-t border-border/70 py-2.5 first:border-t-0"><div className="flex items-start justify-between gap-3"><p className="aw-context-work-row-title min-w-0 flex-1 break-words text-[11.5px] font-semibold leading-[1.4]">{work.title||work.work_id}</p><span className="shrink-0 text-[10.5px] text-muted-foreground">{humanizeToken(evidence?(work.latest_report_ref?"report":work.latest_finding_refs.length?"finding":work.latest_failure_ref?"failure":"evidence"):work.phase)}</span></div><p className="aw-context-work-row-meta mt-1 break-words text-[10.5px] leading-4 text-muted-foreground">{owner?.display_name??(work.owner_actor_ref?"Assigned":"Unassigned")} · gates {work.gate_summary.passed}/{work.gate_summary.required}</p></div>}
function ContextMessageRow({data,message}:{data:AgentWorkspaceData;message:MessageSummary}){const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);return <div className="border-t border-border/70 py-2.5 first:border-t-0"><div className="flex items-baseline justify-between gap-3"><b className="truncate text-[11.5px]">{actor?.display_name??message.sender.id}</b><time className="shrink-0 text-[10.5px] text-muted-foreground">{formatTime(message.created_at)}</time></div><p className="mt-1 line-clamp-1 text-[10.5px] text-muted-foreground">{message.body}</p></div>}
function TagList({values,empty,humanize=false}:{values:string[];empty:string;humanize?:boolean}){return values.length?<div className="aw-profile-token-list">{values.map(value=><span key={value} title={value} className="aw-profile-token">{humanize?humanizeToken(value):value}</span>)}</div>:<p className="aw-profile-empty">{empty}</p>}
function ProfileList({label,values,empty,humanize=false}:{label:string;values:string[];empty:string;humanize?:boolean}){return <div className="mt-3 first:mt-0"><p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</p><TagList values={values} empty={empty} humanize={humanize}/></div>}
function EmptyCanvas({title,detail,compact=false}:{title:string;detail:string;compact?:boolean}){return <div className="aw-empty-state" data-compact={compact||undefined}><Inbox aria-hidden="true"/><div><h3>{title}</h3><p>{detail}</p></div></div>}
function actionLabel(kind:string){return ({send_message:"Send message",assign_work:"Assign work",rebind_work:"Reassign work",close_member_run:"Close member run",reopen_member_run:"Reopen member run",retire_member_run:"Retire agent from team",resume_native_session:"Resume native session",reconcile_delivery:"Reconcile work delivery",reconcile_message_delivery:"Reconcile message delivery",request_gate_evaluation:"Request gate review",request_changes:"Request work changes",accept_work:"Accept work",cancel_work:"Cancel work"} as Record<string,string>)[kind]??humanizeToken(kind)}
function decisionActionRank(kind:string,work?:WorkSummary){if(work?.phase==="review"){if(kind==="accept_work")return 0;if(kind==="request_changes")return 1;if(kind==="request_gate_evaluation")return 2;}if(work?.condition==="blocked"&&/reconcile|rebind|resume/.test(kind))return 0;if(kind==="send_message")return 3;if(/assign|rebind/.test(kind))return 4;if(/close|retire|cancel/.test(kind))return 9;return 5}
function keyForAction(action:AllowedAction){return `${action.kind}:${action.target_ref.kind}:${action.target_ref.id}`}
function meaningfulRecovery(work:WorkSummary){const value=work.delivery_summary.recovery_class;return typeof value==="string"&&!['','none','null','not_modeled','not_projected'].includes(value)}
function workVisualRank(work:WorkSummary,currentWorkId:string|null){if(work.work_id===currentWorkId)return 0;if(work.condition==="blocked")return 1;if(work.phase==="review")return 2;if(work.phase==="active")return 3;if(work.phase==="open")return 4;return 5}
function workGroupLabel(work:WorkSummary,current:boolean,lens:string){if(lens!=="current")return humanizeToken(lens);if(current)return "Current Work";if(work.condition==="blocked")return "Blocked";if(work.phase==="review")return "Awaiting review";if(work.phase==="active")return "Active responsibility";return "Open responsibility"}
function workNarrative(work:WorkSummary){if(work.condition==="blocked")return work.blocker_reason?`Blocked: ${work.blocker_reason}`:"Progress is blocked; resolution has not yet been recorded.";if(work.phase==="review")return "Submitted responsibility is awaiting review against its gates and evidence.";if(work.phase==="active")return "Execution is active under the current owner and revision.";if(work.phase==="open")return "Responsibility is open and has not entered active execution.";if(work.phase==="closed")return work.result_summary??"Responsibility is closed.";return `${humanizeToken(work.phase)} responsibility.`}
function shortId(value:string|null|undefined){if(!value)return "Not linked";return value.length>24?`${value.slice(0,12)}…${value.slice(-7)}`:value}
function humanizeToken(value:string){return value.split(/[_-]+/).filter(Boolean).map((part,index)=>index===0?`${part.charAt(0).toUpperCase()}${part.slice(1)}`:part).join(" ")}
function timestampKey(value:string|null|undefined){if(!value)return 0;if(value.startsWith("unix-ms:")){const parsed=Number(value.slice(8));return Number.isFinite(parsed)?parsed:0;}const parsed=Date.parse(value);return Number.isFinite(parsed)?parsed:0}
function isUnexpiredActivity(activity:AgentWorkspaceLiveActivity){const expiresAt=timestampKey(activity.expires_at);return expiresAt>0&&expiresAt>Date.now()}
function formatTime(value:string|null|undefined){if(!value)return "unknown";const timestamp=timestampKey(value);if(!timestamp)return value;return new Date(timestamp).toLocaleString([], {month:"short",day:"numeric",hour:"2-digit",minute:"2-digit"})}
function activateOnKeyDown(event:React.KeyboardEvent<HTMLElement>,activate:()=>void){if(event.target!==event.currentTarget)return;if(event.key==="Enter"){event.preventDefault();activate();}else if(event.key===" ")event.preventDefault();}
function activateOnKeyUp(event:React.KeyboardEvent<HTMLElement>,activate:()=>void){if(event.target===event.currentTarget&&event.key===" "){event.preventDefault();activate();}}
