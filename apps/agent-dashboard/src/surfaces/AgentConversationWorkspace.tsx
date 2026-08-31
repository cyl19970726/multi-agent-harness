import * as ScrollArea from "@radix-ui/react-scroll-area";
import * as Tabs from "@radix-ui/react-tabs";
import * as Tooltip from "@radix-ui/react-tooltip";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import {
  Activity, AlertTriangle, ArrowLeft, ChevronDown, ChevronRight,
  History, Inbox, Info, KeyRound, MessageSquare,
  PanelRight, Search, ShieldCheck, SlidersHorizontal, Sparkles,
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
import { OperationalFactRow } from "@/components/workbench/agent/AgentStreamPrimitives";
import { ToolEpisodeDetails, ToolEpisodeRow } from "@/components/workbench/agent/ProviderEventTimeline";
import type { SelectionState } from "../app/selection";
import { projectProviderTimeline, type ProviderTimelineItem, type ToolEpisode } from "../model/providerEventTimeline";
import {
  fetchRoleView,
  type AgentWorkspaceData,
  type AgentWorkspaceRosterItem,
  type AllowedAction,
  type MessageSummary,
  type PersistedSessionProjection,
  type ProviderEventFragment,
  type ProviderNativeEventRecord,
  type RoleActionExecutor,
  type RoleView,
  type WorkSummary,
} from "../model/roleViews";
import { RoleActionPanel } from "./RoleActionPanel";
import { ViewProvenance, ViewState } from "./RoleViewPrimitives";
import "./agent-workspace.css";

type WorkspaceMode = "session" | "messages" | "work";
type ContextSelection =
  | {kind:"event"; record:ProviderNativeEventRecord; fragment:ProviderEventFragment}
  | {kind:"tool"; episode:ToolEpisode}
  | {kind:"message"; message:MessageSummary}
  | {kind:"work"; work:WorkSummary}
  | null;

export function AgentConversationWorkspace({
  apiUrl,space,project,company,routeIdentity,selection,refreshKey,onAction,onSelectionChange,
}:{
  apiUrl:string; space:string; project:string; company?:string; routeIdentity:string; selection:SelectionState;
  refreshKey?:string; onAction:RoleActionExecutor;
  onSelectionChange:(next:Partial<SelectionState>)=>void;
}) {
  const [view,setView]=useState<RoleView<AgentWorkspaceData>|null>(null);
  const [viewRequestPath,setViewRequestPath]=useState<string|null>(null);
  const [error,setError]=useState<string|null>(null);
  const [loading,setLoading]=useState(true);
  const [loadingOlder,setLoadingOlder]=useState(false);
  const [refresh,setRefresh]=useState(0);
  const [contextSelection,setContextSelection]=useState<ContextSelection>(null);
  const [profileOpen,setProfileOpen]=useState(false);
  const [rosterOpen,setRosterOpen]=useState(false);
  const [contextOpen,setContextOpen]=useState(false);
  const profileTriggerRef=useRef<HTMLButtonElement>(null);
  const profileCloseRef=useRef<HTMLButtonElement>(null);
  const workspaceRef=useRef<HTMLElement>(null);
  const committedViewRef=useRef<RoleView<AgentWorkspaceData>|null>(null);
  // Path of the last committed view. Loading (and the composer lock it drives)
  // only applies while no view exists for the current request path; background
  // refetches revalidate silently against the committed view.
  const committedPathRef=useRef<string|null>(null);
  const mode:WorkspaceMode=selection.agentWorkspaceMode ?? "session";
  const agentId=selection.teamConversation && selection.teamConversation !== "host" ? selection.teamConversation : undefined;
  const requestQuery=new URLSearchParams();
  if(agentId)requestQuery.set("agent_id",agentId);
  const requestPath=`/v1/views/agent-workspace/${encodeURIComponent(routeIdentity)}${requestQuery.size?`?${requestQuery.toString()}`:""}`;
  const requestIdentity=`${apiUrl}\u0000${space}\u0000${project}\u0000${company??""}\u0000${requestPath}`;
  const expectedIdentityRef=useRef(requestIdentity);
  expectedIdentityRef.current=requestIdentity;

  useEffect(()=>{
    let live=true;
    if(committedPathRef.current!==requestIdentity)setLoading(true);
    fetchRoleView<AgentWorkspaceData>(apiUrl,requestPath,{space,project,company})
      .then((next)=>{if(live){
        const identityChanged=committedPathRef.current!==requestIdentity;
        const committed=next;
        committedPathRef.current=requestIdentity;committedViewRef.current=committed;setView(committed);setViewRequestPath(requestIdentity);setError(null);
        // A background revalidate keeps the selection alive; an identity switch
        // or a selection whose canonical record left the projection honestly drops it.
        setContextSelection(current=>identityChanged?null:revalidateContextSelection(current,committed.data));
      };})
      .catch((reason)=>{if(live)setError(String(reason));})
      .finally(()=>{if(live)setLoading(false);});
    return()=>{live=false;};
  },[apiUrl,space,project,company,requestPath,requestIdentity,refresh]);
  // Snapshot traffic can change continuously while providers are active. It
  // must not repeatedly cancel the first owner-private RoleView load. Recheck
  // only after the ambient snapshot has been quiet briefly; live activity has
  // its own authenticated SSE channel.
  const observedRefreshKeyRef=useRef(refreshKey);
  useEffect(()=>{
    if(observedRefreshKeyRef.current===refreshKey)return;
    observedRefreshKeyRef.current=refreshKey;
    const timer=window.setTimeout(()=>setRefresh(value=>value+1),500);
    return()=>window.clearTimeout(timer);
  },[refreshKey]);
  useEffect(()=>{
    const frame=window.requestAnimationFrame(()=>{
      const root=workspaceRef.current;
      root?.querySelector<HTMLElement>('[role="tabpanel"][data-state="active"] [data-radix-scroll-area-viewport]')?.scrollTo({top:0,left:0,behavior:"auto"});
    });
    return()=>window.cancelAnimationFrame(frame);
  },[mode,selection.teamConversation]);

  const currentView=viewRequestPath===requestIdentity?view:null;
  const privateData=currentView?.data??null;
  const persistedSessionStream=usePersistedSessionTimeline({
    apiUrl,space,project,company,
    teamId:privateData?.team.team_id??null,
    agentId:privateData?.selected_agent.agent_member_ref.id??null,
    sessionId:privateData?.current_session?.agent_session_id??null,
    sessionGeneration:privateData?.current_session?.agent_session_generation??null,
    initialProjection:privateData?.persisted_session_projection??null,
  });
  if(!currentView)return <main className="agent-team-surface h-full min-h-0 flex-1"><ViewState loading={loading} error={error} identityLabel={`Agent Workspace · ${routeIdentity}`} onRetry={()=>setRefresh(value=>value+1)}>{null}</ViewState></main>;
  const data=currentView.data;
  // This surface owns an independently authenticated RoleView. Its write
  // freshness must therefore follow that exact projection, not the ambient
  // snapshot domains used by the surrounding dashboard shell.
  const actionsCurrent=currentView.freshness==="current" && !loading && !error;
  const selected=data.selected_agent;
  const selectedRunId=selected.current_member_run_ref;
  const sessionProjection=persistedSessionStream.projection;
  const currentSession=data.current_session??null;
  const visibleSessionId=currentSession?.agent_session_id??null;
  const visibleSessionGeneration=currentSession?.agent_session_generation??null;
  const loadOlderSessionEvents=async()=>{
    if(loadingOlder||!sessionProjection?.available||!sessionProjection.has_more||!sessionProjection.next_before)return;
    const requestedIdentity=requestIdentity;
    setLoadingOlder(true);
    try{
      const olderQuery=new URLSearchParams(requestQuery);
      olderQuery.set("session_before",String(sessionProjection.next_before.ordering_key.value));
      olderQuery.set("session_cursor_kind",sessionProjection.next_before.ordering_key.kind);
      olderQuery.set("session_source_generation",sessionProjection.next_before.source_generation);
      const olderPath=`/v1/views/agent-workspace/${encodeURIComponent(routeIdentity)}?${olderQuery.toString()}`;
      const older=await fetchRoleView<AgentWorkspaceData>(apiUrl,olderPath,{space,project,company});
      if(expectedIdentityRef.current!==requestedIdentity)return;
      persistedSessionStream.mergeOlder(older.data.persisted_session_projection);
    }catch(reason){if(expectedIdentityRef.current===requestedIdentity)setError(String(reason));}finally{setLoadingOlder(false);}
  };
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
  const context=<AgentContextRail view={currentView} data={data} mode={mode} selected={contextSelection} currentWork={currentWork} actions={currentView.allowed_actions} onOpenWork={(work)=>{if(work){setContextSelection({kind:"work",work});onSelectionChange({agentWorkspaceMode:"work",teamWorkId:work.work_id});}else{setContextSelection(null);onSelectionChange({agentWorkspaceMode:"work"});}}}/>;

  return <Tooltip.Provider delayDuration={350}>
    <main ref={workspaceRef} className="agent-team-surface agent-workspace h-full min-h-0 flex-1 overflow-hidden" data-testid="agent-workspace">
      <div className="agent-workspace-layout grid h-full min-h-0 grid-cols-1 lg:grid-cols-[15.25rem_minmax(0,1fr)_23rem]" data-host={selected.is_host||undefined}>
        <aside className="agent-workspace-roster hidden min-h-0 border-r border-border lg:flex lg:flex-col" aria-label="Agent roster">
          <AgentRoster data={data} selectedId={selected.agent_member_ref.id} onBack={closeWorkspace} onSelect={selectAgent}/>
        </aside>

        <section className="agent-workspace-center flex min-h-0 min-w-0 flex-col">
          <header data-testid="agent-workspace-identity" className="agent-workspace-header flex min-h-[5.5rem] shrink-0 items-center gap-3 border-b border-border px-4 sm:px-7">
            <Button size="icon" variant="secondary" className="lg:hidden" onClick={closeWorkspace} aria-label="Back to Team Workspace"><ArrowLeft className="size-4"/></Button>
            <button ref={profileTriggerRef} type="button" className="group flex min-w-0 flex-1 items-center gap-3 text-left" onClick={()=>setProfileOpen(true)} aria-label={`Open ${selected.display_name} configuration`}>
              <Avatar name={selected.display_name} identity={`${selected.agent_member_ref.id} ${selected.role}`} size="lg" tone={data.runtime_truth.harness_control.state==="running"?"running":data.runtime_truth.harness_control.state==="ready"?"good":"idle"}/>
              <span className="min-w-0">
                <span className="flex min-w-0 items-center gap-2"><span className="truncate text-[1.28rem] font-semibold leading-tight tracking-[-0.025em] text-foreground">{selected.display_name}</span><span className="aw-header-role-badge">{humanizeToken(selected.role)}</span><ChevronRight className="size-3.5 text-muted-foreground transition-transform group-hover:translate-x-0.5"/></span>
                <span className="mt-1.5 flex min-w-0 flex-wrap items-center gap-1.5"><span className="aw-header-chip">{currentSession?.provider ? humanizeToken(currentSession.provider) : selected.provider ? humanizeToken(selected.provider) : "No active provider Session"}</span><span className="aw-header-chip" data-status={data.runtime_truth.harness_control.state}>Harness {humanizeToken(data.runtime_truth.harness_control.state)}</span><span className="aw-header-chip">Native {humanizeToken(data.runtime_truth.provider_native_activity.state)}</span>{selected.is_host&&selected.host_session_mode==="external_interactive"&&<span className="aw-header-chip">External · unmanaged</span>}{currentSession&&<span className="aw-header-chip">{humanizeToken(currentSession.effective_permission_ceiling)}</span>}{selected.current_member_run_ref&&<span className="aw-header-chip">{selected.current_member_run_ref}</span>}<span className="aw-header-chip">{visibleSessionId&&visibleSessionGeneration!==null ? `Session ${shortId(visibleSessionId)} · gen ${visibleSessionGeneration}` : "Native Session unavailable"}</span></span>
              </span>
            </button>
            <div className="hidden items-center gap-1 md:flex">{currentSession?.native_session_open_target&&<Button asChild size="sm" variant="outline"><a href={currentSession.native_session_open_target.uri} title={`Open exact ${humanizeToken(currentSession.provider)} native Session`}>Open native chat</a></Button>}<Button size="icon" variant="ghost" className="text-muted-foreground" aria-label="Agent Session details" title="Agent Session details" onClick={()=>setProfileOpen(true)}><Info className="size-4"/></Button><Button size="icon" variant="ghost" className="text-muted-foreground" aria-label="Agent Session history" title="Provider-native history"><History className="size-4"/></Button></div>
            <Button size="icon" variant="secondary" className="lg:hidden" onClick={()=>setRosterOpen(true)} aria-label="Open Agent roster"><Users className="size-4"/></Button>
            <Button size="icon" variant="secondary" className="lg:hidden" onClick={()=>setContextOpen(true)} aria-label="Open Agent context"><PanelRight className="size-4"/></Button>
          </header>

          {error&&<div role="alert" className="flex items-center gap-3 border-b border-status-warn/25 bg-status-warn/5 px-6 py-2 text-[11px]"><span className="min-w-0 flex-1">Refresh failed; writes are disabled until the authoritative view returns. {error}</span><Button size="sm" variant="secondary" onClick={()=>setRefresh(value=>value+1)}>Retry authenticated view</Button></div>}
          <Tabs.Root value={mode} onValueChange={value=>{setContextSelection(null);onSelectionChange({agentWorkspaceMode:value as WorkspaceMode});}} className="flex min-h-0 flex-1 flex-col">
            <div data-testid="agent-workspace-modebar" className="aw-modebar flex min-h-12 shrink-0 items-end border-b border-border px-4 sm:px-7">
              <Tabs.List aria-label="Agent Workspace modes" className="agent-workspace-tabs flex h-full items-end gap-7">
                <WorkspaceTab value="session" label="Session" count={sessionProjection?.available?sessionProjection.records.reduce((count,record)=>count+record.fragments.length,0):0}/>
                <WorkspaceTab value="messages" label="Messages" count={data.context_summary.unread_count}/>
                <WorkspaceTab value="work" label="Work" count={data.works.length}/>
              </Tabs.List>
              <RuntimeTruthStrip truth={data.runtime_truth}/>
              <div className="hidden h-full items-center gap-3 text-[10px] text-muted-foreground xl:flex"><span className="flex items-center gap-1.5"><ShieldCheck className="size-3.5"/>{sessionProjection?.available?"Persisted provider-native Session":"Native Session unavailable"}</span></div>
            </div>
            <Tabs.Content value="session" className="min-h-0 flex-1 outline-none"><SessionCanvas data={data} projection={sessionProjection} connectionState={persistedSessionStream.connectionState} selectedMessageId={contextSelection?.kind==="message"?contextSelection.message.message_id:null} onSelect={setContextSelection} loadingOlder={loadingOlder} onLoadOlder={loadOlderSessionEvents}/></Tabs.Content>
            <Tabs.Content value="messages" className="min-h-0 flex-1 outline-none"><MessagesCanvas data={data} onSelect={setContextSelection}/></Tabs.Content>
            <Tabs.Content value="work" className="min-h-0 flex-1 outline-none"><WorkCanvas data={data} onSelect={(work)=>{setContextSelection({kind:"work",work});onSelectionChange({teamWorkId:work.work_id});}}/></Tabs.Content>
          </Tabs.Root>

          {selected.is_host&&currentSession?.native_session_open_target&&<div className="shrink-0 border-t border-border bg-primary/[0.035] px-4 py-2 text-[11px] text-muted-foreground sm:px-7"><span>Direct conversation stays in the provider-native transcript. </span><a className="font-semibold text-primary hover:underline" href={currentSession.native_session_open_target.uri}>Continue this exact Host Session in Codex Desktop</a><span>. Message authoring in Agent Workspace is intentionally deferred.</span></div>}
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

function WorkspaceTab({value,label,count}:{value:WorkspaceMode;label:string;count:number}){
  return <Tabs.Trigger value={value} className="relative flex h-11 items-center gap-2 text-[12px] font-semibold text-muted-foreground outline-none data-[state=active]:text-foreground">{label}{count>0&&<span className="text-[9px] font-medium text-[color:var(--aw-faint)]">{count}</span>}<span className="absolute inset-x-0 bottom-0 h-0.5 origin-center scale-x-0 bg-primary transition-transform data-[state=active]:scale-x-100"/></Tabs.Trigger>;
}

function RuntimeTruthStrip({truth}:{truth:AgentWorkspaceData["runtime_truth"]}){
  const attention=["blocked","recovery_required"].includes(truth.harness_control.state);
  return <section className="aw-runtime-truth" data-attention={attention||undefined} aria-label="Work, coordination, Harness control and provider-native activity">
    <div><span>Work</span><strong>{humanizeToken(truth.work.phase)}</strong></div>
    <div><span>Coordination</span><strong>{humanizeToken(truth.coordination.state)}</strong></div>
    <div><span>Harness control</span><strong>{humanizeToken(truth.harness_control.state)}</strong></div>
    <div><span>Native activity</span><strong>{humanizeToken(truth.provider_native_activity.state)}</strong></div>
    <p>{truth.explanation}</p>
  </section>;
}

function RuntimeControlBoundary({truth}:{truth:AgentWorkspaceData["runtime_truth"]}){
  return <section className="aw-runtime-control-boundary" role="note" aria-label="Harness control loss boundary">
    <AlertTriangle aria-hidden="true"/>
    <div><strong>Harness control was {humanizeToken(truth.harness_control.state).toLowerCase()} here.</strong><p>Provider-native records explicitly observed after this boundary do not prove recovery or Work completion. Rows without comparable provider time keep provider source order.</p><span>{truth.harness_control.reason_code} · {formatTime(truth.harness_control.occurred_at)}</span></div>
  </section>;
}

function AgentRoster({data,selectedId,onBack,onSelect}:{data:AgentWorkspaceData;selectedId:string;onBack:()=>void;onSelect:(agent:AgentWorkspaceRosterItem)=>void}){
  const [query,setQuery]=useState("");
  const visible=data.roster.filter(agent=>`${agent.display_name} ${agent.role} ${agent.agent_member_ref.id}`.toLowerCase().includes(query.toLowerCase().trim()));
  return <div className="flex min-h-0 flex-1 flex-col bg-transparent">
    <header className="shrink-0 px-4 pb-3 pt-5"><p className="agent-team-eyebrow">Agent Team</p><div className="mt-1 flex items-start gap-1"><div className="min-w-0 flex-1"><h2 className="line-clamp-2 max-h-[2.65rem] text-[18px] font-semibold leading-[1.15] tracking-[-0.025em]">{data.team.display_name||data.team.team_id}</h2><button type="button" onClick={onBack} className="mt-1 text-[10px] text-muted-foreground hover:text-foreground">← Back to Team Workspace</button></div><Tooltip.Root><Tooltip.Trigger asChild><Button size="icon" variant="ghost" onClick={onBack} aria-label="Back to Team Workspace"><X className="size-4"/></Button></Tooltip.Trigger><Tooltip.Portal><Tooltip.Content side="right" className="rounded-md bg-foreground px-2 py-1 text-[10px] text-background">Back to Team Workspace</Tooltip.Content></Tooltip.Portal></Tooltip.Root></div>
      <label className="relative mt-4 block"><Search className="pointer-events-none absolute left-3 top-2.5 size-3.5 text-muted-foreground"/><span className="sr-only">Search Agents</span><input value={query} onChange={event=>setQuery(event.target.value)} placeholder="Search agents" className="h-9 w-full rounded-lg border border-border/80 bg-background/75 pl-9 pr-3 text-xs outline-none focus:border-primary/55"/></label>
    </header>
    <ScrollArea.Root className="min-h-0 flex-1 overflow-hidden"><ScrollArea.Viewport className="size-full min-w-0 [&>div]:!block [&>div]:!min-w-0"><div className="px-2 pb-5">
      {visible.map((agent,index)=>{const isSelected=agent.agent_member_ref.id===selectedId;const stateLabel=isSelected?selectedRosterStateLabel(data):rosterStateLabel(agent);return <div key={agent.agent_member_ref.id}>{index===0&&<p className="px-3 pb-1 pt-2 text-[9px] font-semibold uppercase tracking-[.15em] text-muted-foreground">Host Agent</p>}{index===1&&<p className="px-3 pb-1 pt-5 text-[9px] font-semibold uppercase tracking-[.15em] text-muted-foreground">Team Members</p>}<button type="button" onClick={()=>onSelect(agent)} data-selected={isSelected} className="agent-roster-row group flex w-full items-center gap-2.5 px-2.5 py-3 text-left">
        <Avatar name={agent.display_name} identity={`${agent.agent_member_ref.id} ${agent.role}`} size="md" tone={isSelected?selectedAvatarTone(data):agent.runtime_state==="running"?"running":agent.capacity==="available"?"good":"idle"}/>
        <span className="min-w-0 flex-1"><span className="flex items-center gap-2"><span className="agent-roster-name min-w-0 truncate text-[13.5px] font-semibold leading-4">{agent.display_name}</span>{agent.is_host&&<span className="shrink-0 text-[9px] font-semibold text-primary">Host</span>}</span><span className="agent-roster-meta mt-1 block truncate text-[10.5px] leading-4 text-muted-foreground">{humanizeToken(agent.role)} · <span className={stateLabel.tone}>{stateLabel.word}</span></span></span>
        {(agent.queued_work_count??0)>0&&<span className="aw-roster-tail"><span aria-label={`${agent.queued_work_count} queued Work`}>{agent.queued_work_count}</span></span>}
      </button></div>;})}
    </div></ScrollArea.Viewport><ScrollArea.Scrollbar orientation="vertical" className="flex w-2 p-0.5"><ScrollArea.Thumb className="flex-1 rounded-full bg-border"/></ScrollArea.Scrollbar></ScrollArea.Root>
  </div>;
}

type PersistedSessionConnectionState="inactive"|"connecting"|"connected"|"disconnected";
function SessionCanvas({data,projection,connectionState,selectedMessageId,onSelect,loadingOlder,onLoadOlder}:{data:AgentWorkspaceData;projection:PersistedSessionProjection|null;connectionState:PersistedSessionConnectionState;selectedMessageId:string|null;onSelect:(next:ContextSelection)=>void;loadingOlder:boolean;onLoadOlder:()=>void}){
  const [selectedTimelineId,setSelectedTimelineId]=useState<string|null>(null);
  const viewportRef=useRef<HTMLDivElement>(null);
  const chronologyRef=useRef<HTMLDivElement>(null);
  const rows=useMemo(()=>{
    const sorted:Array<SessionMessageRow|SessionProviderRow|SessionBoundaryRow>=mergeSessionRows(
      data.messages.map(message=>({kind:"message" as const,at:message.created_at,message,continuation:false})).sort((left,right)=>timestampKey(left.at)-timestampKey(right.at)),
      (projection?.available?projectProviderTimeline(projection.records).map(item=>({kind:"provider" as const,at:recordTime(providerTimelineRecord(item))??"",record:providerTimelineRecord(item),item})):[]),
    );
    const boundaryAt=data.runtime_truth.harness_control.occurred_at;
    if(boundaryAt&&["blocked","recovery_required"].includes(data.runtime_truth.harness_control.state)){
      const boundary:SessionBoundaryRow={kind:"control_boundary",at:boundaryAt};
      const observedAfterBoundary=sorted.findIndex(row=>row.kind==="provider"&&providerTimelineObservedAfter(row.item,boundaryAt));
      const chronologicalBoundary=sorted.findIndex(row=>timestampKey(row.at)>=timestampKey(boundaryAt));
      const index=observedAfterBoundary>=0?observedAfterBoundary:chronologicalBoundary;
      sorted.splice(index<0?sorted.length:index,0,boundary);
    }
    const seen=new Set<string>();
    return sorted.map(row=>{
      if(row.kind!=="message")return row;
      const continuation=seen.has(row.message.correlation_id);seen.add(row.message.correlation_id);
      return {...row,continuation};
    });
  },[data.messages,data.runtime_truth,projection]);
  const hasNativeRowsWithoutComparableProviderTime=rows.some(row=>row.kind==="provider"&&!recordTime(row.record));
  const virtualizer=useVirtualizer({count:rows.length,getScrollElement:()=>viewportRef.current,estimateSize:index=>rows[index]?.kind==="provider"?240:rows[index]?.kind==="control_boundary"?112:150,overscan:10,scrollMargin:chronologyRef.current?.offsetTop??0});
  const currentWork=data.works.find(work=>work.work_id===data.context_summary.current_work_id);
  return <ScrollArea.Root className="h-full overflow-hidden"><ScrollArea.Viewport ref={viewportRef} className="size-full min-w-0 [&>div]:!block [&>div]:!min-w-0"><div className="agent-session-stream w-full px-5 pb-8 sm:px-7">
    <div className="aw-session-context-strip">
      <span className="aw-session-context-strip__label"><Sparkles aria-hidden="true"/>Messages + persisted native Session</span>
      <strong>{currentWork?.title??"Harness Messages and provider-native Session activity"}</strong>
      <span>{currentWork?`${humanizeToken(currentWork.phase)} Work · `:""}{data.messages.length} messages · {projection?.available?projection.records.reduce((count,record)=>count+record.fragments.length,0):0} native fragments{connectionState==="disconnected"?" · persisted stream disconnected":""}{projection?.available&&projection.incomplete_tail?" · provider file has an incomplete tail":""}{projection?.available&&projection.source_reset?" · source generation reset":""}</span>
    </div>
    <div className="aw-authority-legend" aria-label="Session timeline fact sources"><span data-source="message">Harness Message · coordination</span><span data-source="work">Work link · context only</span><span data-source="provider">Provider-native · execution evidence</span></div>
    {hasNativeRowsWithoutComparableProviderTime&&<p className="mb-3 text-[10px] leading-4 text-muted-foreground">Native records without comparable provider timestamps remain in provider source order; their position relative to Harness Messages is not a recorded chronology.</p>}
    {projection?.available&&projection.has_more&&<div className="flex justify-center py-3"><Button type="button" variant="outline" size="sm" disabled={loadingOlder} onClick={onLoadOlder}>{loadingOlder?"Loading provider-native events…":"Load earlier native Session events"}</Button></div>}
    {rows.length
      ? <div ref={chronologyRef} className="aw-session-chronology relative" style={{height:virtualizer.getTotalSize()}} aria-label="Harness Messages and persisted provider-native records in their honest partial order">{virtualizer.getVirtualItems().map(item=>{const row=rows[item.index]!;const providerId=row.kind==="provider"?providerTimelineId(row.item):null;const rowKey=row.kind==="message"?`message:${row.message.message_id}`:row.kind==="control_boundary"?`control-boundary:${row.at}`:providerId!;return <div key={rowKey} data-index={item.index} data-session-row-kind={row.kind} ref={virtualizer.measureElement} className="absolute left-0 top-0 w-full" style={{transform:`translateY(${item.start-virtualizer.options.scrollMargin}px)`}}>{(()=>{
        if(row.kind==="message"){
          return <AuthoredTurn data={data} message={row.message} selectedAgentId={data.selected_agent.agent_member_ref.id} selected={selectedMessageId===row.message.message_id} continuation={row.continuation} onSelect={()=>onSelect({kind:"message",message:row.message})}/>;
        }
        if(row.kind==="control_boundary")return <RuntimeControlBoundary truth={data.runtime_truth}/>;
        return <ProviderTimelineRecord item={row.item} actorName={data.selected_agent.display_name} selected={selectedTimelineId===providerId} onToggle={()=>{const next=selectedTimelineId===providerId?null:providerId;setSelectedTimelineId(next);onSelect(next?(row.item.kind==="tool_episode"?{kind:"tool",episode:row.item}:{kind:"event",record:row.item.record,fragment:row.item.fragment}):null);}}/>;
      })()}</div>})}</div>
      : <EmptyCanvas compact title={projection&&!projection.available?"Provider-native Session unavailable":data.selected_agent.is_host?"No Host Session events or Team Messages yet":"No Session activity yet"} detail={projection&&!projection.available?`${humanizeToken(projection.reason_code)}${projection.detail?`: ${projection.detail}`:""}`:"Original provider-native events and authored Team Messages will appear here when persisted by the provider."}/>
    }
    {projection?.available&&projection.has_more&&<p className="mt-4 border-t border-border pt-3 text-[10px] text-muted-foreground">Earlier original provider events remain available on demand.</p>}
  </div></ScrollArea.Viewport><ScrollArea.Scrollbar orientation="vertical" className="flex w-2 p-0.5"><ScrollArea.Thumb className="rounded-full bg-border"/></ScrollArea.Scrollbar></ScrollArea.Root>;
}

function ProviderTimelineRecord({item,actorName,selected,onToggle}:{item:ProviderTimelineItem;actorName:string;selected:boolean;onToggle:()=>void}){
  const record=providerTimelineRecord(item);
  if(item.kind==="tool_episode")return <section className="aw-provider-episode" data-native-ordering-position={record.ordering_key.value} aria-label={`Tool call ${item.tool_name??"unknown"}`}><ToolEpisodeRow episode={item} expanded={selected} selected={selected} timestamp={toolEpisodeTime(item)} onToggle={onToggle}/></section>;
  return <NativeEventRecord record={item.record} fragment={item.fragment} actorName={actorName} selected={selected} onOpen={onToggle}/>;
}

function NativeEventRecord({record,fragment,actorName,selected,onOpen}:{record:ProviderNativeEventRecord;fragment:ProviderEventFragment;actorName:string;selected:boolean;onOpen:()=>void}){
  return <section className="aw-provider-episode" data-native-ordering-position={record.ordering_key.value} data-terminal={fragment.lifecycle_phase==="terminal"||undefined} aria-label={`Provider-native ${humanizeToken(fragment.semantic_kind)} event`}>
    {fragment.semantic_kind==="assistant_response"
      ? <NativeAuthoredRecord record={record} fragment={fragment} actorName={actorName} selected={selected} onSelect={onOpen}/>
      : <div className="aw-native-facts-trail"><ExpandableEvent record={record} fragment={fragment} selected={selected} onSelect={onOpen}/></div>}
  </section>;
}

function NativeAuthoredRecord({record,fragment,actorName,selected,onSelect}:{record:ProviderNativeEventRecord;fragment:ProviderEventFragment;actorName:string;selected:boolean;onSelect:()=>void}){
  return <article role="button" tabIndex={0} aria-label={`Open native Session response from ${actorName}`} data-selected={selected||undefined} className="aw-native-authored-record" onClick={onSelect} onKeyDown={eventKey=>activateOnKeyDown(eventKey,onSelect)} onKeyUp={eventKey=>activateOnKeyUp(eventKey,onSelect)}>
    <span className="aw-native-authored-record__mark"><MessageSquare aria-hidden="true"/></span>
    <div className="min-w-0 flex-1"><header><strong>{actorName}</strong><span>Provider-native event</span><span className="aw-kind-chip">{humanizeToken(fragment.semantic_kind)}</span><time>{formatTime(recordTime(record))}</time></header><FragmentBody fragment={fragment}/><footer>{humanizeToken(record.provider)} · {humanizeToken(fragment.completeness)} · source {record.native_source_ref}</footer></div>
  </article>;
}

function AuthoredTurn({data,message,selectedAgentId,selected,continuation,onSelect}:{data:AgentWorkspaceData;message:MessageSummary;selectedAgentId:string;selected:boolean;continuation:boolean;onSelect:()=>void}){
  const fromSelected=message.sender.id===selectedAgentId;
  const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);
  const name=actor?.display_name??(fromSelected?data.selected_agent.display_name:message.sender.id);
  return <article role="button" tabIndex={0} aria-label={`Open authored Message from ${name}`} data-from-selected={fromSelected||undefined} data-selected={selected||undefined} data-thread-continuation={continuation||undefined} className="agent-authored-turn" onClick={onSelect} onKeyDown={event=>activateOnKeyDown(event,onSelect)} onKeyUp={event=>activateOnKeyUp(event,onSelect)}>
    <Avatar name={name} identity={`${message.sender.id} ${actor?.role??""}`} size="md" tone={actor?.runtime_state==="running"?"running":"idle"}/><div className="min-w-0 flex-1"><header className="mb-1 flex items-baseline gap-2"><p className="truncate text-[13px] font-semibold">{name}</p>{actor?.role&&<span className="text-[10.5px] text-muted-foreground">{humanizeToken(actor.role)}</span>}<span className="aw-kind-chip">{humanizeToken(message.kind)}</span><time className="ml-auto text-[10px] text-muted-foreground">{formatTime(message.created_at)}</time></header>
    <div className="aw-authored-body"><Markdown source={message.body}/></div>
    <div className="aw-record-meta">{message.work_id&&<span data-message-fact="work-context" title={`Work context only ${message.work_id}`}>Work context only · {shortId(message.work_id)}</span>}<span title={`Correlation ${message.correlation_id}`}>Conversation · {shortId(message.correlation_id)}</span>{message.causation_id&&<span title={`Reply to ${message.causation_id}`}>Reply · {shortId(message.causation_id)}</span>}<MessageEvidenceLabels message={message}/></div></div>
  </article>;
}

function ExpandableEvent({record,fragment,selected,onSelect}:{record:ProviderNativeEventRecord;fragment:ProviderEventFragment;selected:boolean;onSelect:()=>void}){
  const payload=fragment.payload;
  const summary=payload.type==="native"?`${payload.event_type??"Unknown type"} · ${humanizeToken(payload.classification_reason??"unsupported event type")}`:payload.type==="malformed"?humanizeToken(payload.reason_code):`${humanizeToken(record.provider)} native fragment · ${record.native_source_ref}`;
  return <div data-boundary-aligned={selected||undefined}><OperationalFactRow kind={fragmentPresentationKind(fragment)} status={fragmentStatus(fragment)} title={humanizeToken(fragment.semantic_kind)} summary={summary} timestamp={formatTime(recordTime(record))} expanded={selected} selected={selected} onToggle={onSelect}/>{selected&&<><FragmentBody fragment={fragment}/><details className="mt-2"><summary>Original provider-native record</summary><NativeEventBody value={record.native_event}/></details></>}</div>;
}

function FragmentBody({fragment}:{fragment:ProviderEventFragment}){
  const payload=fragment.payload;
  if(payload.type==="assistant_response"||payload.type==="reasoning")return payload.text?<div className="aw-authored-body"><Markdown source={payload.text}/></div>:<p className="text-[11px] italic text-muted-foreground">Provider content unavailable: {humanizeToken(fragment.content_unavailable_reason??"not projected")}.</p>;
  if(payload.type==="native")return <div className="aw-unclassified-native"><p><strong>{payload.event_type??"Unknown native event"}</strong>{payload.event_subtype&&<span> · {payload.event_subtype}</span>}</p><p>{humanizeToken(payload.classification_reason??"unsupported event type")}</p></div>;
  if(payload.type==="malformed")return <div className="aw-unclassified-native" data-error="true"><p><strong>Malformed provider record</strong></p><p>{humanizeToken(payload.reason_code)}</p></div>;
  return <pre className="aw-native-event-body" aria-label="Provider-native event fragment">{JSON.stringify(payload,null,2)}</pre>;
}

function NativeEventBody({value}:{value:unknown}){
  return <pre className="aw-native-event-body" aria-label="Original provider-native event">{typeof value==="string"?value:JSON.stringify(value,null,2)}</pre>;
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
  return <ScrollArea.Root className="h-full overflow-hidden"><ScrollArea.Viewport className="size-full min-w-0 [&>div]:!block [&>div]:!min-w-0"><div className="agent-messages-canvas mx-auto max-w-[58rem] px-5 pb-10 sm:px-7" data-empty={!visible.length||undefined}>
    <WorkspaceCanvasIntro compact eyebrow="Harness messages" title={`${data.selected_agent.display_name} · Inbox and Outbox`} detail="Authored coordination stays linked to its sender, recipient, delivery and Work." facts={[`${data.messages.length} total`,`${data.context_summary.unread_count} unread`]}/>
    <div className="agent-message-toolbar aw-filter-strip flex flex-wrap items-center gap-2 border-y border-border"><div className="flex">{(["all","inbox","outbox","unread"] as const).map(item=><button key={item} type="button" data-active={lens===item} onClick={()=>setLens(item)} className="px-3 text-[10px] font-semibold capitalize text-muted-foreground">{item}</button>)}</div><label className="relative ml-auto min-w-[14rem] flex-1 sm:max-w-[20rem]"><Search className="pointer-events-none absolute left-3 top-2.5 size-3.5 text-muted-foreground"/><span className="sr-only">Search messages</span><input value={query} onChange={event=>setQuery(event.target.value)} placeholder="Search messages" className="h-9 w-full border-0 bg-transparent pl-9 pr-3 text-xs outline-none"/></label></div>
    {visible.length
      ? <MessageThreads data={data} messages={visible} selectedId={selectedId} onSelect={onSelect}/>
      : <EmptyCanvas title={query||lens!=="all"?"No messages match this view":"No authored messages yet"} detail={query||lens!=="all"?"Clear the current filter to return to the full authored record.":"Use the command surface below to start a durable, Work-linked conversation."}/>
    }
  </div></ScrollArea.Viewport></ScrollArea.Root>;
}

function MessageThreads({data,messages,selectedId,onSelect}:{data:AgentWorkspaceData;messages:MessageSummary[];selectedId:string;onSelect:(next:ContextSelection)=>void}){
  const threads=[...messages.reduce((index,message)=>{const key=message.correlation_id||message.message_id;const existing=index.get(key);if(existing)existing.push(message);else index.set(key,[message]);return index;},new Map<string,MessageSummary[]>()).entries()]
    .map(([correlationId,items])=>({correlationId,items:[...items].sort((left,right)=>timestampKey(left.created_at)-timestampKey(right.created_at))}))
    .sort((left,right)=>timestampKey(right.items[right.items.length-1]?.created_at)-timestampKey(left.items[left.items.length-1]?.created_at));
  return <div className="agent-message-stream">{threads.map(thread=>{
    const workContexts=[...new Set(thread.items.flatMap(message=>message.work_id?[message.work_id]:[]))];
    const linkedWork=workContexts.length===1?data.works.find(work=>work.work_id===workContexts[0]):undefined;
    const contextLabel=workContexts.length===0
      ? "General coordination"
      : workContexts.length===1
        ? linkedWork?.title??`Work context · ${shortId(workContexts[0]!)}`
        : "Multiple Work contexts";
    const participantIds=[...new Set(thread.items.flatMap(message=>[message.sender.id,...message.recipients.map(recipient=>recipient.id)]))];
    const participants=participantIds.map(id=>data.roster.find(item=>item.agent_member_ref.id===id)?.display_name??id);
    const latest=thread.items[thread.items.length-1]!;
    return <section key={thread.correlationId} className="aw-message-thread" aria-label={`Conversation about ${contextLabel}`}>
      <header className="aw-message-thread__header"><h3>{contextLabel}</h3><span>{participants.join(" ↔ ")} · {thread.items.length} {thread.items.length===1?"message":"messages"} · {formatTime(latest.created_at)}</span></header>
      <div className="aw-message-thread__turns">{thread.items.map((message,index)=>{
        const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);
        const actorName=message.sender.display_name??actor?.display_name??message.sender.id;
        const recipients=message.recipients.map(recipient=>recipient.display_name??data.roster.find(item=>item.agent_member_ref.id===recipient.id)?.display_name??recipient.id).join(", ");
        const unread=message.deliveries.some(item=>["queued","delivered"].includes(item.status));
        const messageWork=data.works.find(work=>work.work_id===message.work_id);
        return <button key={message.message_id} type="button" data-thread-continuation={index>0||undefined} className="agent-message-row flex w-full gap-3 text-left" onClick={()=>onSelect({kind:"message",message})}><Avatar name={actorName} identity={`${message.sender.id} ${actor?.role??""}`} size="md" tone={actor?.runtime_state==="running"?"running":"idle"}/><span className="min-w-0 flex-1"><span className="flex items-baseline gap-2">{unread&&<span className="aw-message-unread-dot" title="Unread"/>}<b className="truncate text-[12.5px]">{actorName}</b><span className="aw-kind-chip">{humanizeToken(message.kind)}</span><span className="aw-record-kind">{message.sender.id===selectedId?"Outbox":"Inbox"} → {recipients}</span><time className="ml-auto text-[10.5px] text-muted-foreground">{formatTime(message.created_at)}</time></span><span className="mt-1.5 block max-w-[42rem] whitespace-pre-wrap text-[13.5px] leading-[1.58] text-foreground/90">{message.body}</span><span className="aw-record-meta">{message.work_id&&<span data-message-fact="work-context" title={message.work_id}>Work context only · {messageWork?.title??shortId(message.work_id)}</span>}{message.causation_id&&<span>Reply · {shortId(message.causation_id)}</span>}<MessageEvidenceLabels message={message}/></span></span></button>;
      })}</div>
    </section>;
  })}</div>;
}

