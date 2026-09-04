import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Activity, BriefcaseBusiness, MessageSquare, RefreshCw, ShieldCheck, Users } from "lucide-react";

import { Button } from "@/components/ui/button";
import { TeamCapacityStrip } from "@/components/workbench/team/TeamCapacityStrip";
import { TeamConversationStream } from "@/components/workbench/team/TeamConversation";
import { TeamMembersCapacity } from "@/components/workbench/team/TeamMembersCapacity";
import { TeamWorksBoard } from "@/components/workbench/team/TeamWorksBoard";
import { TeamInboxPanel } from "@/components/workbench/team/TeamInboxPanel";
import { AgentTeamTab, AgentTeamTabs } from "@/components/workbench/team/AgentTeamVisualPrimitives";
import { fetchRoleView, type MessageSummary, type RoleActionExecutor, type RoleView, type TeamWorkspaceData, type ViewerContextData, type ViewerContextTeam } from "../model/roleViews";
import type { SelectionState } from "../app/selection";
import { AttentionStrip, ViewProvenance, ViewState } from "./RoleViewPrimitives";
import { HostConsole } from "./HostConsole";
import { HostActivityComposer, HostActivityLeadInbox } from "./HostActivityComposer";
import { AgentConversationWorkspace } from "./AgentConversationWorkspace";

type TeamTab = "works" | "activity" | "members";
const TABS = [{id:"works",label:"Works",icon:BriefcaseBusiness},{id:"activity",label:"Activity",icon:Activity},{id:"members",label:"Members",icon:Users}] as const;

