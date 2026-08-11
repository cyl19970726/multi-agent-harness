import { Activity, ArrowRight, CircleAlert, Inbox, Radio, UserRound } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import type { MemberCapacitySummary } from "../../../model/roleViews";

export function TeamMembersCapacity({ members, selectedMemberRunId, onOpenMember }: {
  members: MemberCapacitySummary[];
  selectedMemberRunId?: string;
  onOpenMember: (memberRunId:string) => void;
}) {
  if (!members.length) return <div className="rounded-xl border border-dashed border-border p-10 text-center"><UserRound className="mx-auto size-6 text-muted-foreground"/><h3 className="mt-3 text-sm font-medium">No Team members are projected</h3><p className="mt-1 text-xs text-muted-foreground">The Host can provision durable membership from an authorized control when the server exposes one.</p></div>;
  return <section aria-labelledby="team-members-title"><header><h2 id="team-members-title" className="text-base font-semibold">Members and runtime capacity</h2><p className="text-xs text-muted-foreground">Addressability, Work pressure, runtime and native-session health remain separate facts.</p></header><div className="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-3">{members.map((member) => {
    const selected = member.current_member_run_ref === selectedMemberRunId;
    const canOpen = Boolean(member.current_member_run_ref);
    return <article key={member.agent_member_ref.id} data-member-capacity={member.agent_member_ref.id} className={cn("min-w-0 rounded-xl border border-border bg-card p-4", selected && "border-primary ring-1 ring-primary/15")}><div className="flex min-w-0 items-start gap-3"><Avatar name={member.display_name} identity={`${member.agent_member_ref.id} ${member.role}`} tone={member.runtime_state === "running" ? "running" : member.capacity === "available" ? "good" : member.blocked_work_count ? "warn" : "idle"}/><div className="min-w-0 flex-1"><h3 className="truncate text-sm font-semibold" title={member.display_name}>{member.display_name}</h3><p className="truncate text-[11px] text-muted-foreground">{member.role} · {[member.provider,member.model].filter(Boolean).join(" / ") || "provider not observed"}</p></div><Badge>{member.capacity}</Badge></div>
      <dl className="mt-3 grid grid-cols-2 gap-2 text-[10px]"><MemberFact icon={Radio} label="Runtime" value={`${member.runtime_state ?? "unknown"}${member.runtime_generation != null ? ` · g${member.runtime_generation}` : ""}`}/><MemberFact icon={Activity} label="Native session" value={member.native_session_health || "unknown"}/><MemberFact icon={Inbox} label="Work pressure" value={`${member.queued_work_count} queued · ${member.active_work_count} active`}/><MemberFact icon={CircleAlert} label="Attention" value={`${member.review_work_count} review · ${member.blocked_work_count} blocked`}/></dl>
      {member.latest_action && <p className="mt-3 rounded-md bg-muted/35 px-2.5 py-2 text-[10px] text-muted-foreground"><span className="font-medium text-foreground">Latest:</span> {member.latest_action.summary ?? member.latest_action.kind}{member.latest_action.status ? ` · ${member.latest_action.status}` : ""}</p>}
      <button type="button" disabled={!canOpen} onClick={() => member.current_member_run_ref && onOpenMember(member.current_member_run_ref)} className="mt-3 flex min-h-10 w-full items-center justify-between rounded-md border border-border px-3 text-xs font-medium enabled:hover:border-primary/35 disabled:text-muted-foreground" title={canOpen ? "Open this MemberRun" : "No current MemberRun is projected"}><span>{canOpen ? "addressable" : "not addressable"} · {member.coordination_status ?? "not running"}</span><ArrowRight className="size-3.5"/></button>
    </article>;
  })}</div></section>;
}

function MemberFact({icon:Icon,label,value}:{icon:typeof Activity;label:string;value:string}) { return <div className="min-w-0 rounded-md bg-muted/25 p-2"><dt className="flex items-center gap-1 text-muted-foreground"><Icon className="size-3"/>{label}</dt><dd className="mt-1 break-words font-medium text-foreground">{value}</dd></div>; }
