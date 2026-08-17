import { useEffect, useState } from "react";
import { RadioTower, ServerCog, ShieldAlert } from "lucide-react";
import {
  fetchRoleView,
  type OperatorViewData,
  type RoleActionExecutor,
  type RoleView,
} from "../model/roleViews";
import { AttentionStrip, ViewProvenance, ViewState } from "./RoleViewPrimitives";
import { RoleActionPanel } from "./RoleActionPanel";

export function OperatorView({
  apiUrl,
  space,
  project,
  company,
  nodeId,
  onAction,
  actionsCurrent,
}: {
  apiUrl: string;
  space: string;
  project: string;
  company: string;
  nodeId: string;
  onAction: RoleActionExecutor;
  actionsCurrent: boolean;
}) {
  const [view, setView] = useState<RoleView<OperatorViewData> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let live = true;
    setLoading(true);
    fetchRoleView<OperatorViewData>(
      apiUrl,
      `/v1/views/operator/${encodeURIComponent(nodeId)}`,
      { space, project, company },
    )
      .then((next) => {
        if (live) {
          setView(next);
          setError(null);
        }
      })
      .catch((cause) => live && setError(String(cause)))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [apiUrl, space, project, company, nodeId, refresh]);

  const fabric = view?.data.remote_fabric;
  const fabricObserved = fabric?.state === "observed";
  return (
    <div className="space-y-5">
      <header className="flex flex-wrap justify-between gap-3">
        <div>
          <div className="mb-2 flex items-center gap-2 text-xs uppercase tracking-[.16em] text-primary">
            <ServerCog className="size-4" /> Machine authority
          </div>
          <h1 className="text-2xl font-semibold">Nodes</h1>
          <p className="text-sm text-muted-foreground">
            Daemon, Remote Fabric, delivery and recovery truth. No Work acceptance.
          </p>
        </div>
        {view && <ViewProvenance view={view} />}
      </header>
      {loading && view && (
        <div className="text-xs text-muted-foreground" role="status">
          Refreshing authoritative OperatorView…
        </div>
      )}
      <ViewState loading={loading && !view} error={error}>
        {view && (
          <>
            <AttentionStrip view={view} />
            <div className="grid gap-3 sm:grid-cols-4">
              <Card label="Daemon generation" value={String(view.data.node.daemon_generation ?? "unknown")} />
              <Card label="Local delivery backlog" value={String(view.data.delivery_backlog.depth)} />
              <Card label="Runtime recoveries" value={String(view.data.runtime_recovery.length)} />
              <Card label="Build" value={view.data.build.build_sha.slice(0, 8)} />
            </div>
            <section className="rounded-xl border border-border p-4">
              <div className="mb-3 flex items-center justify-between gap-3">
                <div className="flex items-center gap-2">
                  {fabricObserved ? (
                    <RadioTower className="size-4 text-emerald-500" />
                  ) : (
                    <ShieldAlert className="size-4 text-amber-500" />
                  )}
                  <div>
                    <h2 className="text-sm font-semibold">Remote Node Fabric</h2>
                    <p className="text-xs text-muted-foreground">
                      Node-local journal only; online status remains Control Plane lease-derived.
                    </p>
                  </div>
                </div>
                <span className="rounded-full border px-2 py-1 text-[11px] uppercase tracking-wide">
                  {fabric?.state ?? "not selected"}
                </span>
              </div>
              {fabricObserved ? (
                <div className="grid gap-3 sm:grid-cols-3 lg:grid-cols-6">
                  <Card label="Gateway generation" value={String(fabric.gateway_session?.gateway_generation ?? "none")} />
                  <Card label="Remote outbox" value={String(fabric.outbox_depth ?? 0)} />
                  <Card label="Remote inbox" value={String(fabric.inbox_depth ?? 0)} />
                  <Card label="Oldest queued age" value={formatAge(fabric.control_plane_metrics?.oldest_queued_age_ms ?? fabric.oldest_outbox_age_ms)} />
                  <Card label="Gateway lease age" value={formatAge(fabric.control_plane_metrics?.gateway_lease_age_ms)} />
                  <Card label="Reconcile lag" value={String(fabric.control_plane_metrics?.reconcile_lag ?? "unknown")} />
                  <Card label="Reconcile required" value={String(fabric.recovery_required?.length ?? 0)} />
                  <Card label="Delegations" value={String(fabric.collaboration?.delegation_count ?? "unavailable")} />
                  <Card label="Collaboration attention" value={String(fabric.collaboration?.attention_count ?? "unavailable")} />
                </div>
              ) : (
                <p className="text-xs text-muted-foreground">
                  {fabric?.reason ?? "Select a Company to inspect this Node’s Remote Fabric journal."}
                </p>
              )}
            </section>
            <div className="rounded-xl border border-border p-4 text-xs">
              <pre className="overflow-auto whitespace-pre-wrap">
                {JSON.stringify(view.data.diagnostics, null, 2)}
              </pre>
            </div>
            <RoleActionPanel
              actions={view.allowed_actions}
              onAction={onAction}
              actionsCurrent={actionsCurrent && !loading && view.freshness === "current"}
              context={{ nodeId }}
              onCompleted={() => setRefresh((value) => value + 1)}
            />
          </>
        )}
      </ViewState>
    </div>
  );
}

function Card({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-xl border border-border p-4">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-2 font-mono text-lg font-semibold">{value}</div>
    </div>
  );
}

function formatAge(value: number | null | undefined): string {
  if (value === null || value === undefined) return "unknown";
  if (value < 1000) return `${value} ms`;
  return `${Math.floor(value / 1000)} s`;
}
