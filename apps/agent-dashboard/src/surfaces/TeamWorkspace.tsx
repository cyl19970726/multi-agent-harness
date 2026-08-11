import { useEffect, useState, type KeyboardEvent } from "react";
import { Activity, BriefcaseBusiness, RefreshCw, ShieldCheck, Users } from "lucide-react";

import { Button } from "@/components/ui/button";
import { TeamCapacityStrip } from "@/components/workbench/team/TeamCapacityStrip";
import { TeamConversationStream } from "@/components/workbench/team/TeamConversation";
import { TeamMembersCapacity } from "@/components/workbench/team/TeamMembersCapacity";
import { TeamWorksBoard } from "@/components/workbench/team/TeamWorksBoard";
import { cn } from "@/lib/utils";
import { fetchRoleView, type RoleActionExecutor, type RoleView, type TeamWorkspaceData } from "../model/roleViews";
import type { SelectionState } from "../app/selection";
import { AttentionStrip, ViewProvenance, ViewState } from "./RoleViewPrimitives";
import { HostConsole } from "./HostConsole";

type TeamTab = "works" | "activity" | "members";
const TABS = [{id:"works",label:"Works",icon:BriefcaseBusiness},{id:"activity",label:"Activity",icon:Activity},{id:"members",label:"Members",icon:Users}] as const;

export function TeamWorkspace({apiUrl,space,project,teamId,teamRunId,refreshKey,selection,onAction,actionsCurrent,onSelectionChange}:{apiUrl:string;space:string;project:string;teamId:string;teamRunId?:string;refreshKey?:string;selection:SelectionState;onAction:RoleActionExecutor;actionsCurrent:boolean;onSelectionChange:(next:Partial<SelectionState>)=>void}){
  if(selection.teamMode === "host") return <HostConsole apiUrl={apiUrl} space={space} project={project} teamId={teamId} teamRunId={teamRunId} selectedWorkId={selection.teamWorkId} selectedMemberRunId={selection.memberRunId} refreshKey={refreshKey} onAction={onAction} actionsCurrent={actionsCurrent} onSelectionChange={onSelectionChange}/>;
  return <AuthenticatedTeamWorkspace apiUrl={apiUrl} space={space} project={project} teamId={teamId} refreshKey={refreshKey} selection={selection} onSelectionChange={onSelectionChange}/>;
}

