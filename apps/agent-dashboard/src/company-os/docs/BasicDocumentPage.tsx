import { Link2, MoreHorizontal } from "lucide-react";

import { Button } from "@/components/ui/button";
import { DocSection, DocumentSurface } from "@/components/workbench/atoms";

import { RelationChips } from "./RelationChips";
import type { CompanyOsDocumentBlock, CompanyOsDocumentPageData } from "./types";

function SimpleTable({ block }: { block: Extract<CompanyOsDocumentBlock, { type: "table" }> }) {
  const { columns, rows, caption } = block.table;
  return (
    <div className="max-w-full overflow-x-auto rounded-md border border-border">
      <table className="w-full min-w-[36rem] border-collapse text-left text-xs">
        {caption && <caption className="border-b border-border bg-muted/40 px-3 py-2 text-left font-medium text-foreground">{caption}</caption>}
        <thead className="bg-muted/40 text-muted-foreground">
          <tr>{columns.map((column) => <th key={column} className="whitespace-nowrap border-b border-border px-3 py-2 font-medium">{column}</th>)}</tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={index} className="border-b border-border last:border-0">
              {row.map((cell, cellIndex) => <td key={cellIndex} className="px-3 py-2 text-foreground">{cell}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Block({ block }: { block: CompanyOsDocumentBlock }) {
  switch (block.type) {
    case "paragraph":
      return <p className="max-w-[46rem] break-words text-sm leading-6 text-foreground">{block.content}</p>;
    case "heading": {
      const Heading = block.level === 3 ? "h3" : "h2";
      return <Heading className={block.level === 3 ? "pt-2 text-base font-semibold" : "pt-4 text-lg font-semibold tracking-tight"}>{block.content}</Heading>;
    }
    case "bullets":
      return <ul className="space-y-1.5 pl-5 text-sm leading-6 marker:text-muted-foreground">{block.items.map((item, index) => <li key={index}>{item}</li>)}</ul>;
    case "callout":
      return <aside className={block.tone === "warning" ? "rounded-md border border-status-warn/35 bg-status-warn/10 px-3 py-2.5" : block.tone === "success" ? "rounded-md border border-status-good/35 bg-status-good/10 px-3 py-2.5" : "rounded-md border border-border bg-muted/45 px-3 py-2.5"}>
        {block.title && <h3 className="text-sm font-semibold">{block.title}</h3>}
        <div className="mt-1 text-sm leading-6 text-foreground">{block.content}</div>
      </aside>;
    case "table":
      return <SimpleTable block={block} />;
    case "relations":
      return <DocSection label={block.label ?? "Connected records"}><RelationChips links={block.links} /></DocSection>;
    case "custom":
      return <>{block.content}</>;
  }
}

function ContextBlock({ label, links }: { label: string; links?: CompanyOsDocumentPageData["sourceLinks"] }) {
  if (!links?.length) return null;
  return <section className="space-y-2"><h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</h2><RelationChips links={links} /></section>;
}

/** A focused, free-form Company OS page. It displays data supplied by the host; it owns no mutations. */
export function BasicDocumentPage({
  document,
  onRequestAction,
}: {
  document: CompanyOsDocumentPageData;
  onRequestAction?: (action: "new-action" | "ask-agent", document: CompanyOsDocumentPageData) => void;
}) {
  return (
    <main data-company-os-page="document-focus" data-company-os-fixture={document.fixtureId} data-company-os-ref={document.id} data-company-os-ready="true" className="h-full overflow-auto bg-background px-4 py-5 sm:px-6 lg:px-8">
      <div className="mx-auto grid min-w-0 max-w-[1250px] gap-7 lg:grid-cols-[minmax(0,1fr)_260px]">
        <DocumentSurface className="mx-0 min-w-0 max-w-[800px] space-y-6">
          <header className="space-y-3 border-b border-border pb-5">
            {document.breadcrumb?.length ? <nav aria-label="Breadcrumb" className="text-xs text-muted-foreground">{document.breadcrumb.join(" / ")}</nav> : null}
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 space-y-2"><h1 className="text-2xl font-semibold tracking-tight text-foreground sm:text-3xl">{document.title}</h1>{document.description && <p className="max-w-2xl text-sm leading-6 text-muted-foreground">{document.description}</p>}</div>
              <div className="flex shrink-0 gap-1.5"><Button variant="outline" size="sm" onClick={() => onRequestAction?.("new-action", document)}>New action</Button><Button size="sm" onClick={() => onRequestAction?.("ask-agent", document)}>Ask an agent</Button><Button variant="ghost" size="icon" aria-label="More document options"><MoreHorizontal /></Button></div>
            </div>
            {document.properties?.length ? <dl className="flex min-w-0 flex-wrap gap-1.5">{document.properties.map((property, index) => <div key={`${property.ref ?? "property"}:${property.label}:${index}`} data-company-os-ref={property.ref} data-actor-type={property.actorType} className="flex max-w-full min-w-0 items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-xs"><dt className="shrink-0 text-muted-foreground">{property.label}:</dt><dd className="min-w-0 break-words font-medium text-foreground">{property.value}</dd></div>)}</dl> : null}
          </header>
          <article className="min-w-0 space-y-4">{document.blocks.map((block) => <Block key={block.id} block={block} />)}</article>
          {document.updatedLabel && <p className="border-t border-border pt-4 text-xs text-muted-foreground">{document.updatedLabel}</p>}
        </DocumentSurface>
        <aside className="space-y-5 border-t border-border pt-5 lg:border-l lg:border-t-0 lg:pl-5 lg:pt-0" aria-label="Document context">
          <ContextBlock label="Source" links={document.sourceLinks} />
          <ContextBlock label="Results" links={document.resultLinks} />
          <ContextBlock label="Connected records" links={document.connectedRecords} />
          {document.activity?.length ? <section className="space-y-2"><h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Activity</h2><ol className="space-y-3 border-l border-border pl-3">{document.activity.map((item) => <li key={item.id} className="space-y-0.5"><p className="text-xs font-medium text-foreground">{item.label}</p>{item.detail && <p className="text-xs leading-5 text-muted-foreground">{item.detail}</p>}{item.at && <time className="text-[11px] text-muted-foreground">{item.at}</time>}</li>)}</ol></section> : null}
          {!document.sourceLinks?.length && !document.resultLinks?.length && !document.connectedRecords?.length && <p className="rounded-md border border-dashed border-border p-3 text-xs leading-5 text-muted-foreground"><Link2 className="mr-1 inline size-3.5" />Connected records appear here when the host resolves them.</p>}
        </aside>
      </div>
    </main>
  );
}
