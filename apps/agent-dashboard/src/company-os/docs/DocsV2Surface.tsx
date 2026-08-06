/**
 * AI-first Docs v2 surface (ADR 0054 Phase 0).
 *
 * Renders block pages store-live from `/v1/company-os/docs-v2/pages`. This
 * surface never falls back to fixtures: when the live fetch fails it shows an
 * explicit error card. Page embeds resolve their targets live (card or inline
 * transclusion, depth-capped, cycle-safe), matching the CLI/API contract.
 */

import { useEffect, useState, type ReactNode } from "react";
import {
  fetchDocsV2Page,
  fetchDocsV2PageIndex,
  type DocsV2BlockView,
  type DocsV2PageIndexItem,
  type DocsV2PageView,
  type DocsV2ResolvedEmbed,
} from "../../api";
import type { SelectionState } from "../../app/selection";

const MAX_TRANSCLUSION_DEPTH = 2;

interface DocsV2SurfaceProps {
  apiUrl: string;
  selection: SelectionState;
  company?: string | null;
  project?: string | null;
  space?: string | null;
  onSelectionChange?: (selection: Partial<SelectionState>) => void;
}

interface BlockContent {
  text?: string;
  level?: number;
  items?: { text?: string; checked?: boolean }[];
  tone?: string;
  title?: string;
  language?: string;
  header?: string[];
  rows?: string[][];
  target_document_id?: string;
  target?: { kind?: string; id?: string };
  display?: string;
  blob_id?: string;
  alt?: string;
  name?: string;
  caption?: string;
}

function blockContent(block: DocsV2BlockView): BlockContent {
  return (block.content ?? {}) as BlockContent;
}

/** Minimal inline Markdown (bold/italic/code/link); block structure is typed. */
function renderInline(text: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const pattern = /(\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`|\[[^\]]+\]\([^)]+\))/g;
  let cursor = 0;
  let match: RegExpExecArray | null;
  let index = 0;
  while ((match = pattern.exec(text)) !== null) {
    if (match.index > cursor) {
      nodes.push(text.slice(cursor, match.index));
    }
    const token = match[0];
    const key = `${keyPrefix}-inline-${index}`;
    index += 1;
    if (token.startsWith("**")) {
      nodes.push(<strong key={key}>{token.slice(2, -2)}</strong>);
    } else if (token.startsWith("`")) {
      nodes.push(
        <code key={key} className="rounded bg-slate-100 px-1 py-0.5 text-[0.85em] text-slate-800">
          {token.slice(1, -1)}
        </code>,
      );
    } else if (token.startsWith("[")) {
      const linkMatch = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
      if (linkMatch) {
        nodes.push(
          <a key={key} href={linkMatch[2]} className="text-blue-600 underline" target="_blank" rel="noreferrer">
            {linkMatch[1]}
          </a>,
        );
      } else {
        nodes.push(token);
      }
    } else {
      nodes.push(<em key={key}>{token.slice(1, -1)}</em>);
    }
    cursor = match.index + token.length;
  }
  if (cursor < text.length) {
    nodes.push(text.slice(cursor));
  }
  return nodes;
}

function CalloutBox({ block }: { block: DocsV2BlockView }) {
  const content = blockContent(block);
  const toneStyles: Record<string, string> = {
    note: "border-blue-300 bg-blue-50 text-blue-900",
    tip: "border-emerald-300 bg-emerald-50 text-emerald-900",
    warning: "border-amber-300 bg-amber-50 text-amber-900",
    danger: "border-red-300 bg-red-50 text-red-900",
    info: "border-slate-300 bg-slate-50 text-slate-800",
  };
  const tone = content.tone ?? "note";
  return (
    <div data-docs-v2-block="callout" className={`my-3 rounded-md border-l-4 px-4 py-2 text-sm ${toneStyles[tone] ?? toneStyles.info}`}>
      <div className="text-xs font-semibold uppercase tracking-wide">
        {tone}
        {content.title ? ` · ${content.title}` : ""}
      </div>
      <div className="mt-1 whitespace-pre-wrap">{renderInline(content.text ?? "", block.id ?? "callout")}</div>
    </div>
  );
}