function AuthenticatedTeamWorkspace({apiUrl,space,project,teamId,refreshKey,selection,onSelectionChange}:{apiUrl:string;space:string;project:string;teamId:string;refreshKey?:string;selection:SelectionState;onSelectionChange:(next:Partial<SelectionState>)=>void}) {
  const [view,setView] = useState<RoleView<TeamWorkspaceData>|null>(null);
  const [error,setError] = useState<string|null>(null);
  const [loading,setLoading] = useState(true);
  const [refetch,setRefetch] = useState(0);
  const [tab,setTab] = useState<TeamTab>("works");
  const selectAdjacentTab = (event: KeyboardEvent<HTMLButtonElement>, current: TeamTab) => {
    const index = TABS.findIndex(({id}) => id === current);
    const nextIndex = event.key === "ArrowRight" ? (index + 1) % TABS.length
      : event.key === "ArrowLeft" ? (index - 1 + TABS.length) % TABS.length
        : event.key === "Home" ? 0
          : event.key === "End" ? TABS.length - 1
            : -1;
    if (nextIndex < 0) return;
    event.preventDefault();
    const next = TABS[nextIndex].id;
    setTab(next);
    requestAnimationFrame(() => document.getElementById(`team-workspace-tab-${next}`)?.focus());
  };
  useEffect(() => { let live=true; setLoading(true); fetchRoleView<TeamWorkspaceData>(apiUrl,`/v1/views/team-workspace/${encodeURIComponent(teamId)}`,{space,project}).then((value) => { if(live){ setView(value); setError(null); } }).catch((reason) => { if(live) setError(String(reason)); }).finally(() => { if(live) setLoading(false); }); return () => { live=false; }; },[apiUrl,space,project,teamId,refreshKey,refetch]);
  if (!view) return <div className="h-full flex-1 overflow-y-auto"><ViewState loading={loading} error={error}>{null}</ViewState></div>;
  const team = view.data.team;
  const selectedMember = view.data.members.find((member) => member.current_member_run_ref === selection.memberRunId);
  return <main className="h-full flex-1 overflow-y-auto p-3 sm:p-5" data-testid="authenticated-team-workspace"><div className="mx-auto max-w-[1500px] space-y-4">
    <header className="rounded-xl border border-border bg-card px-4 py-3"><div className="flex flex-wrap items-start justify-between gap-3"><div className="min-w-0"><div className="flex flex-wrap items-center gap-2 text-[10px] font-semibold uppercase tracking-[.15em] text-primary"><Users className="size-3.5"/>Team Workspace <span className="text-muted-foreground">· {team.viewer_role} view</span></div><h1 className="mt-1 break-words text-xl font-semibold sm:text-2xl">{team.display_name || team.team_id}</h1><p className="mt-1 text-xs text-muted-foreground">Mission {team.mission_id} · Host {team.host_agent_id}</p></div><div className="flex flex-wrap items-center justify-end gap-2">{team.viewer_role === "host" && <Button size="sm" variant="secondary" onClick={() => onSelectionChange({teamMode:"host"})}><ShieldCheck className="size-3.5"/>Host Console</Button>}<ViewProvenance view={view}/></div></div>
      <dl className="mt-3 grid gap-2 border-t border-border pt-3 text-[10px] text-muted-foreground sm:grid-cols-4"><HeaderFact label="Latest run" value={team.latest_run?.id ?? "No TeamRun"}/><HeaderFact label="Run status" value={team.latest_run?.status ?? "not started"}/><HeaderFact label="Placement" value={`${team.node_id}${team.placement_generation != null ? ` · g${team.placement_generation}` : ""}`}/><HeaderFact label="Project binding" value={team.latest_run?.project_binding_id ?? "not attached"}/></dl>
    </header>
    {error && <div role="alert" className="flex flex-wrap items-center gap-2 rounded-lg border border-status-warn/35 bg-status-warn/10 p-3 text-xs"><span className="min-w-0 flex-1">Refresh failed. Showing the last authoritative view with mutations unavailable. {error}</span><Button size="sm" variant="secondary" onClick={() => setRefetch((value) => value+1)}>Retry</Button></div>}
    {loading && <div role="status" className="flex items-center gap-2 text-xs text-muted-foreground"><RefreshCw className="size-3.5 animate-spin"/>Refreshing authenticated TeamWorkspace…</div>}
    <AttentionStrip view={view}/>
    <TeamCapacityStrip summary={view.data.pressure_summary}/>
    <nav role="tablist" className="grid grid-cols-3 gap-1 rounded-xl border border-border bg-muted/25 p-1" aria-label="Team Workspace sections">{TABS.map(({id,label,icon:Icon}) => <button key={id} id={`team-workspace-tab-${id}`} role="tab" type="button" aria-selected={tab === id} aria-controls={`team-workspace-panel-${id}`} tabIndex={tab === id ? 0 : -1} onKeyDown={(event) => selectAdjacentTab(event,id)} onClick={() => setTab(id)} className={cn("flex min-h-11 items-center justify-center gap-2 rounded-lg px-3 text-xs font-medium", tab === id ? "bg-card text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground")}><Icon className="size-3.5"/>{label}{id === "activity" && view.data.messages.length > 0 && <span className="rounded-full bg-primary/10 px-1.5 text-[9px] text-primary">{view.data.messages.length}</span>}</button>)}</nav>
    {tab === "works" && <div role="tabpanel" id="team-workspace-panel-works" aria-labelledby="team-workspace-tab-works"><TeamWorksBoard works={view.data.works} members={view.data.members} selectedWorkId={selection.teamWorkId} onSelectWork={(teamWorkId) => onSelectionChange({teamWorkId})} onOpenMember={(memberRunId) => onSelectionChange({teamMode:"workspace",memberRunId})} onOpenHost={team.viewer_role === "host" ? (teamWorkId) => onSelectionChange({teamMode:"host",teamWorkId}) : undefined}/></div>}
    {tab === "activity" && <div role="tabpanel" id="team-workspace-panel-activity" aria-labelledby="team-workspace-tab-activity"><TeamConversationStream activity={view.data.activity} messages={view.data.messages} truncated={view.data.activity_truncated} onOpenWork={(teamWorkId) => { setTab("works"); onSelectionChange({teamWorkId}); }}/></div>}
    {tab === "members" && <div role="tabpanel" id="team-workspace-panel-members" aria-labelledby="team-workspace-tab-members" className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_18rem]"><TeamMembersCapacity members={view.data.members} selectedMemberRunId={selection.memberRunId} onOpenMember={(memberRunId) => onSelectionChange({teamMode:"workspace",memberRunId})}/><aside className="rounded-xl border border-border bg-card p-4"><h2 className="text-sm font-semibold">Selected member context</h2>{selectedMember ? <div className="mt-3 space-y-2 text-xs"><p className="font-medium">{selectedMember.display_name}</p><p className="text-muted-foreground">{selectedMember.provider ?? "provider unknown"} · {selectedMember.model ?? "model unknown"}</p><p>Runtime {selectedMember.runtime_state ?? "unknown"} · native session {selectedMember.native_session_health ?? "unknown"}</p><Button size="sm" variant="secondary" onClick={() => selectedMember.current_member_run_ref && onSelectionChange({teamMode:undefined,memberRunId:selectedMember.current_member_run_ref})}>Member deep link</Button></div> : <p className="mt-3 text-xs text-muted-foreground">Select an addressable member to keep its MemberRun in the URL.</p>}</aside></div>}
  </div></main>;
}

function HeaderFact({label,value}:{label:string;value:string}) { return <div className="min-w-0"><dt className="uppercase tracking-wider">{label}</dt><dd className="mt-1 truncate font-medium text-foreground" title={value}>{value}</dd></div>; }