export function TeamWorkspace({apiUrl,space,project,company,teamId,teamRunId,refreshKey,selection,onAction,onSelectionChange,onSelectionReplace}:{apiUrl:string;space:string;project:string;company?:string;teamId:string;teamRunId?:string;refreshKey?:string;selection:SelectionState;onAction:RoleActionExecutor;onSelectionChange:(next:Partial<SelectionState>)=>void;onSelectionReplace:(next:Partial<SelectionState>)=>void}){
  const [view,setView] = useState<RoleView<TeamWorkspaceData>|null>(null);
  const [viewerContext,setViewerContext] = useState<RoleView<ViewerContextData>|null>(null);
  const [error,setError] = useState<string|null>(null);
  const [loading,setLoading] = useState(true);
  const [refetch,setRefetch] = useState(0);
  const [replyTo,setReplyTo] = useState<MessageSummary|null>(null);
  const workspaceScrollRef = useRef<HTMLElement>(null);
  const committedIdentityRef = useRef<string|null>(null);
  const committedViewerIdentityRef = useRef<string|null>(null);
  const observedRefreshKeyRef = useRef(refreshKey);
  const requestIdentity = `${apiUrl}\u0000${space}\u0000${project}\u0000${company??""}\u0000${teamId}`;
  const tab:TeamTab = selection.teamTab ?? "works";
  const hostMode = selection.teamMode === "host";
  const retry = () => setRefetch((value) => value+1);
  const openHostTools = (teamWorkId?:string) => {
    onSelectionChange({teamMode:"host",teamConversation:undefined,memberRunId:undefined,...(teamWorkId ? {teamWorkId} : {})});
    window.requestAnimationFrame(() => window.requestAnimationFrame(() => document.getElementById("host-tools")?.scrollIntoView({block:"start"})));
  };
  const selectTab = (teamTab:TeamTab) => {
    if(teamTab === "activity") workspaceScrollRef.current?.scrollTo({top:0,behavior:"auto"});
    onSelectionChange({teamTab});
    if(teamTab === "activity") window.requestAnimationFrame(() => workspaceScrollRef.current?.scrollTo({top:0,behavior:"auto"}));
  };
  useLayoutEffect(() => {
    if(tab === "activity") workspaceScrollRef.current?.scrollTo({top:0,behavior:"auto"});
  },[tab,teamId]);
  useEffect(() => {
    let live=true;
    const identityChanged=committedIdentityRef.current!==requestIdentity;
    setLoading(identityChanged);
    if(identityChanged){setView(null);setViewerContext(null);}
    setError(null);
    fetchRoleView<ViewerContextData>(apiUrl,"/v1/views/viewer-context",{space,project,company}).then(async (context) => {
      if(!live)return;
      const authorized=context.data.teams.find((team)=>team.team_id===teamId||team.team_run_ids.includes(teamId));
      const nextViewerIdentity=viewerIdentityKey(context.data,authorized);
      if(committedViewerIdentityRef.current!==null&&committedViewerIdentityRef.current!==nextViewerIdentity){
        committedIdentityRef.current=null;
        setLoading(true);
        setView(null);
      }
      setViewerContext(context);
      if(!authorized){
        if(context.data.teams.length===1){
          const target=context.data.teams[0];
          const conversationOpen=Boolean(selection.teamConversation||selection.memberRunId);
          onSelectionReplace({teamId:target.team_id,teamConversation:conversationOpen?target.default_conversation:undefined,memberRunId:conversationOpen?target.current_member_run_id??undefined:undefined,teamWorkId:undefined,agentSessionId:undefined});
        }
        return;
      }
      const value=await fetchRoleView<TeamWorkspaceData>(apiUrl,`/v1/views/team-workspace/${encodeURIComponent(teamId)}`,{space,project,company});
      if(live){committedIdentityRef.current=requestIdentity;committedViewerIdentityRef.current=nextViewerIdentity;setView(value);setError(null);}
    }).catch((reason) => { if(live)setError(String(reason)); }).finally(() => { if(live)setLoading(false); });
    return () => { live=false; };
  },[apiUrl,space,project,company,teamId,requestIdentity,refetch]);
  // Ambient snapshots may advance faster than an authenticated RoleView can
  // load. Never let that traffic repeatedly cancel the first request. Once
  // the stream is quiet, coalesce it into one background revalidation while
  // preserving the last committed TeamWorkspace truth.
  useEffect(() => {
    if(observedRefreshKeyRef.current===refreshKey)return;
    observedRefreshKeyRef.current=refreshKey;
    const timer=window.setTimeout(()=>setRefetch(value=>value+1),500);
    return()=>window.clearTimeout(timer);
  },[refreshKey]);
  const authority=viewerContext?.data.teams.find((team)=>team.team_id===teamId||team.team_run_ids.includes(teamId));
  const viewerIdentity=viewerContext?viewerIdentityKey(viewerContext.data,authority):"";
  const canonicalSelection=view&&authority&&view.data.team.team_id===authority.team_id?canonicalConversationSelection(selection,view.data,authority):null;
  useEffect(() => { if(canonicalSelection)onSelectionReplace(canonicalSelection); },[canonicalSelection?.teamConversation,canonicalSelection?.memberRunId]);
  if(viewerContext&&!authority&&viewerContext.data.teams.length>1)return <AuthorizedTeamChooser teams={viewerContext.data.teams} onChoose={(team)=>onSelectionReplace({teamId:team.team_id,teamConversation:undefined,memberRunId:undefined,teamWorkId:undefined,agentSessionId:undefined})}/>;
  if(viewerContext&&!authority&&viewerContext.data.teams.length===0)return <div className="h-full flex-1 overflow-y-auto"><ViewState loading={false} error="This authenticated AgentMember has no Team in the selected Execution Space." identityLabel="Agent Teams" onRetry={retry}>{null}</ViewState></div>;
  if(view&&authority&&view.data.team.team_id!==authority.team_id)return <div className="h-full flex-1 overflow-y-auto"><ViewState loading error={null} identityLabel={`Agent Team · ${teamId}`} onRetry={retry}>{null}</ViewState></div>;
  if (!view) return <div className="h-full flex-1 overflow-y-auto"><ViewState loading={loading} error={error} identityLabel={`Agent Team · ${teamId}`} onRetry={retry}>{null}</ViewState></div>;
  if(canonicalSelection)return <div className="h-full flex-1 overflow-y-auto"><ViewState loading error={null} identityLabel="authenticated Agent Workspace route" onRetry={retry}>{null}</ViewState></div>;

  const team = view.data.team;
  const resolvedTeamRunId = teamRunId ?? team.latest_run?.id;
  const conversationOpen=Boolean(selection.teamConversation || selection.memberRunId);
  if(conversationOpen) return <AgentConversationWorkspace apiUrl={apiUrl} space={space} project={project} company={company} routeIdentity={teamId} selection={selection} refreshKey={refreshKey} onAction={onAction} onSelectionChange={onSelectionChange}/>;
  if(hostMode && team.viewer_role === "host") return <main className="agent-team-surface h-full flex-1 overflow-y-auto p-3 sm:p-5"><div className="mx-auto max-w-[1500px]"><HostConsole embedded apiUrl={apiUrl} space={space} project={project} company={company} teamId={teamId} teamRunId={resolvedTeamRunId} selectedWorkId={selection.teamWorkId} refreshKey={refreshKey} onAction={onAction} onSelectionChange={onSelectionChange}/></div></main>;
  return <main ref={workspaceScrollRef} className="agent-team-surface h-full min-w-0 flex-1 overflow-y-auto px-3 py-3 sm:px-6 sm:py-5" data-testid="authenticated-team-workspace"><div className="mx-auto w-full min-w-0 max-w-[1500px]">
    <header className="border-b border-border px-1 pb-3"><div className="flex flex-wrap items-start justify-between gap-3"><div className="min-w-0"><div className="agent-team-eyebrow flex flex-wrap items-center gap-2"><Users className="size-3.5"/>Agent Team <span className="text-muted-foreground">· {team.viewer_role} view</span></div><h1 className="company-editorial-title mt-1 break-words text-2xl tracking-[-0.025em] sm:text-[30px]">{team.display_name || team.team_id}</h1><p className="mt-1 text-[12px] text-muted-foreground">Durable Team · node {team.node_id} <span className="px-1.5">·</span> latest attempt {team.latest_run?.id ?? "not started"}</p></div><div className="flex min-w-0 flex-wrap items-center justify-end gap-2"><Button aria-label="Host conversation" size="sm" variant="outline" onClick={() => onSelectionChange({teamMode:"workspace",teamConversation:"host",memberRunId:undefined,teamTab:"members"})}><MessageSquare className="size-3.5"/><span className="sm:hidden">Host chat</span><span className="hidden sm:inline">Host conversation</span></Button>{team.viewer_role === "host" && <Button aria-label="Open Host Console" size="sm" variant="outline" aria-controls="host-tools" onClick={() => openHostTools()}><ShieldCheck className="size-3.5"/><span className="sm:hidden">Host Console</span><span className="hidden sm:inline">Open Host Console</span></Button>}<details className="relative shrink-0"><summary className="flex min-h-8 cursor-pointer list-none items-center whitespace-nowrap rounded-md border border-border bg-card px-3 text-xs font-medium">Context</summary><div className="agent-team-panel absolute right-0 top-10 z-30 w-72 rounded-xl p-4 shadow-xl"><dl className="space-y-2 text-xs"><ContextFact label="Team" value={team.team_id}/><ContextFact label="Legacy Mission" value={team.mission_id || "none"}/><ContextFact label="Host (active membership)" value={team.host_agent_id}/><ContextFact label="Node" value={team.node_id}/><ContextFact label="Run" value={team.latest_run?.id ?? "not started"}/><ContextFact label="Project" value={team.latest_run?.project_binding_id ?? "not attached"}/></dl><p className="mt-3 border-t border-border pt-3 text-[10px] leading-relaxed text-muted-foreground">Mission Log owns Host judgment. Provider transcripts remain native.</p></div></details><span className="hidden min-w-0 overflow-hidden sm:inline-flex"><ViewProvenance view={view}/></span></div></div>
      <div className="mt-4 hidden border-t border-border pt-3 md:block lg:grid lg:grid-cols-[minmax(0,1.12fr)_minmax(30rem,1fr)] lg:gap-7"><dl className="grid grid-cols-5 gap-x-5 gap-y-3 text-[10px] text-muted-foreground"><HeaderFact label="Run status" value={team.latest_run?.status ?? "not started"}/><HeaderFact label="Attempt" value={team.latest_run?.id ?? "not started"}/><HeaderFact label="Placement" value={`${team.node_id}${team.placement_generation != null ? ` · g${team.placement_generation}` : ""}`}/><HeaderFact label="Project binding" value={team.latest_run?.project_binding_id ?? "not attached"}/><HeaderFact label="Workspace" value={team.latest_run?.execution_root ?? "not attached"}/></dl><TeamCapacityStrip summary={view.data.pressure_summary} compact className="mt-3 border-t border-border pt-3 lg:mt-0 lg:border-t-0 lg:pt-0"/></div>
    </header>
    {error && <div role="alert" className="my-3 flex flex-wrap items-center gap-2 rounded-lg border border-status-warn/35 bg-status-warn/10 p-3 text-xs"><span className="min-w-0 flex-1">Refresh failed. Showing the last authoritative view with mutations unavailable. {error}</span><Button size="sm" variant="secondary" onClick={retry}>Retry</Button></div>}
    {loading && <div role="status" className="my-3 flex items-center gap-2 text-xs text-muted-foreground"><RefreshCw className="size-3.5 animate-spin"/>Refreshing authenticated TeamWorkspace…</div>}
    <AttentionStrip view={view}/>
    <section className="my-3 grid grid-cols-3 divide-x divide-border border-y border-border bg-secondary/20" aria-label="Cross-machine collaboration projection"><PressureFact label="Delegations" value={view.data.collaboration?.delegations?.length ?? 0} tone=""/><PressureFact label="Publications" value={view.data.collaboration?.publication_count ?? 0} tone=""/><PressureFact label="Attention" value={view.data.collaboration?.attention_count ?? 0} tone={(view.data.collaboration?.attention_count ?? 0) ? "text-status-warn" : ""}/></section>
    <TeamCapacityStrip summary={view.data.pressure_summary} compact className="border-b border-border py-3 md:hidden"/>
    <div className="min-w-0">
    <AgentTeamTabs value={tab} onValueChange={selectTab} label="Team Workspace sections">{TABS.map(({id,label,icon:Icon}) => <AgentTeamTab key={id} id={`team-workspace-tab-${id}`} value={id} aria-controls={`team-workspace-panel-${id}`}><Icon className="size-3.5"/>{label}{id === "activity" && view.data.messages.length > 0 && <span className="rounded-full bg-primary/10 px-1.5 text-[9px] text-primary">{view.data.messages.length}</span>}</AgentTeamTab>)}</AgentTeamTabs>
    {tab === "works" && <div role="tabpanel" id="team-workspace-panel-works" aria-labelledby="team-workspace-tab-works"><TeamWorksBoard works={view.data.works} graph={view.data.work_graph ?? {nodes:view.data.works,edges:[],ready_work_ids:[],attention_work_ids:[]}} members={view.data.members} allowedActions={view.allowed_actions} teamId={team.team_id} actionsCurrent={!error && view.freshness === "current"} onAction={onAction} onCompleted={retry} viewMode={selection.teamWorkView ?? "graph"} onViewModeChange={(teamWorkView) => onSelectionChange({teamWorkView})} selectedWorkId={selection.teamWorkId} onSelectWork={(teamWorkId) => onSelectionChange({teamWorkId})} onOpenMember={(memberRunId) => { const member=view.data.members.find((candidate) => candidate.current_member_run_ref === memberRunId); onSelectionChange({teamMode:"workspace",teamConversation:member?.agent_member_ref.id,memberRunId,teamTab:"members"}); }} onOpenHost={team.viewer_role === "host" ? (teamWorkId) => openHostTools(teamWorkId) : undefined} onOpenHostTools={team.viewer_role === "host" ? () => openHostTools() : undefined} ownerFilter={selection.teamOwner ?? "all"} attentionFilter={selection.teamAttention ?? "all"} queryFilter={selection.teamQuery ?? ""} onFiltersChange={({owner,attention,query}) => onSelectionChange({teamOwner:owner === "all" ? undefined : owner,teamAttention:attention === "all" ? undefined : attention,teamQuery:query || undefined})}/></div>}
    {tab === "activity" && <div role="tabpanel" id="team-workspace-panel-activity" aria-labelledby="team-workspace-tab-activity" className="space-y-4"><ActivityPressureSummary review={view.data.pressure_summary.review_work} blocked={view.data.pressure_summary.blocked_work} responses={view.data.messages.filter((message) => message.reply_eligible).length}/><TeamInboxPanel apiUrl={apiUrl} space={space} project={project} teamId={teamId} viewerIdentity={viewerIdentity} refreshKey={refreshKey} onOpenWork={(teamWorkId) => onSelectionChange({teamTab:"works",teamWorkId})}/>{team.viewer_role === "host" && resolvedTeamRunId && <HostActivityLeadInbox apiUrl={apiUrl} space={space} project={project} routeIdentity={teamId} refreshKey={refreshKey} onReply={setReplyTo} onOpenWork={(teamWorkId) => onSelectionChange({teamTab:"works",teamWorkId})}/>}<TeamConversationStream activity={view.data.activity} messages={view.data.messages} members={view.data.members} truncated={view.data.activity_truncated} onOpenWork={(teamWorkId) => onSelectionChange({teamTab:"works",teamWorkId})} onReply={team.viewer_role === "host" ? setReplyTo : undefined}/>{team.viewer_role === "host" && resolvedTeamRunId && <details className="group border-y border-border" open={Boolean(replyTo)}><summary role="button" aria-label="Compose team message" className="flex min-h-12 cursor-pointer list-none items-center gap-2 px-1"><MessageSquare className="size-4 text-primary"/><h2 className="text-sm font-semibold group-open:hidden">Team message</h2><span className="text-[10px] text-muted-foreground group-open:hidden">Compose</span><span className="ml-auto text-[10px] text-muted-foreground">ordinary Message · not Steer</span></summary><div className="border-t border-border py-3"><HostActivityComposer apiUrl={apiUrl} space={space} project={project} routeIdentity={teamId} teamRunId={resolvedTeamRunId} replyTo={replyTo} refreshKey={refreshKey} onAction={onAction} onClearReply={() => setReplyTo(null)} collapsibleOnMobile={false}/></div></details>}</div>}
    {tab === "members" && <div role="tabpanel" id="team-workspace-panel-members" aria-labelledby="team-workspace-tab-members"><TeamMembersCapacity members={view.data.members} summary={view.data.pressure_summary} selectedMemberRunId={selection.memberRunId} onOpenMember={(memberRunId) => { const member=view.data.members.find((candidate) => candidate.current_member_run_ref === memberRunId); onSelectionChange({teamMode:"workspace",teamConversation:member?.agent_member_ref.id,memberRunId}); }}/></div>}
    </div>
  </div></main>;
}

