import { useEffect, useState } from "react";
import { ShieldAlert } from "lucide-react";

import { cn } from "@/lib/utils";
import { fetchMeta } from "../../api";
import type { HarnessMeta } from "../../types";

/**
 * This frontend bundle's own build rev, injected at build/dev-server start by
 * `vite.config.ts` (`define: { "import.meta.env.VITE_DASHBOARD_GIT_REV": ... }`).
 * "unknown" only when the build environment had no git available.
 */
const FRONTEND_GIT_REV = import.meta.env.VITE_DASHBOARD_GIT_REV || "unknown";

/** How often the idle footer re-checks `/v1/meta` on its own (independent of
 * the main snapshot poll/SSE — this is a lightweight, self-contained check). */
const META_POLL_MS = 30_000;

interface ProvenanceFooterProps {
  apiUrl: string;
  projectId?: string | null;
  spaceId?: string | null;
}

/**
 * Persistent, unobtrusive provenance strip (issue #307 — 2nd occurrence of "the
 * panel showed something other than Store truth"; 1st was fixture
 * impersonation, PR #291; this one was a dashboard served from a stale,
 * pre-TeamWorksBoard commit while the store had 8 Works the whole time).
 *
 * Always shows the server's `git_rev` + `latest_op_seq` (from `GET /v1/meta`)
 * next to this bundle's OWN build rev, so a screenshot alone carries enough
 * provenance to answer "is this the truth?" without server logs. A prominent
 * banner replaces the quiet strip only when the two revs disagree (a stale
 * build silently impersonating a fresh one) or `/v1/meta` is unreachable —
 * the strip itself stays small and out of the way whenever revs match.
 */
export function ProvenanceFooter({ apiUrl, projectId, spaceId }: ProvenanceFooterProps) {
  const [meta, setMeta] = useState<HarnessMeta | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const next = await fetchMeta(apiUrl, projectId, spaceId);
        if (cancelled) return;
        setMeta(next);
        setError(null);
      } catch (fetchError) {
        if (cancelled) return;
        setMeta(null);
        setError(fetchError instanceof Error ? fetchError.message : String(fetchError));
      }
    }
    void load();
    const id = window.setInterval(() => void load(), META_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [apiUrl, projectId, spaceId]);

  const serverRev = meta?.git_rev ?? null;
  // Neither "unknown" build can prove a mismatch against the other — only flag
  // a disagreement between two builds that both actually know their own rev.
  // Synthetic servers (fixture/capture runners report revs like `fixture0`
  // with a `-fixture` version) are not comparable either; a permanent alert
  // there would train operators to ignore the real staleness channel.
  const serverLooksSynthetic = (meta?.server_version ?? "").endsWith("-fixture");
  const revsComparable = Boolean(serverRev) && serverRev !== "unknown" && !serverLooksSynthetic && FRONTEND_GIT_REV !== "unknown";
  const stale = revsComparable && serverRev !== FRONTEND_GIT_REV;

  return (
    <footer className={cn("shrink-0 bg-card/60", (stale || error) ? "border-t border-border" : "sr-only")}>
      {(stale || error) && (
        <div
          role="alert"
          className="flex items-center gap-1.5 border-b border-status-bad/30 bg-status-bad/10 px-3 py-1 text-[11px] text-status-bad"
        >
          <ShieldAlert className="size-3.5 shrink-0" />
          <span className="min-w-0 flex-1 truncate">
            {error
              ? `Provenance unavailable: cannot reach ${apiUrl}/v1/meta (${error})`
              : `Stale build: frontend ${FRONTEND_GIT_REV} ≠ server ${serverRev}`}
          </span>
        </div>
      )}
      <div
        className={cn(
          "flex flex-wrap items-center gap-x-3 gap-y-0.5 px-3 py-1 font-mono text-[10px] text-muted-foreground",
        )}
        title="Dashboard provenance: which build served this screen, which store it read, and how far that store's operation log has advanced (issue #307)"
      >
        <span>frontend {FRONTEND_GIT_REV}</span>
        <span className="text-border">·</span>
        <span>server {serverRev ?? (error ? "unreachable" : "…")}</span>
        <span className="text-border">·</span>
        <span>op #{meta?.latest_op_seq ?? "…"}</span>
        {meta?.store_root && (
          <>
            <span className="text-border">·</span>
            <span className="min-w-0 truncate" title={meta.store_root}>
              store {meta.store_root}
            </span>
          </>
        )}
      </div>
    </footer>
  );
}