function MessageEvidenceLabels({message}:{message:MessageSummary}){
  const receipts=message.deliveries.filter(item=>item.provider_receipt_id).length;
  return <><span data-message-fact="delivery">Harness delivery · {messageDeliveryLabel(message)}</span><span data-message-fact="provider-receipt">Provider receipt · {receipts?`${receipts} recorded`:"none"}</span></>;
}

function messageDeliveryLabel(message:MessageSummary){
  if(message.delivery_state)return humanizeToken(message.delivery_state);
  if(message.deliveries.length)return message.deliveries.map(delivery=>humanizeToken(delivery.status)).join(" · ");
  return "No recipient delivery";
}

function WorkCanvas({data,onSelect}:{data:AgentWorkspaceData;onSelect:(work:WorkSummary)=>void}){
  const [lens,setLens]=useState<"current"|"open"|"active"|"review"|"closed"|"eligible">("current");
  const memberId=data.selected_agent.agent_member_ref.id;
  const owns=(work:WorkSummary)=>work.owner_actor_ref?.id===memberId;
  const visible=data.works.filter(work=>lens==="current"?owns(work)&&work.phase!=="closed":lens==="eligible"?!owns(work)&&work.eligible_member_ids.includes(memberId):work.phase===lens);
  const ordered=[...visible].sort((left,right)=>workVisualRank(left,data.context_summary.current_work_id)-workVisualRank(right,data.context_summary.current_work_id)||timestampKey(right.updated_at)-timestampKey(left.updated_at));
  const ownedCount=data.works.filter(owns).length;
  const reviewCount=data.works.filter(work=>work.phase==="review").length;
  return <ScrollArea.Root className="h-full overflow-hidden"><ScrollArea.Viewport className="size-full min-w-0 [&>div]:!block [&>div]:!min-w-0"><div className="agent-work-canvas mx-auto max-w-[60rem] px-5 pb-10 sm:px-7">
    <WorkspaceCanvasIntro compact eyebrow="Responsibility" title={`${data.selected_agent.display_name} · Work`} detail="Ownership, execution phase, condition and gate progress stay distinct." facts={[`${ownedCount} owned`,`${reviewCount} in review`,`${data.works.length-ownedCount} eligible or shared`]}/>
    <div className="aw-filter-strip flex flex-wrap items-center gap-5 border-y border-border">{(["current","open","active","review","closed","eligible"] as const).map(item=><button key={item} type="button" data-active={lens===item} onClick={()=>setLens(item)} className="agent-work-lens relative text-[10px] font-semibold capitalize text-muted-foreground data-[active=true]:text-foreground">{item}</button>)}<span className="ml-auto text-[10px] text-muted-foreground">{visible.length} {visible.length===1?"record":"records"}</span></div>
    {ordered.length?<div className="agent-work-stream">{ordered.map((work,index)=>{const current=work.work_id===data.context_summary.current_work_id;const group=workGroupLabel(work,current,lens);const prior=index>0?workGroupLabel(ordered[index-1],ordered[index-1].work_id===data.context_summary.current_work_id,lens):null;const owner=data.roster.find(item=>item.agent_member_ref.id===work.owner_actor_ref?.id);return <div key={work.work_id}>{group!==prior&&<p className="aw-work-group-label">{group}</p>}<button type="button" data-current={current||undefined} data-phase={work.phase} data-condition={work.condition} className="agent-work-row grid w-full grid-cols-[minmax(0,1fr)_auto] gap-5 text-left" onClick={()=>onSelect(work)}><span className="min-w-0"><span className="flex items-center gap-2"><span className="break-words text-[13.5px] font-semibold leading-[1.35]">{work.title||work.work_id}</span>{work.condition!=="normal"&&<WorkspaceState label={humanizeToken(String(work.condition))} tone="bad"/>}</span>{work.completion_criteria_markdown&&<span className="mt-1 block max-w-[42rem] line-clamp-1 text-[12.5px] leading-[1.5] text-foreground/75">{work.completion_criteria_markdown}</span>}<span className="aw-record-meta"><span className="aw-work-owner"><Avatar name={owner?.display_name??"Unassigned"} identity={work.owner_actor_ref?.id??"unassigned"} size="xs" tone={owner?.runtime_state==="running"?"running":"idle"}/><span>{owner?.display_name??(work.owner_actor_ref?"Assigned":"Unassigned")}</span></span><span>{work.owner_actor_ref?.id===memberId?"Owned responsibility":"Eligible responsibility"}</span><span>{shortId(work.work_id)} · revision {work.work_revision}</span><span>{humanizeToken(String(work.priority))} priority</span><span>Gates {work.gate_summary.passed}/{work.gate_summary.required}</span></span></span><span className="aw-work-state"><WorkspaceState label={humanizeToken(work.phase)} tone={work.phase==="active"?"running":work.phase==="review"?"warn":work.phase==="closed"?"good":"muted"}/>{meaningfulRecovery(work)&&<span>{humanizeToken(String(work.delivery_summary.recovery_class))}</span>}<time>{formatTime(work.updated_at)}</time></span></button></div>})}</div>:<EmptyCanvas title="No Work in this view" detail="Eligibility is not ownership. Work remains authoritative in the Team Work kernel."/>}
  </div></ScrollArea.Viewport></ScrollArea.Root>;
}

