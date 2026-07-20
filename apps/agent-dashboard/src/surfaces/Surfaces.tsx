import { Activity, Bot, ChevronRight, Inbox, Send, TerminalSquare, Users } from "lucide-react";
import type { HarnessTurnEvent, ProviderSession } from "../types";

import { Avatar } from "@/components/workbench/Avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";

import { deliverQueued } from "../api/actions";
import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";

interface SurfaceProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
}

function runtimeTone(status?: string): "good" | "running" | "warn" | "idle" {
  if (status === "running" || status === "busy") return "running";
  if (status === "ready" || status === "idle" || status === "succeeded") return "good";
  if (status === "failed" || status === "blocked" || status === "stale") return "warn";
  return "idle";
}

/**
 * Compatibility directory for execution AgentMembers. Durable Standing Agents
 * live in Company OS Organization; MemberRuns live under an AgentTeamRun. This
 * page intentionally does not project either identity into superseded work objects.
 */
export function AgentsList({ model, onSelectionChange }: SurfaceProps) {
  const members = model.snapshot.members ?? [];
  return (
    <section className="space-y-5" aria-labelledby="execution-members-title">
      <header>
        <p className="text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">Execution directory</p>
        <h1 id="execution-members-title" className="mt-1 text-2xl font-semibold tracking-tight">Agent members</h1>
        <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
          Provider-backed execution identities. Standing Agents are managed from Organization and per-attempt members from Agent Teams.
        </p>
      </header>
      {members.length ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {members.map((member) => {
            const status = member.runtime_status ?? member.status;
            return (
              <button
                key={member.id}
                type="button"
                onClick={() => onSelectionChange({ surface: "agents", memberId: member.id })}
                className="rounded-xl border border-border bg-card p-4 text-left transition hover:border-primary/30 hover:shadow-sm"
              >
                <div className="flex items-center gap-3">
                  <Avatar name={member.name ?? member.id} tone={runtimeTone(status)} />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-semibold">{member.name ?? member.id}</p>
                    <p className="truncate text-xs text-muted-foreground">{member.role ?? "Agent member"}</p>
                  </div>
                  <Badge tone={runtimeTone(status)}>{status ?? "unknown"}</Badge>
                </div>
                <div className="mt-3 flex items-center gap-3 text-xs text-muted-foreground">
                  <span>{member.provider ?? "provider unset"}</span>
                  <span>·</span>
                  <span>{member.inbox_count ?? 0} inbox</span>
                </div>
              </button>
            );
          })}
        </div>
      ) : (
        <Card>
          <CardContent className="flex min-h-48 flex-col items-center justify-center text-center">
            <Users className="size-7 text-muted-foreground" />
            <p className="mt-3 text-sm font-medium">No execution members</p>
            <p className="mt-1 text-xs text-muted-foreground">Create members from an Agent Team run when execution needs them.</p>
          </CardContent>
        </Card>
      )}
    </section>
  );
}