function viewerIdentityKey(context:ViewerContextData,authority?:ViewerContextTeam):string {
  return `${context.viewer_actor_ref.kind}\u0000${context.viewer_actor_ref.id}\u0000${authority?.viewer_agent_member_id??""}`;
}

function canonicalConversationSelection(selection:SelectionState,data:TeamWorkspaceData,authority:ViewerContextTeam):Partial<SelectionState>|null {
  const members=data.members.map((member)=>member.agent_member_ref.id);
  let conversation=selection.teamConversation;
  if(!conversation&&selection.memberRunId){
    conversation=data.members.find((member)=>member.current_member_run_ref===selection.memberRunId)?.agent_member_ref.id;
  }
  if(conversation){
    if(conversation!=="host"&&!members.includes(conversation))conversation=authority.default_conversation;
  }
  const selectedAgentId=conversation==="host"?data.team.host_agent_id:conversation;
  const memberRunId=selectedAgentId===authority.viewer_agent_member_id
    ? authority.current_member_run_id??undefined
    : data.members.find((member)=>member.agent_member_ref.id===selectedAgentId)?.current_member_run_ref??undefined;
  if(selection.teamConversation===conversation&&selection.memberRunId===memberRunId)return null;
  return {teamConversation:conversation,memberRunId,agentSessionId:undefined};
}

