import {
  Archive,
  ArrowUpRight,
  BookOpen,
  ChevronDown,
  CircleDollarSign,
  ClipboardList,
  FileText,
  FolderKanban,
  Link2,
  MapPinned,
  MoreHorizontal,
  PackageCheck,
  Route,
  Store,
  UsersRound,
} from "lucide-react";
import { useRef, useState, type ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { DocSection, DocumentSurface } from "@/components/workbench/atoms";
import { cn } from "@/lib/utils";
import { EditorialTitle, ObjectEmblem } from "../visuals";

import { buildDocsAppendBlockCommands, buildDocsChildDocumentCommand, buildDocsInstantiateTemplateBlockCommands, buildDocsReorderBlocksCommand } from "./documentAction";
import { filterDocumentTree, isDocumentTreeNode, selectDocumentDirectoryAnchor } from "./documentTree";
import { RelationChips } from "./RelationChips";
import type { CompanyOsDocsActionCommand, CompanyOsDocumentBlock, CompanyOsDocumentPageData, CompanyOsLink, CompanyOsWorkspaceTreeItem } from "./types";
import { preserveCompanyOsWorkbenchContext } from "./url";

type DocsBlockKind = "rich_text" | "heading" | "callout" | "table";

const blockKindOptions: Array<{ value: DocsBlockKind; label: string; hint: string }> = [
  { value: "rich_text", label: "Paragraph", hint: "Narrative, notes, and result text" },
  { value: "heading", label: "Heading", hint: "Section structure" },
  { value: "callout", label: "Callout", hint: "Decision, risk, or durable note" },
  { value: "table", label: "Table", hint: "Simple document-local table" },
];

const slashCommands: Array<{ value: DocsBlockKind; label: string; hint: string; command: string }> = [
  { value: "rich_text", label: "Paragraph", hint: "Narrative, notes, and result text", command: "/paragraph" },
  { value: "heading", label: "Heading", hint: "Section structure", command: "/heading" },
  { value: "callout", label: "Callout", hint: "Decision, risk, or durable note", command: "/callout" },
  { value: "table", label: "Table", hint: "Simple document-local table", command: "/table" },
];

function SimpleTable({ block }: { block: Extract<CompanyOsDocumentBlock, { type: "table" }> }) {
  const { columns, rows, caption } = block.table;
  return (
    <div className="max-w-full overflow-x-auto rounded-xl border border-border bg-card shadow-sm">
      <table className="w-full min-w-[42rem] border-collapse text-left text-xs">
        {caption && <caption className="border-b border-border bg-muted/35 px-3.5 py-2.5 text-left text-sm font-semibold text-foreground">{caption}</caption>}
        <thead className="bg-muted/35 text-muted-foreground">
          <tr>{columns.map((column) => <th key={column} className="whitespace-nowrap border-b border-border px-3.5 py-2.5 font-medium">{column}</th>)}</tr>
        </thead>
        <tbody>
          {rows.map((row, index) => (
            <tr key={index} className="border-b border-border last:border-0 hover:bg-accent/25">
              {row.map((cell, cellIndex) => <td key={cellIndex} className={cn("px-3.5 py-2.5 align-top leading-5 text-foreground", cellIndex === 0 && "min-w-36 font-medium")}>{cell}</td>)}
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

/**
 * A Document whose Store record declares no Blocks says so, on every layout. Both the
 * standard article and the Wanchengwanling operating layout render this same state, so
 * an empty Document can never read as a page that simply failed to render.
 */
function EmptyDocumentState({ className }: { className?: string }) {
  return (
    <div className={cn("rounded-xl border border-dashed border-border bg-muted/25 p-6 text-sm leading-6 text-muted-foreground", className)} data-docs-empty-document="true">
      This Document has no Blocks yet. Use the governed composer to append the first durable Block; empty UI state is not company truth.
    </div>
  );
}

/**
 * Related work, not "Results": a WorkItem may reference this Document as its source, as
 * context, or as the Document it produced, and only the last of those is a result. Each
 * entry names its own reasons, so the panel never implies a provenance it does not have.
 */
function RelatedWorkBlock({ links }: { links?: CompanyOsDocumentPageData["relatedWork"] }) {
  if (!links?.length) return null;
  return (
    <section className="space-y-2" data-docs-related-work="true" data-docs-related-work-count={links.length}>
      <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Related work</h2>
      <p className="text-[11px] leading-4 text-muted-foreground">Deduplicated from WorkItem source, context, and result references. Each item states why it is listed.</p>
      <RelationChips links={links} />
    </section>
  );
}

/**
 * Navigable location trail. Ancestor crumbs link to their Documents; the current
 * Document renders as plain text. The leading space label is the only non-linked
 * crumb because a grouping space is not a durable Document.
 */
function DocumentBreadcrumbs({ document }: { document: CompanyOsDocumentPageData }) {
  const trail = document.breadcrumbs ?? [];
  if (!trail.length) {
    return document.breadcrumb?.length ? <nav aria-label="Breadcrumb" className="text-xs text-muted-foreground">{document.breadcrumb.join(" / ")}</nav> : null;
  }
  const spaceLabel = document.breadcrumb && document.breadcrumb.length === trail.length + 1 ? document.breadcrumb[0] : undefined;
  const items: Array<{ key: string; label: string; href?: string; ref?: string }> = [
    ...(spaceLabel ? [{ key: "space", label: spaceLabel }] : []),
    ...trail.map((link) => ({ key: link.id, label: link.label, href: link.href, ref: link.id })),
  ];
  return (
    <nav aria-label="Breadcrumb" data-docs-breadcrumbs="true" className="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-xs text-muted-foreground">
      {items.map((item, index) => (
        <span key={item.key} className="flex min-w-0 items-center gap-1.5">
          {index > 0 && <span aria-hidden>/</span>}
          {item.href ? (
            <a href={preserveCompanyOsWorkbenchContext(item.href)} data-company-os-ref={item.ref} className="break-words hover:text-primary hover:underline">{item.label}</a>
          ) : (
            <span data-company-os-ref={item.ref} className={item.ref ? "break-words text-foreground" : "break-words"}>{item.label}</span>
          )}
        </span>
      ))}
    </nav>
  );
}

function DocumentArchitectureTreeItem({ item, depth = 0 }: { item: CompanyOsWorkspaceTreeItem; depth?: number }) {
  const hasChildren = Boolean(item.children?.length);
  const className = cn(
    "group flex w-full items-start gap-1.5 rounded-md py-1.5 pr-2 text-left text-xs leading-4 text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
    depth === 0 ? "pl-2" : depth === 1 ? "pl-5" : depth === 2 ? "pl-8" : "pl-11",
    item.selected && "bg-primary/10 text-primary",
  );
  const body = (
    <>
      {hasChildren ? <ChevronDown className="mt-0.5 size-3 shrink-0 text-muted-foreground" aria-hidden /> : <span className="mt-0.5 w-3 shrink-0" />}
      <FolderKanban className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden />
      <span className="min-w-0 flex-1">
        <span className="block break-words font-medium">{item.label}</span>
        {item.meta && <span className="block text-[10px] text-muted-foreground">{item.meta}</span>}
      </span>
    </>
  );
  return (
    <li>
      {item.href ? (
        <a href={preserveCompanyOsWorkbenchContext(item.href)} data-company-os-ref={item.ref} data-docs-document-architecture-link="true" className={className}>{body}</a>
      ) : (
        <div data-company-os-ref={item.ref} className={className}>{body}</div>
      )}
      {hasChildren && <ul>{item.children?.map((child) => <DocumentArchitectureTreeItem key={child.id} item={child} depth={depth + 1} />)}</ul>}
    </li>
  );
}

function DocumentArchitecture({ tree }: { tree?: CompanyOsDocumentPageData["documentTree"] }) {
  // A document tree renders real Documents and their grouping spaces only;
  // BusinessModule nodes stay in the workspace navigation, not in a document directory.
  const documentOnlyTree = filterDocumentTree(tree);
  if (!documentOnlyTree.length) return null;
  return (
    <section className="space-y-2 rounded-lg border border-border bg-card/70 p-3" aria-label="Document architecture" data-docs-document-architecture="true">
      <div>
        <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Document architecture</h2>
        <p className="mt-1 text-[11px] leading-5 text-muted-foreground">Store projection directory. Links preserve the current api/project context.</p>
      </div>
      <ul className="max-h-[22rem] space-y-0.5 overflow-auto pr-1">
        {documentOnlyTree.map((item) => <DocumentArchitectureTreeItem key={item.id} item={item} />)}
      </ul>
    </section>
  );
}

function moveBlockId(blocks: CompanyOsDocumentBlock[], index: number, delta: -1 | 1): string[] {
  const ids = blocks.map((block) => block.id);
  const target = index + delta;
  if (target < 0 || target >= ids.length) return ids;
  const next = [...ids];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}

function BlockOrderBoundary({
  blocks,
  canReorder,
  submitting,
  unavailableReason,
  onReorder,
}: {
  blocks: CompanyOsDocumentBlock[];
  canReorder: boolean;
  submitting: boolean;
  unavailableReason?: string;
  onReorder: (blockIds: string[]) => void;
}) {
  if (!blocks.length) return null;
  return (
    <section className="space-y-2 rounded-lg border border-border bg-card/70 p-3" aria-label="Block order boundary" data-docs-block-order-boundary="true">
      <div>
        <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Block order</h2>
        <p className="mt-1 text-[11px] leading-5 text-muted-foreground">Order is the native Document.block_ids sequence. Reorder dispatches a governed document.append update that preserves the exact Block set.</p>
      </div>
      <ol className="space-y-1.5">
        {blocks.map((block, index) => (
          <li key={block.id} data-company-os-ref={block.id} className="flex items-center justify-between gap-2 rounded-md border border-border bg-background px-2 py-1.5">
            <span className="min-w-0 truncate text-xs"><span className="text-muted-foreground">{index + 1}.</span> {block.id}</span>
            <span className="flex shrink-0 gap-1">
              <button type="button" className="rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground disabled:opacity-45" disabled={!canReorder || submitting || index === 0} title={unavailableReason} onClick={() => onReorder(moveBlockId(blocks, index, -1))} data-docs-block-reorder="up">Up</button>
              <button type="button" className="rounded border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground disabled:opacity-45" disabled={!canReorder || submitting || index === blocks.length - 1} title={unavailableReason} onClick={() => onReorder(moveBlockId(blocks, index, 1))} data-docs-block-reorder="down">Down</button>
            </span>
          </li>
        ))}
      </ol>
      <p className="text-[11px] leading-5 text-muted-foreground" data-docs-block-reorder-state={canReorder ? "available" : "unavailable"}>{canReorder ? "Drag/drop UI can be layered on this governed reorder Action later." : unavailableReason ?? "Reorder requires Store-live document.append authority."}</p>
    </section>
  );
}

function nodeText(value: ReactNode): string {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map(nodeText).join("");
  return "";
}

function isWanchengwanlingDocument(document: CompanyOsDocumentPageData): boolean {
  const haystack = `${document.id ?? ""} ${document.title ?? ""} ${document.breadcrumb?.join(" ") ?? ""}`;
  return Boolean(document.id?.startsWith("document-wcw-") || /wanchengwanling|万城万灵|agentos dogfood|external gateway/i.test(haystack));
}

function wcwMetric(document: CompanyOsDocumentPageData, key: "physical" | "virtual" | "merchant" | "company" | "magnet" | "lottery" | "gateway" | "workitem"): string {
  const body = document.blocks.map((block) => {
    if (block.type === "table") {
      return [
        block.table.caption,
        ...block.table.columns,
        ...block.table.rows.flat().map(nodeText),
      ].filter(Boolean).join(" ");
    }
    return nodeText("content" in block ? block.content : "");
  }).join(" ");
  const patterns: Record<typeof key, RegExp[]> = {
    physical: [/实体\s*NFC\s*手环[^¥￥]*([¥￥]\s*\d+)/i, /实体手环[^¥￥]*([¥￥]\s*\d+)/i],
    virtual: [/虚拟手环[^¥￥]*([¥￥]\s*\d+)/i],
    merchant: [/商家分成?\s*([¥￥]\s*\d+)/i, /商家\s*([¥￥]\s*\d+)/i],
    company: [/公司(?:留|分成?)?\s*([¥￥]\s*\d+)/i],
    magnet: [/(\d+)\s*个?景点兑换\s*AR\s*冰箱贴/i, /(\d+)\s*点位冰箱贴/i],
    lottery: [/(\d+)\s*个?景点获得抽奖资格/i, /(\d+)\s*点位抽奖/i],
    gateway: [/Gateway\s*v?(\d+)/i, /企业微信\s*Gateway\s*v?(\d+)/i],
    workitem: [/WorkItem[s]?\s*候选/i, /WorkItems/i],
  };
  for (const pattern of patterns[key]) {
    const match = body.match(pattern);
    if (match?.[1]) return match[1].replace("￥", "¥").replace(/\s+/g, "");
  }
  return "待确认";
}

function wcwDocumentKind(document: CompanyOsDocumentPageData): "project" | "business" | "merchant" | "rewards" | "route" | "agentos" | "generic" {
  const id = document.id ?? "";
  const haystack = `${document.id ?? ""} ${document.title ?? ""}`;
  if (/agentos|external-gateway|自举|外部接入/i.test(haystack)) return "agentos";
  if (id === "document-wcw-project-home") return "project";
  if (id === "document-wcw-business-model") return "business";
  if (id.includes("merchant")) return "merchant";
  if (id.includes("rewards") || id.includes("procurement") || id.includes("inventory")) return "rewards";
  if (id.includes("route") || id.includes("ar-experience")) return "route";
  return "generic";
}

function WcwMetrics({ document, tableBlocks }: { document: CompanyOsDocumentPageData; tableBlocks: Array<Extract<CompanyOsDocumentBlock, { type: "table" }>> }) {
  const kind = wcwDocumentKind(document);
  const firstTableRows = tableBlocks[0]?.table.rows.length ?? 0;
  const workCount = (document.resultLinks ?? []).filter((link) => link.kind === "work").length;
  const recordCount = document.connectedRecords?.length ?? 0;
  const agentCount = document.properties?.filter((property) => property.actorType === "Standing Agent").length ?? 0;

  if (kind === "agentos") {
    return (
      <>
        <WcwMetricCard label="Store model" value="1" detail="One Company Store holds Wanchengwanling and AgentOS dogfood truth." icon={<BookOpen className="size-4" />} />
        <WcwMetricCard label="Gateway slice" value="v0" detail="WeCom enters through service actor, Merchant Ops Agent, Docs, and WorkItem." icon={<UsersRound className="size-4" />} />
        <WcwMetricCard label="CLI first" value="Agent" detail="Agents operate through skill + CLI; UI remains the human review surface." icon={<ClipboardList className="size-4" />} />
        <WcwMetricCard label="Custom pages" value={String(tableBlocks.length)} detail="Code-declared pages render Store-backed blocks, tables, Work, Org, and Gateway context." icon={<FileText className="size-4" />} />
      </>
    );
  }

  if (kind === "merchant") {
    return (
      <>
        <WcwMetricCard label="Merchant roles" value={String(firstTableRows || "待建")} detail="Seller, redemption, supplier, benefit, listing roles" icon={<Store className="size-4" />} />
        <WcwMetricCard label="Linked work" value={String(workCount)} detail="Onboarding, list cleanup, communication and console tasks" icon={<ClipboardList className="size-4" />} />
        <WcwMetricCard label="Responsible agents" value={String(agentCount || "待确认")} detail="Merchant Ops plus governance participants" icon={<UsersRound className="size-4" />} />
        <WcwMetricCard label="Finance boundary" value="按需" detail="Settlement, procurement and payout records stay in Finance" icon={<CircleDollarSign className="size-4" />} />
      </>
    );
  }

  if (kind === "rewards") {
    return (
      <>
        <WcwMetricCard label="Reward objects" value={String(firstTableRows || "待建")} detail="Magnet, Polaroid, local snack coupon, logistics and evidence" icon={<PackageCheck className="size-4" />} />
        <WcwMetricCard label="Linked work" value={String(workCount)} detail="Quotes, procurement, logistics, inventory and finance console tasks" icon={<ClipboardList className="size-4" />} />
        <WcwMetricCard label="Connected records" value={String(recordCount)} detail="Inventory policy, procurement boundary and redemption records" icon={<FileText className="size-4" />} />
        <WcwMetricCard label="Finance boundary" value="Commitment" detail="Any purchase must create Finance commitment before payment" icon={<CircleDollarSign className="size-4" />} />
      </>
    );
  }

  if (kind === "route") {
    return (
      <>
        <WcwMetricCard label="Route spots" value="12" detail="MVP route configuration target" icon={<MapPinned className="size-4" />} />
        <WcwMetricCard label="Magnet unlock" value={wcwMetric(document, "magnet")} detail="Fridge magnet redemption rule" icon={<PackageCheck className="size-4" />} />
        <WcwMetricCard label="Lottery unlock" value={wcwMetric(document, "lottery")} detail="Lottery qualification rule" icon={<ClipboardList className="size-4" />} />
        <WcwMetricCard label="Experience records" value={String(recordCount || firstTableRows || "待建")} detail="AR assets, check-in gates and acceptance evidence" icon={<Route className="size-4" />} />
      </>
    );
  }

  const merchantShare = wcwMetric(document, "merchant");
  const companyShare = wcwMetric(document, "company");
  const splitDetail = merchantShare === "待确认" || companyShare === "待确认"
    ? "Seller split is defined in 01 Business Model"
    : `Merchant ${merchantShare} · Company ${companyShare}`;

  return (
    <>
      <WcwMetricCard label="Physical bracelet" value={wcwMetric(document, "physical")} detail={splitDetail} icon={<Store className="size-4" />} />
      <WcwMetricCard label="Virtual bracelet" value={wcwMetric(document, "virtual")} detail="Mini program purchase path" icon={<PackageCheck className="size-4" />} />
      <WcwMetricCard label="Magnet unlock" value={wcwMetric(document, "magnet")} detail="AR fridge magnet redemption rule" icon={<MapPinned className="size-4" />} />
      <WcwMetricCard label="Lottery unlock" value={wcwMetric(document, "lottery")} detail="Full route lottery qualification" icon={<ClipboardList className="size-4" />} />
    </>
  );
}

function WcwMetricCard({ label, value, detail, icon }: { label: string; value: string; detail: string; icon: ReactNode }) {
  return (
    <div className="rounded-2xl border border-border bg-card/80 p-4 shadow-sm">
      <div className="flex items-start justify-between gap-3">
        <p className="text-[11px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">{label}</p>
        <span className="grid size-8 shrink-0 place-items-center rounded-xl border border-primary/20 bg-primary/[0.07] text-primary">{icon}</span>
      </div>
      <p className="mt-3 text-3xl font-semibold tracking-tight text-foreground">{value}</p>
      <p className="mt-1 text-xs leading-5 text-muted-foreground">{detail}</p>
    </div>
  );
}

function WcwLinkCard({ link }: { link: CompanyOsLink }) {
  return (
    <a
      href={preserveCompanyOsWorkbenchContext(link.href ?? `?surface=docs&document=${encodeURIComponent(link.id)}`)}
      data-company-os-ref={link.id}
      className="group flex min-w-0 items-start gap-3 rounded-xl border border-border bg-background/70 p-3 text-left hover:border-primary/30 hover:bg-primary/[0.045]"
    >
      <FileText className="mt-0.5 size-4 shrink-0 text-primary" />
      <span className="min-w-0 flex-1">
        <span className="block break-words text-sm font-semibold leading-5 group-hover:text-primary">{link.label}</span>
        {link.meta && <span className="mt-1 block text-xs text-muted-foreground">{link.meta}</span>}
      </span>
      <ArrowUpRight className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
    </a>
  );
}

function WcwDocumentDirectory({ tree }: { tree?: CompanyOsDocumentPageData["documentTree"] }) {
  // The projection tree is a real parent_document_id hierarchy, so the Wanchengwanling
  // pages sit under a document root rather than directly under the space node. Anchor
  // by Document child count: the space node also holds every BusinessModule, so raw
  // child count would pick the space and expose one page instead of eleven.
  const wcwRoot = selectDocumentDirectoryAnchor(tree, /wanchengwanling|万城万灵/i);
  // A directory card must be a real Document page: module children sharing the
  // anchor never render as documents.
  const children = (wcwRoot?.children ?? tree ?? []).filter(isDocumentTreeNode);
  if (!children.length) return null;
  return (
    <section className="rounded-2xl border border-border bg-card/80 p-4 shadow-sm" data-docs-document-architecture="true">
      <div className="flex items-center gap-2">
        <ObjectEmblem kind="docs" className="size-9 rounded-xl" />
        <div>
          <h2 className="text-sm font-semibold">Wanchengwanling Docs Map</h2>
          <p className="text-xs text-muted-foreground">Store-backed document architecture</p>
        </div>
      </div>
      <div className="mt-4 grid gap-2 sm:grid-cols-2 xl:grid-cols-1">
        {children.map((item) => (
          <a
            key={item.id}
            href={preserveCompanyOsWorkbenchContext(item.href ?? `?surface=docs&document=${encodeURIComponent(item.ref ?? item.id)}`)}
            data-company-os-ref={item.ref}
            data-docs-document-architecture-link="true"
            className={cn(
              "rounded-xl border border-border bg-background/70 px-3 py-2.5 text-xs hover:border-primary/30 hover:bg-primary/[0.045]",
              item.selected && "border-primary/30 bg-primary/[0.08] text-primary",
            )}
          >
            <span className="block font-semibold leading-4">{item.label}</span>
            {item.meta && <span className="mt-1 block text-[11px] text-muted-foreground">{item.meta}</span>}
          </a>
        ))}
      </div>
    </section>
  );
}

function WcwTableCards({ block }: { block: Extract<CompanyOsDocumentBlock, { type: "table" }> }) {
  const { caption, columns, rows } = block.table;
  const normalizedCaption = caption ?? "Table";
  const pageMap = /核心页面职责|页面职责/i.test(normalizedCaption);
  const merchantTags = /商家能力标签/i.test(normalizedCaption);
  const incentiveRules = /8\s*\/\s*12|激励规则/i.test(normalizedCaption);
  const economics = /收入|产品模型|价格|分成/i.test(normalizedCaption);
  const replication = /复制模型/i.test(normalizedCaption);
  const dataTable = /gateway|platform|readiness|pipeline|post|campaign|account|metric|workitem|social|小红书|视频号|抖音|平台|账号|内容闭环|发布|指标/i.test(normalizedCaption);

  if (pageMap) {
    return (
      <section className="rounded-2xl border border-border bg-card/80 p-5 shadow-sm" data-company-os-ref={block.id}>
        <div className="flex items-center gap-2">
          <BookOpen className="size-4 text-primary" />
          <h2 className="text-lg font-semibold tracking-tight">{normalizedCaption}</h2>
        </div>
        <div className="mt-4 grid gap-3 md:grid-cols-2">
          {rows.map((row, index) => (
            <article key={index} className="rounded-xl border border-border bg-background/70 p-3">
              <p className="text-sm font-semibold text-foreground">{nodeText(row[0])}</p>
              <p className="mt-2 text-xs leading-5 text-muted-foreground">{nodeText(row[1])}</p>
              <p className="mt-2 text-xs leading-5 text-foreground">{nodeText(row[2])}</p>
              <p className="mt-3 inline-flex rounded-full border border-primary/20 bg-primary/[0.06] px-2.5 py-1 text-[11px] font-medium text-primary">{nodeText(row[3])}</p>
            </article>
          ))}
        </div>
      </section>
    );
  }

  if (merchantTags) {
    return (
      <section className="rounded-2xl border border-border bg-card/80 p-5 shadow-sm" data-company-os-ref={block.id}>
        <div className="flex items-center gap-2">
          <UsersRound className="size-4 text-primary" />
          <h2 className="text-lg font-semibold tracking-tight">{normalizedCaption}</h2>
        </div>
        <div className="mt-4 grid gap-3 md:grid-cols-2">
          {rows.map((row, index) => (
            <article key={index} className="rounded-xl border border-border bg-background/70 p-3">
              <code className="rounded-md bg-muted px-2 py-1 text-[11px] text-primary">{nodeText(row[0])}</code>
              <p className="mt-3 text-sm font-medium">{nodeText(row[1])}</p>
              <p className="mt-1 text-xs leading-5 text-muted-foreground">{nodeText(row[2])}</p>
              <p className="mt-2 text-[11px] text-muted-foreground">Linked: {nodeText(row[3])}</p>
            </article>
          ))}
        </div>
      </section>
    );
  }

  if (incentiveRules) {
    return (
      <section className="rounded-2xl border border-border bg-card/80 p-5 shadow-sm" data-company-os-ref={block.id}>
        <div className="flex items-center gap-2">
          <Route className="size-4 text-primary" />
          <h2 className="text-lg font-semibold tracking-tight">{normalizedCaption}</h2>
        </div>
        <div className="mt-4 grid gap-3 lg:grid-cols-3">
          {rows.map((row, index) => (
            <article key={index} className="rounded-xl border border-primary/20 bg-primary/[0.045] p-4">
              <p className="text-sm font-semibold text-primary">{nodeText(row[0])}</p>
              <p className="mt-2 text-xl font-semibold tracking-tight">{nodeText(row[1])}</p>
              <p className="mt-2 text-sm leading-5">{nodeText(row[2])}</p>
              <p className="mt-2 text-xs leading-5 text-muted-foreground">{nodeText(row[3])}</p>
            </article>
          ))}
        </div>
      </section>
    );
  }

  if (economics || replication) {
    return (
      <section className="rounded-2xl border border-border bg-card/80 p-5 shadow-sm" data-company-os-ref={block.id}>
        <div className="flex items-center gap-2">
          {economics ? <CircleDollarSign className="size-4 text-primary" /> : <MapPinned className="size-4 text-primary" />}
          <h2 className="text-lg font-semibold tracking-tight">{normalizedCaption}</h2>
        </div>
        <div className={cn("mt-4 grid gap-3", economics ? "xl:grid-cols-2" : "md:grid-cols-2 xl:grid-cols-3")}>
          {rows.map((row, index) => (
            <article key={index} className="rounded-xl border border-border bg-background/70 p-4">
              <p className="text-sm font-semibold">{nodeText(row[0])}</p>
              <dl className="mt-3 space-y-2 text-xs leading-5">
                {row.slice(1).map((cell, cellIndex) => (
                  <div key={cellIndex}>
                    <dt className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{columns[cellIndex + 1]}</dt>
                    <dd className="mt-0.5 text-foreground">{cell}</dd>
                  </div>
                ))}
              </dl>
            </article>
          ))}
        </div>
      </section>
    );
  }

  if (dataTable) {
    return <SimpleTable block={block} />;
  }

  return (
    <section className="rounded-2xl border border-border bg-card/80 p-5 shadow-sm" data-company-os-ref={block.id}>
      <h2 className="text-lg font-semibold tracking-tight">{normalizedCaption}</h2>
      <div className="mt-4"><SimpleTable block={{ ...block, table: { ...block.table, caption: undefined } }} /></div>
    </section>
  );
}

function WcwNarrativeSections({ blocks }: { blocks: CompanyOsDocumentBlock[] }) {
  const sections: Array<{ heading?: ReactNode; items: CompanyOsDocumentBlock[] }> = [];
  for (const block of blocks) {
    if (block.type === "heading") {
      sections.push({ heading: block.content, items: [] });
    } else if (block.type !== "table") {
      const current = sections[sections.length - 1];
      if (current) current.items.push(block);
      else sections.push({ items: [block] });
    }
  }
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      {sections.filter((section) => section.items.length || section.heading).slice(0, 6).map((section, index) => (
        <section key={index} className="rounded-2xl border border-border bg-card/75 p-4 shadow-sm">
          {section.heading && <h2 className="text-base font-semibold tracking-tight">{section.heading}</h2>}
          <div className="mt-3 space-y-3">
            {section.items.map((block) => <Block key={block.id} block={block} />)}
          </div>
        </section>
      ))}
    </div>
  );
}

function WanchengwanlingDocumentPage({
  document,
  actionEnabled,
  onRequestAction,
}: {
  document: CompanyOsDocumentPageData;
  actionEnabled: boolean;
  onRequestAction?: (action: "new-action" | "ask-agent", document: CompanyOsDocumentPageData) => void;
}) {
  const firstParagraph = document.blocks.find((block) => block.type === "paragraph");
  const paragraphText = firstParagraph ? nodeText(firstParagraph.content) : document.description ?? "";
  const journey = document.blocks
    .filter((block) => block.type === "paragraph")
    .map((block) => nodeText(block.content))
    .find((content) => content.includes("→"));
  const journeySteps = journey?.split("→").map((item) => item.trim()).filter(Boolean).slice(0, 7) ?? [];
  const tableBlocks = document.blocks.filter((block): block is Extract<CompanyOsDocumentBlock, { type: "table" }> => block.type === "table");
  const narrativeBlocks = document.blocks.filter((block) => block.type !== "table");
  const relatedLinks = [...(document.resultLinks ?? []), ...(document.connectedRecords ?? [])].slice(0, 8);

  return (
    <main
      data-company-os-page="document-focus"
      data-company-os-fixture={document.fixtureId}
      data-company-os-ref={document.id}
      data-company-os-ready="true"
      data-wcw-document-layout="business-operating-page"
      className="company-workbench h-full overflow-auto bg-[radial-gradient(circle_at_82%_-10%,hsl(var(--primary)/0.13),transparent_30%),linear-gradient(to_bottom,hsl(var(--background)),hsl(var(--muted)/0.28))]"
    >
      <div className="mx-auto grid min-w-0 max-w-[1500px] gap-6 px-5 py-7 lg:grid-cols-[minmax(0,1fr)_330px] lg:px-9">
        <div className="min-w-0 space-y-6">
          <header className="rounded-3xl border border-border bg-card/75 p-6 shadow-sm">
            {document.breadcrumbs?.length ? <DocumentBreadcrumbs document={document} /> : document.breadcrumb?.length ? <nav aria-label="Breadcrumb" className="text-xs text-muted-foreground">{document.breadcrumb.join(" / ")}</nav> : null}
            {document.lifecycleStatus === "archived" && <div role="status" data-docs-archived-history="true" className="mt-4 flex gap-2 rounded-lg border border-status-warn/30 bg-status-warn/[0.07] px-3 py-2.5 text-sm leading-5"><Archive className="mt-0.5 size-4 shrink-0 text-status-warn" /><span><strong>Archived history.</strong> This source remains readable and navigable for Work provenance, but Store-live authoring is withdrawn for archived Documents.</span></div>}
            <div className="mt-4 flex flex-wrap items-start justify-between gap-4">
              <div className="min-w-0">
                <div className="flex items-center gap-3">
                  <ObjectEmblem kind="module" className="size-12 rounded-2xl" />
                  <p className="text-[11px] font-semibold uppercase tracking-[0.16em] text-primary">{wcwDocumentKind(document) === "agentos" ? "AgentOS dogfood operating document" : "Wanchengwanling operating document"}</p>
                </div>
                <EditorialTitle className="mt-5 max-w-4xl text-4xl sm:text-5xl">{document.title}</EditorialTitle>
                <p className="mt-4 max-w-4xl text-sm leading-6 text-muted-foreground">{paragraphText || document.description}</p>
              </div>
              <div className="flex shrink-0 flex-wrap gap-2">
                <Button variant="outline" size="sm" disabled={!onRequestAction} title={!onRequestAction ? "No governed action transport is connected." : undefined} onClick={() => onRequestAction?.("new-action", document)}>New action</Button>
                <Button size="sm" disabled={!onRequestAction} title={!onRequestAction ? "No Standing Agent Inbox transport is connected." : undefined} onClick={() => onRequestAction?.("ask-agent", document)}>Ask agent</Button>
                <Button variant="ghost" size="icon" aria-label="More document options"><MoreHorizontal /></Button>
              </div>
            </div>
          </header>

          <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
            <WcwMetrics document={document} tableBlocks={tableBlocks} />
          </section>

          {journeySteps.length ? (
            <section className="rounded-2xl border border-border bg-card/80 p-5 shadow-sm">
              <div className="flex items-center gap-2">
                <Route className="size-4 text-primary" />
                <h2 className="text-lg font-semibold tracking-tight">MVP journey and business loop</h2>
              </div>
              <div className="mt-4 grid gap-2 md:grid-cols-2 xl:grid-cols-3">
                {journeySteps.map((step, index) => (
                  <div key={`${step}:${index}`} className="rounded-xl border border-border bg-background/70 p-3">
                    <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Step {index + 1}</p>
                    <p className="mt-2 text-sm leading-5 text-foreground">{step}</p>
                  </div>
                ))}
              </div>
            </section>
          ) : null}

          {document.blocks.length ? (
            <>
              <WcwNarrativeSections blocks={narrativeBlocks} />

              <div className="space-y-5">
                {tableBlocks.map((block) => <WcwTableCards key={block.id} block={block} />)}
              </div>
            </>
          ) : <EmptyDocumentState className="rounded-2xl bg-card/60" />}

          {document.updatedLabel && <p className="text-xs text-muted-foreground">{document.updatedLabel}</p>}
        </div>

        <aside className="space-y-5 lg:sticky lg:top-6 lg:self-start" aria-label="Document context">
          <WcwDocumentDirectory tree={document.documentTree} />
          <section className="rounded-2xl border border-border bg-card/80 p-4 shadow-sm">
            <h2 className="text-sm font-semibold">Operating links</h2>
            <div className="mt-3 space-y-2">
              <ContextBlock label="Source" links={document.sourceLinks} />
              <ContextBlock label="Child pages" links={document.childDocuments} />
              <ContextBlock label="Backlinks" links={document.backlinks} />
              <RelatedWorkBlock links={document.relatedWork} />
              <ContextBlock label="Connected records" links={document.connectedRecords} />
            </div>
          </section>
          {relatedLinks.length ? (
            <section className="rounded-2xl border border-border bg-card/80 p-4 shadow-sm">
              <h2 className="text-sm font-semibold">Next records to inspect</h2>
              <div className="mt-3 space-y-2">
                {relatedLinks.map((link) => <WcwLinkCard key={link.id} link={link} />)}
              </div>
            </section>
          ) : null}
          {document.activity?.length ? (
            <section className="rounded-2xl border border-border bg-card/80 p-4 shadow-sm">
              <h2 className="text-sm font-semibold">Activity</h2>
              <ol className="mt-3 space-y-3 border-l border-border pl-3">
                {document.activity.map((item) => <li key={item.id} className="space-y-0.5"><p className="text-xs font-medium text-foreground">{item.label}</p>{item.detail && <p className="text-xs leading-5 text-muted-foreground">{item.detail}</p>}{item.at && <time className="text-[11px] text-muted-foreground">{item.at}</time>}</li>)}
              </ol>
            </section>
          ) : null}
          <section className="rounded-2xl border border-primary/20 bg-primary/[0.055] p-4 text-xs leading-5" aria-label="Store-live Docs authoring" data-docs-authoring-panel="document-focus">
            <h2 className="text-sm font-semibold text-foreground">Write boundary</h2>
            <p className="mt-2 text-muted-foreground">This page is optimized for human review. Durable edits should normally be made by Docs CLI or a governed Agent action. Browser writing is {document.lifecycleStatus === "archived" ? "withdrawn because this Document is archived history" : actionEnabled ? "available when a capability token is supplied" : "disabled until Store-live action capability is connected"}.</p>
          </section>
        </aside>
      </div>
    </main>
  );
}

/** A focused, free-form Company OS page. It displays data supplied by the host; it owns no mutations. */
export function BasicDocumentPage({
  document,
  actionEnabled = false,
  onDocsAction,
  onRequestAction,
}: {
  document: CompanyOsDocumentPageData;
  actionEnabled?: boolean;
  onDocsAction?: (command: CompanyOsDocsActionCommand, capabilityToken: string) => Promise<boolean>;
  onRequestAction?: (action: "new-action" | "ask-agent", document: CompanyOsDocumentPageData) => void;
}) {
  const [capabilityToken, setCapabilityToken] = useState("");
  const [childTitle, setChildTitle] = useState("");
  const [childTemplateRef, setChildTemplateRef] = useState("");
  const [instantiateTemplateBlocks, setInstantiateTemplateBlocks] = useState(false);
  const [blockText, setBlockText] = useState("");
  const [blockKind, setBlockKind] = useState<DocsBlockKind>("rich_text");
  const [calloutTitle, setCalloutTitle] = useState("");
  const [feedback, setFeedback] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const intentIds = useRef<Record<string, string>>({});
  const canAuthor = Boolean(actionEnabled && onDocsAction && document.authoring);
  // The archived lifecycle is itself the authoring boundary: name it instead of
  // blaming a missing policy context, which would be a false reason.
  const archivedReason = document.lifecycleStatus === "archived"
    ? "This Document is archived history: it stays readable and navigable for provenance, and governed Store-live authoring Actions are withdrawn for archived Documents."
    : undefined;
  const unavailableReason = archivedReason
    ?? (document.missingDocumentId
      ? "The requested Document is not present in this projection, so there is no Document to author."
      : !actionEnabled
        ? "Connect a Store-live project and provide a session capability before dispatching governed Docs actions."
        : !document.authoring
          ? "This projection does not expose a CustomPageDefinition with document.append and block.append policies."
          : !capabilityToken.trim()
            ? "Enter the session capability before writing Docs truth."
            : undefined);
  const selectedBlockKind = blockKindOptions.find((option) => option.value === blockKind) ?? blockKindOptions[0];
  const activeTemplate = document.authoring?.templateOptions?.find((template) => template.id === document.authoring?.templateRef);
  const selectedChildTemplate = document.authoring?.templateOptions?.find((template) => template.id === childTemplateRef);
  const slashActive = blockText.trimStart().startsWith("/");
  const slashQuery = slashActive ? blockText.trimStart().slice(1).split(/\s+/)[0]?.toLowerCase() ?? "" : "";
  const filteredSlashCommands = slashCommands.filter((command) => command.command.slice(1).includes(slashQuery) || command.label.toLowerCase().includes(slashQuery));
  const displayedBlocksMatchNativeOrder = Boolean(document.authoring?.blockIds.length === document.blocks.length
    && document.blocks.every((block) => document.authoring?.blockIds.includes(block.id)));
  const reorderUnavailableReason = !displayedBlocksMatchNativeOrder
    ? "Reorder is disabled for generated fallback content; only native Store Blocks in Document.block_ids can be reordered."
    : unavailableReason;

  if (isWanchengwanlingDocument(document)) {
    return <WanchengwanlingDocumentPage document={document} actionEnabled={actionEnabled} onRequestAction={onRequestAction} />;
  }

  function chooseSlashCommand(kind: DocsBlockKind) {
    setBlockKind(kind);
    setBlockText((value) => value.replace(/^\s*\/\S*\s*/i, ""));
  }

  async function createChildDocument() {
    if (!canAuthor || !onDocsAction || !childTitle.trim() || !capabilityToken.trim()) return;
    const createdAt = new Date().toISOString();
    const id = intentIds.current.child ?? `action-browser-docs-document-${crypto.randomUUID()}`;
    intentIds.current.child = id;
    setSubmitting(true);
    setFeedback(null);
    try {
      const command = buildDocsChildDocumentCommand({ document, title: childTitle, templateRef: childTemplateRef || null, commandId: id, createdAt });
      const accepted = await onDocsAction(command, capabilityToken.trim());
      let copiedBlockCount = 0;
      if (accepted && instantiateTemplateBlocks && selectedChildTemplate) {
        const templateCommands = buildDocsInstantiateTemplateBlockCommands({
          parentDocument: document,
          childDocumentCommand: command,
          template: selectedChildTemplate,
          commandId: `${id}-template`,
          createdAt,
        });
        for (const templateCommand of templateCommands) {
          const templateAccepted = await onDocsAction(templateCommand, capabilityToken.trim());
          if (!templateAccepted) throw new Error("Template Block instantiation stopped before all governed Actions were accepted. Retry with the same intent; no non-Docs effects were requested.");
          if (templateCommand.command_name === "block.append") copiedBlockCount += 1;
        }
      }
      if (accepted) {
        setChildTitle("");
        setChildTemplateRef("");
        setInstantiateTemplateBlocks(false);
        setCapabilityToken("");
        delete intentIds.current.child;
      }
      setFeedback(accepted
        ? copiedBlockCount
          ? `Child Document created and ${copiedBlockCount} template Block${copiedBlockCount === 1 ? "" : "s"} copied through governed Store actions.`
          : "Child Document created in Store truth with template provenance preserved."
        : "Child Document was not created. Review the action error and retry with the same intent.");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }

  async function appendBlock() {
    if (!canAuthor || !onDocsAction || !blockText.trim() || !capabilityToken.trim()) return;
    const createdAt = new Date().toISOString();
    const id = intentIds.current.block ?? `action-browser-docs-block-${crypto.randomUUID()}`;
    intentIds.current.block = id;
    setSubmitting(true);
    setFeedback(null);
    try {
      const [blockCommand, documentCommand] = buildDocsAppendBlockCommands({ document, text: blockText, blockKind, calloutTitle, commandId: id, createdAt });
      const blockAccepted = await onDocsAction(blockCommand, capabilityToken.trim());
      const documentAccepted = blockAccepted ? await onDocsAction(documentCommand, capabilityToken.trim()) : false;
      if (blockAccepted && documentAccepted) {
        setBlockText("");
        setCalloutTitle("");
        setCapabilityToken("");
        delete intentIds.current.block;
      }
      setFeedback(blockAccepted && documentAccepted ? "Block appended and Document.block_ids updated in Store truth." : "Block append did not complete both required Actions. Retry with the same intent.");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }

  async function reorderBlocks(blockIds: string[]) {
    if (!canAuthor || !onDocsAction || !capabilityToken.trim()) return;
    const updatedAt = new Date().toISOString();
    const id = intentIds.current.reorder ?? `action-browser-docs-block-reorder-${crypto.randomUUID()}`;
    intentIds.current.reorder = id;
    setSubmitting(true);
    setFeedback(null);
    try {
      const command = buildDocsReorderBlocksCommand({ document, blockIds, commandId: id, updatedAt });
      const accepted = await onDocsAction(command, capabilityToken.trim());
      if (accepted) {
        setCapabilityToken("");
        delete intentIds.current.reorder;
      }
      setFeedback(accepted ? "Document.block_ids reordered in Store truth." : "Block reorder was not recorded. Review the action error and retry with the same intent.");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main data-company-os-page="document-focus" data-company-os-fixture={document.fixtureId} data-company-os-ref={document.id} data-company-os-ready="true" className="h-full overflow-auto bg-background px-4 py-5 sm:px-6 lg:px-8">
      <div className="mx-auto grid min-w-0 max-w-[1250px] gap-7 lg:grid-cols-[minmax(0,1fr)_260px]">
        <DocumentSurface className="mx-0 min-w-0 max-w-[800px] space-y-6">
          <header className="space-y-3 border-b border-border pb-5">
            <DocumentBreadcrumbs document={document} />
            {document.lifecycleStatus === "archived" && <div role="status" data-docs-archived-history="true" className="flex gap-2 rounded-lg border border-status-warn/30 bg-status-warn/[0.07] px-3 py-2.5 text-sm leading-5"><Archive className="mt-0.5 size-4 shrink-0 text-status-warn" /><span><strong>Archived history.</strong> This source remains readable and navigable for Work provenance, but Store-live authoring is withdrawn for archived Documents.</span></div>}
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 space-y-2"><h1 className="text-2xl font-semibold tracking-tight text-foreground sm:text-3xl">{document.title}</h1>{document.description && <p className="max-w-2xl text-sm leading-6 text-muted-foreground">{document.description}</p>}</div>
              <div className="flex shrink-0 gap-1.5"><Button variant="outline" size="sm" disabled={!onRequestAction} title={!onRequestAction ? "No governed action transport is connected." : undefined} onClick={() => onRequestAction?.("new-action", document)}>New action</Button><Button size="sm" disabled={!onRequestAction} title={!onRequestAction ? "No Standing Agent Inbox transport is connected." : undefined} onClick={() => onRequestAction?.("ask-agent", document)}>Ask an agent</Button><Button variant="ghost" size="icon" aria-label="More document options"><MoreHorizontal /></Button></div>
            </div>
            {document.properties?.length ? <dl className="flex min-w-0 flex-wrap gap-1.5">{document.properties.map((property, index) => <div key={`${property.ref ?? "property"}:${property.label}:${index}`} data-company-os-ref={property.ref} data-actor-type={property.actorType} className="flex max-w-full min-w-0 items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-xs"><dt className="shrink-0 text-muted-foreground">{property.label}:</dt><dd className="min-w-0 break-words font-medium text-foreground">{property.value}</dd></div>)}</dl> : null}
          </header>
          <article className="min-w-0 space-y-4">
            {document.missingDocumentId
              ? <div className="rounded-xl border border-dashed border-status-warn/40 bg-status-warn/[0.06] p-6 text-sm leading-6 text-foreground" data-docs-document-not-found="true">No Document with id <code>{document.missingDocumentId}</code> is present in this projection. The link may reference a Document outside this Company Store or one that was pruned; nothing else is substituted under the requested id.</div>
              : document.blocks.length
                ? document.blocks.map((block) => <Block key={block.id} block={block} />)
                : <EmptyDocumentState />}
          </article>
          {document.updatedLabel && <p className="border-t border-border pt-4 text-xs text-muted-foreground">{document.updatedLabel}</p>}
        </DocumentSurface>
        <aside className="space-y-5 border-t border-border pt-5 lg:border-l lg:border-t-0 lg:pl-5 lg:pt-0" aria-label="Document context">
          <DocumentArchitecture tree={document.documentTree} />
          <ContextBlock label="Source" links={document.sourceLinks} />
          <ContextBlock label="Child pages" links={document.childDocuments} />
          <ContextBlock label="Backlinks" links={document.backlinks} />
          <RelatedWorkBlock links={document.relatedWork} />
          <ContextBlock label="Connected records" links={document.connectedRecords} />
          {document.activity?.length ? <section className="space-y-2"><h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Activity</h2><ol className="space-y-3 border-l border-border pl-3">{document.activity.map((item) => <li key={item.id} className="space-y-0.5"><p className="text-xs font-medium text-foreground">{item.label}</p>{item.detail && <p className="text-xs leading-5 text-muted-foreground">{item.detail}</p>}{item.at && <time className="text-[11px] text-muted-foreground">{item.at}</time>}</li>)}</ol></section> : null}
          <section className="space-y-3 rounded-lg border border-border bg-card/70 p-3" aria-label="Store-live Docs authoring" data-docs-authoring-panel="document-focus">
            <div>
              <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Store-live authoring</h2>
              <p className="mt-1 text-xs leading-5 text-muted-foreground">Writes use governed ActionCommands. Appending a block also updates Document.block_ids.</p>
            </div>
            {document.authoring?.templateRef && (
              <div className="rounded-md border border-border bg-muted/35 px-2.5 py-2 text-[11px] leading-5 text-muted-foreground" data-docs-template-provenance={document.authoring.templateRef}>
                Template provenance: <span className="font-medium text-foreground">{activeTemplate?.label ?? document.authoring.templateRef}</span>. Existing Documents keep their recorded template_ref; template Blocks are copied only by an explicit governed instantiation action.
              </div>
            )}
            <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Session capability" value={capabilityToken} onChange={(event) => setCapabilityToken(event.target.value)} disabled={!actionEnabled || submitting} aria-label="Company OS session capability" />
            <div className="space-y-2">
              <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Child page title" value={childTitle} onChange={(event) => setChildTitle(event.target.value)} disabled={!canAuthor || submitting} aria-label="Child document title" />
              {document.authoring?.templateOptions?.length ? (
                <select className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" value={childTemplateRef} onChange={(event) => setChildTemplateRef(event.target.value)} disabled={!canAuthor || submitting} aria-label="Child document template">
                  <option value="">No template</option>
                  {document.authoring.templateOptions.map((template) => <option key={template.id} value={template.id}>{template.label}</option>)}
                </select>
              ) : null}
              {document.authoring?.templateOptions?.length ? (
                <label className="flex items-start gap-2 rounded-md border border-border bg-muted/20 px-2 py-2 text-[11px] leading-5 text-muted-foreground" data-docs-template-instantiation="browser-action">
                  <input
                    type="checkbox"
                    className="mt-0.5"
                    checked={instantiateTemplateBlocks}
                    onChange={(event) => setInstantiateTemplateBlocks(event.target.checked)}
                    disabled={!canAuthor || submitting || !childTemplateRef}
                    aria-label="Instantiate template blocks"
                  />
                  <span>
                    Instantiate template Blocks through Store-live Actions.
                    <span className="block text-[10px]">
                      {selectedChildTemplate
                        ? `${selectedChildTemplate.templateBlockIds.length} ordered Block${selectedChildTemplate.templateBlockIds.length === 1 ? "" : "s"} will be copied; TypedRecords, Relations, WorkItems, Approvals and Finance are not created.`
                        : "Select a template to enable Block copying; otherwise only template_ref provenance is recorded."}
                    </span>
                  </span>
                </label>
              ) : null}
              {document.authoring?.templateRecordPolicy ? (
                <div className="rounded-md border border-border bg-background/70 px-2.5 py-2 text-[11px] leading-5 text-muted-foreground" data-docs-template-record-policy={document.authoring.templateRecordPolicy.status}>
                  <div className="flex items-center justify-between gap-2">
                    <span className="font-medium text-foreground">Template → TypedRecord</span>
                    <span className={document.authoring.templateRecordPolicy.status === "declared" ? "text-status-good" : "text-status-warn"}>{document.authoring.templateRecordPolicy.status}</span>
                  </div>
                  <p className="mt-1">Template Blocks do not create records. Use a governed Relation after the child Document and TypedRecord exist.</p>
                  <code className="mt-2 block break-words rounded bg-muted px-1.5 py-1 text-[10px]">{document.authoring.templateRecordPolicy.commandHint}</code>
                </div>
              ) : null}
              <Button size="sm" variant="outline" className="w-full justify-center" disabled={!canAuthor || !capabilityToken.trim() || !childTitle.trim() || submitting} title={unavailableReason} onClick={() => void createChildDocument()}>Create child Document</Button>
            </div>
            <div className="space-y-2" data-docs-block-composer="true">
              <div className="grid grid-cols-2 gap-1.5" aria-label="Block type quick picks">
                {blockKindOptions.map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    className={option.value === blockKind ? "rounded-md border border-primary/45 bg-primary/10 px-2 py-1.5 text-left" : "rounded-md border border-border bg-background px-2 py-1.5 text-left hover:bg-muted/40"}
                    onClick={() => setBlockKind(option.value)}
                    disabled={!canAuthor || submitting}
                    data-docs-block-kind-option={option.value}
                    data-selected={option.value === blockKind ? "true" : "false"}
                  >
                    <span className="block text-[11px] font-semibold text-foreground">{option.label}</span>
                    <span className="block text-[10px] leading-4 text-muted-foreground">{option.hint}</span>
                  </button>
                ))}
              </div>
              <select className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" value={blockKind} onChange={(event) => setBlockKind(event.target.value as typeof blockKind)} disabled={!canAuthor || submitting} aria-label="Block kind">
                {blockKindOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
              </select>
              {blockKind === "callout" && <input className="w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder="Callout title" value={calloutTitle} onChange={(event) => setCalloutTitle(event.target.value)} disabled={!canAuthor || submitting} aria-label="Callout title" />}
              <textarea className="min-h-20 w-full rounded-md border border-input bg-background px-2 py-1.5 text-xs" placeholder={blockKind === "table" ? "Table rows: Header A | Header B\\nCell A | Cell B" : "New block content"} value={blockText} onChange={(event) => setBlockText(event.target.value)} disabled={!canAuthor || submitting} aria-label="New document block content" />
              {slashActive && (
                <div className="rounded-md border border-border bg-background p-1.5 shadow-sm" role="listbox" aria-label="Slash menu block commands" data-docs-slash-menu="true">
                  {filteredSlashCommands.length ? filteredSlashCommands.map((option) => (
                    <button key={option.value} type="button" role="option" className="flex w-full items-start justify-between gap-2 rounded px-2 py-1.5 text-left hover:bg-muted/50" disabled={!canAuthor || submitting} onClick={() => chooseSlashCommand(option.value)} data-docs-slash-command={option.command}>
                      <span><span className="block text-xs font-medium text-foreground">{option.command} · {option.label}</span><span className="block text-[10px] leading-4 text-muted-foreground">{option.hint}</span></span>
                      <span className="text-[10px] text-muted-foreground">Block</span>
                    </button>
                  )) : <p className="px-2 py-1.5 text-[11px] text-muted-foreground">No governed Block command matches this slash query.</p>}
                </div>
              )}
              <p className="rounded-md bg-muted/35 px-2 py-1.5 text-[11px] leading-5 text-muted-foreground" data-docs-block-composer-hint={blockKind}>{selectedBlockKind.hint}. Composer content becomes a native Block only after both governed Actions are accepted.</p>
              <Button size="sm" className="w-full justify-center" disabled={!canAuthor || !capabilityToken.trim() || !blockText.trim() || submitting} title={unavailableReason} onClick={() => void appendBlock()}>{submitting ? "Writing…" : "Append Block"}</Button>
            </div>
            <p className={feedback && !/created|appended/i.test(feedback) ? "rounded-md border border-status-warn/35 bg-status-warn/10 px-2 py-1.5 text-[11px] leading-5 text-foreground" : "text-[11px] leading-5 text-muted-foreground"} role="status" data-docs-authoring-state={canAuthor ? "available" : "unavailable"} data-docs-authoring-error-boundary={feedback && !/created|appended/i.test(feedback) ? "true" : undefined}>{feedback ?? unavailableReason ?? "The server validates definition, policy, actor permission, module scope and idempotency before writing."}</p>
          </section>
          <BlockOrderBoundary blocks={document.blocks} canReorder={canAuthor && Boolean(capabilityToken.trim()) && displayedBlocksMatchNativeOrder} submitting={submitting} unavailableReason={reorderUnavailableReason} onReorder={(blockIds) => void reorderBlocks(blockIds)} />
          {!document.sourceLinks?.length && !document.resultLinks?.length && !document.connectedRecords?.length && !document.childDocuments?.length && !document.backlinks?.length && <p className="rounded-md border border-dashed border-border p-3 text-xs leading-5 text-muted-foreground"><Link2 className="mr-1 inline size-3.5" />Connected records appear here when the host resolves them.</p>}
        </aside>
      </div>
    </main>
  );
}
