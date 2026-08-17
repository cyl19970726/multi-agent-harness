import { useEffect, useMemo, useState } from "react";
import { BriefcaseBusiness, ListFilter, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { fetchRoleView, type GlobalWorkIndexData, type RoleView } from "../model/roleViews";
import type { SelectionState } from "../app/selection";
import { AttentionStrip, ViewProvenance, ViewState, WorkTable } from "./RoleViewPrimitives";

/**
 * Global Work (DOC-106/DOC-107): the read-only aggregate over authoritative
 * TeamWork, served by `/v1/views/global-work`. Team is the primary filter;
 * Host/Member/assignee/phase/priority narrow the same server projection. Rows
 * route to the durable Team id, never to a TeamRun attempt.
 */
export function GlobalWorkIndex({apiUrl,space,project,refreshKey,selection,onSelectionChange,teams=[]}:{apiUrl:string;space:string;project:string;refreshKey?:string;selection:SelectionState;onSelectionChange:(next:Partial<SelectionState>)=>void;teams?:Array<{id:string;name?:string}>}){
  const [view,setView]=useState<RoleView<GlobalWorkIndexData>|null>(null);const [error,setError]=useState<string|null>(null);const [loading,setLoading]=useState(true);
  useEffect(()=>{let live=true;setLoading(true);setError(null);const params=new URLSearchParams();for(const [key,value] of [["team_id",selection.workTeamId],["host_id",selection.workHostId],["member_id",selection.workMemberId],["assignee_kind",selection.workAssignee],["phase",selection.workStatus],["priority",selection.workPriority]] as const)if(value)params.append(key,value);const qs=params.toString();fetchRoleView<GlobalWorkIndexData>(apiUrl,`/v1/views/global-work${qs?`?${qs}`:""}`,{space,project}).then(v=>live&&setView(v)).catch(e=>live&&setError(String(e))).finally(()=>live&&setLoading(false));return()=>{live=false}},[apiUrl,space,project,refreshKey,selection.workTeamId,selection.workHostId,selection.workMemberId,selection.workAssignee,selection.workStatus,selection.workPriority]);
  const teamNames=useMemo(()=>new Map(teams.map((team)=>[team.id,team.name?.trim()||team.id])),[teams]);
  const facets=view?.data.facets;
  const filtersApplied=Boolean(selection.workTeamId||selection.workHostId||selection.workMemberId||selection.workAssignee||selection.workStatus||selection.workPriority);
  const setFilter=(next:Partial<SelectionState>)=>onSelectionChange(next);
  const clearFilters=()=>onSelectionChange({workTeamId:undefined,workHostId:undefined,workMemberId:undefined,workAssignee:undefined,workStatus:undefined,workPriority:undefined});
  const select=(label:string,value:string|undefined,current:string|undefined,options:string[],names?:Map<string,string>,onChange?:(value:string)=>void)=>(<label key={label} className="min-w-0"><span className="sr-only">{label}</span><select aria-label={label} value={current??""} onChange={(event)=>onChange?onChange(event.target.value):undefined} className="agent-team-control h-9 w-full px-2 text-xs"><option value="">{label}: all</option>{options.map((option)=><option key={option} value={option}>{names?.get(option)??option.replace(/_/g," ")}</option>)}</select></label>);
  return <div className="space-y-5"><header className="flex flex-wrap items-end justify-between gap-3"><div><div className="mb-2 flex items-center gap-2 text-xs uppercase tracking-[.16em] text-primary"><BriefcaseBusiness className="size-4"/>Global Work aggregate</div><h1 className="text-2xl font-semibold">Global Work</h1><p className="mt-1 text-sm text-muted-foreground">Read-only aggregate over authoritative Team Work. Every row remains owned by its flat Agent Team.</p></div>{view&&<ViewProvenance view={view}/>}</header><ViewState loading={loading} error={error}>{view&&<><AttentionStrip view={view}/>
  <div className="grid gap-2 border-y border-border/80 py-3 sm:grid-cols-2 lg:grid-cols-6" aria-label="Global Work filters" data-testid="global-work-filters">
    {select("Team",undefined,selection.workTeamId,facets?.teams??[],teamNames,(value)=>setFilter({workTeamId:value||undefined}))}
    {select("Host",undefined,selection.workHostId,facets?.hosts??[],undefined,(value)=>setFilter({workHostId:value||undefined}))}
    {select("Member",undefined,selection.workMemberId,facets?.members??[],undefined,(value)=>setFilter({workMemberId:value||undefined}))}
    {select("Assignee",undefined,selection.workAssignee,["host","member","unassigned"],undefined,(value)=>setFilter({workAssignee:value||undefined}))}
    {select("Phase",undefined,selection.workStatus,facets?.phases??[],undefined,(value)=>setFilter({workStatus:value||undefined}))}
    <div className="flex min-w-0 items-center gap-2">{select("Priority",undefined,selection.workPriority,Array.from(new Set(view.data.items.map((item)=>item.priority).filter(Boolean))),undefined,(value)=>setFilter({workPriority:value||undefined}))}{filtersApplied&&<Button size="sm" variant="ghost" aria-label="Clear Global Work filters" onClick={clearFilters}><X className="size-3.5"/></Button>}</div>
  </div>
  <div className="flex items-center gap-2"><ListFilter className="size-4 text-muted-foreground"/><span className="text-xs text-muted-foreground">{view.data.page.item_count} Work items</span></div>
  <WorkTable items={view.data.items} teamNames={teamNames} onOpen={work=>onSelectionChange({surface:"team",teamId:work.accountable_team_id??work.team_id,teamWorkId:work.work_id})}/></>}</ViewState></div>
}