function AgentContextRail({view,data,mode,selected,currentWork,actions,onOpenWork}:{view:RoleView<AgentWorkspaceData>;data:AgentWorkspaceData;mode:WorkspaceMode;selected:ContextSelection;currentWork?:WorkSummary;actions:AllowedAction[];onOpenWork:(work?:WorkSummary)=>void}){
  const isHost=data.selected_agent.is_host;
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
  const responsibility={open:ownedWorks.filter(work=>work.phase==="open").length,active:ownedWorks.filter(work=>work.phase==="active").length,review:ownedWorks.filter(work=>work.phase==="review").length,closed:ownedWorks.filter(work=>work.phase==="closed").length};
  const runBoundWork=data.works.find(work=>work.current_member_run_ref&&work.current_member_run_ref===data.selected_agent.current_member_run_ref);
  const runGeneration=runBoundWork&&typeof runBoundWork.runtime_summary.generation==="number"?runBoundWork.runtime_summary.generation:null;
  const executionDriver=[data.configuration.provider_profile_ref?humanizeToken(data.configuration.provider_profile_ref):null,data.configuration.model_preference].filter(Boolean).join(" · ");
  const otherOwnedWorks=ownedWorks.filter(work=>work.work_id!==anchoredWork?.work_id&&!attentionWorks.some(attention=>attention.work_id===work.work_id));
  const prioritizedActions=[...actionIndex].sort((left,right)=>decisionActionRank(left.kind,anchoredWork)-decisionActionRank(right.kind,anchoredWork));
  const workSection=<WorkContext work={anchoredWork} title={isHost&&anchoredNeedsJudgment?"Current decision":"Current Work"} onOpenWork={onOpenWork}/>;
  const selectionInset=selected&&<div className="aw-context-selection-inset" aria-label="Selected context">{selected.kind==="message"?<MessageContext data={data} message={selected.message}/>:selected.kind==="event"?<EventContext record={selected.record} fragment={selected.fragment}/>:selected.kind==="tool"?<ToolContext episode={selected.episode}/>:<WorkSelectionContext data={data} work={selected.work}/>}</div>;
  const responsibilitySection=!isHost&&<ContextSection title="Responsibility" hint={eligibleWorks.length?`${eligibleWorks.length} eligible Work`:undefined}><ResponsibilityStrip values={responsibility}/><button type="button" className="aw-context-link" onClick={()=>onOpenWork()}>View ready work ↗</button>{latestExchange&&<div className="mt-3"><ContextMessageRow data={data} message={latestExchange}/></div>}</ContextSection>;
  const needsHostSection=isHost&&attentionWorks.length>0&&<ContextSection title="Needs Host" hint={`${attentionWorks.length} ${attentionWorks.length===1?"responsibility":"responsibilities"}`}>{attentionWorks.filter(work=>work.work_id!==anchoredWork?.work_id).slice(0,2).map(work=><ContextWorkRow key={work.work_id} data={data} work={work}/>)}</ContextSection>;
  const inboxSection=isHost&&hostInbox.length>0&&<ContextSection title="Team Inbox" hint={unreadIncoming.length?`${unreadIncoming.length} unsettled`:`${hostInbox.length} recent`}>{hostInbox.map(message=><ContextMessageRow key={message.message_id} data={data} message={message}/>)}</ContextSection>;
  const assignedSection=isHost&&otherOwnedWorks.length>0&&<ContextSection title="Assigned Work" hint={`${ownedWorks.length} total`}>{otherOwnedWorks.slice(0,2).map(work=><ContextWorkRow key={work.work_id} data={data} work={work}/>)}</ContextSection>;
  const conversationSection=mode==="messages"&&(unreadIncoming.length>0||latestExchange)&&<ContextSection title="Conversation" hint={unreadIncoming.length?`${unreadIncoming.length} unread`:`${incoming.length} incoming`}>{latestExchange&&<ContextMessageRow data={data} message={latestExchange}/>}{hostInbox.filter(message=>message.message_id!==latestExchange?.message_id).map(message=><ContextMessageRow key={message.message_id} data={data} message={message}/>)}</ContextSection>;
  const evidenceSection=evidenceWorks.some(work=>work.work_id===anchoredWork?.work_id)&&<ContextSection title="Evidence"><ContextWorkRow data={data} work={evidenceWorks.find(work=>work.work_id===anchoredWork?.work_id)!} evidence/></ContextSection>;
  const privateProjection=data.persisted_session_projection;
  const sessionSection=<ContextSection title="Current Session" hint={data.selected_agent.is_host&&data.selected_agent.host_session_mode==="external_interactive"?"External · unmanaged":privateProjection.available?humanizeToken(data.selected_agent.runtime_status??"available"):"Unavailable"}><ContextFact label="Provider" value={data.selected_agent.provider?humanizeToken(data.selected_agent.provider):"Not bound"}/><ContextFact label="Session" value={shortId(data.current_session?.agent_session_id)}/><ContextFact label="Persisted rows" value={String(privateProjection.available?privateProjection.records.length:0)}/><ContextFact label="Last activity" value={formatTime(data.context_summary.last_activity_at)}/></ContextSection>;
  const controlsSection=prioritizedActions.some(action=>!action.disabled_reason)&&<ContextSection title={isHost&&anchoredNeedsJudgment?"Decision actions":"Next"}><WorkspaceActionIndex label={isHost&&anchoredNeedsJudgment?"Resolve in composer":isHost?"Available Host Controls":"Available Controls"} actions={prioritizedActions.filter(action=>!action.disabled_reason).slice(0,6).map(action=>({key:action.kind,label:actionLabel(action.kind)}))}/></ContextSection>;
  const projectionDetails=<details className="mt-5 border-t border-border pt-4"><summary className="cursor-pointer text-[10px] font-semibold text-muted-foreground">Projection · {view.freshness} · seq {view.as_of_event_sequence}</summary><div className="mt-3"><ViewProvenance view={view}/></div></details>;
  const memberRunSection=data.selected_agent.current_member_run_ref&&<ContextSection title="Current MemberRun"><ContextFact label="Generation" value={`${data.selected_agent.current_member_run_ref}${runGeneration!=null?` (gen ${runGeneration})`:""}`}/>{executionDriver&&<ContextFact label="Execution driver" value={executionDriver}/>}<ContextFact label="Assigned work" value={String(ownedWorks.length)}/>{data.selected_agent.runtime_status&&<ContextFact label="Runtime" value={humanizeToken(data.selected_agent.runtime_status)}/>}</ContextSection>;
  const truth=data.runtime_truth;
  const runtimeTruthSection=<ContextSection title="Runtime truth" primary><p className="aw-context-focus-copy">{truth.explanation}</p><div className="mt-3"><ContextFact label="Work" value={`${humanizeToken(truth.work.phase)} · ${humanizeToken(truth.work.condition)}`}/><ContextFact label="Coordination" value={humanizeToken(truth.coordination.state)}/><ContextFact label="Harness control" value={humanizeToken(truth.harness_control.state)} canonical={truth.harness_control.reason_code}/><ContextFact label="Native activity" value={`${humanizeToken(truth.provider_native_activity.state)}${truth.provider_native_activity.observed_after_control_loss?" · after control loss":""}`}/>{truth.harness_control.occurred_at&&<ContextFact label="Control boundary" value={formatTime(truth.harness_control.occurred_at)}/>}<ContextFact label="Next action" value={truth.harness_control.next_action}/>{truth.harness_control.last_command&&<ContextFact label="Last controlled command" value={`${humanizeToken(truth.harness_control.last_command.command)} · ${humanizeToken(truth.harness_control.last_command.status)}`} canonical={truth.harness_control.last_command.id}/>}</div></ContextSection>;
  const sections=mode==="messages"
    ?[runtimeTruthSection,workSection,selectionInset,conversationSection,evidenceSection,controlsSection,memberRunSection,sessionSection]
    :mode==="work"
      ?[runtimeTruthSection,workSection,selectionInset,responsibilitySection,evidenceSection,needsHostSection,assignedSection,controlsSection,memberRunSection,sessionSection]
      :[runtimeTruthSection,workSection,selectionInset,responsibilitySection,needsHostSection,inboxSection,assignedSection,evidenceSection,memberRunSection,sessionSection,controlsSection];
  return <ScrollArea.Root className="h-full min-w-0 overflow-hidden"><ScrollArea.Viewport className="size-full min-w-0 [&>div]:!block [&>div]:!min-w-0"><div className="aw-context-story min-w-0 overflow-hidden px-5 pb-8 pt-5">
    <p className="aw-context-story__eyebrow">{isHost?"Host operations":"Agent operations"}</p>
    {sections.map((section,index)=><Fragment key={index}>{section}</Fragment>)}
    {projectionDetails}
  </div></ScrollArea.Viewport><ScrollArea.Scrollbar orientation="vertical" className="flex w-2 p-0.5"><ScrollArea.Thumb className="rounded-full bg-border"/></ScrollArea.Scrollbar></ScrollArea.Root>;
}

