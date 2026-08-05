import { Activity, Bot, Inbox, MessageSquare, Send, TerminalSquare, Users } from "lucide-react";

import { Avatar } from "@/components/workbench/Avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";

import { deliverQueued } from "../api/actions";
import { providerDisplayName, providerStackLine } from "@/lib/provider";
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
                <div className="mt-3 space-y-1 text-xs text-muted-foreground">
                  <p className="truncate">{providerStackLine(member.provider, member.native_session?.execution_mode, member.model)}</p>
                  <p>{member.inbox_count ?? 0} inbox</p>
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
  const nativeSession = member.native_session;
  const messages = (model.snapshot.messages ?? []).filter(
    (message) => message.from_agent_id === member.id || message.to_agent_id === member.id,
  );
  // Chat with a live runtime happens in the team surface; resolve the newest
  // MemberRun that explicitly links this durable identity.
  const latestMemberRun = (model.snapshot.member_runs ?? [])
    .filter((run) => run.agent_member_id === member.id)
    .sort((a, b) => (b.started_at ?? "").localeCompare(a.started_at ?? ""))[0];

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
            <p className="mt-1 text-sm text-muted-foreground">{member.role ?? "Agent member"} · {providerDisplayName(member.provider)}</p>
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
          <Metric icon={<Activity className="size-3.5" />} label="Native session" value={nativeSession ? 1 : 0} />
        </div>
        <div className="mt-5 space-y-2">
          {nativeSession && (
            <div className="rounded-lg border border-border bg-background/70 p-3">
              <div className="flex items-center gap-2">
                <TerminalSquare className="size-3.5 text-muted-foreground" />
                <p className="min-w-0 flex-1 truncate text-xs font-medium">{nativeSession.provider} native session</p>
                <Badge tone={nativeSession.availability === "available" ? "good" : "warn"}>{nativeSession.availability}</Badge>
              </div>
              <p className="mt-1 truncate font-mono text-[10px] text-muted-foreground">{nativeSession.native_session_id}</p>
            </div>
          )}
          {!nativeSession && <p className="text-xs text-muted-foreground">No provider-native session is bound yet.</p>}
        </div>

        <div className="mt-5">
          <h2 className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">Model & configuration</h2>
          <dl className="mt-3 space-y-2 rounded-lg border border-border bg-background/70 p-3 text-xs">
            <ConfigRow label="Model" value={member.model ?? "Not configured"} mono />
            <ConfigRow label="Execution mode" value={nativeSession?.execution_mode ?? "Not recorded"} />
            {member.profile && <ConfigRow label="Profile" value={member.profile} />}
            {member.provider_config?.permission_profile && <ConfigRow label="Permission profile" value={member.provider_config.permission_profile} />}
            {member.provider_config?.approval_policy && <ConfigRow label="Approval policy" value={member.provider_config.approval_policy} />}
            {member.provider_config?.approvals_reviewer && <ConfigRow label="Approvals reviewer" value={member.provider_config.approvals_reviewer} />}
            {member.provider_config?.sandbox_policy && <ConfigRow label="Sandbox policy" value={member.provider_config.sandbox_policy} />}
            {member.provider_config?.service_tier && <ConfigRow label="Service tier" value={member.provider_config.service_tier} />}
            {member.provider_config?.collaboration_mode && <ConfigRow label="Collaboration mode" value={member.provider_config.collaboration_mode} />}
            {member.provider_config?.environment_id && <ConfigRow label="Environment" value={member.provider_config.environment_id} mono />}
            {(member.provider_config?.runtime_workspace_roots?.length ?? 0) > 0 && (
              <ConfigRow label="Workspace roots" value={member.provider_config!.runtime_workspace_roots!.join(", ")} mono />
            )}
            {(member.provider_config?.mcp?.servers?.length ?? 0) > 0 && (
              <ConfigRow label="MCP servers" value={member.provider_config!.mcp!.servers!.map((server) => server.id).join(", ")} />
            )}
            <ConfigRow
              label="Runtime"
              value={member.runtime_alive
                ? `alive${member.runtime_pid ? ` · pid ${member.runtime_pid}` : ""}`
                : member.runtime_status ?? "not running"}
            />
            {member.control_endpoint && <ConfigRow label="Control endpoint" value={member.control_endpoint} mono />}
            {member.provider_thread_id && <ConfigRow label="Provider thread" value={member.provider_thread_id} mono />}
            {(member.provider_child_thread_count ?? 0) > 0 && <ConfigRow label="Child threads" value={String(member.provider_child_thread_count)} />}
            {member.runtime_health?.protocol_probe && <ConfigRow label="Protocol probe" value={member.runtime_health.protocol_probe} />}
            {member.runtime_health?.delivery_probe && <ConfigRow label="Delivery probe" value={member.runtime_health.delivery_probe} />}
            {member.prompt_ref && <ConfigRow label="Prompt ref" value={member.prompt_ref} mono />}
            {(member.skill_refs?.length ?? 0) > 0 && <ConfigRow label="Skill refs" value={member.skill_refs!.join(", ")} />}
          </dl>
        </div>

        <div className="mt-5">
          <h2 className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">Chat</h2>
          {latestMemberRun ? (
            <>
              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                Messages are delivered through this member's team run, not the directory page.
              </p>
              <Button
                size="sm"
                variant="outline"
                className="mt-2"
                onClick={() => onSelectionChange({ surface: "team", teamId: latestMemberRun.team_run_id, memberRunId: latestMemberRun.id })}
              >
                <MessageSquare className="size-3.5" /> Open team chat
              </Button>
            </>
          ) : (
            <p className="mt-2 text-xs leading-5 text-muted-foreground">
              This member has no active team run. Chat is delivered through team assignments; use Deliver inbox for queued messages.
            </p>
          )}
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

function ConfigRow({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="grid grid-cols-[7rem_1fr] gap-2">
      <dt className="min-w-0 text-muted-foreground">{label}</dt>
      <dd className={cn("min-w-0 break-words text-foreground", mono && "font-mono text-[10px]")}>{value}</dd>
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
    ["Bound native sessions", String(snapshot.members?.filter((member) => member.native_session).length ?? 0)],
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