function TableBlock({ block }: { block: DocsV2BlockView }) {
  const content = blockContent(block);
  return (
    <div data-docs-v2-block="table" className="my-3 overflow-x-auto rounded-md border border-slate-200">
      <table className="w-full border-collapse text-sm">
        <thead>
          <tr className="bg-slate-50">
            {(content.header ?? []).map((cell, i) => (
              <th key={`h-${i}`} className="border-b border-slate-200 px-3 py-1.5 text-left font-semibold text-slate-700">
                {cell}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {(content.rows ?? []).map((row, rowIdx) => (
            <tr key={`r-${rowIdx}`} className={rowIdx % 2 === 1 ? "bg-slate-50/60" : undefined}>
              {row.map((cell, cellIdx) => (
                <td key={`c-${rowIdx}-${cellIdx}`} className="border-b border-slate-100 px-3 py-1.5 text-slate-700">
                  {renderInline(cell, `${block.id ?? "table"}-${rowIdx}-${cellIdx}`)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

interface EmbedContext {
  apiUrl: string;
  company?: string | null;
  project?: string | null;
  space?: string | null;
  depth: number;
  visited: string[];
  /** F4: live-resolved entity embed targets from the page endpoint. */
  resolvedEmbeds: Record<string, DocsV2ResolvedEmbed>;
  onOpenDocument?: (documentId: string) => void;
}

function PageEmbedCard({ targetId, display, ctx, blockId }: { targetId: string; display: string; ctx: EmbedContext; blockId: string }) {
  const [target, setTarget] = useState<DocsV2PageView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchDocsV2Page(ctx.apiUrl, targetId, ctx.project, ctx.company, ctx.space)
      .then((page) => {
        if (!cancelled) setTarget(page);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [ctx.apiUrl, ctx.company, ctx.project, ctx.space, targetId]);

  const cyclic = ctx.visited.includes(targetId);
  if (cyclic) {
    return (
      <div data-docs-v2-embed={targetId} className="my-3 rounded-md border border-amber-300 bg-amber-50 px-4 py-2 text-sm text-amber-900">
        Transclusion cycle detected for <span className="font-mono">{targetId}</span>; not rendered inline.
      </div>
    );
  }

  if (display === "inline" && ctx.depth < MAX_TRANSCLUSION_DEPTH) {
    if (error) {
      return (
        <div data-docs-v2-embed={targetId} className="my-3 rounded-md border border-red-200 bg-red-50 px-4 py-2 text-sm text-red-800">
          Embedded page <span className="font-mono">{targetId}</span> unavailable: {error}
        </div>
      );
    }
    if (!target) {
      return (
        <div data-docs-v2-embed={targetId} data-docs-v2-loading="embed" className="my-3 rounded-md border border-slate-200 bg-slate-50 px-4 py-2 text-sm text-slate-500">
          Loading embedded page…
        </div>
      );
    }
    return (
      <div data-docs-v2-embed={targetId} className="my-3 rounded-md border border-slate-200 bg-white p-4 shadow-sm">
        <div className="mb-2 flex items-center gap-2 border-b border-slate-100 pb-2">
          <span className="rounded bg-indigo-50 px-1.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide text-indigo-600">inline</span>
          <button
            type="button"
            onClick={() => ctx.onOpenDocument?.(targetId)}
            className="text-sm font-semibold text-slate-800 hover:text-blue-600"
          >
            {target.title}
          </button>
        </div>
        <BlockList blocks={target.blocks} ctx={{ ...ctx, depth: ctx.depth + 1, visited: [...ctx.visited, targetId] }} />
      </div>
    );
  }

  // Card display (or inline beyond the depth cap).
  return (
    <button
      type="button"
      data-docs-v2-embed={targetId}
      onClick={() => ctx.onOpenDocument?.(targetId)}
      className="my-3 flex w-full items-center gap-3 rounded-md border border-slate-200 bg-white px-4 py-3 text-left shadow-sm transition hover:border-blue-300 hover:shadow"
    >
      <span className="rounded bg-slate-100 px-1.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide text-slate-500">page</span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-semibold text-slate-800">
          {error ? `Unavailable embed: ${targetId}` : target ? target.title : targetId}
        </span>
        <span className="block truncate text-xs text-slate-500">
          {error
            ? error
            : target
              ? `r${target.revision_number ?? 0} · ${target.blocks.length} blocks · ${target.lifecycle_status ?? "unknown"}`
              : "resolving live target…"}
        </span>
      </span>
      <span className="text-slate-400">→</span>
    </button>
  );
}

function EntityEmbedCard({ block, ctx }: { block: DocsV2BlockView; ctx: EmbedContext }) {
  const content = blockContent(block);
  const kind = content.target?.kind ?? "entity";
  const id = content.target?.id ?? "?";
  const resolved = ctx.resolvedEmbeds[`${kind}:${id}`];
  const statusLine = resolved?.found
    ? [resolved.record_type, resolved.lifecycle_status ?? resolved.mode ?? resolved.status]
        .filter(Boolean)
        .join(" · ")
    : "not found in the owning system";
  return (
    <div
      data-docs-v2-embed={`${kind}:${id}`}
      data-docs-v2-embed-resolved={resolved?.found ? "true" : "false"}
      className="my-3 flex items-center gap-3 rounded-md border border-dashed border-slate-300 bg-slate-50 px-4 py-3"
    >
      <span className="rounded bg-slate-200 px-1.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide text-slate-600">{kind}</span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-semibold text-slate-700">
          {resolved?.found && resolved.title ? resolved.title : <span className="font-mono">{id}</span>}
        </span>
        <span className="block truncate text-xs text-slate-500">
          {statusLine} · display={content.display ?? "card"}
        </span>
      </span>
    </div>
  );
}

function BlockView({ block, ctx }: { block: DocsV2BlockView; ctx: EmbedContext }) {
  const content = blockContent(block);
  const key = block.id ?? block.markdown;
  switch (block.kind) {
    case "heading": {
      const level = Math.min(Math.max(content.level ?? 1, 1), 6);
      const className = `mt-6 mb-2 font-semibold text-slate-900 ${level === 1 ? "text-2xl" : level === 2 ? "text-xl" : "text-lg"}`;
      const children = renderInline(content.text ?? "", key);
      if (level === 1) return <h2 data-docs-v2-block="heading" className={className}>{children}</h2>;
      if (level === 2) return <h3 data-docs-v2-block="heading" className={className}>{children}</h3>;
      return <h4 data-docs-v2-block="heading" className={className}>{children}</h4>;
    }
    case "paragraph":
      return (
        <p data-docs-v2-block="paragraph" className="my-2 text-sm leading-6 text-slate-700">
          {renderInline(content.text ?? "", key)}
        </p>
      );
    case "bullet_list":
    case "ordered_list":
    case "checklist": {
      const items = content.items ?? [];
      return (
        <ul data-docs-v2-block={block.kind} className="my-2 space-y-1 pl-5 text-sm text-slate-700">
          {items.map((item, i) => (
            <li key={`${key}-item-${i}`} className={block.kind === "ordered_list" ? "list-decimal" : "list-disc"}>
              {block.kind === "checklist" ? (
                <label className="inline-flex items-center gap-2">
                  <input type="checkbox" checked={item.checked ?? false} readOnly className="accent-blue-600" />
                  <span className={item.checked ? "text-slate-400 line-through" : undefined}>
                    {renderInline(item.text ?? "", `${key}-item-${i}`)}
                  </span>
                </label>
              ) : (
                renderInline(item.text ?? "", `${key}-item-${i}`)
              )}
            </li>
          ))}
        </ul>
      );
    }
    case "quote":
      return (
        <blockquote data-docs-v2-block="quote" className="my-3 border-l-4 border-slate-300 pl-4 text-sm italic text-slate-600">
          {renderInline(content.text ?? "", key)}
        </blockquote>
      );
    case "callout":
      return <CalloutBox block={block} />;
    case "code":
      return (
        <pre data-docs-v2-block="code" className="my-3 overflow-x-auto rounded-md bg-slate-900 p-3 text-xs leading-5 text-slate-100">
          <code>{content.text ?? ""}</code>
        </pre>
      );
    case "table":
      return <TableBlock block={block} />;
    case "divider":
      return <hr data-docs-v2-block="divider" className="my-4 border-slate-200" />;
    case "page_embed":
      return (
        <PageEmbedCard
          targetId={content.target_document_id ?? ""}
          display={content.display ?? "card"}
          ctx={ctx}
          blockId={block.id ?? "page-embed"}
        />
      );
    case "entity_embed":
      return <EntityEmbedCard block={block} ctx={ctx} />;
    case "image":
    case "attachment":
      return (
        <div data-docs-v2-block={block.kind} className="my-3 flex items-center gap-2 rounded-md border border-slate-200 bg-slate-50 px-4 py-2 text-sm text-slate-600">
          <span>{block.kind === "image" ? "🖼" : "📎"}</span>
          <span className="font-mono text-xs">{content.name ?? content.alt ?? content.blob_id ?? "blob"}</span>
          <span className="text-xs text-slate-400">blob:{content.blob_id ?? "?"}</span>
        </div>
      );
    default:
      return (
        <div data-docs-v2-block={block.kind} className="my-2 rounded border border-slate-200 px-3 py-2 font-mono text-xs text-slate-500">
          {block.markdown}
        </div>
      );
  }
}

function BlockList({ blocks, ctx }: { blocks: DocsV2BlockView[]; ctx: EmbedContext }) {
  return (
    <div>
      {blocks.map((block, index) => (
        <BlockView key={block.id ?? `block-${index}`} block={block} ctx={ctx} />
      ))}
    </div>
  );
}

function RevisionBanner({ page }: { page: DocsV2PageView }) {
  return (
    <div
      data-docs-v2-revision={`${page.revision_number ?? 0}`}
      className="mb-4 flex flex-wrap items-center gap-2 rounded-md border border-emerald-200 bg-emerald-50 px-3 py-1.5 text-xs text-emerald-800"
    >
      <span className="rounded bg-emerald-100 px-1.5 py-0.5 font-semibold uppercase tracking-wide">store-live</span>
      <span>r{page.revision_number ?? 0}</span>
      {page.content_digest ? <span className="font-mono">sha256:{page.content_digest.slice(0, 12)}…</span> : null}
      {page.scope?.fragment ? <span className="text-emerald-600">fragment view</span> : null}
    </div>
  );
}

function DocsV2Index({ apiUrl, company, project, space, onOpenDocument }: { apiUrl: string; company?: string | null; project?: string | null; space?: string | null; onOpenDocument?: (id: string) => void }) {
  const [items, setItems] = useState<DocsV2PageIndexItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchDocsV2PageIndex(apiUrl, project, company, space)
      .then((index) => {
        if (!cancelled) setItems(index.items ?? []);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [apiUrl, company, project, space]);

  if (error) {
    return (
      <div data-docs-v2-error className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-800">
        Docs v2 index unavailable (store-live fetch failed): {error}
      </div>
    );
  }
  if (!items) {
    return (
      <div data-docs-v2-loading="index" className="rounded-md border border-slate-200 bg-slate-50 px-4 py-3 text-sm text-slate-500">
        Loading Docs v2 page index…
      </div>
    );
  }
  return (
    <div data-docs-v2-index className="space-y-2">
      <div className="mb-3">
        <h2 className="text-lg font-semibold text-slate-900">Docs v2 pages</h2>
        <p className="text-xs text-slate-500">
          Store-live index over the ADR 0054 page model — revisions, digests, and block counts resolve from the Docs write service.
        </p>
      </div>
      {items.length === 0 ? (
        <div className="rounded-md border border-dashed border-slate-300 px-4 py-6 text-center text-sm text-slate-500">
          No docs-v2 pages yet. Create one with{" "}
          <code className="rounded bg-slate-100 px-1 py-0.5 text-xs">harness company docs page create</code>.
        </div>
      ) : (
        items.map((item) => (
          <button
            key={item.document_id}
            type="button"
            data-docs-v2-page={item.document_id}
            onClick={() => onOpenDocument?.(item.document_id)}
            className="flex w-full items-center gap-3 rounded-md border border-slate-200 bg-white px-4 py-3 text-left shadow-sm transition hover:border-blue-300 hover:shadow"
          >
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-semibold text-slate-800">{item.title}</span>
              <span className="block truncate font-mono text-xs text-slate-400">{item.document_id}</span>
            </span>
            <span className="text-right text-xs text-slate-500">
              <span className="block">r{item.revision_number ?? 0} · {item.block_count ?? 0} blocks</span>
              <span className="block">{item.lifecycle_status ?? "unknown"}</span>
            </span>
          </button>
        ))
      )}
    </div>
  );
}

function DocsV2PageDocument({ apiUrl, documentId, company, project, space, onOpenDocument }: { apiUrl: string; documentId: string; company?: string | null; project?: string | null; space?: string | null; onOpenDocument?: (id: string) => void }) {
  const [page, setPage] = useState<DocsV2PageView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setPage(null);
    setError(null);
    fetchDocsV2Page(apiUrl, documentId, project, company, space)
      .then((result) => {
        if (!cancelled) setPage(result);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [apiUrl, company, documentId, project, space]);

  if (error) {
    return (
      <div data-docs-v2-error className="rounded-md border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-800">
        Docs v2 page <span className="font-mono">{documentId}</span> unavailable (store-live fetch failed): {error}
      </div>
    );
  }
  if (!page) {
    return (
      <div data-docs-v2-loading="page" className="rounded-md border border-slate-200 bg-slate-50 px-4 py-3 text-sm text-slate-500">
        Loading page…
      </div>
    );
  }

  const ctx: EmbedContext = {
    apiUrl,
    company,
    project,
    space,
    depth: 0,
    visited: [documentId],
    resolvedEmbeds: page.resolved_embeds ?? {},
    onOpenDocument,
  };

  return (
    <article data-docs-v2-page={documentId} className="mx-auto max-w-3xl rounded-lg border border-slate-200 bg-white px-8 py-6 shadow-sm">
      <header className="mb-4 border-b border-slate-100 pb-3">
        <h1 className="text-2xl font-bold text-slate-900">{page.title}</h1>
        <div className="mt-1 font-mono text-xs text-slate-400">{documentId}</div>
      </header>
      <RevisionBanner page={page} />
      <BlockList blocks={page.blocks} ctx={ctx} />
    </article>
  );
}

export function DocsV2Surface({ apiUrl, selection, company, project, space, onSelectionChange }: DocsV2SurfaceProps) {
  const openDocument = (documentId: string) => {
    onSelectionChange?.({ documentId });
  };
  return (
    <section data-docs-v2-surface className="mx-auto w-full max-w-5xl px-6 py-6">
      {selection.documentId ? (
        <DocsV2PageDocument
          apiUrl={apiUrl}
          documentId={selection.documentId}
          company={company}
          project={project}
          space={space}
          onOpenDocument={openDocument}
        />
      ) : (
        <DocsV2Index apiUrl={apiUrl} company={company} project={project} space={space} onOpenDocument={openDocument} />
      )}
    </section>
  );
}