function WorkContext({work,title,onOpenWork}:{work?:WorkSummary;title:string;onOpenWork:(work?:WorkSummary)=>void}){return <ContextSection title={title} primary>{work?<><div><ContextFact label="Work ID" value={work.work_id}/><ContextFact label="Revision" value={`rev ${work.work_revision} (latest)`}/><div className="aw-fact-row"><span>Phase</span><strong><WorkspaceState label={humanizeToken(work.phase)} tone={work.phase==="active"?"good":work.phase==="review"?"warn":"muted"}/></strong></div><div className="aw-fact-row"><span>Condition</span><strong><WorkspaceState label={humanizeToken(String(work.condition))} tone={work.condition==="blocked"?"bad":work.condition==="normal"?"muted":"warn"}/></strong></div>{work.condition==="blocked"&&work.blocker_reason&&<ContextFact label="Blocker" value={work.blocker_reason}/>}{work.resolution&&<ContextFact label="Resolution" value={humanizeToken(String(work.resolution))}/>}<ContextFact label="Gates" value={`${work.gate_summary.passed}/${work.gate_summary.required}`}/></div><button type="button" className="aw-context-link" onClick={()=>onOpenWork(work)}>Open work ↗</button></>:<p className="text-[11px] leading-5 text-muted-foreground">No current Work is projected for this Agent.</p>}</ContextSection>}
function MessageContext({data,message}:{data:AgentWorkspaceData;message:MessageSummary}){
  const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);
  const providerReceipts=message.deliveries.flatMap(item=>item.provider_receipt_id?[item.provider_receipt_id]:[]);
  return <ContextSection title="Message in focus" hint={formatTime(message.created_at)}><p className="aw-context-focus-title">{message.sender.display_name??actor?.display_name??message.sender.id}</p><p className="aw-context-focus-copy">{message.body}</p><div className="mt-3"><ContextFact label="Harness Message delivery" value={messageDeliveryLabel(message)}/><ContextFact label="Provider receipt" value={providerReceipts.length?`${providerReceipts.length} recorded`:"Not recorded"} canonical={providerReceipts.join(", ")||undefined}/><ContextFact label="Response intent" value={humanizeToken(message.response_intent)}/><ContextFact label="Conversation" value={shortId(message.correlation_id)} canonical={message.correlation_id}/>{message.causation_id&&<ContextFact label="Reply to Message" value={shortId(message.causation_id)} canonical={message.causation_id}/>} {message.work_id&&<><ContextFact label="Work context only" value={shortId(message.work_id)} canonical={message.work_id}/><p className="aw-context-boundary-copy">This link adds reading context. It does not mutate Work or prove a Member Result or Host acceptance.</p></>}</div></ContextSection>;
}
function EventContext({record,fragment}:{record:ProviderNativeEventRecord;fragment:ProviderEventFragment}){return <ContextSection title="Native event in focus" hint={formatTime(recordTime(record))}><p className="aw-context-focus-title">{humanizeToken(fragment.semantic_kind)}</p><FragmentBody fragment={fragment}/><details className="mt-3"><summary>Original provider-native record</summary><NativeEventBody value={record.native_event}/></details><div className="mt-3"><ContextFact label="Provider" value={humanizeToken(record.provider)}/><ContextFact label="Native session" value={record.provider_thread_id??record.agent_session_id}/><ContextFact label="Source" value={record.native_source_ref}/><ContextFact label="Lifecycle" value={humanizeToken(fragment.lifecycle_phase)}/><ContextFact label="Completeness" value={humanizeToken(fragment.completeness)}/></div></ContextSection>}
function ToolContext({episode}:{episode:ToolEpisode}){const record=episode.occurrences[0]!.record;return <ContextSection title="Tool call in focus" hint={formatTime(recordTime(record))}><p className="aw-context-focus-title">{episode.tool_name??"Unknown tool"}</p>{episode.primary_target&&<p className="aw-context-focus-summary">{episode.primary_target}</p>}<ToolEpisodeDetails episode={episode} context/></ContextSection>}
function WorkSelectionContext({data,work}:{data:AgentWorkspaceData;work:WorkSummary}){
  const owner=work.owner_actor_ref?data.roster.find(item=>item.agent_member_ref.id===work.owner_actor_ref!.id):undefined;
  const runtimeState=typeof work.runtime_summary.state==="string"?work.runtime_summary.state:null;
  const runtimeMeaningful=runtimeState&&!["","none","null","not_modeled","not_projected","unknown"].includes(runtimeState)?runtimeState:null;
  const runtimeGeneration=typeof work.runtime_summary.generation==="number"?work.runtime_summary.generation:null;
  const delivery=Object.entries(work.delivery_summary).filter(([,value])=>typeof value==="number"&&value>0).map(([key,value])=>`${humanizeToken(key)} ${value}`).join(" · ");
  const latestEventActor=work.latest_event?.actor_ref?data.roster.find(item=>item.agent_member_ref.id===work.latest_event!.actor_ref!.id)?.display_name??work.latest_event!.actor_ref!.id:null;
  const hostAcceptance=work.resolution==="accepted"?"Accepted":work.phase==="review"?"Pending Host decision":"Not accepted";
  return <ContextSection title="Work in focus" hint={formatTime(work.updated_at)}><p className="aw-context-focus-title">{work.title||work.work_id}</p><div className="mt-3"><ContextFact label="Revision" value={`rev ${work.work_revision} (latest)`}/><ContextFact label="Owner" value={owner?.display_name??(work.owner_actor_ref?"Assigned":"Unassigned")} canonical={work.owner_actor_ref?.id}/>{runtimeMeaningful&&<ContextFact label="Runtime" value={`${humanizeToken(runtimeMeaningful)}${runtimeGeneration!=null?` (gen ${runtimeGeneration})`:""}`}/>}<div className="aw-fact-row"><span>Condition</span><strong><WorkspaceState label={humanizeToken(String(work.condition))} tone={work.condition==="blocked"?"bad":work.condition==="normal"?"muted":"warn"}/></strong></div>{work.condition==="blocked"&&work.blocker_reason&&<ContextFact label="Blocker" value={work.blocker_reason}/>}<ContextFact label="Member Result" value={work.latest_report_ref?work.result_summary?`Submitted · ${work.result_summary}`:"Submitted":"Not submitted"} canonical={work.latest_report_ref??undefined}/><ContextFact label="Host acceptance" value={hostAcceptance}/><ContextFact label="Gates" value={`${work.gate_summary.passed}/${work.gate_summary.required}`}/>{delivery&&<ContextFact label="Work execution delivery" value={delivery}/>}{meaningfulRecovery(work)&&<ContextFact label="Delivery recovery" value={humanizeToken(String(work.delivery_summary.recovery_class))}/>}{work.latest_event&&<ContextFact label="Latest Work event" value={`${humanizeToken(work.latest_event.kind)}${latestEventActor?` · ${latestEventActor}`:""} · ${formatTime(work.latest_event.created_at)}`}/>}</div></ContextSection>;
}

