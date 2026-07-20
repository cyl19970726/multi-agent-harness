import { ArrowRight, FileText, Landmark, ListTodo, ShieldCheck, UserRound } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { RelationChips } from "./RelationChips";
import type { CompanyOsHomeData } from "./types";

function Summary({
  title,
  icon: Icon,
  items,
}: {
  title: string;
  icon: typeof ListTodo;
  items: Array<{ id?: string; label: string; value: string; detail?: string; financialRecordType?: "commitment" | "invoice" | "payment" | "budget" }>;
}) {
  return <section className="rounded-lg border border-border bg-card"><header className="flex items-center gap-2 border-b border-border px-3 py-2.5"><Icon className="size-4 text-primary" aria-hidden /><h2 className="text-sm font-semibold">{title}</h2></header><dl className="divide-y divide-border">{items.map((item) => <div key={item.label} data-company-os-ref={item.id} data-financial-record-type={item.financialRecordType} className="px-3 py-3"><dt className="text-xs text-muted-foreground">{item.label}</dt><dd className="mt-1 text-lg font-semibold tracking-tight">{item.value}</dd>{item.detail && <p className="mt-1 text-xs leading-5 text-muted-foreground">{item.detail}</p>}</div>)}</dl></section>;
}

/** A compact company morning review, intentionally backed by supplied document and record links. */
export function CompanyHome({
  data,
  onReviewDecision,
}: {
  data: CompanyOsHomeData;
  onReviewDecision?: (decision: NonNullable<CompanyOsHomeData["decisionRequired"]>) => void;
}) {
  const actorType = data.decisionActor?.kind === "human" ? "Human" : data.decisionActor?.kind === "agent" ? "Standing Agent" : data.decisionActor?.kind === "external" ? "External" : "Service";
  const decisionCta = data.decisionRequired && (onReviewDecision ? <Button size="sm" onClick={() => onReviewDecision(data.decisionRequired!)}>Review decision <ArrowRight /></Button> : data.decisionRequired.href ? <Button asChild size="sm"><a href={data.decisionRequired.href}>Review decision <ArrowRight /></a></Button> : <Button size="sm" disabled>Review decision <ArrowRight /></Button>);
  return <main data-company-os-page="home" data-company-os-fixture={data.fixtureId} data-company-os-ready="true" className="h-full overflow-auto bg-background px-4 py-5 sm:px-6 lg:px-8"><div className="mx-auto max-w-[1180px] space-y-6"><header><p className="text-xs font-medium text-primary">Company OS</p><h1 className="mt-1 text-3xl font-semibold tracking-tight">{data.title}</h1>{data.subtitle && <p className="mt-2 text-sm text-muted-foreground">{data.subtitle}</p>}</header>{data.decisionRequired ? <section data-company-os-ref={data.decisionRequired.id} className="rounded-lg border border-primary/25 bg-card p-4"><div className="flex flex-wrap items-start justify-between gap-4"><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><ShieldCheck className="size-4 text-primary" aria-hidden /><p className="text-sm font-semibold">Decision required</p><Badge tone="warn">Human approval</Badge></div><h2 className="mt-3 text-lg font-semibold">{data.decisionRequired.label}</h2>{data.decisionSummary && <p className="mt-1 max-w-2xl text-sm leading-6 text-muted-foreground">{data.decisionSummary}</p>}<div className="mt-4">{decisionCta}</div></div><div className="min-w-[13rem] space-y-3 rounded-md border border-border bg-muted/35 p-3 text-xs">{data.decisionActor && <div data-company-os-ref={data.decisionActor.id} data-actor-type={actorType}><p className="text-muted-foreground">Required approver</p><p className="mt-1 font-medium">{data.decisionActor.name} · {actorType}</p></div>}{data.decisionRequester && <div data-company-os-ref={data.decisionRequester.id} data-actor-type={data.decisionRequester.actorType}><p className="text-muted-foreground">Requested by</p><p className="mt-1 font-medium">{data.decisionRequester.label} · {data.decisionRequester.actorType}</p></div>}</div></div>{data.decisionCollaborators?.length ? <div className="mt-4 border-t border-border pt-3"><div className="mb-2 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground"><UserRound className="size-3.5" />Review contributors</div><RelationChips links={data.decisionCollaborators} /></div> : null}</section> : <section role="status" className="rounded-lg border border-dashed border-border bg-card p-4 text-sm text-muted-foreground">No approval is currently supplied by this projection.</section>}<div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_280px]"><section className="rounded-lg border border-border bg-card"><header className="flex items-center gap-2 border-b border-border px-3 py-2.5"><FileText className="size-4 text-primary" aria-hidden /><h2 className="text-sm font-semibold">Document changes</h2></header><div className="p-3"><RelationChips links={data.changes} emptyLabel="No document changes supplied." /></div></section><Summary title="Work" icon={ListTodo} items={data.workSummary} /><Summary title="Finance" icon={Landmark} items={data.financeSummary} /></div></div></main>;
}
