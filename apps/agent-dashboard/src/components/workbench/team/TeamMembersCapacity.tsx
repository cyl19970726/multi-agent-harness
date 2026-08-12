import { Activity, ArrowRight, Inbox, MessageSquare, Radio, UserRound, Users } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import type { MemberCapacitySummary, TeamPressureSummary } from "../../../model/roleViews";

export function TeamMembersCapacity({ members, summary, selectedMemberRunId, onOpenMember }: {
  members: MemberCapacitySummary[];
  summary?: TeamPressureSummary;
  selectedMemberRunId?: string;
  onOpenMember: (memberRunId:string) => void;
}) {
  if (!members.length) return <div className="agent-team-panel rounded-xl border-dashed p-10 text-center"><UserRound className="mx-auto size-7 text-muted-foreground"/><h3 className="mt-3 text-sm font-semibold">No Team members are projected</h3><p className="mx-auto mt-1 max-w-md text-xs leading-relaxed text-muted-foreground">The Team still exists with its explicit Host Agent. Durable members appear here only after an authorized membership action is projected and recorded.</p></div>;
  return <section aria-labelledby="team-members-title">
    <header className="flex flex-wrap items-end justify-between gap-3"><div><h2 id="team-members-title" className="text-base font-semibold">Members and runtime capacity</h2><p className="text-xs text-muted-foreground">Durable identity, MemberRun, runtime and native-session truth remain separate.</p></div><p className="text-[10px] text-muted-foreground">Select a member to open the Team conversation.</p></header>
    {summary && <div className="mt-3 grid grid-cols-4 divide-x divide-y divide-border border-y border-border lg:grid-cols-8"><CapacityFact icon={Users} label="AgentMembers" value={summary.total_members}/><CapacityFact label="Ready" value={summary.ready_members} tone="good"/><CapacityFact label="Active turns" value={summary.active_turns} tone="running"/><CapacityFact label="Ready Work" value={summary.ready_work}/><CapacityFact label="Needs review" value={summary.review_work} tone="warn"/><CapacityFact label="Blocked" value={summary.blocked_work} tone="bad"/><CapacityFact icon={Radio} label="Native available" value={members.filter((member) => member.native_session_health === "available").length} qualifier="projected rows"/><CapacityFact icon={Activity} label="Addressable" value={members.filter((member) => Boolean(member.current_member_run_ref)).length} qualifier="projected rows"/></div>}
    <div className="agent-team-panel mt-3 overflow-hidden rounded-xl">
      <div className="hidden grid-cols-[minmax(14rem,1.35fr)_minmax(11rem,1fr)_minmax(11rem,1fr)_minmax(9rem,.8fr)_minmax(10rem,.9fr)_8.5rem] gap-3 border-b border-border bg-secondary/60 px-4 py-2.5 text-[9px] font-semibold uppercase tracking-[.11em] text-muted-foreground lg:grid"><span>AgentMember / organization</span><span>Current MemberRun</span><span>Native session</span><span>Capacity</span><span>Status &amp; pressure</span><span className="text-right">Action</span></div>
      <div>{members.map((member) => <MemberRow key={member.agent_member_ref.id} member={member} selected={member.current_member_run_ref === selectedMemberRunId} onOpenMember={onOpenMember}/>)}</div>
    </div>
  </section>;
}