function AgentComposer({data,actions,actionsCurrent,selectedRunId,onAction,onCompleted}:{data:AgentWorkspaceData;actions:AllowedAction[];actionsCurrent:boolean;selectedRunId:string|null;onAction:RoleActionExecutor;onCompleted:()=>void}){
  const usable=actions.filter(action=>action.kind!=="send_message"&&action.kind!=="reply_message");
  const [selectedKey,setSelectedKey]=useState(usable[0]?keyForAction(usable[0]):"");
  const selected=usable.find(action=>keyForAction(action)===selectedKey)??usable[0];
  useEffect(()=>{if(selectedKey&&!usable.some(action=>keyForAction(action)===selectedKey))setSelectedKey(usable[0]?keyForAction(usable[0]):"");},[selectedKey,usable]);
  if(!actionsCurrent)return <div className="agent-workspace-composer shrink-0 border-t border-border bg-background/95 px-4 py-3 text-xs text-muted-foreground" role="status">Authoritative Agent Workspace refresh is pending or failed. Action writes are unavailable.</div>;
  const actionControl=<label className="flex min-w-0 items-center gap-2 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground"><SlidersHorizontal className="size-3 shrink-0 text-primary"/><span className="sr-only">Action</span><span className="aw-command-action__select"><select aria-label="Composer action" value={selected?keyForAction(selected):""} onChange={event=>setSelectedKey(event.target.value)} title={selected?.disabled_reason??actionLabel(selected?.kind??"")}><option value="" disabled>No Work or runtime action authorized</option>{usable.map(action=><option key={keyForAction(action)} value={keyForAction(action)} disabled={Boolean(action.disabled_reason)}>{actionLabel(action.kind)}</option>)}</select><ChevronDown aria-hidden="true"/></span></label>;
  return <div data-testid="agent-workspace-composer" data-composer-kind={selected?"action":"deferred"} className="agent-workspace-composer shrink-0 border-t border-border bg-background/95"><div className="mx-auto max-w-4xl px-4 py-3"><p className="aw-composer-boundary"><MessageSquare aria-hidden="true"/>Member messaging composer is a later capability. This workspace currently reads canonical Messages without inventing a writable conversation model.</p>{selected?<><div className="mb-2 mt-2">{actionControl}</div><RoleActionPanel compact actions={[selected]} onAction={onAction} context={{teamId:data.team.team_id,teamRunId:data.team.latest_run_id??undefined}} actionsCurrent={actionsCurrent} onCompleted={onCompleted}/>{selected.target_ref.kind==="member_run"&&selectedRunId&&selected.target_ref.id!==selectedRunId&&<p className="mt-2 text-[10px] text-status-warn">This action targets a different MemberRun and is not executed from this selected Agent context.</p>}</>:<p className="mt-2 text-xs text-muted-foreground">No canonical Work or runtime action is authorized for this identity and state.</p>}</div></div>;
}

