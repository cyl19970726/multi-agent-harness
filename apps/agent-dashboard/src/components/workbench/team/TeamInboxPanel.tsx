import { useEffect, useState } from "react";
import { Inbox, MailQuestion } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { formatDate, formatAbsolute, isoTime } from "@/components/workbench/team/teamFormat";
import {
  fetchRoleView,
  type RoleView,
  type TeamInboxData,
  type TeamInboxItem,
} from "../../../model/roleViews";
import { ViewProvenance, ViewState } from "../../../surfaces/RoleViewPrimitives";

function deliveryTone(status: string): "good" | "warn" | "bad" | "muted" {
  if (status === "queued" || status === "routed") return "warn";
  if (status === "claimed" || status === "provider_received" || status === "acknowledged") return "good";
  if (status === "failed" || status === "invalidated") return "bad";
  return "muted";
}

function InboxRow({ item, teamId, onOpenWork }: { item: TeamInboxItem; teamId: string; onOpenWork?: (workId: string) => void }) {
  const message = item.message;
  const sourceTeam = message?.source_team_id ?? null;
  const peer = sourceTeam != null && sourceTeam !== teamId;
  return (
    <article className="rounded-lg border border-border bg-card/60 px-3 py-2.5" data-delivery-id={item.delivery_id}>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <Badge tone={deliveryTone(item.delivery_status)}>{item.delivery_status.replace(/_/g, " ")}</Badge>
        <span className="text-[10px] font-semibold uppercase tracking-[.1em] text-muted-foreground">
          {peer ? `from peer Team ${sourceTeam}` : sourceTeam ? "from this Team" : "peer source unrecorded"}
        </span>
        <span className="ml-auto text-[10px] text-muted-foreground">
          <time dateTime={isoTime(item.created_at)} title={formatAbsolute(item.created_at)}>{formatDate(item.created_at)}</time>
        </span>
      </div>
      <p className="mt-1.5 line-clamp-2 break-words text-[12px] leading-[1.45] text-foreground">
        {message?.body?.trim() || "No body summary."}
      </p>
      <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-muted-foreground">
        <span>author · {message?.sender_actor_ref?.display_name ?? message?.sender_actor_ref?.id ?? "unknown"}</span>
        <span className="truncate font-mono" title={item.message_id}>{message?.kind ?? "message"} · {item.message_id}</span>
        {message?.work_id && (
          <button
            type="button"
            className="text-primary hover:underline"
            onClick={() => onOpenWork?.(message.work_id!)}
          >
            Work context · {message.work_id}
          </button>
        )}
        <span className="ml-auto">
          {item.claim_id
            ? `claimed by ${item.resolved_team_membership_id ?? "membership"} · gen ${item.claimed_node_daemon_generation ?? "—"}`
            : "unclaimed — one eligible Host/Member membership may claim (CLI: team inbox claim)"}
        </span>
      </div>
    </article>
  );
}

/**
 * Shared Team Inbox (DOC-106): Team-addressed peer Messages land as one
 * Team-subject canonical delivery each — no member fan-out. This panel is a
 * read projection: claim/dispatch remain authenticated mutations, not reads.
 */
export function TeamInboxPanel({ apiUrl, space, project, teamId, refreshKey, onOpenWork }: {
  apiUrl: string; space: string; project: string; teamId: string; refreshKey?: string;
  onOpenWork?: (workId: string) => void;
}) {
  const [view, setView] = useState<RoleView<TeamInboxData> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let live = true;
    setLoading(true);
    setError(null);
    fetchRoleView<TeamInboxData>(apiUrl, `/v1/views/team-inbox/${encodeURIComponent(teamId)}`, { space, project })
      .then((value) => { if (live) setView(value); })
      .catch((reason) => { if (live) setError(String(reason)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [apiUrl, space, project, teamId, refreshKey]);

  return (
    <section aria-label="Shared Team Inbox" className="space-y-2" data-testid="team-inbox-panel">
      <div className="flex items-center justify-between gap-2">
        <h2 className="flex items-center gap-2 text-sm font-semibold">
          <Inbox className="size-4 text-primary" /> Team Inbox
          {view && <span className="rounded-full bg-primary/10 px-1.5 text-[9px] text-primary">{view.data.items.length}</span>}
        </h2>
        {view && <ViewProvenance view={view} />}
      </div>
      <ViewState loading={loading} error={error} identityLabel="Team Inbox">
        {view && (
          view.data.items.length === 0 ? (
            <p className="rounded-lg border border-dashed border-border px-4 py-5 text-center text-xs text-muted-foreground">
              <MailQuestion className="mx-auto mb-1.5 size-4" />
              No Team-addressed deliveries. Peer Teams can message this Team without a WorkDelegation.
            </p>
          ) : (
            <div className="space-y-2">
              {view.data.items.map((item) => (
                <InboxRow key={item.delivery_id} item={item} teamId={teamId} onOpenWork={onOpenWork} />
              ))}
            </div>
          )
        )}
      </ViewState>
    </section>
  );
}
