import { ChevronDown, ChevronRight, Database, FilePlus2, FolderKanban, Search, Sparkles } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { ArtField, EditorialTitle, ObjectEmblem } from "../visuals";

import { RelationChips } from "./RelationChips";
import type { CompanyOsWorkspaceData, CompanyOsWorkspaceTreeItem } from "./types";

function TreeItem({ item, depth = 0 }: { item: CompanyOsWorkspaceTreeItem; depth?: number }) {
  const hasChildren = Boolean(item.children?.length);
  const body = <span className="flex min-w-0 flex-1 items-start gap-1.5"><FolderKanban className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden /> <span className="min-w-0 whitespace-normal leading-4">{item.label}{item.meta && <span className="text-muted-foreground"> · {item.meta}</span>}</span></span>;
  const indent = depth === 0 ? "pl-2" : depth === 1 ? "pl-5" : depth === 2 ? "pl-8" : "pl-11";
  const className = cn("flex w-full items-center gap-1.5 rounded-md py-1.5 text-left text-xs text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", indent, item.selected && "bg-primary/10 text-primary");
  return <li>
    {item.href ? <a href={item.href} data-company-os-ref={item.ref} className={className}>{hasChildren ? <ChevronDown className="size-3 shrink-0" /> : <span className="w-3" />}{body}</a> : <div data-company-os-ref={item.ref} className={className}>{hasChildren ? <ChevronDown className="size-3 shrink-0" /> : <span className="w-3" />}{body}</div>}
    {hasChildren && <ul>{item.children?.map((child) => <TreeItem key={child.id} item={child} depth={depth + 1} />)}</ul>}
  </li>;
}

function LinkList({ title, links, empty }: { title: string; links?: CompanyOsWorkspaceData["recentlyUpdated"]; empty: string }) {
  return <section className="rounded-lg border border-border bg-card"><div className="flex items-center justify-between border-b border-border px-3 py-2.5"><h2 className="text-sm font-semibold">{title}</h2></div><div className="p-3">{links?.length ? <RelationChips links={links} /> : <p className="text-xs text-muted-foreground">{empty}</p>}</div></section>;
}

/** Docs home: a company knowledge workspace, not a filesystem browser. */
export function DocsWorkspace({
  workspace,
  onCreate,
}: {
  workspace: CompanyOsWorkspaceData;
  onCreate?: (kind: "page" | "database") => void;
}) {
  return (
    <main data-company-os-page="docs-workspace" data-company-os-fixture={workspace.fixtureId} data-company-os-ready="true" className="company-workbench h-full overflow-auto bg-background">
      <ArtField />
      <div className="relative mx-auto grid min-h-full max-w-[1480px] lg:grid-cols-[240px_minmax(0,1fr)_280px]">
        <aside className="border-b border-border bg-card/55 p-4 backdrop-blur-sm lg:border-b-0 lg:border-r" aria-label="Document tree"><div className={cn("mb-4 flex items-center justify-between rounded-xl border border-transparent px-2 py-2", workspace.rootSelected && "border-primary/20 bg-primary/[0.07] text-primary")}><div className="flex items-center gap-2"><ObjectEmblem kind="docs" className="size-8 rounded-lg" /><p className="text-sm font-semibold">Company</p></div><Button size="icon" variant="ghost" aria-label="Create a document"><FilePlus2 /></Button></div><ul className="space-y-0.5">{workspace.tree.map((item) => <TreeItem key={item.id} item={item} />)}</ul></aside>
        <section className="min-w-0 p-5 sm:p-8"><header className="flex flex-wrap items-start justify-between gap-4 border-b border-border/80 pb-6"><div><p className="text-[11px] font-semibold uppercase tracking-[0.2em] text-primary">Company knowledge</p><EditorialTitle className="mt-3">{workspace.title ?? "Company workspace"}</EditorialTitle><p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">{workspace.description ?? "Documents, typed records, and their connected operating context."}</p></div><div className="flex flex-wrap gap-2"><Button variant="outline" size="sm" onClick={() => onCreate?.("page")}><FilePlus2 />New page</Button><Button size="sm" onClick={() => onCreate?.("database")}><Database />New database</Button></div></header>
          <div className="mt-5 flex h-8 max-w-xl items-center gap-2 rounded-md border border-input bg-card px-2 text-xs text-muted-foreground"><Search className="size-3.5" aria-hidden /><span>Search pages, databases, spaces…</span></div>
          <section className="mt-7"><h2 className="company-editorial-title mb-3 text-2xl">Operating spaces</h2><div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{workspace.spaces.map((space) => <a key={space.id} href={space.href} className="group rounded-xl border border-border bg-card/80 p-4 shadow-sm transition-all hover:-translate-y-0.5 hover:border-primary/40 hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><div className="flex items-start justify-between gap-2"><ObjectEmblem kind={/brand|legal|trademark/i.test(space.name) ? "module" : "docs"} className="size-9 rounded-lg" />{space.status && <Badge tone="warn">{space.status}</Badge>}</div><h3 className="mt-5 font-semibold">{space.name}</h3>{space.summary && <p className="mt-1 text-xs leading-5 text-muted-foreground">{space.summary}</p>}{space.countLabel && <p className="mt-3 text-xs text-muted-foreground">{space.countLabel}</p>}</a>)}</div></section>
          <div className="mt-6 grid gap-3 md:grid-cols-2"><LinkList title="Recently updated" links={workspace.recentlyUpdated} empty="No recent document updates." /><LinkList title="Templates" links={workspace.templates} empty="No templates are available in this space." /><LinkList title="Databases" links={workspace.databases} empty="No typed record views are available." /></div>
        </section>
        <aside className="border-t border-border bg-card/55 p-5 backdrop-blur-sm lg:border-l lg:border-t-0" aria-label="Structure context"><section className="rounded-xl border border-border bg-card/75 p-4"><div className="flex items-center gap-2"><ObjectEmblem kind="docs" className="size-8 rounded-lg" /><h2 className="company-editorial-title text-xl">Structure health</h2></div>{workspace.structureNotes?.length ? <dl className="mt-4 space-y-3">{workspace.structureNotes.map((note) => <div key={note.label} className="flex items-center justify-between gap-3 text-xs"><dt className="text-muted-foreground">{note.label}</dt><dd className={note.tone === "warning" ? "font-medium text-status-warn" : "font-medium text-foreground"}>{note.value}</dd></div>)}</dl> : <p className="mt-3 text-xs leading-5 text-muted-foreground">Structure signals appear when a governed audit supplies them.</p>}</section>
          <section className="mt-6 space-y-2"><h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Connected structure</h2><p className="text-xs leading-5 text-muted-foreground">These records are affected by the current space structure.</p><RelationChips links={workspace.structureLinks ?? workspace.suggestions} emptyLabel="No connected structure is supplied." /></section>
          <section className="mt-6 space-y-2"><h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Suggested connections</h2><RelationChips links={workspace.suggestions} emptyLabel="No suggestions supplied." /></section>
          {workspace.proposal && <section data-company-os-ref={workspace.proposal.id} className="mt-6 rounded-lg border border-primary/20 bg-primary/5 p-3"><Sparkles className="size-4 text-primary" aria-hidden /><h2 className="mt-2 text-sm font-semibold">Structure proposal</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">A proposal is visible for review; it does not change the structure by itself.</p><a href={workspace.proposal.href} className="mt-3 inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline">{workspace.proposal.label}<ChevronRight className="size-3" /></a></section>}
        </aside>
      </div>
    </main>
  );
}
