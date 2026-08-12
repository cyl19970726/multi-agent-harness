import type { ComponentProps, ReactNode } from "react";

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";

export function AgentTeamTabs<T extends string>({ value, onValueChange, children, label }: { value:T; onValueChange:(value:T)=>void; children:ReactNode; label:string }) {
  return <Tabs value={value} onValueChange={(next) => onValueChange(next as T)}><TabsList aria-label={label} className="agent-team-tabs h-auto w-full justify-start gap-0 rounded-none border-x-0 border-t-0 bg-transparent p-0">{children}</TabsList></Tabs>;
}

export function AgentTeamTab({ className, ...props }: ComponentProps<typeof TabsTrigger>) {
  return <TabsTrigger className={cn("agent-team-tab relative min-h-12 min-w-0 flex-1 rounded-none bg-transparent px-4 text-[13px] font-medium shadow-none outline-none ring-0 focus-visible:outline-none focus-visible:ring-0 after:absolute after:inset-x-4 after:bottom-0 after:h-0.5 after:bg-transparent data-[state=active]:bg-transparent data-[state=active]:font-semibold data-[state=active]:shadow-none data-[state=active]:after:bg-primary sm:max-w-28",className)} {...props}/>;
}

export function AgentTeamSection({ as:Tag="section", variant="plain", className, children, ...props }: { as?:"section"|"div"|"aside"; variant?:"plain"|"recessed"|"decision"; className?:string; children:ReactNode; [key:string]:unknown }) {
  return <Tag className={cn("agent-team-section",variant === "recessed" && "agent-team-section-recessed",variant === "decision" && "agent-team-section-decision",className)} {...props}>{children}</Tag>;
}

export function AgentTeamMetricStrip({ className, children }: { className?:string; children:ReactNode }) {
  return <dl className={cn("agent-team-metric-strip",className)}>{children}</dl>;
}

export function AgentTeamRecordRow({ selected=false, attention=false, className, children, ...props }: { selected?:boolean; attention?:boolean; className?:string; children:ReactNode; [key:string]:unknown }) {
  return <article className={cn("agent-team-record-row",selected && "agent-team-selected",attention && "agent-team-attention",className)} {...props}>{children}</article>;
}
