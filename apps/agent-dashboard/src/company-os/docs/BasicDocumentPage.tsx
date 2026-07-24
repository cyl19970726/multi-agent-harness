import { Link2, MoreHorizontal } from "lucide-react";
import { useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { DocSection, DocumentSurface } from "@/components/workbench/atoms";

import { buildDocsAppendBlockCommands, buildDocsChildDocumentCommand, buildDocsInstantiateTemplateBlockCommands, buildDocsReorderBlocksCommand } from "./documentAction";
import { RelationChips } from "./RelationChips";
import type { CompanyOsDocsActionCommand, CompanyOsDocumentBlock, CompanyOsDocumentPageData } from "./types";

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
  const unavailableReason = !actionEnabled
    ? "Connect a Store-live project and provide a session capability before dispatching governed Docs actions."
    : !document.authoring
      ? "This projection does not expose a CustomPageDefinition with document.append and block.append policies."
      : !capabilityToken.trim()
        ? "Enter the session capability before writing Docs truth."
        : undefined;
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
            {document.breadcrumb?.length ? <nav aria-label="Breadcrumb" className="text-xs text-muted-foreground">{document.breadcrumb.join(" / ")}</nav> : null}
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0 space-y-2"><h1 className="text-2xl font-semibold tracking-tight text-foreground sm:text-3xl">{document.title}</h1>{document.description && <p className="max-w-2xl text-sm leading-6 text-muted-foreground">{document.description}</p>}</div>
              <div className="flex shrink-0 gap-1.5"><Button variant="outline" size="sm" onClick={() => onRequestAction?.("new-action", document)}>New action</Button><Button size="sm" onClick={() => onRequestAction?.("ask-agent", document)}>Ask an agent</Button><Button variant="ghost" size="icon" aria-label="More document options"><MoreHorizontal /></Button></div>
            </div>
            {document.properties?.length ? <dl className="flex min-w-0 flex-wrap gap-1.5">{document.properties.map((property, index) => <div key={`${property.ref ?? "property"}:${property.label}:${index}`} data-company-os-ref={property.ref} data-actor-type={property.actorType} className="flex max-w-full min-w-0 items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-xs"><dt className="shrink-0 text-muted-foreground">{property.label}:</dt><dd className="min-w-0 break-words font-medium text-foreground">{property.value}</dd></div>)}</dl> : null}
          </header>
          <article className="min-w-0 space-y-4">
            {document.blocks.length
              ? document.blocks.map((block) => <Block key={block.id} block={block} />)
              : <div className="rounded-xl border border-dashed border-border bg-muted/25 p-6 text-sm leading-6 text-muted-foreground" data-docs-empty-document="true">This Document has no Blocks yet. Use the governed composer to append the first durable Block; empty UI state is not company truth.</div>}
          </article>
          {document.updatedLabel && <p className="border-t border-border pt-4 text-xs text-muted-foreground">{document.updatedLabel}</p>}
        </DocumentSurface>
        <aside className="space-y-5 border-t border-border pt-5 lg:border-l lg:border-t-0 lg:pl-5 lg:pt-0" aria-label="Document context">
          <ContextBlock label="Source" links={document.sourceLinks} />
          <ContextBlock label="Results" links={document.resultLinks} />
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
          {!document.sourceLinks?.length && !document.resultLinks?.length && !document.connectedRecords?.length && <p className="rounded-md border border-dashed border-border p-3 text-xs leading-5 text-muted-foreground"><Link2 className="mr-1 inline size-3.5" />Connected records appear here when the host resolves them.</p>}
        </aside>
      </div>
    </main>
  );
}
