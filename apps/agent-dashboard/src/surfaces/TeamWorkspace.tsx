import { useEffect, useState, type KeyboardEvent } from "react";
import { Activity, BriefcaseBusiness, MessageSquare, RefreshCw, ShieldCheck, Users } from "lucide-react";

import { Button } from "@/components/ui/button";
import { TeamCapacityStrip } from "@/components/workbench/team/TeamCapacityStrip";
import { TeamConversationStream } from "@/components/workbench/team/TeamConversation";
import { TeamMembersCapacity } from "@/components/workbench/team/TeamMembersCapacity";
import { TeamWorksBoard } from "@/components/workbench/team/TeamWorksBoard";
import { cn } from "@/lib/utils";
import { fetchRoleView, type MessageSummary, type RoleActionExecutor, type RoleView, type TeamWorkspaceData } from "../model/roleViews";
import type { SelectionState } from "../app/selection";
import { AttentionStrip, ViewProvenance, ViewState } from "./RoleViewPrimitives";
import { HostConsole } from "./HostConsole";
import { HostActivityComposer } from "./HostActivityComposer";
import { AgentConversationWorkspace } from "./AgentConversationWorkspace";

type TeamTab = "works" | "activity" | "members";
const TABS = [{id:"works",label:"Works",icon:BriefcaseBusiness},{id:"activity",label:"Activity",icon:Activity},{id:"members",label:"Members",icon:Users}] as const;

