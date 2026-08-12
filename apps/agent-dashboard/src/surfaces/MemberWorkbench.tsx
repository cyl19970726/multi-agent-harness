import { useEffect, useState, type ReactNode } from "react";
import {
  Activity,
  ArrowRight,
  BriefcaseBusiness,
  CheckCircle2,
  FolderGit2,
  History,
  Inbox,
  MessageSquare,
  RadioTower,
  UserRound,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import {
  AgentTeamFactStrip,
  AgentTeamRecordRow,
} from "@/components/workbench/team/AgentTeamVisualPrimitives";
import {
  fetchRoleView,
  type MemberWorkbenchData,
  type MessageSummary,
  type RoleActionExecutor,
  type RoleRecordSummary,
  type RoleView,
  type WorkSummary,
} from "../model/roleViews";
import { AttentionStrip, ViewProvenance, ViewState } from "./RoleViewPrimitives";
import { RoleActionPanel } from "./RoleActionPanel";

export function MemberWorkbench({apiUrl,space,project,memberRunId,onAction,actionsCurrent}:{
  apiUrl:string; space:string; project:string; memberRunId:string;
  onAction:RoleActionExecutor; actionsCurrent:boolean;
}) {
  const [view,setView] = useState<RoleView<MemberWorkbenchData>|null>(null);
  const [error,setError] = useState<string|null>(null);
  const [loading,setLoading] = useState(true);
  const [refresh,setRefresh] = useState(0);
  const retry=()=>setRefresh((value)=>value+1);
  useEffect(() => {
    let live=true;
    setLoading(true);
    fetchRoleView<MemberWorkbenchData>(apiUrl,`/v1/views/member-workbench/${encodeURIComponent(memberRunId)}`,{space,project})
      .then((value)=>{if(live){setView(value);setError(null);}})
      .catch((reason)=>{if(live)setError(String(reason));})
      .finally(()=>{if(live)setLoading(false);});
    return()=>{live=false;};
  },[apiUrl,space,project,memberRunId,refresh]);

  const teamRunId=view?.data.member_run.team_run_id;
  const teamId=view ? [...view.data.my_works,...view.data.eligible_ready_pool].find((work)=>work.team_id)?.team_id : undefined;
  const currentWork=view?.data.my_works.find((work)=>work.current_member_run_ref===memberRunId) ?? view?.data.my_works[0];
  const evidence=view ? [...view.data.report_history,...view.data.finding_history,...view.data.failure_history]
    .sort((left,right)=>(right.created_at??"").localeCompare(left.created_at??"")) : [];

  return <main className="agent-team-surface h-full flex-1 overflow-y-auto p-4 sm:p-6" data-testid="member-home">
    <div className="mx-auto max-w-[1400px] space-y-5">
      <h1 className="sr-only">Member Workbench</h1>
      <ViewState loading={loading} error={error} identityLabel={`Member home · ${memberRunId}`} onRetry={retry}>
        {view && <>
          <header className="border-b border-border pb-5">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="agent-team-eyebrow flex items-center gap-2"><UserRound className="size-3.5"/>Agent Member · live responsibility</p>
              <ViewProvenance view={view}/>
            </div>
            <div className="mt-5 grid items-center gap-5 lg:grid-cols-[5.5rem_minmax(18rem,.9fr)_minmax(0,1.4fr)]">
              <Avatar name={view.data.agent_member.display_name} identity={`${view.data.agent_member.id} ${view.data.agent_member.role}`} size="xl" tone={view.data.member_run.runtime_status==="active"?"running":"idle"}/>
              <div className="min-w-0">
                <h2 className="company-editorial-title truncate text-3xl">{view.data.agent_member.display_name}</h2>
                <div className="mt-1 flex flex-wrap items-center gap-2 text-sm"><span className="font-semibold">{view.data.agent_member.role}</span><Badge tone={view.data.agent_member.organization_status==="active"?"good":"muted"}>{view.data.agent_member.organization_status}</Badge></div>
                {view.data.agent_member.description && <p className="mt-2 max-w-xl text-[13px] leading-5 text-muted-foreground">{view.data.agent_member.description}</p>}
                <p className="mt-2 truncate font-mono text-[10px] text-muted-foreground">{view.data.agent_member.id}</p>
              </div>
              <AgentTeamFactStrip className="grid-cols-2 lg:grid-cols-4">
                <HeroFact icon={Activity} label="Current run" value={`g${view.data.member_run.runtime_generation}`} detail={view.data.member_run.runtime_status}/>
                <HeroFact icon={RadioTower} label="Native session" value={view.data.native_session_health} detail={view.data.member_run.coordination_status}/>
                <HeroFact icon={FolderGit2} label="Workspace" value={view.data.workspace_binding?"bound":"not bound"} detail={`${view.data.runtime_fabric.work_execution_bindings.length} execution bindings`}/>
                <HeroFact icon={BriefcaseBusiness} label="Responsibility" value={`${view.data.my_works.length} Work`} detail={`${view.data.eligible_ready_pool.length} ready to claim`}/>
              </AgentTeamFactStrip>
            </div>
          </header>
          <AttentionStrip view={view}/>

          <div className="grid min-w-0 gap-7 xl:grid-cols-[minmax(0,1.12fr)_minmax(26rem,.88fr)]">
            <div className="min-w-0 space-y-6">
              <section aria-labelledby="member-current-work">
                <SectionTitle icon={BriefcaseBusiness} title="Current responsibility" detail="Work this member owns now — ordered by operational relevance." count={view.data.my_works.length}/>
                {currentWork && <CurrentWork work={currentWork}/>}
                <div className="divide-y divide-border border-b border-border">
                  {view.data.my_works.filter((work)=>work.work_id!==currentWork?.work_id).map((work)=><WorkRow key={work.work_id} work={work}/>) }
                </div>
              </section>

              {view.data.eligible_ready_pool.length>0 && <section aria-labelledby="member-ready-work">
                <SectionTitle icon={CheckCircle2} title="Ready to claim" detail="Server-authorized eligibility; not ownership until an action succeeds." count={view.data.eligible_ready_pool.length}/>
                <div className="divide-y divide-border border-y border-border">{view.data.eligible_ready_pool.map((work)=><WorkRow key={work.work_id} work={work} eligible/>)}</div>
              </section>}

              <section aria-labelledby="member-history">
                <SectionTitle icon={History} title="Run history" detail="Durable MemberRuns for this AgentMember; provider transcript remains native." count={view.data.member_run_history.length}/>
                <div className="grid border-y border-border sm:grid-cols-3">{view.data.member_run_history.slice(0,6).map((run)=><div key={run.id} className="border-b border-border px-3 py-3 last:border-b-0 sm:border-b-0 sm:border-r sm:last:border-r-0"><p className="truncate text-xs font-semibold">generation {run.runtime_generation}</p><p className="mt-1 truncate text-[11px] text-muted-foreground">{run.runtime_status} · {run.native_session_health}</p><p className="mt-2 truncate font-mono text-[9px] text-muted-foreground">{run.id}</p></div>)}</div>
              </section>
            </div>

            <aside className="min-w-0 space-y-6">
              <section aria-labelledby="member-replies">
                <SectionTitle icon={MessageSquare} title="Messages and replies" detail="Authored coordination involving this member — not a mirrored provider transcript." count={view.data.unread_messages.length}/>
                {view.data.unread_messages.length ? <div className="divide-y divide-border border-y border-border">{view.data.unread_messages.slice(0,5).map((message)=><MessageRow key={message.message_id} message={message}/>)}</div> : <QuietState>No unread authored message currently needs this member.</QuietState>}
              </section>

              <section aria-labelledby="member-execution">
                <SectionTitle icon={Activity} title="Latest execution evidence" detail="What this member produced, found or failed — source-linked to exact Work." count={evidence.length}/>
                {evidence.length ? <div className="divide-y divide-border border-y border-border">{evidence.slice(0,6).map((record)=><EvidenceRow key={`${record.kind}:${record.id}`} record={record}/>)}</div> : <QuietState>No submitted execution evidence is projected for this member yet.</QuietState>}
              </section>

              {(view.data.queued_deliveries.length>0 || view.data.gate_requirements.length>0 || view.data.pending_provider_interactions.length>0) && <section aria-labelledby="member-pressure">
                <SectionTitle icon={Inbox} title="Needs attention" detail="Delivery, gate and provider interaction pressure."/>
                <AgentTeamFactStrip className="grid-cols-3">
                  <MiniFact label="Deliveries" value={view.data.queued_deliveries.length}/>
                  <MiniFact label="Gates" value={view.data.gate_requirements.length}/>
                  <MiniFact label="Interactions" value={view.data.pending_provider_interactions.length}/>
                </AgentTeamFactStrip>
              </section>}

              <section aria-labelledby="member-actions">
                <SectionTitle icon={UserRound} title="Next authorized action" detail="Only exact server-projected controls appear here."/>
                {view.allowed_actions.length ? <div className="[&>div]:rounded-none [&>div]:border-x-0 [&>div]:shadow-none"><RoleActionPanel actions={view.allowed_actions} onAction={onAction} actionsCurrent={actionsCurrent&&view.freshness==="current"} context={{teamId,teamRunId}} onCompleted={retry}/></div> : <QuietState>No action is authorized for this exact identity and state.</QuietState>}
              </section>
            </aside>
          </div>
        </>}
      </ViewState>
    </div>
  </main>;
}

function HeroFact({icon:Icon,label,value,detail}:{icon:typeof Activity;label:string;value:string;detail:string}) { return <div className="min-w-0 px-3 py-3"><dt className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[.1em] text-muted-foreground"><Icon className="size-3.5"/>{label}</dt><dd className="mt-1.5 truncate text-sm font-semibold">{value}</dd><p className="mt-0.5 truncate text-[10.5px] text-muted-foreground">{detail}</p></div>; }

function SectionTitle({icon:Icon,title,detail,count}:{icon:typeof Activity;title:string;detail:string;count?:number}) { return <div className="mb-3 flex items-end justify-between gap-3"><div><div className="flex items-center gap-2"><Icon className="size-4 text-primary"/><h2 id={`member-${title.toLowerCase().replace(/ /g,"-")}`} className="company-editorial-title text-lg">{title}</h2></div><p className="mt-1 text-[11px] leading-4 text-muted-foreground">{detail}</p></div>{count!==undefined&&<span className="text-sm font-semibold tabular-nums text-muted-foreground">{count}</span>}</div>; }

function CurrentWork({work}:{work:WorkSummary}) { return <section className="agent-team-decision-surface mb-2 p-4"><div className="flex items-start justify-between gap-4"><div className="min-w-0"><p className="agent-team-eyebrow">In focus now</p><h3 className="company-editorial-title mt-2 text-xl">{work.title||work.work_id}</h3><p className="mt-1 truncate font-mono text-[10px] text-muted-foreground">{work.work_id} · revision {work.work_revision}</p></div><Badge tone={work.condition==="blocked"?"bad":work.phase==="review"?"warn":"running"}>{work.condition!=="normal"?work.condition:work.phase}</Badge></div><div className="mt-3 line-clamp-2 text-[13px] leading-5 text-foreground/80"><Markdown source={work.result_summary||work.context_markdown||"No current execution context."} compact/></div><div className="mt-4 flex flex-wrap gap-x-5 gap-y-1 border-t border-primary/15 pt-3 text-[10.5px] text-muted-foreground"><span>Gates {work.gate_summary.passed}/{work.gate_summary.required}</span><span>Runtime {String(work.runtime_summary.state??"unknown")}</span><span>Updated {formatTime(work.updated_at)}</span></div></section>; }

function WorkRow({work,eligible=false}:{work:WorkSummary;eligible?:boolean}) { return <AgentTeamRecordRow className="grid gap-2 py-3 sm:grid-cols-[minmax(0,1fr)_7rem_8rem] sm:items-center"><div className="min-w-0"><p className="truncate text-[13px] font-semibold">{work.title||work.work_id}</p><p className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">{work.work_id} · v{work.work_revision}</p></div><div><Badge tone={work.condition==="blocked"?"bad":work.phase==="review"?"warn":"muted"}>{eligible?"eligible":work.phase}</Badge></div><p className="text-[10.5px] text-muted-foreground sm:text-right">{work.gate_summary.passed}/{work.gate_summary.required} gates</p></AgentTeamRecordRow>; }

function MessageRow({message}:{message:MessageSummary}) { return <AgentTeamRecordRow className="py-3"><div className="flex items-center gap-2"><MessageSquare className="size-3.5 text-primary"/><p className="text-[11px] font-semibold">{message.sender.id}</p><span className="ml-auto text-[10px] text-muted-foreground">{formatTime(message.created_at)}</span></div><p className="mt-2 text-[13px] leading-5 text-foreground/85">{message.body}</p><div className="mt-2 flex items-center gap-2 text-[10px] text-muted-foreground"><span>{message.kind.replace(/_/g," ")}</span>{message.work_id&&<><ArrowRight className="size-3"/><span className="truncate font-mono">{message.work_id}</span></>}</div></AgentTeamRecordRow>; }

function EvidenceRow({record}:{record:RoleRecordSummary}) { return <AgentTeamRecordRow className="grid gap-1 py-3 sm:grid-cols-[7rem_minmax(0,1fr)_auto] sm:items-baseline"><p className="text-[10px] font-semibold uppercase tracking-[.08em] text-primary">{record.kind.replace(/_/g," ")}</p><p className="text-[12px] leading-5 text-foreground/80">{record.summary||record.status||record.id}</p><p className="text-[10px] text-muted-foreground">{formatTime(record.created_at)}</p></AgentTeamRecordRow>; }

function MiniFact({label,value}:{label:string;value:number}) { return <div className="px-3 py-3"><dt className="text-[9px] uppercase tracking-wider text-muted-foreground">{label}</dt><dd className="mt-1 text-xl font-semibold tabular-nums">{value}</dd></div>; }
function QuietState({children}:{children:ReactNode}) { return <p className="border-y border-dashed border-border py-4 text-[11px] leading-5 text-muted-foreground">{children}</p>; }
function formatTime(value:string|null) { return value ? new Date(value).toLocaleString([],{month:"short",day:"numeric",hour:"2-digit",minute:"2-digit"}) : "time unavailable"; }
