import { ArrowUpRight, Bot, ChevronDown, ChevronRight, CircleAlert, FilePlus2, FolderKanban, Info, Search, Sparkles } from "lucide-react";
import { useMemo, useState } from "react";

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

/** Docs home: a company knowledge workspace, not a filesystem browser. */
export function DocsWorkspace({
  workspace,
  onCreate,
}: {
  workspace: CompanyOsWorkspaceData;
  onCreate?: (kind: "page" | "database") => void;
}) {
  const maintainedBy = workspace.maintainers ?? [];
  const templates = workspace.templates ?? [];
  const [searchQuery, setSearchQuery] = useState("");
  const normalizedSearch = searchQuery.trim().toLowerCase();
  const matchesSearch = (values: Array<string | undefined>) => !normalizedSearch || values.some((value) => value?.toLowerCase().includes(normalizedSearch));
  const filteredSpaces = useMemo(
    () => workspace.spaces.filter((space) => matchesSearch([space.name, space.summary, space.countLabel])),
    [workspace.spaces, normalizedSearch],
  );
  const filteredTemplates = useMemo(
    () => templates.filter((template) => matchesSearch([template.label, template.id, `${template.templateBlockIds.length} blocks`])),
    [templates, normalizedSearch],
  );
  const filteredRecent = useMemo(
    () => (workspace.recentlyUpdated ?? []).filter((link) => matchesSearch([link.label, link.id, link.meta, link.kind])),
    [workspace.recentlyUpdated, normalizedSearch],
  );
  return (
    <main data-company-os-page="docs-workspace" data-company-os-fixture={workspace.fixtureId} data-company-os-ready="true" className="company-workbench h-full overflow-auto bg-background">
      <ArtField />
      <div className="relative mx-auto grid min-h-full max-w-[1480px] lg:grid-cols-[240px_minmax(0,1fr)_280px]">
        <aside className="hidden border-b border-border bg-card/55 p-4 backdrop-blur-sm lg:block lg:border-b-0 lg:border-r" aria-label="Document tree"><div className={cn("mb-4 flex items-center justify-between rounded-xl border border-transparent px-2 py-2", workspace.rootSelected && "border-primary/20 bg-primary/[0.07] text-primary")}><div className="flex items-center gap-2"><ObjectEmblem kind="docs" className="size-8 rounded-lg" /><p className="text-sm font-semibold">Company</p></div><Button size="icon" variant="ghost" aria-label="Create a document"><FilePlus2 /></Button></div><ul className="space-y-0.5">{workspace.tree.map((item) => <TreeItem key={item.id} item={item} />)}</ul></aside>
        <section className="min-w-0 px-6 py-7 sm:px-9"><header className="flex flex-wrap items-start justify-between gap-4"><div><p className="text-[11px] text-muted-foreground">Company&nbsp;&nbsp; / &nbsp;&nbsp;Operating system</p><EditorialTitle className="mt-7">Company knowledge</EditorialTitle><p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">{workspace.description ?? "The source of truth for how the company works—connecting operating structure, durable records, decisions, and accountable actors."}</p></div><div className="flex flex-wrap gap-2"><Button variant="outline" size="sm" onClick={() => onCreate?.("page")}><FilePlus2 />New page</Button><Button variant="outline" size="sm"><Sparkles />Ask an agent</Button></div></header>
          <section className="mt-7 flex max-w-xl items-start gap-3 border-b border-border pb-6"><Info className="mt-0.5 size-5 shrink-0 text-primary" /><div><h2 className="company-editorial-title text-xl">How this company works</h2><p className="mt-2 text-sm leading-6 text-muted-foreground">We organize durable context into clear operating areas. Documents create governed work; results and evidence return here.</p></div></section>
          <section className="mt-6 rounded-xl border border-border bg-card/65 p-3" aria-label="Projection-backed Docs search" data-docs-workspace-search="projection">
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <Search className="size-3.5" />
              <span className="sr-only">Search projection-backed Docs workspace</span>
              <input
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
                placeholder="Filter spaces, templates, and recent records from this projection…"
                className="h-8 min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
                aria-label="Search projection-backed Docs workspace"
              />
            </label>
            <p className="mt-2 text-[11px] leading-4 text-muted-foreground" data-docs-workspace-search-boundary="projection-only">
              Filters the current projection only: {filteredSpaces.length} area{filteredSpaces.length === 1 ? "" : "s"}, {filteredTemplates.length} template{filteredTemplates.length === 1 ? "" : "s"}, {filteredRecent.length} recent record{filteredRecent.length === 1 ? "" : "s"}.
            </p>
          </section>
          <div className="mt-6 grid gap-6 xl:grid-cols-[0.9fr_1.1fr]">
            <section className="border-r-0 border-border xl:border-r xl:pr-6"><h2 className="company-editorial-title text-2xl">Operating areas</h2><div className="mt-3 divide-y divide-border/70">{filteredSpaces.length ? filteredSpaces.map((space) => <a key={space.id} href={space.href} className="group flex items-start gap-3 py-2.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><ObjectEmblem kind={/brand|legal|trademark/i.test(space.name) ? "module" : "docs"} className="size-7 rounded-md" /><span className="min-w-0 flex-1"><span className="block text-sm font-medium group-hover:text-primary">{space.name}</span><span className="mt-0.5 block text-[11px] leading-4 text-muted-foreground">{space.summary ?? space.countLabel ?? "Connected company knowledge"}</span></span><ArrowUpRight className="mt-1 size-3 text-muted-foreground opacity-0 transition group-hover:opacity-100" /></a>) : <p className="py-3 text-xs leading-5 text-muted-foreground" data-docs-workspace-search-empty="spaces">No operating area in this projection matches the current filter.</p>}</div></section>
            <section><h2 className="company-editorial-title text-2xl">Needs structure</h2><p className="mt-2 text-xs leading-5 text-muted-foreground">Governance proposals identify areas that need a durable home or accountable owner.</p>{workspace.proposal ? <a data-company-os-ref={workspace.proposal.id} href={workspace.proposal.href} className="mt-4 flex items-center gap-3 rounded-lg border border-primary/25 bg-primary/[0.035] p-3 hover:bg-primary/[0.07]"><ObjectEmblem kind="module" className="size-9 rounded-lg" /><span className="min-w-0 flex-1"><span className="block text-sm font-medium">{workspace.proposal.label}</span><span className="text-xs text-muted-foreground">Structure proposal awaiting governed review</span></span><Badge tone="warn">Proposed</Badge></a> : <div className="mt-4 flex gap-2 rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground"><CircleAlert className="size-4" />No unresolved structure proposal.</div>}</section>
          </div>
          <section className="mt-6 border-t border-border pt-5" aria-label="Template library" data-docs-template-library="true">
            <div className="flex flex-wrap items-end justify-between gap-3">
              <div>
                <h2 className="company-editorial-title text-xl">Template library</h2>
                <p className="mt-1 text-xs leading-5 text-muted-foreground">Templates are native Documents. They can be used as provenance-only references or explicitly instantiated into ordered Blocks through governed Docs Actions.</p>
              </div>
            <Badge tone="info">{filteredTemplates.length} template{filteredTemplates.length === 1 ? "" : "s"}</Badge>
            </div>
            {filteredTemplates.length ? (
              <div className="mt-3 grid gap-3 md:grid-cols-2">
                {filteredTemplates.slice(0, 4).map((template) => (
                  <article key={template.id} data-company-os-ref={template.id} data-docs-template-block-count={template.templateBlockIds.length} data-docs-template-lifecycle={template.meta ?? "Draft"} className="rounded-xl border border-border bg-card/70 p-3">
                    <div className="flex items-start gap-3">
                      <ObjectEmblem kind="docs" className="size-9 rounded-lg" />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <h3 className="truncate text-sm font-medium">{template.label}</h3>
                          <Badge tone={template.meta === "Active" ? "good" : template.meta === "Archived" ? "muted" : "warn"}>{template.meta ?? "Draft"}</Badge>
                        </div>
                        <p className="mt-1 text-[11px] leading-5 text-muted-foreground">{template.templateBlockIds.length} ordered Block{template.templateBlockIds.length === 1 ? "" : "s"} available for explicit instantiation.</p>
                      </div>
                    </div>
                    <div className="mt-3 grid grid-cols-2 gap-2 text-[11px]">
                      <div className="rounded-md border border-border bg-background/70 px-2 py-1.5"><span className="block font-medium text-foreground">Default</span><span className="text-muted-foreground">template_ref only</span></div>
                      <div className="rounded-md border border-primary/20 bg-primary/[0.04] px-2 py-1.5"><span className="block font-medium text-foreground">Opt-in</span><span className="text-muted-foreground">copy Blocks via Actions</span></div>
                    </div>
                    <p className="mt-2 text-[10px] leading-4 text-muted-foreground">Lifecycle changes use <code>harness company docs template status</code>; archiving a template does not mutate existing Documents that recorded its template_ref.</p>
                  </article>
                ))}
              </div>
            ) : <div className="mt-3 rounded-lg border border-dashed border-border p-3 text-xs leading-5 text-muted-foreground">{templates.length ? "No template Document in this projection matches the current filter." : "No template Documents are supplied by this projection. Docs Governance can propose templates when repeated work patterns appear."}</div>}
            {workspace.templateRecordPolicy && (
              <div className="mt-4 rounded-xl border border-border bg-card/70 p-3" data-docs-template-record-policy={workspace.templateRecordPolicy.status}>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h3 className="text-sm font-semibold">Template → TypedRecord policy</h3>
                    <p className="mt-1 text-xs leading-5 text-muted-foreground">
                      Template instantiation never creates TypedRecords or Relations. After a child Document and TypedRecord exist, link them through an explicit governed Relation.
                    </p>
                  </div>
                  <Badge tone={workspace.templateRecordPolicy.status === "declared" ? "good" : "warn"}>{workspace.templateRecordPolicy.status}</Badge>
                </div>
                <div className="mt-3 grid gap-2 text-[11px] sm:grid-cols-2">
                  <div className="rounded-md border border-border bg-background/70 px-2 py-1.5">
                    <span className="block font-medium text-foreground">Record types</span>
                    <span className="text-muted-foreground">{workspace.templateRecordPolicy.recordTypes.length ? workspace.templateRecordPolicy.recordTypes.join(", ") : "No module record_types declared"}</span>
                  </div>
                  <div className="rounded-md border border-border bg-background/70 px-2 py-1.5">
                    <span className="block font-medium text-foreground">Relation types</span>
                    <span className="text-muted-foreground">{workspace.templateRecordPolicy.relationTypes.length ? workspace.templateRecordPolicy.relationTypes.join(", ") : "No Document → TypedRecord rule declared"}</span>
                  </div>
                </div>
                <code className="mt-3 block break-words rounded-md bg-muted px-2 py-1.5 text-[11px] leading-5 text-muted-foreground">{workspace.templateRecordPolicy.commandHint}</code>
              </div>
            )}
          </section>
          <section className="mt-6 border-t border-border pt-5"><div className="flex items-center justify-between"><h2 className="company-editorial-title text-xl">Recent company records</h2><p className="text-[11px] text-muted-foreground">Projection-backed filter</p></div><div className="mt-3 overflow-hidden rounded-lg border border-border bg-card/70"><div className="grid grid-cols-[minmax(0,1fr)_8rem_6rem] border-b border-border bg-muted/25 px-3 py-2 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground"><span>Record</span><span>System</span><span>Status</span></div>{filteredRecent.length ? filteredRecent.slice(0, 4).map((link) => <a key={link.id} href={link.href} data-company-os-ref={link.id} className="grid grid-cols-[minmax(0,1fr)_8rem_6rem] items-center border-b border-border/70 px-3 py-2.5 text-xs last:border-0 hover:bg-muted/30"><span className="truncate font-medium">{link.label}</span><span className="text-muted-foreground">Docs</span><span className="text-status-good">Linked</span></a>) : <p className="p-3 text-xs text-muted-foreground" data-docs-workspace-search-empty="recent">No recent record in this projection matches the current filter.</p>}</div></section>
          <section className="mt-6"><h2 className="company-editorial-title text-xl">Maintained by agents</h2><p className="mt-1 text-xs text-muted-foreground">Standing Agents keep structure and operating context current through governed Actions.</p><div className="mt-3 grid gap-3 sm:grid-cols-2">{maintainedBy.length ? maintainedBy.map((agent) => <a key={agent.id} href={agent.href} data-company-os-ref={agent.id} className="flex items-center gap-3 rounded-lg border border-border bg-card/65 p-3"><span className="grid size-10 shrink-0 place-items-center rounded-full bg-primary/10 text-primary"><Bot className="size-5" /></span><span className="min-w-0"><span className="block truncate text-sm font-medium">{agent.label}</span><span className="block text-[11px] text-muted-foreground">Standing Agent · governed maintainer</span></span></a>) : <div className="rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground">No Standing Agent maintainer is present in this projection.</div>}</div></section>
        </section>
        <aside className="hidden border-t border-border bg-card/55 p-5 backdrop-blur-sm lg:block lg:border-l lg:border-t-0" aria-label="Structure context"><section className="rounded-xl border border-border bg-card/75 p-4"><div className="flex items-center gap-2"><ObjectEmblem kind="docs" className="size-8 rounded-lg" /><h2 className="company-editorial-title text-xl">Structure health</h2></div>{workspace.structureNotes?.length ? <dl className="mt-4 space-y-3">{workspace.structureNotes.map((note) => <div key={note.label} className="flex items-center justify-between gap-3 text-xs"><dt className="text-muted-foreground">{note.label}</dt><dd className={note.tone === "warning" ? "font-medium text-status-warn" : "font-medium text-foreground"}>{note.value}</dd></div>)}</dl> : <p className="mt-3 text-xs leading-5 text-muted-foreground">Structure signals appear when a governed audit supplies them.</p>}</section>
          <section className="mt-6 space-y-2"><h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Connected structure</h2><p className="text-xs leading-5 text-muted-foreground">These records are affected by the current space structure.</p><RelationChips links={workspace.structureLinks ?? workspace.suggestions} emptyLabel="No connected structure is supplied." /></section>
          <section className="mt-6 space-y-2" aria-label="Docs CLI and skill authoring commands">
            <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">CLI / Skill authoring</h2>
            <p className="text-xs leading-5 text-muted-foreground">Agents use these contracts to change Docs truth. Governance commands require a Human admin; module actions require a declared page policy.</p>
            <div className="space-y-2">
              {workspace.authoringCommands?.map((item) => (
                <article key={item.id} data-docs-authoring-command={item.id} className={cn("rounded-lg border bg-background/75 p-3", item.disabledReason ? "border-dashed border-border" : "border-primary/20")}>
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-xs font-medium">{item.label}</p>
                    <Badge tone={item.scope === "governance" ? "warn" : "info"}>{item.scope === "governance" ? "Governance" : "Action"}</Badge>
                  </div>
                  <code className="mt-2 block break-words rounded-md bg-muted px-2 py-1.5 text-[11px] leading-5 text-muted-foreground">{item.command}</code>
                  {item.disabledReason && <p className="mt-2 text-[11px] leading-4 text-muted-foreground">{item.disabledReason}</p>}
                </article>
              )) ?? <p className="rounded-lg border border-dashed border-border p-3 text-xs leading-5 text-muted-foreground">No Docs authoring command contract is supplied.</p>}
            </div>
          </section>
          <section className="mt-6 space-y-2"><h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Suggested connections</h2><RelationChips links={workspace.suggestions} emptyLabel="No suggestions supplied." /></section>
          {workspace.proposal && <section data-company-os-ref={workspace.proposal.id} className="mt-6 rounded-lg border border-primary/20 bg-primary/5 p-3"><Sparkles className="size-4 text-primary" aria-hidden /><h2 className="mt-2 text-sm font-semibold">Structure proposal</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">A proposal is visible for review; it does not change the structure by itself.</p><a href={workspace.proposal.href} className="mt-3 inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline">{workspace.proposal.label}<ChevronRight className="size-3" /></a></section>}
        </aside>
      </div>
    </main>
  );
}
