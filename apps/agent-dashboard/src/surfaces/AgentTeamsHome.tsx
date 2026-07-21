import { ArrowRight, Users } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import {
  DocumentSurface,
  EmptyState,
  StatusDot,
  type StatusTone,
} from "@/components/workbench/atoms";
import { cn } from "@/lib/utils";

import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";
import type { MemberRun, TeamRun, Wave } from "../types";

interface AgentTeamsHomeProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
}

interface NativeAttempt {
  run: TeamRun;
  wave: Wave;
  members: MemberRun[];
}

/**
 * Native Agent Team entry point. Only Mission/Wave-linked attempts belong in
 * the active product surface; deliberately unlinked historical rows are not a
 * compatibility UI and remain visible only through raw diagnostics/export.
 */
export function AgentTeamsHome({ model, onSelectionChange }: AgentTeamsHomeProps) {
  const snapshot = model.snapshot;
  const waves = new Map((snapshot.waves ?? []).map((wave) => [wave.id, wave]));
  const membersByRun = groupBy(snapshot.member_runs ?? [], (member) => member.team_run_id);
  const attempts = (snapshot.team_runs ?? [])
    .flatMap((run): NativeAttempt[] => {
      if (!run.mission_id || !run.wave_id) return [];
      const wave = waves.get(run.wave_id);
      if (!wave || wave.mission_id !== run.mission_id) return [];
      return [{ run, wave, members: membersByRun.get(run.id) ?? [] }];
    })
    .sort((left, right) => timestamp(right.run.updated_at ?? right.run.created_at) - timestamp(left.run.updated_at ?? left.run.created_at));

  return (
    <DocumentSurface className="max-w-[1120px]">
      <header className="flex flex-wrap items-end justify-between gap-5 border-b border-border/70 pb-5">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">
            Native execution
          </p>
          <h1 className="mt-1 text-2xl font-semibold tracking-tight text-foreground">Agent Teams</h1>
          <p className="mt-1 max-w-2xl text-sm leading-6 text-muted-foreground">
            Mission/Wave-linked team attempts. Open a team to inspect members, assignments,
            durable activity, current pressure, and operator controls.
          </p>
        </div>
        <button
          type="button"
          onClick={() => onSelectionChange({ surface: "missions", missionId: undefined, waveId: undefined, teamId: undefined })}
          className="inline-flex items-center gap-1.5 rounded-md border border-border bg-background px-3 py-2 text-xs font-medium text-foreground transition-colors hover:border-primary/30 hover:bg-primary/[0.035]"
        >
          Open Missions <ArrowRight className="size-3.5" />
        </button>
      </header>

      {attempts.length === 0 ? (
        <div className="pt-6">
          <EmptyState
            icon={Users}
            title="No native Agent Team attempts"
            description="Create an Agent Team executor from a Mission Wave. Unlinked compatibility runs are not shown in the active product UI."
          />
        </div>
      ) : (
        <section className="pt-5" aria-label="Agent Team attempts">
          <div className="grid gap-3 lg:grid-cols-2">
            {attempts.map(({ run, wave, members }) => {
              const tone = runTone(run.status);
              const pressure = members.filter((member) => ["blocked", "failed", "waiting", "reviewing"].includes(member.status ?? ""));
              return (
                <button
                  key={run.id}
                  type="button"
                  onClick={() => onSelectionChange({ surface: "team", teamId: run.id, memberRunId: undefined })}
                  className={cn(
                    "group min-w-0 rounded-xl border border-border/80 bg-card/65 p-4 text-left transition-all",
                    "hover:-translate-y-0.5 hover:border-primary/25 hover:bg-card hover:shadow-[0_14px_35px_-30px_rgba(17,24,39,.4)]",
                  )}
                >
                  <div className="flex min-w-0 items-start gap-3">
                    <span className="relative mt-0.5 grid size-10 shrink-0 place-items-center rounded-xl border border-primary/15 bg-primary/[0.055] text-primary">
                      <Users className="size-4" />
                      <StatusDot tone={tone} pulse={tone === "running"} className="absolute -bottom-0.5 -right-0.5 ring-2 ring-card" />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="flex min-w-0 items-center gap-2">
                        <span className="truncate text-sm font-semibold text-foreground">{run.objective ?? wave.title}</span>
                        <Badge tone={tone}>{run.status ?? "unknown"}</Badge>
                      </span>
                      <span className="mt-1 block truncate text-xs text-muted-foreground">
                        Wave {wave.index} · {wave.title}
                      </span>
                    </span>
                    <ArrowRight className="mt-2 size-3.5 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-primary" />
                  </div>

                  <div className="mt-4 flex items-center justify-between gap-3 border-t border-border/60 pt-3">
                    <span className="flex min-w-0 items-center">
                      {members.slice(0, 4).map((member, index) => (
                        <span key={member.id} className={cn("rounded-full ring-2 ring-card", index > 0 && "-ml-2")}>
                          <Avatar
                            name={member.name ?? member.id}
                            tone={memberTone(member.status)}
                            size="sm"
                          />
                        </span>
                      ))}
                      <span className="ml-2 text-[11px] text-muted-foreground">
                        {members.length} {members.length === 1 ? "member" : "members"}
                      </span>
                    </span>
                    <span className={cn("text-[11px] font-medium", pressure.length > 0 ? "text-status-warn" : "text-muted-foreground")}>
                      {pressure.length > 0 ? `${pressure.length} need attention` : formatRelative(run.updated_at ?? run.created_at)}
                    </span>
                  </div>
                </button>
              );
            })}
          </div>
        </section>
      )}
    </DocumentSurface>
  );
}

function groupBy<T>(items: T[], key: (item: T) => string | undefined | null): Map<string, T[]> {
  const groups = new Map<string, T[]>();
  for (const item of items) {
    const id = key(item);
    if (!id) continue;
    groups.set(id, [...(groups.get(id) ?? []), item]);
  }
  return groups;
}

function runTone(status?: string | null): StatusTone {
  if (status === "running") return "running";
  if (status === "completed") return "good";
  if (["failed", "cancelled"].includes(status ?? "")) return "bad";
  if (["waiting", "reviewing"].includes(status ?? "")) return "warn";
  if (status === "planning") return "info";
  return "idle";
}

function memberTone(status?: string | null): StatusTone {
  if (status === "running") return "running";
  if (status === "completed") return "good";
  if (["blocked", "failed"].includes(status ?? "")) return "bad";
  if (["waiting", "reviewing"].includes(status ?? "")) return "warn";
  return "idle";
}

function timestamp(value?: string | null): number {
  if (!value) return 0;
  if (value.startsWith("unix-ms:")) return Number(value.slice(8)) || 0;
  return Date.parse(value) || 0;
}

function formatRelative(value?: string | null): string {
  const time = timestamp(value);
  if (!time) return "No activity";
  const minutes = Math.max(0, Math.floor((Date.now() - time) / 60_000));
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}