export function TeamWorkspace({apiUrl,space,project,teamId,teamRunId,refreshKey,selection,onAction,actionsCurrent,onSelectionChange}:{apiUrl:string;space:string;project:string;teamId:string;teamRunId?:string;refreshKey?:string;selection:SelectionState;onAction:RoleActionExecutor;actionsCurrent:boolean;onSelectionChange:(next:Partial<SelectionState>)=>void}){
  const [view,setView] = useState<RoleView<TeamWorkspaceData>|null>(null);
  const [error,setError] = useState<string|null>(null);
  const [loading,setLoading] = useState(true);
  const [refetch,setRefetch] = useState(0);
  const [replyTo,setReplyTo] = useState<MessageSummary|null>(null);
  const tab:TeamTab = selection.teamTab ?? "works";
  const hostMode = selection.teamMode === "host";
  const retry = () => setRefetch((value) => value+1);
  const openHostTools = (teamWorkId?:string) => {
    onSelectionChange({teamMode:"host",teamConversation:undefined,memberRunId:undefined,...(teamWorkId ? {teamWorkId} : {})});
    window.requestAnimationFrame(() => window.requestAnimationFrame(() => document.getElementById("host-tools")?.scrollIntoView({block:"start"})));
  };
  const selectAdjacentTab = (event: KeyboardEvent<HTMLButtonElement>, current: TeamTab) => {
    const index = TABS.findIndex(({id}) => id === current);
    const nextIndex = event.key === "ArrowRight" ? (index + 1) % TABS.length : event.key === "ArrowLeft" ? (index - 1 + TABS.length) % TABS.length : event.key === "Home" ? 0 : event.key === "End" ? TABS.length - 1 : -1;
    if (nextIndex < 0) return;
    event.preventDefault();
    const next = TABS[nextIndex].id;
    onSelectionChange({teamTab:next});
    requestAnimationFrame(() => document.getElementById(`team-workspace-tab-${next}`)?.focus());
  };
  useEffect(() => { let live=true; setLoading(true); fetchRoleView<TeamWorkspaceData>(apiUrl,`/v1/views/team-workspace/${encodeURIComponent(teamId)}`,{space,project}).then((value) => { if(live){ setView(value); setError(null); } }).catch((reason) => { if(live) setError(String(reason)); }).finally(() => { if(live) setLoading(false); }); return () => { live=false; }; },[apiUrl,space,project,teamId,refreshKey,refetch]);
  if (!view) return <div className="h-full flex-1 overflow-y-auto"><ViewState loading={loading} error={error} identityLabel={`Agent Team · ${teamId}`} onRetry={retry}>{null}</ViewState></div>;

  const team = view.data.team;
  const resolvedTeamRunId = teamRunId ?? team.latest_run?.id;
  const conversationOpen=Boolean(selection.teamConversation || selection.memberRunId);
  if(conversationOpen) return <AgentConversationWorkspace apiUrl={apiUrl} space={space} project={project} routeIdentity={teamId} view={view} selection={selection} refreshKey={refreshKey} onAction={onAction} actionsCurrent={actionsCurrent} onSelectionChange={onSelectionChange}/>;
  if(hostMode && team.viewer_role === "host") return <main className="agent-team-surface h-full flex-1 overflow-y-auto p-3 sm:p-5"><div className="mx-auto max-w-[1500px]"><HostConsole embedded apiUrl={apiUrl} space={space} project={project} teamId={teamId} teamRunId={resolvedTeamRunId} selectedWorkId={selection.teamWorkId} refreshKey={refreshKey} onAction={onAction} actionsCurrent={actionsCurrent} onSelectionChange={onSelectionChange}/></div></main>;
  return <main className="agent-team-surface h-full flex-1 overflow-y-auto p-3 sm:p-5" data-testid="authenticated-team-workspace"><div className="mx-auto max-w-[1500px] space-y-4">
    <header className="border-b border-border px-1 pb-4 pt-1"><div className="flex flex-wrap items-start justify-between gap-3"><div className="min-w-0"><div className="agent-team-eyebrow flex flex-wrap items-center gap-2"><Users className="size-3.5"/>Agent Team <span className="text-muted-foreground">· {team.viewer_role} view</span></div><h1 className="mt-1 break-words text-xl font-semibold tracking-[-0.02em] sm:text-[26px]">{team.display_name || team.team_id}</h1><p className="mt-1 text-xs text-muted-foreground">Mission {team.mission_id} · latest attempt {team.latest_run?.id ?? "not started"}</p></div><div className="flex flex-wrap items-center justify-end gap-2"><Button aria-label="Host conversation" size="sm" variant="secondary" onClick={() => onSelectionChange({teamMode:"workspace",teamConversation:"host",memberRunId:undefined,teamTab:"members"})}><MessageSquare className="size-3.5"/><span className="sm:hidden">Host chat</span><span className="hidden sm:inline">Host conversation</span></Button>{team.viewer_role === "host" && <Button aria-label="Open Host Console" size="sm" variant="secondary" aria-controls="host-tools" onClick={() => openHostTools()}><ShieldCheck className="size-3.5"/><span className="sm:hidden">Host Console</span><span className="hidden sm:inline">Open Host Console</span></Button>}<details className="relative"><summary className="flex min-h-9 cursor-pointer list-none items-center rounded-md border border-border bg-card px-3 text-xs font-medium">Context</summary><div className="agent-team-panel absolute right-0 top-11 z-30 w-72 rounded-xl p-4 shadow-xl"><dl className="space-y-2 text-xs"><ContextFact label="Team" value={team.team_id}/><ContextFact label="Mission" value={team.mission_id}/><ContextFact label="Team Lead" value={team.host_agent_id}/><ContextFact label="Node" value={team.node_id}/><ContextFact label="Run" value={team.latest_run?.id ?? "not started"}/><ContextFact label="Project" value={team.latest_run?.project_binding_id ?? "not attached"}/></dl><p className="mt-3 border-t border-border pt-3 text-[9px] leading-relaxed text-muted-foreground">Mission Log owns Host judgment. Provider transcripts remain native.</p></div></details><span className="hidden sm:inline-flex"><ViewProvenance view={view}/></span></div></div>
      <dl className="mt-4 grid grid-cols-2 gap-x-5 gap-y-3 border-t border-border pt-3 text-[10px] text-muted-foreground sm:grid-cols-3 lg:grid-cols-5"><HeaderFact label="Run status" value={team.latest_run?.status ?? "not started"}/><HeaderFact label="Attempt" value={team.latest_run?.id ?? "not started"}/><HeaderFact label="Placement" value={`${team.node_id}${team.placement_generation != null ? ` · g${team.placement_generation}` : ""}`}/><HeaderFact label="Project binding" value={team.latest_run?.project_binding_id ?? "not attached"}/><HeaderFact label="Workspace" value={team.latest_run?.execution_root ?? "not attached"}/></dl>
    </header>
    {error && <div role="alert" className="flex flex-wrap items-center gap-2 rounded-lg border border-status-warn/35 bg-status-warn/10 p-3 text-xs"><span className="min-w-0 flex-1">Refresh failed. Showing the last authoritative view with mutations unavailable. {error}</span><Button size="sm" variant="secondary" onClick={retry}>Retry</Button></div>}
    {loading && <div role="status" className="flex items-center gap-2 text-xs text-muted-foreground"><RefreshCw className="size-3.5 animate-spin"/>Refreshing authenticated TeamWorkspace…</div>}
    <AttentionStrip view={view}/>
    <TeamCapacityStrip summary={view.data.pressure_summary}/>
    <div className="min-w-0 space-y-4">
    <nav role="tablist" className="flex min-w-0 border-b border-border/70" aria-label="Team Workspace sections">{TABS.map(({id,label,icon:Icon}) => <button key={id} id={`team-workspace-tab-${id}`} role="tab" type="button" aria-selected={tab === id} aria-controls={`team-workspace-panel-${id}`} tabIndex={tab === id ? 0 : -1} onKeyDown={(event) => selectAdjacentTab(event,id)} onClick={() => onSelectionChange({teamTab:id})} className={cn("relative flex min-h-11 min-w-0 flex-1 items-center justify-center gap-2 px-3 text-xs font-medium after:absolute after:inset-x-5 after:bottom-0 after:h-0.5 after:rounded-full", tab === id ? "text-foreground after:bg-primary" : "text-muted-foreground after:bg-transparent hover:text-foreground")}><Icon className="size-3.5"/>{label}{id === "activity" && view.data.messages.length > 0 && <span className="rounded-full bg-primary/10 px-1.5 text-[9px] text-primary">{view.data.messages.length}</span>}</button>)}</nav>
    {tab === "works" && <div role="tabpanel" id="team-workspace-panel-works" aria-labelledby="team-workspace-tab-works"><TeamWorksBoard works={view.data.works} members={view.data.members} selectedWorkId={selection.teamWorkId} onSelectWork={(teamWorkId) => onSelectionChange({teamWorkId})} onOpenMember={(memberRunId) => { const member=view.data.members.find((candidate) => candidate.current_member_run_ref === memberRunId); onSelectionChange({teamMode:"workspace",teamConversation:member?.agent_member_ref.id,memberRunId,teamTab:"members"}); }} onOpenHost={team.viewer_role === "host" ? (teamWorkId) => openHostTools(teamWorkId) : undefined} onOpenHostTools={team.viewer_role === "host" ? () => openHostTools() : undefined} ownerFilter={selection.teamOwner ?? "all"} attentionFilter={selection.teamAttention ?? "all"} queryFilter={selection.teamQuery ?? ""} onFiltersChange={({owner,attention,query}) => onSelectionChange({teamOwner:owner === "all" ? undefined : owner,teamAttention:attention === "all" ? undefined : attention,teamQuery:query || undefined})}/></div>}
    {tab === "activity" && <div role="tabpanel" id="team-workspace-panel-activity" aria-labelledby="team-workspace-tab-activity" className="space-y-4"><TeamConversationStream activity={view.data.activity} messages={view.data.messages} members={view.data.members} truncated={view.data.activity_truncated} onOpenWork={(teamWorkId) => onSelectionChange({teamTab:"works",teamWorkId})} onReply={team.viewer_role === "host" ? setReplyTo : undefined}/>{team.viewer_role === "host" && <details className="agent-team-panel group rounded-xl" open={Boolean(replyTo)}><summary role="button" aria-label="Compose team message" className="flex min-h-12 cursor-pointer list-none items-center gap-2 px-4"><MessageSquare className="size-4 text-primary"/><h2 className="text-sm font-semibold group-open:hidden">Team message</h2><span className="text-[10px] text-muted-foreground group-open:hidden">Compose</span><span className="ml-auto text-[10px] text-muted-foreground">ordinary Message · not Steer</span></summary><div className="border-t border-border p-3"><HostActivityComposer apiUrl={apiUrl} space={space} project={project} routeIdentity={teamId} teamRunId={resolvedTeamRunId} replyTo={replyTo} refreshKey={refreshKey} actionsCurrent={actionsCurrent} onAction={onAction} onClearReply={() => setReplyTo(null)} collapsibleOnMobile={false}/></div></details>}</div>}
    {tab === "members" && <div role="tabpanel" id="team-workspace-panel-members" aria-labelledby="team-workspace-tab-members"><TeamMembersCapacity members={view.data.members} summary={view.data.pressure_summary} selectedMemberRunId={selection.memberRunId} onOpenMember={(memberRunId) => { const member=view.data.members.find((candidate) => candidate.current_member_run_ref === memberRunId); onSelectionChange({teamMode:"workspace",teamConversation:member?.agent_member_ref.id,memberRunId}); }}/></div>}
    </div>
  </div></main>;
}

function HeaderFact({label,value}:{label:string;value:string}) { return <div className="min-w-0"><dt className="uppercase tracking-wider">{label}</dt><dd className="mt-1 truncate font-medium text-foreground" title={value}>{value}</dd></div>; }

function ContextFact({label,value}:{label:string;value:string}) { return <div className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2"><dt className="text-muted-foreground">{label}</dt><dd className="truncate text-right font-medium" title={value}>{value}</dd></div>; }