function ProfileDialog({data,onClose,closeRef,openerRef}:{data:AgentWorkspaceData;onClose:()=>void;closeRef:React.RefObject<HTMLButtonElement>;openerRef:React.RefObject<HTMLButtonElement>}){
  const selected=data.selected_agent,c=data.configuration;
  const currentSession=data.current_session??null;
  const hasProviderConfiguration=Boolean(selected.provider||selected.execution_mode||c.provider_profile_ref||c.permission_ceiling||c.workspace_policy);
  const sessionProjection=data.persisted_session_projection;
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
  return <div className="fixed inset-0 z-50 bg-[#3b2f27]/12" role="presentation" onMouseDown={event=>{if(event.target===event.currentTarget)onClose();}}><section ref={dialogRef} role="dialog" aria-modal="true" aria-label={`${selected.display_name} configuration`} tabIndex={-1} className="agent-profile-drawer absolute inset-y-0 right-0 w-[min(92vw,29rem)] overflow-y-auto border-l border-border bg-background">
    <header className="sticky top-0 z-10 flex min-h-14 items-center gap-3 border-b border-border bg-background/95 px-5 py-2"><Avatar name={selected.display_name} identity={`${selected.agent_member_ref.id} ${selected.role}`} size="lg" tone="running"/><div className="min-w-0 flex-1"><h2 className="truncate text-xl font-semibold tracking-[-0.02em]">{selected.display_name}</h2><p className="mt-0.5 text-[10px] text-muted-foreground">{humanizeToken(selected.role)} · durable AgentMember</p></div><Button ref={closeRef} size="icon" variant="ghost" onClick={onClose} aria-label="Close Agent configuration"><X className="size-4"/></Button></header>
    <div className="space-y-7 px-6 py-6"><ProfileSection title="Who"><ContextFact label="Role" value={humanizeToken(selected.role)} canonical={selected.role}/><ContextFact label="Member lifecycle" value={humanizeToken(selected.organization_status)} canonical={selected.organization_status}/>{c.description&&<p className="aw-profile-description">{c.description}</p>}<p className="aw-profile-canonical" title={selected.agent_member_ref.id}>Durable AgentMember · {shortId(selected.agent_member_ref.id)}</p></ProfileSection>
      <ProfileSection title="Authority">{hasProviderConfiguration?<><ContextFact label="Durable permission ceiling" value={c.permission_ceiling?humanizeToken(c.permission_ceiling):"Not projected"} canonical={c.permission_ceiling??undefined}/><ContextFact label="Effective Session permission" value={c.effective_permission_ceiling?humanizeToken(c.effective_permission_ceiling):"No active Session"} canonical={c.effective_permission_ceiling??undefined}/><ContextFact label="Resolved cwd" value={c.resolved_workspace_cwd??"No active Session"} canonical={c.resolved_workspace_cwd??undefined}/><ContextFact label="Workspace policy" value={c.workspace_policy?humanizeToken(c.workspace_policy):"Not projected"} canonical={c.workspace_policy??undefined}/>{c.prompt_ref&&<ContextFact label="Prompt reference" value={c.prompt_ref}/>} {c.forbidden_actions.length>0&&<ProfileList label="Forbidden actions" values={c.forbidden_actions} humanize empty=""/>}</>:<p className="aw-profile-empty">No execution authority is projected for this Agent.</p>}</ProfileSection>
      {(c.skill_refs.length>0||c.tool_refs.length>0||c.capabilities.length>0)&&<ProfileSection title="Capabilities">{c.skill_refs.length>0&&<ProfileList label="Skills" values={c.skill_refs} empty=""/>}{c.capabilities.length>0&&<ProfileList label="Capabilities" values={c.capabilities} humanize empty=""/>}{c.tool_refs.length>0&&<ProfileList label="Configured tools" values={c.tool_refs} humanize empty=""/>}</ProfileSection>}
      <ProfileSection title="Runtime">{hasProviderConfiguration?<><ContextFact label="Provider" value={currentSession?.provider?humanizeToken(currentSession.provider):selected.provider?humanizeToken(selected.provider):"Not bound"} canonical={currentSession?.provider??selected.provider??undefined}/><ContextFact label="Execution mode" value={currentSession?.native_session_ref?.execution_mode?humanizeToken(currentSession.native_session_ref.execution_mode):selected.execution_mode?humanizeToken(selected.execution_mode):"Not bound"} canonical={currentSession?.native_session_ref?.execution_mode??selected.execution_mode??undefined}/>{c.provider_profile_ref&&<ContextFact label="Provider profile" value={humanizeToken(c.provider_profile_ref)} canonical={c.provider_profile_ref}/>}</>:<p className="aw-profile-empty">No provider runtime is projected for this Agent.</p>}{currentSession&&<div className="aw-profile-current-session"><div><span>Current AgentSession</span><Badge tone={currentSession.lifecycle==="closed"?"muted":"good"}>{humanizeToken(currentSession.lifecycle)}</Badge></div><strong>{humanizeToken(currentSession.provider)} · {humanizeToken(currentSession.effective_permission_ceiling)}</strong><p><span title={currentSession.agent_session_id}>{shortId(currentSession.agent_session_id)}</span> · gen {currentSession.agent_session_generation} · {humanizeToken(currentSession.runtime_residency)}</p>{currentSession.native_session_open_target&&<a className="mt-2 inline-block text-xs font-semibold text-primary hover:underline" href={currentSession.native_session_open_target.uri}>Open exact native Session</a>}</div>}{c.workspace_binding&&<div className="mt-3"><ContextFact label="Workspace" value={c.workspace_binding.status?humanizeToken(c.workspace_binding.status):"Bound"} canonical={c.workspace_binding.id}/>{c.workspace_binding.locator&&<ContextFact label="Path" value={c.workspace_binding.locator}/>}</div>}</ProfileSection>
      <ProfileSection title="History">{sessionProjection.available?<><ContextFact label="Persisted native rows" value={String(sessionProjection.records.length)}/><ContextFact label="Source generation" value={shortId(sessionProjection.source_generation)}/><p className="aw-profile-empty">History remains provider-native and is read on demand. Opening or resuming a Session is a separate authorized action.</p></>:<p className="aw-profile-empty">{humanizeToken(sessionProjection.reason_code)}</p>}</ProfileSection>
    </div>
  </section></div>;
}

