import { useEffect, useState } from "react";
import { ArrowLeft, FileWarning } from "lucide-react";
import { fetchSource, type SourceViewerDocument } from "@/api";
import { Button } from "@/components/ui/button";
import { Markdown } from "./Markdown";

export function SourceViewer({ apiUrl, project, space, path, line, messageId, onBack }: {
  apiUrl: string;
  project: string;
  space: string;
  path: string;
  line?: number;
  messageId?: string;
  onBack: () => void;
}) {
  const [document, setDocument] = useState<SourceViewerDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setDocument(null);
    setError(null);
    fetchSource(apiUrl, project, space, path, line)
      .then((value) => { if (live) setDocument(value); })
      .catch((reason) => { if (live) setError(String(reason)); });
    return () => { live = false; };
  }, [apiUrl, project, space, path, line]);
  return <section data-testid="source-viewer" aria-label="Local evidence viewer" className="absolute inset-0 z-30 flex min-w-0 flex-col bg-background">
    <header className="flex items-center gap-3 border-b border-border px-4 py-3">
      <Button type="button" size="sm" variant="ghost" onClick={onBack}><ArrowLeft className="size-4"/>Back</Button>
      <div className="min-w-0"><h1 className="truncate text-sm font-semibold">Local evidence</h1><p className="truncate font-mono text-[10px] text-muted-foreground">{path}{line ? `:${line}` : ""}</p></div>
      {messageId && <span className="ml-auto hidden text-[10px] text-muted-foreground sm:block">Message {messageId}</span>}
    </header>
    <div className="min-h-0 flex-1 overflow-auto p-4 sm:p-6">
      {error ? <EvidenceError kind="resolution_error" detail={error}/> : document ? <SourceViewerContent document={document}/> : <p className="text-sm text-muted-foreground">Resolving local evidence…</p>}
    </div>
  </section>;
}

export function SourceViewerContent({ document }: { document: SourceViewerDocument }) {
  if ((document.kind === "markdown" || document.kind === "text") && document.content !== undefined) {
    const lines = document.content.replace(/\r\n/g, "\n").split("\n");
    const selected = document.line ? lines[document.line - 1] : undefined;
    return <article className="mx-auto max-w-5xl" data-source-kind={document.kind}>
      <div className="mb-4 flex flex-wrap gap-3 border-b border-border pb-3 text-[10px] text-muted-foreground"><span className="break-all font-mono">{document.path}</span><span>{document.size} bytes</span></div>
      {selected !== undefined && <p className="mb-4 rounded-md border border-primary/30 bg-primary/5 p-3 font-mono text-xs"><span className="mr-3 select-none text-muted-foreground">{document.line}</span><mark data-highlighted-line={document.line} className="bg-primary/20 text-foreground">{selected || " "}</mark></p>}
      {document.kind === "markdown" ? <Markdown source={document.content}/> : <pre className="overflow-x-auto rounded-md border border-border bg-muted/30 p-4 text-xs leading-relaxed">{lines.map((text, index) => <span key={index} data-highlighted-line={document.line === index + 1 ? index + 1 : undefined} className={document.line === index + 1 ? "block bg-primary/15" : "block"}><span className="mr-4 inline-block w-8 select-none text-right text-muted-foreground">{index + 1}</span>{text || " "}</span>)}</pre>}
    </article>;
  }
  const detail = document.kind === "binary"
    ? `This file is binary or exceeds the 512 KiB viewer limit (${document.size} bytes).`
    : document.kind === "missing"
      ? "The cited file does not exist inside the selected workspace."
      : "The cited path is outside the selected project and attached member workspaces.";
  return <EvidenceError kind={document.kind} detail={detail}/>;
}

function EvidenceError({ kind, detail }: { kind: string; detail: string }) {
  return <div role="alert" data-source-error={kind} className="mx-auto max-w-2xl rounded-lg border border-status-warn/30 bg-status-warn/5 p-6"><FileWarning className="size-6 text-status-warn"/><h2 className="mt-3 text-sm font-semibold">Evidence could not be resolved</h2><p className="mt-1 text-xs leading-relaxed text-muted-foreground">{detail}</p></div>;
}
