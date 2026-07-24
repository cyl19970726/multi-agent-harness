import { ArrowRight, CalendarDays, Code2, Columns3, ShieldCheck, TableProperties } from "lucide-react";
import { useMemo, useRef, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

import { buildDocsRelationCommand, buildDocsTypedRecordCommand, buildDocsViewCommand } from "./documentAction";
import { RelationChips } from "./RelationChips";
import type { CompanyOsDocsActionCommand, CompanyOsRecord, CompanyOsStructuredViewData, CompanyOsViewKind } from "./types";

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

function StandardViewProvenance({ view }: { view: CompanyOsStructuredViewData }) {
  const provenance = view.provenance;
  if (!provenance) return null;
  const facts = [
    { label: "Module scope", value: provenance.moduleLabel ?? provenance.moduleId ?? "Unscoped", ref: provenance.moduleId },
    { label: "Native View", value: provenance.viewTitle ?? provenance.viewId ?? "Standard projection", ref: provenance.viewId },
    { label: "Source kinds", value: provenance.sourceKinds?.length ? provenance.sourceKinds.join(", ") : "typed_record" },
    { label: "Query", value: provenance.querySummary ?? "Projection supplied by module scope" },
    { label: "Records", value: `${provenance.recordCount ?? view.records.length}` },
  ];
  return (
    <section className="rounded-lg border border-border bg-card/70 p-3" aria-label="Standard view provenance" data-docs-standard-view-provenance="true" data-docs-standard-view-module={provenance.moduleId} data-docs-standard-view-ref={provenance.viewId}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Standard View provenance</h2>
        <span className="rounded-full border border-border px-2 py-0.5 text-[10px] text-muted-foreground">View is presentation, not a second truth</span>
      </div>
      <dl className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-5">
        {facts.map((fact) => (
          <div key={fact.label} data-company-os-ref={fact.ref} className="rounded-md border border-border bg-background px-2 py-1.5">
            <dt className="text-[10px] uppercase tracking-wider text-muted-foreground">{fact.label}</dt>
            <dd className="mt-0.5 break-words text-xs font-medium text-foreground">{fact.value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

function StandardViewConfiguration({ view }: { view: CompanyOsStructuredViewData }) {
  const configuration = view.configuration;
  if (!configuration) return null;
  const filterLabel = configuration.filters?.length
    ? configuration.filters.map((filter) => `${filter.field}=${filter.value}`).join(", ")
    : "No saved filters";
  const facts = [
    { label: "Mode", value: configuration.mode ?? "table" },
    { label: "Filter", value: filterLabel },
    { label: "Group by", value: configuration.groupBy ?? "Not saved" },
    { label: "Sort by", value: configuration.sortBy ?? "Not saved" },
  ];
  return (
    <section className="rounded-lg border border-border bg-muted/25 p-3" aria-label="Standard view configuration" data-docs-standard-view-configuration="true">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Saved View configuration</h2>
        <span className="rounded-full border border-border px-2 py-0.5 text-[10px] text-muted-foreground">Configuration is stored in native View.query</span>
      </div>
      <dl className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
        {facts.map((fact) => (
          <div key={fact.label} className="rounded-md border border-border bg-background px-2 py-1.5">
            <dt className="text-[10px] uppercase tracking-wider text-muted-foreground">{fact.label}</dt>
            <dd className="mt-0.5 break-words text-xs font-medium text-foreground">{fact.value}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-2 text-[11px] leading-5 text-muted-foreground">This controls presentation over the same source records. It does not copy, mutate, or approve TypedRecords, WorkItems, Approvals, or FinancialRecords.</p>
    </section>
  );
}

function CustomPageContractCard({ view }: { view: CompanyOsStructuredViewData }) {
  const page = view.customPage;
  if (!page) return null;
  const facts = [
    { label: "Definition", value: page.definitionId },
    { label: "Active package", value: [page.activePackageId, page.activeVersion].filter(Boolean).join("@") || "Not bound" },
    { label: "Latest package", value: [page.latestPackageId, page.latestVersion].filter(Boolean).join("@") || "No candidate" },
    { label: "Fallback View", value: page.fallbackViewId ?? "Not supplied" },
    { label: "Owner", value: page.ownerLabel ?? "Owner ref unresolved" },
  ];
  return (
    <section
      className="rounded-xl border border-primary/25 bg-primary/5 p-4"
      aria-label="Code-declared custom page contract"
      data-docs-custom-page-contract="true"
      data-docs-custom-page-definition={page.definitionId}
      data-docs-custom-page-status={page.status}
      data-docs-custom-page-active-package={page.activePackageId}
      data-docs-custom-page-latest-package={page.latestPackageId}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 gap-3">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-primary/20 bg-card text-primary">
            <Code2 className="size-4" aria-hidden />
          </div>
          <div className="min-w-0">
            <h2 className="text-sm font-semibold">Code-declared custom page</h2>
            <p className="mt-1 max-w-3xl text-xs leading-5 text-muted-foreground">{page.purpose ?? "This module may use a custom page when standard Views are not expressive enough."}</p>
          </div>
        </div>
        <Badge tone={page.status === "active" ? "good" : page.status === "candidate_recorded" ? "warn" : "muted"}>{page.statusLabel}</Badge>
      </div>
      <dl className="mt-3 grid gap-2 sm:grid-cols-2 xl:grid-cols-5">
        {facts.map((fact) => (
          <div key={fact.label} className="rounded-md border border-border bg-background/80 px-2 py-1.5">
            <dt className="text-[10px] uppercase tracking-wider text-muted-foreground">{fact.label}</dt>
            <dd className="mt-0.5 break-words text-xs font-medium text-foreground">{fact.value}</dd>
          </div>
        ))}
      </dl>
      <div className="mt-3 grid gap-3 lg:grid-cols-3">
        <div className="rounded-md border border-border bg-background/70 p-2">
          <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Declared queries</h3>
          <p className="mt-1 text-xs leading-5 text-foreground">{page.allowedQueries.length ? page.allowedQueries.join(" · ") : "No custom query scope declared."}</p>
        </div>
        <div className="rounded-md border border-border bg-background/70 p-2">
          <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Declared Actions</h3>
          <p className="mt-1 text-xs leading-5 text-foreground">{page.declaredActions.length ? page.declaredActions.join(", ") : "No write action declared."}</p>
        </div>
        <div className="rounded-md border border-border bg-background/70 p-2">
          <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Visual contract</h3>
          <p className="mt-1 break-words text-xs leading-5 text-foreground">{page.visualContractRef ?? "No visual contract supplied."}</p>
        </div>
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2 text-[11px] leading-5 text-muted-foreground">
        <ShieldCheck className="size-3.5 text-primary" aria-hidden />
        <span data-docs-custom-page-boundary="true">{page.boundaryNote}</span>
        {page.artifactRef && <span className="rounded-full border border-border px-2 py-0.5">artifact: {page.artifactRef}</span>}
        {page.integrityDigest && <span className="rounded-full border border-border px-2 py-0.5">digest: {page.integrityDigest}</span>}
      </div>
    </section>
  );
}

/** Standard record projection with a local presentation switch and an explicit fallback route. */
export function StructuredDocumentView({
  view,
  initialView = "table",
  actionEnabled = false,
  onDocsAction,
}: {
  view: CompanyOsStructuredViewData;
  initialView?: CompanyOsViewKind;
  actionEnabled?: boolean;
  onDocsAction?: (command: CompanyOsDocsActionCommand, capabilityToken: string) => Promise<boolean>;
}) {
  const allowed: CompanyOsViewKind[] = view.availableViews?.length
    ? view.availableViews
    : ["table", "board", "timeline"];
  const [activeView, setActiveView] = useState<CompanyOsViewKind>(allowed.includes(initialView) ? initialView : allowed[0]);
  const [capabilityToken, setCapabilityToken] = useState("");
  const [recordTitle, setRecordTitle] = useState("");
  const [recordType, setRecordType] = useState("record");
  const [viewTitle, setViewTitle] = useState("");
  const [viewMode, setViewMode] = useState<CompanyOsViewKind>("table");
  const [viewFilterField, setViewFilterField] = useState("record_type");
  const [viewFilterValue, setViewFilterValue] = useState("");
  const [viewGroupBy, setViewGroupBy] = useState("");
  const [viewSortBy, setViewSortBy] = useState("updated_at");
  const [relationRecordId, setRelationRecordId] = useState("");
  const [feedback, setFeedback] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const intents = useRef<Record<string, string>>({});
  const canAuthor = Boolean(actionEnabled && onDocsAction && view.authoring);
  const unavailableReason = !actionEnabled
    ? "Connect a Store-live project and provide a session capability before dispatching governed module Docs actions."
    : !view.authoring
      ? "This projection does not expose typed_record.append, view.append, and relation.append policies."
      : !capabilityToken.trim()
        ? "Enter the session capability before writing Docs truth."
        : undefined;
  async function submitCommand(kind: "typed-record" | "view" | "relation") {
    if (!canAuthor || !onDocsAction || !capabilityToken.trim()) return;
    const createdAt = new Date().toISOString();
    const id = intents.current[kind] ?? `action-browser-docs-${kind}-${crypto.randomUUID()}`;
    intents.current[kind] = id;
    setSubmitting(true);
    setFeedback(null);
    try {
      const command = kind === "typed-record"
        ? buildDocsTypedRecordCommand({ view, title: recordTitle, recordType, commandId: id, createdAt })
        : kind === "view"
          ? buildDocsViewCommand({
              view,
              title: viewTitle,
              mode: viewMode,
              sourceKinds: ["typed_record"],
              query: {
                ...(viewFilterField.trim() && viewFilterValue.trim() ? { filters: [{ field: viewFilterField.trim(), value: viewFilterValue.trim() }] } : {}),
                ...(viewGroupBy.trim() ? { group_by: viewGroupBy.trim() } : {}),
                ...(viewSortBy.trim() ? { sort_by: viewSortBy.trim() } : {}),
              },
              commandId: id,
              createdAt,
            })
          : buildDocsRelationCommand({ view, typedRecordId: relationRecordId, commandId: id, createdAt });
      const accepted = await onDocsAction(command, capabilityToken.trim());
      if (accepted) {
        if (kind === "typed-record") setRecordTitle("");
        if (kind === "view") {
          setViewTitle("");
          setViewFilterValue("");
        }
        if (kind === "relation") setRelationRecordId("");
        setCapabilityToken("");
        delete intents.current[kind];
      }
      setFeedback(accepted ? `${command.command_name} recorded in Store truth.` : `${command.command_name} was not recorded. Review the action error and retry with the same intent.`);
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }
  const visual = useMemo(() => activeView === "table" ? <TableView view={view} /> : activeView === "board" ? <BoardView records={view.records} /> : <TimelineView records={view.records} />, [activeView, view]);
  return <section data-company-os-page="business-module-focus" data-company-os-fixture={view.fixtureId} data-company-os-ref={view.id} data-company-os-ready="true" className="space-y-4"><header className="flex flex-wrap items-end justify-between gap-3"><div><h1 className="text-2xl font-semibold tracking-tight">{view.title}</h1>{view.description && <p className="mt-1 text-sm text-muted-foreground">{view.description}</p>}</div><div className="flex rounded-md border border-border bg-card p-0.5" role="tablist" aria-label="Record view"><>{allowed.map((kind) => { const Icon = viewIcons[kind]; return <Button key={kind} type="button" size="sm" variant={activeView === kind ? "secondary" : "ghost"} role="tab" aria-selected={activeView === kind} onClick={() => setActiveView(kind)}><Icon />{kind}</Button>; })}</></div></header>
    <CustomPageContractCard view={view} />
    <StandardViewProvenance view={view} />
    <StandardViewConfiguration view={view} />
    {view.records.length ? visual : <p role="status" className="rounded-lg border border-dashed border-border p-6 text-sm text-muted-foreground" data-docs-standard-view-empty="true">No records match this standard View. Empty state means the declared query returned no records; it does not delete the BusinessModule, Document, or TypedRecord truth.</p>}
    <div className="grid gap-4 border-t border-border pt-4 md:grid-cols-2"><section><h2 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Source records</h2><RelationChips links={view.sourceLinks} /></section><section><h2 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Result records</h2><RelationChips links={view.resultLinks} /></section></div>
    <section className="rounded-lg border border-border bg-card/70 p-4" aria-label="Store-live module Docs authoring" data-docs-authoring-panel="business-module-focus">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold">Module Docs authoring</h2>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">Create source-linked records, standard views, and Document ↔ TypedRecord relations through governed Docs Actions.</p>
        </div>
        <span className="rounded-full border border-border px-2 py-1 text-[11px] text-muted-foreground">{canAuthor ? "Store-live Action" : "Read-only"}</span>
      </div>
      <div className="mt-3 grid gap-3 lg:grid-cols-3">
        <div className="space-y-2">
          <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Session capability" value={capabilityToken} onChange={(event) => setCapabilityToken(event.target.value)} disabled={!actionEnabled || submitting} aria-label="Company OS session capability" />
          <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Record title" value={recordTitle} onChange={(event) => setRecordTitle(event.target.value)} disabled={!canAuthor || submitting} aria-label="TypedRecord title" />
          <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Record type" value={recordType} onChange={(event) => setRecordType(event.target.value)} disabled={!canAuthor || submitting} aria-label="TypedRecord type" />
          <Button size="sm" variant="outline" className="w-full justify-center" disabled={!canAuthor || !capabilityToken.trim() || !recordTitle.trim() || !recordType.trim() || submitting} title={unavailableReason} onClick={() => void submitCommand("typed-record")}>Create TypedRecord</Button>
        </div>
        <div className="space-y-2">
          <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="New view title" value={viewTitle} onChange={(event) => setViewTitle(event.target.value)} disabled={!canAuthor || submitting} aria-label="View title" />
          <select className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" value={viewMode} onChange={(event) => setViewMode(event.target.value as CompanyOsViewKind)} disabled={!canAuthor || submitting} aria-label="View mode">
            <option value="table">table</option>
            <option value="board">board</option>
            <option value="timeline">timeline</option>
          </select>
          <div className="grid grid-cols-2 gap-2">
            <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Filter field" value={viewFilterField} onChange={(event) => setViewFilterField(event.target.value)} disabled={!canAuthor || submitting} aria-label="View filter field" />
            <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Filter value" value={viewFilterValue} onChange={(event) => setViewFilterValue(event.target.value)} disabled={!canAuthor || submitting} aria-label="View filter value" />
          </div>
          <div className="grid grid-cols-2 gap-2">
            <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Group by" value={viewGroupBy} onChange={(event) => setViewGroupBy(event.target.value)} disabled={!canAuthor || submitting} aria-label="View group by" />
            <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Sort by" value={viewSortBy} onChange={(event) => setViewSortBy(event.target.value)} disabled={!canAuthor || submitting} aria-label="View sort by" />
          </div>
          <Button size="sm" variant="outline" className="w-full justify-center" disabled={!canAuthor || !capabilityToken.trim() || !viewTitle.trim() || submitting} title={unavailableReason} onClick={() => void submitCommand("view")}>Create View</Button>
        </div>
        <div className="space-y-2">
          <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="TypedRecord id to link" value={relationRecordId} onChange={(event) => setRelationRecordId(event.target.value)} disabled={!canAuthor || submitting} aria-label="TypedRecord id to link" />
          <Button size="sm" className="w-full justify-center" disabled={!canAuthor || !capabilityToken.trim() || !relationRecordId.trim() || submitting} title={unavailableReason} onClick={() => void submitCommand("relation")}>{submitting ? "Writing…" : "Link Relation"}</Button>
        </div>
      </div>
      <p className="mt-3 text-[11px] leading-5 text-muted-foreground" data-docs-authoring-state={canAuthor ? "available" : "unavailable"}>{feedback ?? unavailableReason ?? "The server validates module scope, source Document, actor permission, policy and idempotency before writing."}</p>
    </section>
    {view.fallback && <aside className="flex flex-wrap items-center justify-between gap-3 rounded-md border border-border bg-muted/35 px-3 py-2.5"><p className="text-xs leading-5 text-muted-foreground">{view.fallback.description ?? "This standard view remains available if a custom page is unavailable."}</p>{view.fallback.href ? <a className="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline" href={view.fallback.href}>{view.fallback.label}<ArrowRight className="size-3" /></a> : <span className="text-xs font-medium text-foreground">{view.fallback.label}</span>}</aside>}
  </section>;
}
