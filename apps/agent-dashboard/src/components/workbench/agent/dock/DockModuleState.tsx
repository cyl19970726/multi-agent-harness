import { AlertTriangle, Inbox, LoaderCircle, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import type { DockModuleStatus } from "./types";

export function DockModuleState({ status, emptyTitle, emptyDetail }: {
  status?: DockModuleStatus;
  emptyTitle?: string;
  emptyDetail?: string;
}) {
  if (!status || status.kind === "ready") {
    if (!emptyTitle) return null;
    return <div className="agent-dock-state" data-kind="empty"><Inbox aria-hidden="true"/><h3>{emptyTitle}</h3><p>{emptyDetail}</p></div>;
  }
  const loading = status.kind === "loading";
  return <div className="agent-dock-state" data-kind={status.kind} role={status.kind === "error" ? "alert" : "status"}>
    {loading ? <LoaderCircle className="animate-spin" aria-hidden="true"/> : <AlertTriangle aria-hidden="true"/>}
    <h3>{loading ? "Loading" : status.kind === "stale" ? "Showing last-known data" : status.kind === "unavailable" ? "Unavailable" : "Could not load this module"}</h3>
    <p>{status.message ?? (loading ? "Reading canonical facts…" : "This module is unavailable; Session remains readable.")}</p>
    {status.lastGoodAt && <p>Last successful read: {status.lastGoodAt}</p>}
    {status.onRetry && <Button size="sm" variant="secondary" onClick={status.onRetry}><RefreshCw className="size-3.5"/>Retry</Button>}
  </div>;
}
