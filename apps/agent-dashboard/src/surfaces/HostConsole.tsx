import { useEffect, useState } from "react";
import { GitMerge, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { fetchRoleView, type HostConsoleData, type RoleActionExecutor, type RoleView } from "../model/roleViews";
import { AttentionStrip, ViewProvenance, ViewState, WorkTable } from "./RoleViewPrimitives";
import { RoleActionPanel } from "./RoleActionPanel";

export function HostConsole({apiUrl,space,project,teamId,teamRunId,refreshKey,onAction,onSelectionChange}:{apiUrl:string;space:string;project:string;teamId:string;teamRunId?:string;refreshKey?:string;onAction:RoleActionExecutor;onSelectionChange:(next:Record<string,unknown>)=>void}) {
  const [view,setView]=useState<RoleView<HostConsoleData>|null>(null);
  const [error,setError]=useState<string|null>(null);
  const [loading,setLoading]=useState(true);
  const [refresh,setRefresh]=useState(0);
  useEffect(()=>{let live=true;setLoading(true);fetchRoleView<HostConsoleData>(apiUrl,`/v1/views/host-console/${encodeURIComponent(teamId)}`,{space,project}).then(value=>live&&setView(value)).catch(reason=>live&&setError(String(reason))).finally(()=>live&&setLoading(false));return()=>{live=false}},[apiUrl,space,project,teamId,refreshKey,refresh]);
  return <div className="h-full flex-1 overflow-y-auto p-4 sm:p-6"><div className="mx-auto max-w-[1500px] space-y-5">
    <header className="flex flex-wrap justify-between gap-3"><div><div className="mb-2 flex items-center gap-2 text-xs uppercase tracking-[.16em] text-primary"><ShieldCheck className="size-4"/>Host responsibility</div><h1 className="text-2xl font-semibold">Host Console</h1><p className="text-sm text-muted-foreground">Resource allocation, convergence, recovery and acceptance.</p></div><div className="flex items-center gap-2"><Button size="sm" variant="secondary" onClick={()=>onSelectionChange({teamMode:"workspace"})}>Team Workspace</Button>{view&&<ViewProvenance view={view}/>}</div></header>
    <ViewState loading={loading} error={error}>{view&&<><AttentionStrip view={view}/><div className="grid gap-3 md:grid-cols-4">{Object.entries(view.data.work_queues).slice(0,4).map(([name,items])=><div key={name} className="rounded-xl border border-border p-4"><div className="text-xs uppercase text-muted-foreground">{name}</div><div className="mt-2 text-2xl font-semibold">{items.length}</div></div>)}</div><section><h2 className="mb-3 flex items-center gap-2 font-medium"><GitMerge className="size-4"/>Review and convergence queue</h2><WorkTable items={view.data.work_queues.review??[]}/></section><div className="grid gap-4 lg:grid-cols-3"><Panel title="Member capacity" value={view.data.member_capacity}/><Panel title="Workspace conflicts" value={view.data.workspace_conflicts}/><Panel title="Delivery recovery" value={view.data.deliveries_requiring_reconcile}/></div><RoleActionPanel actions={view.allowed_actions} onAction={onAction} context={{teamId,teamRunId}} onCompleted={()=>setRefresh(value=>value+1)}/></>}</ViewState>
  </div></div>;
}
function Panel({title,value}:{title:string;value:unknown[]}){return <section className="rounded-xl border border-border p-4"><h2 className="font-medium">{title}</h2><div className="mt-3 text-2xl font-semibold">{value.length}</div><p className="text-xs text-muted-foreground">canonical attention rows</p></section>}