function AuthorizedTeamChooser({teams,onChoose}:{teams:ViewerContextTeam[];onChoose:(team:ViewerContextTeam)=>void}) {
  return <main className="agent-team-surface h-full flex-1 overflow-y-auto p-6"><section className="mx-auto max-w-2xl rounded-xl border border-border bg-card p-6"><p className="agent-team-eyebrow">Available Team context</p><h1 className="mt-2 text-2xl font-semibold">Choose the Agent Team to open</h1><p className="mt-2 text-sm text-muted-foreground">The saved link belongs to another Team. Select one available to this local Operator or remote Team context.</p><div className="mt-5 grid gap-2">{teams.map((team)=><Button key={team.team_id} variant="outline" className="h-auto justify-start py-3 text-left" onClick={()=>onChoose(team)}><span><strong className="block">{team.display_name||team.team_id}</strong><span className="text-xs text-muted-foreground">{team.viewer_role} · {team.team_id}</span></span></Button>)}</div></section></main>;
}

function HeaderFact({label,value}:{label:string;value:string}) { return <div className="min-w-0"><dt className="uppercase tracking-wider">{label}</dt><dd className="mt-1 truncate font-medium text-foreground" title={value}>{value}</dd></div>; }

function ContextFact({label,value}:{label:string;value:string}) { return <div className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2"><dt className="text-muted-foreground">{label}</dt><dd className="truncate text-right font-medium" title={value}>{value}</dd></div>; }

function ActivityPressureSummary({review,blocked,responses}:{review:number;blocked:number;responses:number}) { return <section aria-label="Team activity pressure" className="mt-3 grid grid-cols-3 divide-x divide-border border-y border-border bg-secondary/20"><PressureFact label="Responses" value={responses} tone={responses ? "text-primary" : ""}/><PressureFact label="Needs review" value={review} tone={review ? "text-status-warn" : ""}/><PressureFact label="Blocked" value={blocked} tone={blocked ? "text-status-bad" : ""}/></section>; }
function PressureFact({label,value,tone}:{label:string;value:number;tone:string}) { return <div className="flex items-center justify-between gap-3 px-3 py-2"><span className="text-[10px] font-semibold uppercase tracking-[.09em] text-muted-foreground">{label}</span><strong className={`text-sm tabular-nums ${tone}`}>{value}</strong></div>; }