function MemberRow({member,selected,onOpenMember}:{member:MemberCapacitySummary;selected:boolean;onOpenMember:(memberRunId:string)=>void}) {
  const canOpen=Boolean(member.current_member_run_ref);
  const tone=member.runtime_state === "running" ? "running" : member.blocked_work_count ? "bad" : member.review_work_count ? "warn" : member.capacity === "available" ? "good" : "muted";
  return <article data-member-capacity={member.agent_member_ref.id} className={cn("agent-team-record-row min-w-0 px-3 py-3 sm:px-4",selected && "agent-team-selected")}>
    <div className="grid min-w-0 gap-3 lg:grid-cols-[minmax(14rem,1.35fr)_minmax(11rem,1fr)_minmax(11rem,1fr)_minmax(9rem,.8fr)_minmax(10rem,.9fr)_8.5rem] lg:items-center">
      <div className="flex min-w-0 items-center gap-3"><Avatar name={member.display_name} identity={`${member.agent_member_ref.id} ${member.role}`} size="lg" tone={member.runtime_state === "running" ? "running" : member.capacity === "available" ? "good" : member.blocked_work_count ? "warn" : "idle"}/><div className="min-w-0"><div className="flex min-w-0 flex-wrap items-center gap-1.5"><h3 className="truncate text-sm font-semibold" title={member.display_name}>{member.display_name}</h3><Badge>{member.role}</Badge><Badge tone={member.organization_status === "active" ? "good" : "muted"}>org {member.organization_status}</Badge></div><p className="mt-1 truncate text-[10px] text-muted-foreground">{[member.provider,member.model].filter(Boolean).join(" · ") || "provider not projected"}</p><p className="mt-1 truncate font-mono text-[9px] text-muted-foreground">{member.agent_member_ref.id}</p></div></div>
      <FactBlock icon={Radio} label="Current MemberRun" primary={member.current_member_run_ref ?? "No current run"} secondary={`${member.coordination_status ?? "not participating"}${member.runtime_generation != null ? ` · generation ${member.runtime_generation}` : ""}`}/>
      <FactBlock icon={Activity} label="Native session" primary={member.native_session_health ?? "not projected"} secondary={`runtime ${member.runtime_state ?? "unknown"}`}/>
      <FactBlock icon={Inbox} label="Capacity" primary={member.capacity} secondary={`${member.active_work_count} active · ${member.queued_work_count} queued`}/>
      <div className="min-w-0"><Badge tone={tone}>{member.runtime_state === "running" ? "running · active turn" : member.blocked_work_count ? "needs attention" : member.review_work_count ? "review waiting" : member.capacity}</Badge><p className="mt-2 text-[10px] text-muted-foreground">{member.review_work_count} review · {member.blocked_work_count} blocked</p>{member.latest_action && <p className="mt-1 line-clamp-1 text-[10px] text-muted-foreground">Latest · {member.latest_action.summary ?? member.latest_action.kind}</p>}</div>
      <button type="button" disabled={!canOpen} onClick={() => member.current_member_run_ref && onOpenMember(member.current_member_run_ref)} className="flex min-h-10 items-center justify-center gap-2 rounded-md border border-border bg-card px-3 text-xs font-medium enabled:hover:border-primary/45 enabled:hover:bg-primary/[0.03] disabled:text-muted-foreground" title={canOpen ? "Open this MemberRun conversation" : "No current MemberRun is projected"}><MessageSquare className="size-3.5"/><span>{canOpen ? "Conversation" : "Not addressable"}</span><ArrowRight className="size-3.5"/></button>
    </div>
  </article>;
}

function CapacityFact({icon:Icon,label,value,qualifier,tone}:{icon?:typeof Users;label:string;value:number;qualifier?:string;tone?:"good"|"warn"|"bad"|"running"}) { return <div className="min-w-0 px-2 py-2.5 sm:px-3"><dt className="flex min-h-6 items-start gap-1 text-[8px] font-semibold uppercase leading-tight tracking-[.08em] text-muted-foreground sm:text-[9px]">{Icon && <Icon className="size-3 shrink-0 sm:size-3.5"/>}{label}</dt><dd className={cn("mt-1 text-lg font-semibold tabular-nums",tone === "good" && "text-status-good",tone === "warn" && "text-status-warn",tone === "bad" && "text-status-bad",tone === "running" && "text-status-running")}>{value}</dd>{qualifier && <p className="hidden truncate text-[8px] text-muted-foreground sm:block">{qualifier}</p>}</div>; }
function FactBlock({icon:Icon,label,primary,secondary}:{icon:typeof Activity;label:string;primary:string;secondary:string}) { return <div className="min-w-0 lg:block"><p className="flex items-center gap-1 text-[9px] uppercase tracking-[.08em] text-muted-foreground lg:hidden"><Icon className="size-3"/>{label}</p><p className="truncate text-xs font-medium" title={primary}>{primary}</p><p className="mt-1 truncate text-[10px] text-muted-foreground" title={secondary}>{secondary}</p></div>; }
