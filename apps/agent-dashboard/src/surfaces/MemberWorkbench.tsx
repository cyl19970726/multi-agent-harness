import { useEffect, useState } from "react";
import { UserRound } from "lucide-react";

import { Avatar } from "@/components/workbench/Avatar";
import { fetchRoleView, type MemberWorkbenchData, type RoleActionExecutor, type RoleView } from "../model/roleViews";
import { AttentionStrip, ViewProvenance, ViewState, WorkTable } from "./RoleViewPrimitives";
import { RoleActionPanel } from "./RoleActionPanel";

export function MemberWorkbench({apiUrl,space,project,memberRunId,onAction,actionsCurrent}:{apiUrl:string;space:string;project:string;memberRunId:string;onAction:RoleActionExecutor;actionsCurrent:boolean}) {
  const [view,setView]=useState<RoleView<MemberWorkbenchData>|null>(null);
  const [error,setError]=useState<string|null>(null);
  const [loading,setLoading]=useState(true);
  const [refresh,setRefresh]=useState(0);
  const retry=()=>setRefresh((value)=>value+1);
  useEffect(()=>{let live=true;setLoading(true);fetchRoleView<MemberWorkbenchData>(apiUrl,`/v1/views/member-workbench/${encodeURIComponent(memberRunId)}`,{space,project}).then(value=>{if(live){setView(value);setError(null);}}).catch(reason=>{if(live)setError(String(reason));}).finally(()=>{if(live)setLoading(false);});return()=>{live=false;};},[apiUrl,space,project,memberRunId,refresh]);
  const teamRunId=view?.data.member_run.team_run_id;
  const teamId=view ? [...view.data.my_works,...view.data.eligible_ready_pool].find((work)=>work.team_id)?.team_id : undefined;
  return <div className="h-full flex-1 overflow-y-auto p-4 sm:p-6"><div className="mx-auto max-w-[1400px] space-y-5"><header className="flex flex-wrap justify-between gap-3"><div className="flex min-w-0 items-center gap-3"><Avatar name={view?.data.agent_member.id ?? memberRunId} identity={`${view?.data.agent_member.id ?? ""} ${view?.data.agent_member.role ?? ""}`} size="lg" tone={view?.data.member_run.runtime_status === "active" ? "running" : "idle"}/><div className="min-w-0"><div className="mb-1 flex items-center gap-2 text-xs uppercase tracking-[.16em] text-primary"><UserRound className="size-4"/>Member responsibility</div><h1 className="text-2xl font-semibold">Member Workbench</h1><p className="text-sm text-muted-foreground">Exact-self Work, messages, workspace and evidence.</p></div></div>{view&&<ViewProvenance view={view}/>}</header><ViewState loading={loading} error={error} identityLabel={`Member home · ${memberRunId}`} onRetry={retry}>{view&&<><AttentionStrip view={view}/><section><h2 className="mb-3 font-medium">My Work</h2><WorkTable items={view.data.my_works}/></section><section><h2 className="mb-3 font-medium">Eligible ready pool</h2><WorkTable items={view.data.eligible_ready_pool}/></section><div className="grid gap-3 sm:grid-cols-3"><Stat label="Unread messages" value={view.data.unread_messages.length}/><Stat label="Queued deliveries" value={view.data.queued_deliveries.length}/><Stat label="Gate requirements" value={view.data.gate_requirements.length}/></div><RoleActionPanel actions={view.allowed_actions} onAction={onAction} actionsCurrent={actionsCurrent&&view.freshness==="current"} context={{teamId,teamRunId}} onCompleted={retry}/></>}</ViewState></div></div>;
}

function Stat({label,value}:{label:string;value:number}) { return <div className="rounded-xl border border-border p-4"><div className="text-xs text-muted-foreground">{label}</div><div className="mt-2 text-2xl font-semibold">{value}</div></div>; }
