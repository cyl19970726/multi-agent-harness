import { Fragment, type ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * Minimal, dependency-free markdown renderer for project docs (ADR 0019, Vision
 * `source_refs`). Handles headings, paragraphs, unordered/ordered lists, fenced
 * code blocks, inline code, bold, and links — enough for prose docs. React
 * escapes all text, so no HTML injection. Tables/mermaid render as plain text;
 * this is intentionally a first-cut renderer, not a full CommonMark engine.
 */
export function Markdown({ source, compact = false }: { source: string; compact?: boolean }) {
  return (
    <div className={cn(
      "text-foreground/90",
      compact ? "space-y-2 text-[12px] leading-relaxed" : "space-y-3 text-[13px] leading-relaxed",
    )}>
      {render(source, compact)}
    </div>
  );
}

function render(source: string, compact: boolean): ReactNode[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  let key = 0;

  while (i < lines.length) {
    const line = lines[i];

    // Fenced code block
    if (line.trimStart().startsWith("```")) {
      const code: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].trimStart().startsWith("```")) {
        code.push(lines[i]);
        i += 1;
      }
      i += 1; // closing fence
      blocks.push(
        <pre
          key={key++}
          className={cn(
            "overflow-x-auto rounded-md border border-border bg-muted/50 font-mono text-foreground/90",
            compact ? "p-2.5 text-[10px]" : "p-3 text-[12px]",
          )}
        >
          {code.join("\n")}
        </pre>,
      );
      continue;
    }

    // Heading
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      const text = heading[2];
      const cls =
        level === 1
          ? compact ? "text-[14px] font-semibold tracking-tight" : "text-lg font-semibold"
          : level === 2
            ? compact ? "mt-1 text-[13px] font-semibold text-foreground" : "mt-1 text-base font-semibold"
            : compact ? "text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground" : "text-[13px] font-semibold uppercase tracking-wide text-muted-foreground";
      blocks.push(
        <p key={key++} className={cls}>
          {inline(text)}
        </p>,
      );
      i += 1;
      continue;
    }

    // Horizontal rule
    if (/^(-{3,}|\*{3,})\s*$/.test(line)) {
      blocks.push(<hr key={key++} className="border-border" />);
      i += 1;
      continue;
    }

    // List (unordered or ordered) — consume a contiguous run, JOINING multi-line
    // items (indented continuation lines belong to the current item) and tolerating
    // blank lines between items, so a loose list stays ONE <ol>/<ul> with correct
    // 1..n numbering instead of collapsing into many single-item lists.
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\.\s+/.test(line);
      const items: string[] = [];
      while (i < lines.length) {
        const l = lines[i];
        const marker = /^\s*([-*]|\d+\.)\s+(.*)$/.exec(l);
        if (marker) {
          items.push(marker[2]);
          i += 1;
        } else if (l.trim() === "") {
          // A blank line continues the list only if another item follows it.
          let j = i + 1;
          while (j < lines.length && lines[j].trim() === "") j += 1;
          if (j < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[j])) {
            i = j;
          } else {
            break;
          }
        } else if (items.length > 0 && /^\s+\S/.test(l)) {
          // Indented continuation → fold it onto the current item.
          items[items.length - 1] += " " + l.trim();
          i += 1;
        } else {
          break;
        }
      }
      const ListTag = ordered ? "ol" : "ul";
      blocks.push(
        <ListTag
          key={key++}
          className={
            ordered
              ? "list-decimal space-y-1.5 pl-5 marker:text-muted-foreground"
              : "list-disc space-y-1.5 pl-5 marker:text-muted-foreground"
          }
        >
          {items.map((item, index) => (
            <li key={index} className="pl-1">
              {inline(item)}
            </li>
          ))}
        </ListTag>,
      );
      continue;
    }

    // Blank line
    if (line.trim() === "") {
      i += 1;
      continue;
    }

    // Paragraph — gather until blank line
    const para: string[] = [];
    while (i < lines.length && lines[i].trim() !== "" && !/^\s*([-*]|\d+\.)\s+/.test(lines[i]) && !/^#{1,4}\s/.test(lines[i]) && !lines[i].trimStart().startsWith("```")) {
      para.push(lines[i]);
      i += 1;
    }
    blocks.push(
      <p key={key++}>{inline(para.join(" "))}</p>,
    );
  }

  return blocks;
}

/** Inline formatting: `code`, **bold**, [text](url). */
function inline(text: string): ReactNode {
  const nodes: ReactNode[] = [];
  const regex = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\[[^\]]+\]\([^)]+\))/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let key = 0;
  while ((match = regex.exec(text)) !== null) {
    if (match.index > last) nodes.push(<Fragment key={key++}>{text.slice(last, match.index)}</Fragment>);
    const token = match[0];
    if (token.startsWith("`")) {
      nodes.push(
        <code key={key++} className="rounded bg-muted px-1 py-0.5 font-mono text-[12px]">
          {token.slice(1, -1)}
        </code>,
      );
    } else if (token.startsWith("**")) {
      nodes.push(
        <strong key={key++} className="font-semibold text-foreground">
          {token.slice(2, -2)}
        </strong>,
      );
    } else {
      const linkMatch = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token);
      if (linkMatch) {
        const href = safeMarkdownHref(linkMatch[2]);
        nodes.push(href ? (
          <a
            key={key++}
            href={href}
            rel="noreferrer"
            className="font-medium text-primary underline decoration-primary/25 underline-offset-2 hover:decoration-primary"
          >
            {linkMatch[1]}
          </a>
        ) : <Fragment key={key++}>{linkMatch[1]}</Fragment>);
      }
    }
    last = match.index + token.length;
  }
  if (last < text.length) nodes.push(<Fragment key={key++}>{text.slice(last)}</Fragment>);
  return nodes;
}

function safeMarkdownHref(value: string): string | undefined {
  const href = value.trim();
  return /^(https?:\/\/|\/|#)/i.test(href) ? href : undefined;
}