function MobileSheet({title,onClose,children}:{title:string;onClose:()=>void;children:React.ReactNode}){return <div className="aw-sheet-backdrop fixed inset-0 z-40 lg:hidden" onMouseDown={event=>{if(event.target===event.currentTarget)onClose();}}><section role="dialog" aria-modal="true" aria-label={title} className="aw-mobile-sheet absolute inset-y-0 right-0 w-[min(92vw,25rem)] overflow-y-auto border-l"><header className="sticky top-0 z-10 flex min-h-12 items-center justify-between border-b px-4"><h2 className="text-sm font-semibold">{title}</h2><Button size="icon" variant="secondary" onClick={onClose} aria-label={`Close ${title}`}><X className="size-4"/></Button></header>{children}</section></div>}
function ContextSection({title,hint,primary=false,children}:{title:string;hint?:string;primary?:boolean;children:React.ReactNode}){return <WorkspaceSection title={title} hint={hint} primary={primary}>{children}</WorkspaceSection>}
function ProfileSection({title,children}:{title:string;children:React.ReactNode}){const Icon=title==="Who"?UserRound:title==="Authority"?KeyRound:title==="Capabilities"?Wrench:title==="Runtime"?Activity:History;return <section className="aw-profile-section"><h3><Icon aria-hidden="true"/>{title}</h3><div>{children}</div></section>}
function ContextFact({label,value,canonical}:{label:string;value:string;canonical?:string}){return <WorkspaceFact label={label} value={value} canonicalValue={canonical}/>}
function ResponsibilityStrip({values}:{values:{open:number;active:number;review:number;closed:number}}){return <div className="aw-responsibility-strip" aria-label={`${values.open} open, ${values.active} active, ${values.review} in review, ${values.closed} closed`}>{Object.entries(values).map(([label,value])=><div key={label}><span>{label}</span><strong>{value}</strong></div>)}</div>}
function ContextWorkRow({data,work,evidence=false}:{data:AgentWorkspaceData;work:WorkSummary;evidence?:boolean}){const owner=data.roster.find(agent=>agent.agent_member_ref.id===work.owner_actor_ref?.id);return <div className="border-t border-border/70 py-2.5 first:border-t-0"><div className="flex items-start justify-between gap-3"><p className="aw-context-work-row-title min-w-0 flex-1 break-words text-[11.5px] font-semibold leading-[1.4]">{work.title||work.work_id}</p><span className="shrink-0 text-[10.5px] text-muted-foreground">{humanizeToken(evidence?(work.latest_report_ref?"report":work.latest_finding_refs.length?"finding":work.latest_failure_ref?"failure":"evidence"):work.phase)}</span></div><p className="aw-context-work-row-meta mt-1 break-words text-[10.5px] leading-4 text-muted-foreground">{owner?.display_name??(work.owner_actor_ref?"Assigned":"Unassigned")} · gates {work.gate_summary.passed}/{work.gate_summary.required}</p></div>}
function ContextMessageRow({data,message}:{data:AgentWorkspaceData;message:MessageSummary}){const actor=data.roster.find(item=>item.agent_member_ref.id===message.sender.id);return <div className="border-t border-border/70 py-2.5 first:border-t-0"><div className="flex items-baseline justify-between gap-3"><b className="truncate text-[11.5px]">{actor?.display_name??message.sender.id}</b><time className="shrink-0 text-[10.5px] text-muted-foreground">{formatTime(message.created_at)}</time></div><p className="mt-1 line-clamp-1 text-[10.5px] text-muted-foreground">{message.body}</p></div>}
function TagList({values,empty,humanize=false}:{values:string[];empty:string;humanize?:boolean}){return values.length?<div className="aw-profile-token-list">{values.map(value=><span key={value} title={value} className="aw-profile-token">{humanize?humanizeToken(value):value}</span>)}</div>:<p className="aw-profile-empty">{empty}</p>}
function ProfileList({label,values,empty,humanize=false}:{label:string;values:string[];empty:string;humanize?:boolean}){return <div className="mt-3 first:mt-0"><p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</p><TagList values={values} empty={empty} humanize={humanize}/></div>}
function EmptyCanvas({title,detail,compact=false}:{title:string;detail:string;compact?:boolean}){return <div className="aw-empty-state" data-compact={compact||undefined}><Inbox aria-hidden="true"/><div><h3>{title}</h3><p>{detail}</p></div></div>}
function actionLabel(kind:string){return ({send_message:"Send message",assign_work:"Assign work",interrupt_member_run:"Interrupt current turn",close_member_run:"Close member run",reopen_member_run:"Reopen member run",retire_member_run:"Retire agent from team",resume_native_session:"Resume native session",reconcile_message_delivery:"Reconcile message delivery",request_gate_evaluation:"Request gate review",request_changes:"Request work changes",accept_work:"Accept work",cancel_work:"Cancel work"} as Record<string,string>)[kind]??kind.replace(/_/g," ")}
function decisionActionRank(kind:string,work?:WorkSummary){if(work?.phase==="review"){if(kind==="accept_work")return 0;if(kind==="request_changes")return 1;if(kind==="request_gate_evaluation")return 2;}if(work?.condition==="blocked"&&/reconcile|resume/.test(kind))return 0;if(kind==="send_message")return 3;if(kind==="assign_work")return 4;if(/close|retire|cancel/.test(kind))return 9;return 5}
function keyForAction(action:AllowedAction){return `${action.kind}:${action.target_ref.kind}:${action.target_ref.id}`}
function meaningfulRecovery(work:WorkSummary){const value=work.delivery_summary.recovery_class;return typeof value==="string"&&!['','none','null','not_modeled','not_projected'].includes(value)}
function workVisualRank(work:WorkSummary,currentWorkId:string|null){if(work.work_id===currentWorkId)return 0;if(work.condition==="blocked")return 1;if(work.phase==="review")return 2;if(work.phase==="active")return 3;if(work.phase==="open")return 4;return 5}
function workGroupLabel(work:WorkSummary,current:boolean,lens:string){if(lens!=="current")return humanizeToken(lens);if(current)return "Current Work";if(work.condition==="blocked")return "Blocked";if(work.phase==="review")return "Awaiting review";if(work.phase==="active")return "Active responsibility";return "Open responsibility"}
function shortId(value:string|null|undefined){if(!value)return "Not linked";return value.length>24?`${value.slice(0,12)}…${value.slice(-7)}`:value}
function revalidateContextSelection(current:ContextSelection,data:AgentWorkspaceData):ContextSelection{
  if(!current)return null;
  if(current.kind==="message"){const message=data.messages.find(item=>item.message_id===current.message.message_id);return message?{kind:"message",message}:null;}
  if(current.kind==="work"){const work=data.works.find(item=>item.work_id===current.work.work_id);return work?{kind:"work",work}:null;}
  const projection=data.persisted_session_projection;
  if(projection.available){
    if(current.kind==="tool"){const episode=projectProviderTimeline(projection.records).find(item=>item.kind==="tool_episode"&&item.episode_id===current.episode.episode_id);return episode?.kind==="tool_episode"?{kind:"tool",episode}:null;}
    for(const record of projection.records){const fragment=record.fragments.find(item=>item.fragment_id===current.fragment.fragment_id);if(fragment)return{kind:"event",record,fragment};}
  }
  return null;
}
type SessionMessageRow={kind:"message";at:string;message:MessageSummary;continuation:boolean};
type SessionProviderRow={kind:"provider";at:string;record:ProviderNativeEventRecord;item:ProviderTimelineItem};
type SessionBoundaryRow={kind:"control_boundary";at:string};
function mergeSessionRows(messages:SessionMessageRow[],nativeEvents:SessionProviderRow[]):Array<SessionMessageRow|SessionProviderRow>{
  const rows:Array<SessionMessageRow|SessionProviderRow>=[];
  let messageIndex=0;
  for(const nativeEvent of nativeEvents){
    const providerTime=recordTime(nativeEvent.record);
    if(providerTime){
      while(messageIndex<messages.length&&timestampKey(messages[messageIndex]!.at)<=timestampKey(providerTime)){
        rows.push(messages[messageIndex]!);
        messageIndex+=1;
      }
    }
    rows.push(nativeEvent);
  }
  while(messageIndex<messages.length){rows.push(messages[messageIndex]!);messageIndex+=1;}
  return rows;
}
function providerTimelineRecord(item:ProviderTimelineItem){return item.kind==="tool_episode"?item.occurrences[0]!.record:item.record}
function providerTimelineObservedAfter(item:ProviderTimelineItem,boundaryAt:string){return item.kind==="tool_episode"?item.occurrences.some(({record})=>record.observed_at>boundaryAt):item.record.observed_at>boundaryAt}
function providerTimelineId(item:ProviderTimelineItem){return item.kind==="tool_episode"?item.episode_id:`native:${item.fragment.fragment_id}`}
function toolEpisodeTime(episode:ToolEpisode){const times=episode.occurrences.map(({record})=>recordTime(record)).filter((value):value is string=>Boolean(value));if(times.length)return times.length===1?formatTime(times[0]):`${formatTime(times[0])} – ${formatTime(times[times.length-1])}`;const positions=episode.occurrences.map(({record})=>record.ordering_key.value);return positions.length===1?`source #${positions[0]}`:`source #${positions[0]}–${positions[positions.length-1]}`}
function humanizeToken(value:string){return value.split(/[_-]+/).filter(Boolean).map((part,index)=>index===0?`${part.charAt(0).toUpperCase()}${part.slice(1)}`:part).join(" ")}
function rosterStateTone(state:string){if(/running|active/.test(state))return "text-status-good";if(/wait|pending|review/.test(state))return "text-status-warn";if(/block/.test(state))return "text-status-bad";return "text-muted-foreground";}
function rosterStateLabel(agent:AgentWorkspaceRosterItem){const state=agent.runtime_state??agent.capacity??"unknown";if(agent.is_host&&agent.host_session_mode==="external_interactive"&&/running|active/.test(state))return{word:"External · unmanaged",tone:"text-status-warn"};return{word:humanizeToken(state),tone:rosterStateTone(state)};}
function selectedRosterStateLabel(data:AgentWorkspaceData){const control=data.runtime_truth.harness_control.state;const native=data.runtime_truth.provider_native_activity.state;return{word:`Harness ${humanizeToken(control)} · native ${humanizeToken(native)}`,tone:control==="running"||control==="ready"?"text-status-good":control==="blocked"||control==="recovery_required"?"text-status-warn":"text-muted-foreground"};}
function selectedAvatarTone(data:AgentWorkspaceData){const control=data.runtime_truth.harness_control.state;return control==="running"?"running":control==="ready"?"good":"idle";}
function timestampKey(value:string|null|undefined){if(!value)return 0;if(value.startsWith("unix-ms:")){const parsed=Number(value.slice(8));return Number.isFinite(parsed)?parsed:0;}const parsed=Date.parse(value);return Number.isFinite(parsed)?parsed:0}
function recordTime(record:ProviderNativeEventRecord){const value=record.occurred_at;if(!value)return null;if(value.startsWith("unix-ms:")){const parsed=Number(value.slice(8));return Number.isFinite(parsed)?value:null;}if(!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value))return null;return Number.isFinite(Date.parse(value))?value:null}
function compareOrderingKey(left:ProviderNativeEventRecord["ordering_key"],right:ProviderNativeEventRecord["ordering_key"]){return left.kind===right.kind?left.value-right.value:left.kind.localeCompare(right.kind)}
function fragmentPresentationKind(fragment:ProviderEventFragment){if(fragment.semantic_kind==="reasoning")return "thinking";if(fragment.semantic_kind.startsWith("tool_call_"))return "tool";if(fragment.semantic_kind==="assistant_response")return "message";if(fragment.semantic_kind==="artifact_created")return "artifact";return "runtime"}
function fragmentStatus(fragment:ProviderEventFragment){if(fragment.semantic_kind==="tool_call_failed"||fragment.semantic_kind==="turn_failed")return "failed";if(fragment.lifecycle_phase==="terminal")return "completed";return "running"}
type AvailablePersistedSessionProjection=Extract<PersistedSessionProjection,{available:true}>;
function normalizePersistedSessionResponse(value:unknown,sessionId:string,sessionGeneration:number):AvailablePersistedSessionProjection|null{
  if(!value||typeof value!=="object")return null;
  const response=value as Partial<AvailablePersistedSessionProjection>;
  if(response.schema_version!=="agentfirm.native_session_read.v1"||!Array.isArray(response.records)||typeof response.source_generation!=="string")return null;
  const records=response.records.filter(record=>record.schema_version==="agentfirm.provider_native_event_record.v3"&&record.agent_session_id===sessionId&&record.agent_session_generation===sessionGeneration);
  if(records.length!==response.records.length)return null;
  return {...response,available:true,records} as AvailablePersistedSessionProjection;
}
function mergePersistedSessionProjection(current:PersistedSessionProjection|null,incoming:PersistedSessionProjection,mode:"head"|"older"):PersistedSessionProjection{
  if(mode==="older"&&current&&(!current.available||!incoming.available||current.source_generation!==incoming.source_generation))return current;
  if(!current||!current.available||!incoming.available||current.source_generation!==incoming.source_generation)return incoming;
  const records=new Map(current.records.map(record=>[record.record_id,record]));
  for(const record of incoming.records)records.set(record.record_id,record);
  return {...incoming,records:[...records.values()].sort((left,right)=>compareOrderingKey(left.ordering_key,right.ordering_key)),has_more:mode==="older"?incoming.has_more:current.has_more,next_before:mode==="older"?incoming.next_before:current.next_before};
}
function usePersistedSessionTimeline({apiUrl,space,project,company,teamId,agentId,sessionId,sessionGeneration,initialProjection}:{apiUrl:string;space:string;project:string;company?:string;teamId:string|null;agentId:string|null;sessionId:string|null;sessionGeneration:number|null;initialProjection:PersistedSessionProjection|null}){
  const [projection,setProjection]=useState<PersistedSessionProjection|null>(initialProjection);
  const [connectionState,setConnectionState]=useState<PersistedSessionConnectionState>("inactive");
  const identity=`${space}\u0000${project}\u0000${teamId??""}\u0000${agentId??""}\u0000${sessionId??""}\u0000${sessionGeneration??""}`;
  const initialProjectionRef=useRef(initialProjection);initialProjectionRef.current=initialProjection;
  useEffect(()=>{
    setProjection(initialProjectionRef.current);
    const token=window.__AGENTFIRM_BOOTSTRAP__?.capabilityToken;
    if(!sessionId||sessionGeneration==null||!teamId||!agentId){setConnectionState("inactive");return;}
    setConnectionState("connecting");
    const controller=new AbortController();
    const url=new URL("/v1/events",apiUrl.endsWith("/")?apiUrl:`${apiUrl}/`);
    url.searchParams.set("space",space);url.searchParams.set("project",project);url.searchParams.set("team_id",teamId);url.searchParams.set("agent_id",agentId);if(company)url.searchParams.set("company",company);
    void (async()=>{
      try{
        const headers:Record<string,string>={Accept:"text/event-stream"};if(token)headers["X-AgentFirm-Token"]=token;
        const response=await fetch(url,{headers,signal:controller.signal});
        if(!response.ok||!response.body)throw new Error(`Persisted native Session stream failed (${response.status})`);
        setConnectionState("connected");
        const reader=response.body.getReader(),decoder=new TextDecoder();let buffer="";
        while(true){const {done,value}=await reader.read();if(done)break;buffer+=decoder.decode(value,{stream:true});let boundary=buffer.indexOf("\n\n");while(boundary>=0){const block=buffer.slice(0,boundary).replace(/\r/g,"");buffer=buffer.slice(boundary+2);boundary=buffer.indexOf("\n\n");const eventName=block.split("\n").find(line=>line.startsWith("event:"))?.slice(6).trim();if(!["native_session_snapshot","native_session_append","native_session_source_reset"].includes(eventName??""))continue;const data=block.split("\n").filter(line=>line.startsWith("data:")).map(line=>line.slice(5).trimStart()).join("\n");if(!data)continue;const next=normalizePersistedSessionResponse(JSON.parse(data),sessionId,sessionGeneration);if(!next)continue;if(eventName==="native_session_source_reset")setProjection(next);else setProjection(current=>mergePersistedSessionProjection(current,next,"head"));}}
        if(!controller.signal.aborted)setConnectionState("disconnected");
      }catch(error){if(!controller.signal.aborted){setConnectionState("disconnected");console.warn("Agent Workspace persisted Session stream disconnected",error);}}
    })();
    return()=>controller.abort();
  },[apiUrl,space,project,company,teamId,agentId,sessionGeneration,sessionId,identity]);
  useEffect(()=>{if(initialProjection)setProjection(current=>mergePersistedSessionProjection(current,initialProjection,"head"));},[initialProjection]);
  return {projection,connectionState,mergeOlder:(older:PersistedSessionProjection)=>setProjection(current=>mergePersistedSessionProjection(current,older,"older"))};
}
function formatTime(value:string|null|undefined){if(!value)return "unknown";const timestamp=timestampKey(value);if(!timestamp)return value;return new Date(timestamp).toLocaleString([], {month:"short",day:"numeric",hour:"2-digit",minute:"2-digit"})}
function activateOnKeyDown(event:React.KeyboardEvent<HTMLElement>,activate:()=>void){if(event.target!==event.currentTarget)return;if(event.key==="Enter"){event.preventDefault();activate();}else if(event.key===" ")event.preventDefault();}
function activateOnKeyUp(event:React.KeyboardEvent<HTMLElement>,activate:()=>void){if(event.target===event.currentTarget&&event.key===" "){event.preventDefault();activate();}}