export function AgentDetail({ model, onSelectionChange, actionsEnabled, onAction }: SurfaceProps) {
  const member = model.selectedMember;
  if (!member) return <AgentsList model={model} onSelectionChange={onSelectionChange} />;

  const status = member.runtime_status ?? member.status;
  const sessions = (model.snapshot.provider_sessions ?? []).filter((session) => session.agent_member_id === member.id);
  const messages = (model.snapshot.messages ?? []).filter(
    (message) => message.from_agent_id === member.id || message.to_agent_id === member.id,
  );

  return (
    <div className="flex h-full min-h-0 w-full flex-col bg-background lg:flex-row">
      <main className="min-w-0 flex-1 overflow-y-auto p-5 sm:p-8">
        <button type="button" onClick={() => onSelectionChange({ surface: "agents", memberId: undefined })} className="text-xs text-muted-foreground hover:text-foreground">
          ← Agent members
        </button>
        <div className="mt-5 flex items-start gap-4">
          <Avatar name={member.name ?? member.id} tone={runtimeTone(status)} size="lg" />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="text-2xl font-semibold tracking-tight">{member.name ?? member.id}</h1>
              <Badge tone={runtimeTone(status)}>{status ?? "unknown"}</Badge>
            </div>
            <p className="mt-1 text-sm text-muted-foreground">{member.role ?? "Agent member"} · {member.provider ?? "provider unset"}</p>
            {member.description && <p className="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">{member.description}</p>}
          </div>
          <Button
            size="sm"
            disabled={!actionsEnabled || !onAction}
            onClick={() => {
              if (!onAction) return;
              const action = deliverQueued(member.id, { startRuntime: true });
              onAction(action.path, action.body);
            }}
          >
            <Send className="size-3.5" /> Deliver inbox
          </Button>
        </div>

        <section className="mt-8 space-y-3" aria-labelledby="member-activity-title">
          <h2 id="member-activity-title" className="text-sm font-semibold">Conversation and activity</h2>
          {messages.length ? messages.slice(-30).reverse().map((message) => (
            <article key={message.id} className={cn("rounded-xl border p-4", message.from_agent_id === member.id ? "border-primary/15 bg-primary/[0.03]" : "border-border bg-card")}>
              <div className="flex items-center justify-between gap-3 text-xs text-muted-foreground">
                <span>{message.from_agent_id === member.id ? member.name ?? member.id : message.from_agent_id}</span>
                <span>{message.created_at ? new Date(message.created_at).toLocaleString() : ""}</span>
              </div>
              <p className="mt-2 whitespace-pre-wrap text-sm leading-6">{message.content}</p>
            </article>
          )) : (
            <div className="rounded-xl border border-dashed border-border p-8 text-center text-sm text-muted-foreground">
              No recorded messages for this execution identity.
            </div>
          )}
        </section>
      </main>

      <aside className="w-full shrink-0 border-t border-border bg-card/50 p-5 lg:w-80 lg:border-l lg:border-t-0">
        <h2 className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">Runtime context</h2>
        <div className="mt-4 grid grid-cols-2 gap-2">
          <Metric icon={<Inbox className="size-3.5" />} label="Inbox" value={member.inbox_count ?? 0} />
          <Metric icon={<Activity className="size-3.5" />} label="Sessions" value={sessions.length} />
        </div>
        <div className="mt-5 space-y-2">
          {sessions.slice(0, 8).map((session) => (
            <div key={session.id} className="rounded-lg border border-border bg-background/70 p-3">
              <div className="flex items-center gap-2">
                <TerminalSquare className="size-3.5 text-muted-foreground" />
                <p className="min-w-0 flex-1 truncate text-xs font-medium">{session.provider ?? "Provider session"}</p>
                <Badge tone={runtimeTone(session.status)}>{session.status}</Badge>
              </div>
              <p className="mt-1 truncate font-mono text-[10px] text-muted-foreground">{session.id}</p>
            </div>
          ))}
          {!sessions.length && <p className="text-xs text-muted-foreground">No provider sessions recorded.</p>}
        </div>
      </aside>
    </div>
  );
}

function Metric({ icon, label, value }: { icon: React.ReactNode; label: string; value: number }) {
  return (
    <div className="rounded-lg border border-border bg-background/70 p-3">
      <div className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-muted-foreground">{icon}{label}</div>
      <p className="mt-1 text-xl font-semibold tabular-nums">{value}</p>
    </div>
  );
}

export function DebugSurface({ model, sourceLabel }: { model: WorkbenchModel; sourceLabel: string }) {
  const snapshot = model.snapshot;
  const rows = [
    ["Source", sourceLabel],
    ["Generated", snapshot.generated_at ?? "unknown"],
    ["Missions", String(snapshot.missions?.length ?? 0)],
    ["Waves", String(snapshot.waves?.length ?? 0)],
    ["Agent team runs", String(snapshot.team_runs?.length ?? 0)],
    ["Workflow runs", String(snapshot.workflow_runs?.length ?? 0)],
    ["Provider sessions", String(snapshot.provider_sessions?.length ?? 0)],
  ];
  return (
    <section className="space-y-5">
      <header>
        <p className="text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">Platform</p>
        <h1 className="mt-1 text-2xl font-semibold tracking-tight">Diagnostics</h1>
      </header>
      <Card>
        <CardHeader><CardTitle className="flex items-center gap-2 text-sm"><Bot className="size-4" /> Native execution snapshot</CardTitle></CardHeader>
        <CardContent className="divide-y divide-border">
          {rows.map(([label, value]) => <div key={label} className="flex items-center justify-between gap-4 py-3 text-sm"><span className="text-muted-foreground">{label}</span><span className="font-mono text-xs">{value}</span></div>)}
        </CardContent>
      </Card>
    </section>
  );
}

/** Compact provider-turn disclosure used by the Workflow run surface. */
export function TurnDrillIn({
  session,
  liveNormalizedEvents = [],
  historical = false,
  defaultOpen = false,
}: {
  session: ProviderSession;
  apiUrl?: string;
  liveNormalizedEvents?: HarnessTurnEvent[];
  historical?: boolean;
  defaultOpen?: boolean;
}) {
  return (
    <details open={defaultOpen} className="min-w-0 flex-1 rounded-md border border-border bg-background/50">
      <summary className="flex cursor-pointer list-none items-center gap-2 px-2.5 py-2 text-[10px] text-muted-foreground">
        <ChevronRight className="size-3" />
        <span>{historical ? "Recorded provider turn" : "Live provider turn"}</span>
        <span className="ml-auto font-mono">{liveNormalizedEvents.length} events</span>
      </summary>
      <div className="border-t border-border px-2.5 py-2 text-[11px] text-muted-foreground">
        <p className="font-mono">{session.id}</p>
        <p className="mt-1">{session.status} · {session.provider ?? "provider"}</p>
      </div>
    </details>
  );
}
