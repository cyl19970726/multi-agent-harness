import { AlertTriangle, ArrowRight } from "lucide-react";
import type { ReactNode } from "react";

import { Badge } from "@/components/ui/badge";
import { preserveCompanyOsWorkbenchContext } from "../docs/url";
import { WanchengwanlingCommandCenter } from "./wanchengwanling/WanchengwanlingCommandCenter";

type Json = Record<string, unknown>;

function objects(value: unknown): Json[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is Json => Boolean(item) && typeof item === "object" && !Array.isArray(item));
}

function unbox(value: Json): Json {
  const record = value.record;
  return record && typeof record === "object" && !Array.isArray(record)
    ? { ...(record as Json), ...value }
    : value;
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function pageDefinition(source: unknown, pageId: string): Json | undefined {
  const root = source && typeof source === "object" && !Array.isArray(source) ? source as Json : {};
  return objects(root.custom_page_definitions).map(unbox).find((entry) => text(entry.id) === pageId);
}

function standardFallbackHref(definition?: Json): string {
  const moduleId = text(definition?.module_id);
  if (moduleId) return preserveCompanyOsWorkbenchContext(`?surface=docs&module=${encodeURIComponent(moduleId)}`) ?? `?surface=docs&module=${encodeURIComponent(moduleId)}`;
  return preserveCompanyOsWorkbenchContext("?surface=docs") ?? "?surface=docs";
}

function CustomPageFallback({ pageId, source, children }: { pageId: string; source: unknown; children?: ReactNode }) {
  const definition = pageDefinition(source, pageId);
  return (
    <main className="h-full overflow-auto bg-background p-5 sm:p-8" data-company-os-custom-page={pageId} data-company-os-custom-page-state={definition ? "fallback" : "missing_definition"}>
      <div className="mx-auto max-w-3xl rounded-2xl border border-border bg-card p-6">
        <div className="flex items-center gap-3">
          <AlertTriangle className="size-5 text-status-warn" />
          <Badge tone={definition ? "warn" : "bad"}>{definition ? "Fallback" : "Missing definition"}</Badge>
        </div>
        <h1 className="mt-5 text-2xl font-semibold tracking-tight">Custom page unavailable</h1>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          {definition
            ? "The CustomPageDefinition exists, but this dashboard build does not include a renderer for it. Use the standard module view until the package is implemented."
            : "No CustomPageDefinition with this id exists in the current Store projection."}
        </p>
        {children}
        <a href={standardFallbackHref(definition)} className="mt-5 inline-flex items-center gap-2 rounded-lg bg-primary px-3 py-2 text-sm font-semibold text-primary-foreground">
          Open standard view <ArrowRight className="size-4" />
        </a>
      </div>
    </main>
  );
}

export function CustomPageHost({ pageId, source }: { pageId?: string; source: unknown }) {
  if (!pageId) return <CustomPageFallback pageId="<missing-page>" source={source} />;
  const definition = pageDefinition(source, pageId);
  if (!definition) return <CustomPageFallback pageId={pageId} source={source} />;

  switch (pageId) {
    case "page-wcw-command-center":
      return <WanchengwanlingCommandCenter source={source} pageId={pageId} />;
    default:
      return <CustomPageFallback pageId={pageId} source={source} />;
  }
}
