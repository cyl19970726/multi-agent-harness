import { ArrowRight, CalendarDays, Columns3, TableProperties } from "lucide-react";
import { useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

import { RelationChips } from "./RelationChips";
import type { CompanyOsRecord, CompanyOsStructuredViewData, CompanyOsViewKind } from "./types";

function statusTone(status?: string): "muted" | "warn" | "good" | "info" {
  if (!status) return "muted";
  if (/approval|pending|waiting|proposed/i.test(status)) return "warn";
  if (/complete|approved|on track/i.test(status)) return "good";
  return "info";
}

function RecordCard({ record }: { record: CompanyOsRecord }) {
  return <article data-company-os-ref={record.id} className="rounded-md border border-border bg-card p-3"><div className="flex items-start justify-between gap-2"><h3 className="text-sm font-medium">{record.title}</h3>{record.status && <Badge tone={statusTone(record.status)}>{record.status}</Badge>}</div>{record.type && <p className="mt-1 text-xs text-muted-foreground">{record.type}</p>}{record.links?.length ? <RelationChips className="mt-3" links={record.links} /> : null}</article>;
}

function TableView({ view }: { view: CompanyOsStructuredViewData }) {
  return <div className="overflow-x-auto rounded-lg border border-border"><table className="w-full min-w-[42rem] text-left text-xs"><thead className="bg-muted/50 text-muted-foreground"><tr>{view.columns.map((column) => <th key={column.id} className="border-b border-border px-3 py-2 font-medium">{column.label}</th>)}</tr></thead><tbody>{view.records.map((record) => <tr key={record.id} data-company-os-ref={record.id} className="border-b border-border last:border-0 hover:bg-accent/30">{view.columns.map((column) => <td key={column.id} className="px-3 py-2.5">{column.cell(record)}</td>)}</tr>)}</tbody></table></div>;
}

function BoardView({ records }: { records: CompanyOsRecord[] }) {
  const groups = new Map<string, CompanyOsRecord[]>();
  records.forEach((record) => { const key = record.group ?? record.status ?? "Uncategorized"; groups.set(key, [...(groups.get(key) ?? []), record]); });
  return <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">{[...groups].map(([name, group]) => <section key={name} className="rounded-lg border border-border bg-muted/35 p-2.5"><h2 className="mb-2 px-0.5 text-xs font-semibold">{name} <span className="font-normal text-muted-foreground">{group.length}</span></h2><div className="space-y-2">{group.map((record) => <RecordCard key={record.id} record={record} />)}</div></section>)}</div>;
}

function TimelineView({ records }: { records: CompanyOsRecord[] }) {
  return <ol className="space-y-0 border-l border-border pl-4">{records.map((record) => <li key={record.id} className="relative pb-5 last:pb-0"><span className="absolute -left-[1.31rem] top-1.5 size-2.5 rounded-full border-2 border-card bg-primary" /><time className="text-xs text-muted-foreground">{record.date ?? "No date supplied"}</time><div className="mt-1"><RecordCard record={record} /></div></li>)}</ol>;
}

const viewIcons = { table: TableProperties, board: Columns3, timeline: CalendarDays };

/** Standard record projection with a local presentation switch and an explicit fallback route. */
export function StructuredDocumentView({
  view,
  initialView = "table",
}: {
  view: CompanyOsStructuredViewData;
  initialView?: CompanyOsViewKind;
}) {
  const allowed: CompanyOsViewKind[] = view.availableViews?.length
    ? view.availableViews
    : ["table", "board", "timeline"];
  const [activeView, setActiveView] = useState<CompanyOsViewKind>(allowed.includes(initialView) ? initialView : allowed[0]);
  const visual = useMemo(() => activeView === "table" ? <TableView view={view} /> : activeView === "board" ? <BoardView records={view.records} /> : <TimelineView records={view.records} />, [activeView, view]);
  return <section data-company-os-page="business-module-focus" data-company-os-fixture={view.fixtureId} data-company-os-ref={view.id} data-company-os-ready="true" className="space-y-4"><header className="flex flex-wrap items-end justify-between gap-3"><div><h1 className="text-2xl font-semibold tracking-tight">{view.title}</h1>{view.description && <p className="mt-1 text-sm text-muted-foreground">{view.description}</p>}</div><div className="flex rounded-md border border-border bg-card p-0.5" role="tablist" aria-label="Record view"><>{allowed.map((kind) => { const Icon = viewIcons[kind]; return <Button key={kind} type="button" size="sm" variant={activeView === kind ? "secondary" : "ghost"} role="tab" aria-selected={activeView === kind} onClick={() => setActiveView(kind)}><Icon />{kind}</Button>; })}</></div></header>
    {view.records.length ? visual : <p role="status" className="rounded-lg border border-dashed border-border p-6 text-sm text-muted-foreground">No records match this view.</p>}
    <div className="grid gap-4 border-t border-border pt-4 md:grid-cols-2"><section><h2 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Source records</h2><RelationChips links={view.sourceLinks} /></section><section><h2 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Result records</h2><RelationChips links={view.resultLinks} /></section></div>
    {view.fallback && <aside className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border bg-muted/35 px-3 py-2.5"><p className="text-xs leading-5 text-muted-foreground">{view.fallback.description ?? "This standard view remains available if a custom page is unavailable."}</p>{view.fallback.href ? <a className="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline" href={view.fallback.href}>{view.fallback.label}<ArrowRight className="size-3" /></a> : <span className="text-xs font-medium text-foreground">{view.fallback.label}</span>}</aside>}
  </section>;
}
