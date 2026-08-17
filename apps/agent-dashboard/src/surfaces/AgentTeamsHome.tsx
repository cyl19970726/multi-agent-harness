import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowRight, Play, Plus, Server, Users } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import {
  DocumentSurface,
  EmptyState,
  StatusDot,
  type StatusTone,
} from "@/components/workbench/atoms";
import {
  Dialog,
  DialogFooter,
  Field,
  Select,
  TextArea,
  TextInput,
} from "@/components/workbench/OperatorForms";
import { cn } from "@/lib/utils";
import { TEAM_MEMBER_PROVIDER_MODES } from "@/lib/provider";

import { createTeam, createTeamRun, startTeamRun, type TeamRunMemberSpec } from "../api/actions";
import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";
import type { AgentTeam, MemberRun, Mission, ProviderLaunchProfile, TeamRun } from "../types";

interface AgentTeamsHomeProps {
  model: WorkbenchModel;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  loading?: boolean;
  onAction?: (path: string, body?: unknown) => Promise<boolean>;
}

interface NativeAttempt {
  run: TeamRun;
  team?: AgentTeam;
  mission?: Mission;
  members: MemberRun[];
}

/** Native entry point for flat, Mission-owned Agent Teams and their attempts. */
export function AgentTeamsHome({ model, onSelectionChange, actionsEnabled = false, loading = false, onAction }: AgentTeamsHomeProps) {
  const snapshot = model.snapshot;
  const [teamOpen, setTeamOpen] = useState(false);
  const [runDialogTeam, setRunDialogTeam] = useState<AgentTeam | undefined>();
  const missions = new Map((snapshot.missions ?? []).map((mission) => [mission.id, mission]));
  const teams = new Map((snapshot.teams ?? []).map((team) => [team.id, team]));
  const membersByRun = groupBy(snapshot.member_runs ?? [], (member) => member.team_run_id);
  const attempts = (snapshot.team_runs ?? [])
    .flatMap((run): NativeAttempt[] => {
      const team = teams.get(run.agent_team_id);
      if (!team) return [];
      const mission = missions.get(team.mission_id);
      if (!mission) return [];
      return [{ run, team, mission, members: membersByRun.get(run.id) ?? [] }];
    })
    .sort((left, right) => timestamp(right.run.updated_at ?? right.run.created_at) - timestamp(left.run.updated_at ?? left.run.created_at));

  // A durable team with no runs yet never appears as an attempt card, so it
  // gets its own row — otherwise a freshly created team would be invisible
  // here and could never receive its first run from the console.
  const teamsWithRuns = new Set(attempts.map((attempt) => attempt.team?.id).filter(Boolean));
  const teamsWithoutRuns = [...teams.values()].filter((team) => !teamsWithRuns.has(team.id));

  // Attempts of the same team are numbered chronologically so repeated team
  // names on this page read as attempts, not duplicated teams.
  const attemptNumberByRun = new Map<string, number>();
  const attemptTotalByTeam = new Map<string, number>();
  {
    const runsByTeam = new Map<string, TeamRun[]>();
    for (const attempt of attempts) {
      const teamKey = attempt.team?.id;
      if (!teamKey) continue;
      runsByTeam.set(teamKey, [...(runsByTeam.get(teamKey) ?? []), attempt.run]);
    }
    for (const [teamKey, runs] of runsByTeam) {
      attemptTotalByTeam.set(teamKey, runs.length);
      runs.slice().reverse().forEach((run, index) => attemptNumberByRun.set(run.id, index + 1));
    }
  }

  // The per-team "New run" affordance lives on that team's latest run card only,
  // so repeated attempts never duplicate the control.
  const latestRunIdByTeam = new Map<string, string>();
  for (const attempt of attempts) {
    const teamKey = attempt.team?.id;
    if (teamKey && !latestRunIdByTeam.has(teamKey)) latestRunIdByTeam.set(teamKey, attempt.run.id);
  }
  const durableMembers = snapshot.members ?? [];
  const executionNodes = snapshot.execution_nodes ?? [];
  const nodeRegistrations = snapshot.node_project_registrations ?? [];
  const nodeDaemonLeases = snapshot.node_daemon_leases ?? [];

  return (
    <DocumentSurface className="max-w-[1120px]">
      <header className="flex flex-wrap items-end justify-between gap-5 border-b border-border/70 pb-5">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">
            Native execution
          </p>
          <h1 className="mt-1 text-2xl font-semibold tracking-tight text-foreground">Agent Teams</h1>
          <p className="mt-1 max-w-2xl text-sm leading-6 text-muted-foreground">
            Flat Mission-owned teams and their runtime attempts. Open a run to
            inspect members, assignments, native sessions, pressure, and controls.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant="secondary"
            disabled={!actionsEnabled}
            title={actionsEnabled ? undefined : "Connect a live source to enable actions"}
            onClick={() => setTeamOpen(true)}
          >
            <Plus className="size-3.5" /> New Agent Team
          </Button>
        </div>
      </header>

      <section className="pt-5" aria-label="Execution Node operator view">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h2 className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">Execution Nodes</h2>
            <p className="mt-1 text-xs text-muted-foreground">Stable placement, registered Execution Spaces, and current NodeDaemon generation.</p>
          </div>
          <Badge tone={executionNodes.length ? "good" : "warn"}>{executionNodes.length} nodes</Badge>
        </div>
        <div className="mt-2 grid gap-2 lg:grid-cols-2">
          {executionNodes.map((node) => {
            const registrations = nodeRegistrations.filter((registration) => registration.node_id === node.id && registration.status === "active");
            const lease = nodeDaemonLeases.find((candidate) => candidate.node_id === node.id);
            const daemonCurrent = lease?.status === "active";
            return (
              <div key={node.id} className="flex min-w-0 items-center gap-3 rounded-xl border border-border/80 bg-card/65 px-4 py-3">
                <span className="grid size-9 shrink-0 place-items-center rounded-lg border border-primary/15 bg-primary/[0.055] text-primary"><Server className="size-4" /></span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-2"><span className="truncate text-sm font-semibold text-foreground">{node.display_name}</span><Badge tone={node.status === "active" ? "good" : "warn"}>{node.status}</Badge></span>
                  <span className="mt-0.5 block truncate font-mono text-[10px] text-muted-foreground">{node.id}</span>
                  <span className="mt-1 block text-[11px] text-muted-foreground">{registrations.length} registered spaces · {daemonCurrent ? `daemon generation ${lease?.generation}` : "daemon unavailable"}</span>
                </span>
                <StatusDot tone={daemonCurrent ? "good" : "warn"} pulse={daemonCurrent} />
              </div>
            );
          })}
          {executionNodes.length === 0 && <p role={loading ? "status" : undefined} className="rounded-xl border border-dashed border-border px-4 py-3 text-xs text-muted-foreground">{loading ? "Loading Execution Nodes…" : "No ExecutionNode has been initialized."}</p>}
        </div>
      </section>

      {attempts.length === 0 && teamsWithoutRuns.length === 0 && loading ? (
        <p role="status" className="mt-6 rounded-xl border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">Loading Agent Teams…</p>
      ) : attempts.length === 0 ? (
        <div className="pt-6">
          <EmptyState
            icon={Users}
            title="No Agent Team runs"
            description="Create one flat Agent Team for a Mission, then create its first TeamRun."
          />
        </div>
      ) : (
        <section className="pt-5" aria-label="Agent Team attempts">
          <div className="grid gap-3 lg:grid-cols-2">
            {attempts.map(({ run, team, mission, members }) => {
              const tone = runTone(run.status);
              const pressure = members.filter((member) => ["blocked", "failed", "waiting", "reviewing", "disconnected"].includes(member.status ?? ""));
              const attemptTotal = team ? attemptTotalByTeam.get(team.id) : undefined;
              const attemptNumber = attemptNumberByRun.get(run.id);
              const openRun = () => onSelectionChange({ surface: "team", teamId: run.id, memberRunId: undefined });
              const showNewRun = Boolean(team) && latestRunIdByTeam.get(team?.id ?? "") === run.id;
              return (
                // A div-with-role instead of <button> so the per-team "New run"
                // affordance can live on the card without nesting interactive
                // elements.
                <div
                  key={run.id}
                  role="button"
                  tabIndex={0}
                  onClick={openRun}
                  onKeyDown={(event) => {
                    if (event.target !== event.currentTarget) return;
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      openRun();
                    }
                  }}
                  className={cn(
                    "group min-w-0 cursor-pointer rounded-xl border border-border/80 bg-card/65 p-4 text-left transition-all",
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
                        <span className="truncate text-sm font-semibold text-foreground">{team?.name ?? run.objective}</span>
                        <Badge tone={tone}>{run.status ?? "unknown"}</Badge>
                        {(attemptTotal ?? 0) > 1 && attemptNumber && <Badge tone="muted">Attempt {attemptNumber} of {attemptTotal}</Badge>}
                      </span>
                      <span className="mt-1 block truncate text-xs text-muted-foreground">
                        {mission
                          ? `${mission.title} · Mission-scoped`
                          : "Mission relation unavailable"}
                      </span>
                      <span className="mt-1 block truncate text-[11px] text-muted-foreground">
                        Host Agent · {teamLeadLabel(team?.host_agent_id)}
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
                    <span className="flex min-w-0 items-center gap-2 text-[11px]">
                      {pressure.length > 0 && <span className="shrink-0 font-medium text-status-warn">{pressure.length} need attention</span>}
                      <span className="truncate text-muted-foreground">{formatRelative(run.updated_at ?? run.created_at)}</span>
                      {showNewRun && (
                        <button
                          type="button"
                          disabled={!actionsEnabled}
                          title={actionsEnabled ? "Create another run of this team" : "Connect a live source to enable actions"}
                          onClick={(event) => {
                            event.stopPropagation();
                            if (team) setRunDialogTeam(team);
                          }}
                          className="inline-flex shrink-0 items-center gap-1 rounded-md border border-border bg-background px-2 py-1 font-medium text-foreground transition-colors hover:border-primary/30 hover:bg-primary/[0.035] disabled:cursor-default disabled:text-muted-foreground"
                        >
                          <Plus className="size-3" /> New run
                        </button>
                      )}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {teamsWithoutRuns.length > 0 && (
        <section className="pt-5" aria-label="Teams without runs">
          <h2 className="text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">
            Teams without a run yet
          </h2>
          <div className="mt-2 divide-y divide-border/60 overflow-hidden rounded-xl border border-border/80 bg-card/65">
            {teamsWithoutRuns.map((team) => (
              <div key={team.id} className="flex min-w-0 items-center gap-3 px-4 py-3">
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-semibold text-foreground">{team.name}</span>
                  <span className="mt-0.5 block truncate text-xs text-muted-foreground">{team.description || "No description"}</span>
                </span>
                <button
                  type="button"
                  disabled={!actionsEnabled}
                  title={actionsEnabled ? "Create the first run of this team" : "Connect a live source to enable actions"}
                  onClick={() => setRunDialogTeam(team)}
                  className="inline-flex shrink-0 items-center gap-1 rounded-md border border-border bg-background px-2 py-1 font-medium text-foreground transition-colors hover:border-primary/30 hover:bg-primary/[0.035] disabled:cursor-default disabled:text-muted-foreground"
                >
                  <Plus className="size-3" /> New run
                </button>
              </div>
            ))}
          </div>
        </section>
      )}

      <TeamDialog
        open={teamOpen}
        durableMembers={durableMembers}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setTeamOpen(false)}
      />
      {runDialogTeam && (
        <RunDialog
          team={runDialogTeam}
          model={model}
          actionsEnabled={actionsEnabled}
          onAction={onAction}
          onSelectionChange={onSelectionChange}
          onClose={() => setRunDialogTeam(undefined)}
        />
      )}
    </DocumentSurface>
  );
}

/** Create the flat AgentTeam bound one-to-one to its Mission (POST /v1/teams). */
function TeamDialog({
  open,
  durableMembers,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  durableMembers: ProviderLaunchProfile[];
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => Promise<boolean>;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [missionId, setMissionId] = useState("");
  const [nodeId, setNodeId] = useState("");
  const [leadAgentId, setLeadAgentId] = useState("host");
  const [memberIds, setMemberIds] = useState<string[]>([]);

  useEffect(() => {
    if (open) {
      setName("");
      setDescription("");
      setMissionId("");
      setNodeId("");
      setLeadAgentId("host");
      setMemberIds([]);
    }
  }, [open]);

  const valid = Boolean(name.trim() && description.trim() && missionId.trim() && nodeId.trim() && leadAgentId);
  const submit = () => {
    if (!valid) return;
    const descriptor = createTeam({
      name: name.trim(),
      description: description.trim(),
      missionId: missionId.trim(),
      nodeId: nodeId.trim(),
      hostAgentId: leadAgentId,
      memberIds,
    });
    void onAction?.(descriptor.path, descriptor.body);
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="New Agent Team"
      description="Create the one flat AgentTeam for a Mission on one Node. Runs are created from it separately."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Team name" required>
          {(id) => <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} />}
        </Field>
        <Field label="Description" required hint="Purpose of the team, shown on its cards and runs.">
          {(id) => <TextArea id={id} value={description} onChange={(event) => setDescription(event.target.value)} />}
        </Field>
        <Field label="Mission ID" required hint="One Team equals one Mission; this relation is immutable.">
          {(id) => <TextInput id={id} value={missionId} onChange={(event) => setMissionId(event.target.value)} />}
        </Field>
        <Field label="Node ID" required hint="Stable UUID of the machine that owns every Member in this Team.">
          {(id) => <TextInput id={id} value={nodeId} onChange={(event) => setNodeId(event.target.value)} />}
        </Field>
        <Field label="Team Lead" required hint="The Host leads by default; a durable member may lead instead.">
          {(id) => (
            <Select id={id} value={leadAgentId} onChange={(event) => setLeadAgentId(event.target.value)}>
              <option value="host">Current Host Agent</option>
              {durableMembers.map((member) => (
                <option key={member.id} value={member.id}>{member.name ?? member.id}</option>
              ))}
            </Select>
          )}
        </Field>
        <Field label="Members" hint="Durable Agent Members belonging to this team definition.">
          {() =>
            durableMembers.length === 0 ? (
              <p className="text-[11px] text-muted-foreground">No durable Agent Members exist yet; create them in the Agents directory first.</p>
            ) : (
              <div className="max-h-40 space-y-1 overflow-y-auto rounded-md border border-border bg-background/60 p-2">
                {durableMembers.map((member) => {
                  const checked = memberIds.includes(member.id);
                  return (
                    <label key={member.id} className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-[12px] text-foreground hover:bg-accent/50">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={(event) =>
                          setMemberIds((current) =>
                            event.target.checked
                              ? [...current, member.id]
                              : current.filter((id) => id !== member.id),
                          )
                        }
                        className="size-3.5 accent-primary"
                      />
                      <span className="min-w-0 flex-1 truncate">{member.name ?? member.id}</span>
                      <span className="truncate text-[10px] text-muted-foreground">{member.role ?? "member"}</span>
                    </label>
                  );
                })}
              </div>
            )
          }
        </Field>
        <p className="rounded-md border border-border bg-muted/35 px-3 py-2 text-[11px] text-muted-foreground">
          Creating a team does not start any runtime; members run when a team run starts.
        </p>
        <DialogFooter
          submitLabel="Create team"
          actionsEnabled={actionsEnabled}
          canSubmit={valid}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

/**
 * Create one run of an existing team (POST /v1/team-runs with agent_team_id).
 *
 * Start-now wiring: `runAction` in App.tsx resolves to a boolean only — it does
 * not return the created row. After a successful create, this dialog therefore
 * discovers the new run from the refreshed snapshot prop (runs absent at dialog
 * open) and offers Start once it can name the run id. This keeps the create →
 * start gap closed without changing the shared onAction contract.
 */
function RunDialog({
  team,
  model,
  actionsEnabled,
  onAction,
  onSelectionChange,
  onClose,
}: {
  team: AgentTeam;
  model: WorkbenchModel;
  actionsEnabled: boolean;
  onAction?: (path: string, body?: unknown) => Promise<boolean>;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  onClose: () => void;
}) {
  const durableMembers = model.snapshot.members ?? [];
  const resolvedMembers = (team.member_ids ?? [])
    .map((id) => durableMembers.find((member) => member.id === id))
    .filter((member): member is ProviderLaunchProfile => Boolean(member));
  // Runs that already existed when the dialog opened; anything newer with this
  // team id is the run this dialog created.
  const priorRunIds = useRef(
    new Set(
      (model.snapshot.team_runs ?? [])
        .filter((run) => run.agent_team_id === team.id)
        .map((run) => run.id),
    ),
  );
  const [objective, setObjective] = useState("");
  const [executionRoot, setExecutionRoot] = useState("");
  const [budget, setBudget] = useState("");
  const [providerOverrides, setProviderOverrides] = useState<Record<string, string>>({});
  const [phase, setPhase] = useState<"form" | "created" | "started">("form");
  const [submitting, setSubmitting] = useState(false);

  const createdRun = useMemo(() => {
    if (phase === "form") return undefined;
    const candidates = (model.snapshot.team_runs ?? [])
      .filter((run) => run.agent_team_id === team.id && !priorRunIds.current.has(run.id));
    candidates.sort((left, right) => (right.created_at ?? "").localeCompare(left.created_at ?? ""));
    return candidates[0];
  }, [phase, model, team.id]);

  const providerEntryFor = (provider?: string | null) =>
    TEAM_MEMBER_PROVIDER_MODES.find((entry) => entry.provider === provider);
  const effectiveProvider = (member: ProviderLaunchProfile): string =>
    providerOverrides[member.id] ?? member.provider ?? "";
  // The roster derives from the team definition unless the operator overrides a
  // provider mode; then the run carries explicit member specs instead.
  const rosterEdited = resolvedMembers.some(
    (member) => effectiveProvider(member) !== (member.provider ?? ""),
  );

  const valid = Boolean(objective.trim());
  const submit = async () => {
    if (!valid || submitting) return;
    setSubmitting(true);
    const members: TeamRunMemberSpec[] = rosterEdited
      ? resolvedMembers.map((member) => {
          const entry = providerEntryFor(effectiveProvider(member));
          return {
            name: member.name ?? member.id,
            role: member.role ?? "member",
            provider: entry?.provider ?? member.provider ?? "codex",
            executionMode: (entry?.mode ?? "codex_app_server") as TeamRunMemberSpec["executionMode"],
            model: member.model ?? undefined,
          };
        })
      : [];
    const budgetValue = Number(budget);
    const descriptor = createTeamRun({
      objective: objective.trim(),
      agentTeamId: team.id,
      executionRoot: executionRoot.trim() || undefined,
      budgetLimitUsd: Number.isFinite(budgetValue) && budgetValue > 0 ? budgetValue : undefined,
      members,
    });
    const ok = await onAction?.(descriptor.path, descriptor.body);
    setSubmitting(false);
    if (ok) setPhase("created");
  };

  const startNow = async () => {
    if (!createdRun || submitting) return;
    setSubmitting(true);
    const descriptor = startTeamRun(createdRun.id);
    const ok = await onAction?.(descriptor.path, descriptor.body);
    setSubmitting(false);
    if (ok) setPhase("started");
  };

  return (
    <Dialog
      open
      title={`New run · ${team.name ?? team.id}`}
      description="Create one TeamRun of this team. The roster derives from the team definition unless you override a provider mode."
      onClose={onClose}
    >
      {phase === "form" ? (
        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          <Field label="Objective" required hint="What this attempt must accomplish; it can differ from the team's standing purpose.">
            {(id) => <TextArea id={id} value={objective} onChange={(event) => setObjective(event.target.value)} />}
          </Field>
          <Field label="Members" hint="Provider modes derive from the team definition; override only when this attempt needs a different mode.">
            {() =>
              resolvedMembers.length === 0 ? (
                <p className="text-[11px] text-muted-foreground">
                  This team definition has no durable members; the run starts empty and members can be added from its War Room.
                </p>
              ) : (
                <div className="space-y-1.5 rounded-md border border-border bg-background/60 p-2">
                  {resolvedMembers.map((member) => (
                    <div key={member.id} className="flex min-w-0 items-center gap-2">
                      <span className="min-w-0 flex-1 truncate text-[12px] text-foreground">
                        {member.name ?? member.id}
                        <span className="ml-1.5 text-[10px] text-muted-foreground">{member.role ?? "member"}</span>
                      </span>
                      <Select
                        aria-label={`Provider mode for ${member.name ?? member.id}`}
                        value={effectiveProvider(member)}
                        onChange={(event) =>
                          setProviderOverrides((current) => ({ ...current, [member.id]: event.target.value }))
                        }
                        className="h-8 w-44 shrink-0 text-[11px]"
                      >
                        {TEAM_MEMBER_PROVIDER_MODES.map((entry) => (
                          <option key={entry.provider} value={entry.provider}>
                            {entry.label} · {entry.mode}
                          </option>
                        ))}
                        {!providerEntryFor(member.provider) && member.provider && (
                          <option value={member.provider}>{member.provider} (unregistered)</option>
                        )}
                      </Select>
                    </div>
                  ))}
                </div>
              )
            }
          </Field>
          <Field label="Execution root" hint="Optional workspace path; defaults to the selected project binding.">
            {(id) => <TextInput id={id} value={executionRoot} onChange={(event) => setExecutionRoot(event.target.value)} />}
          </Field>
          <Field label="Budget (USD)" hint="Optional per-run budget limit.">
            {(id) => (
              <TextInput
                id={id}
                type="number"
                min="0"
                step="0.01"
                value={budget}
                onChange={(event) => setBudget(event.target.value)}
                placeholder="No limit"
              />
            )}
          </Field>
          <DialogFooter
            submitLabel={submitting ? "Creating…" : "Create run"}
            actionsEnabled={actionsEnabled}
            canSubmit={valid && !submitting}
            onCancel={onClose}
            onSubmit={() => void submit()}
          />
        </form>
      ) : (
        <div className="space-y-3">
          <p className="rounded-md border border-border bg-muted/35 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
            {createdRun
              ? phase === "started"
                ? "Start dispatched. The run executes in the background; watch it from its War Room."
                : "Run created — it is not running yet. Start it now or open its War Room."
              : "Run created — waiting for the refreshed snapshot to show it. If it does not appear, open the team's latest War Room and start it there."}
          </p>
          <div className="flex flex-wrap justify-end gap-2">
            <Button variant="secondary" size="sm" type="button" onClick={onClose}>Close</Button>
            {createdRun && (
              <Button
                variant="secondary"
                size="sm"
                type="button"
                onClick={() => {
                  onSelectionChange({ surface: "team", teamId: createdRun.id, memberRunId: undefined });
                  onClose();
                }}
              >
                Open War Room
              </Button>
            )}
            {phase === "created" && (
              <Button
                size="sm"
                type="button"
                disabled={!createdRun || submitting || !actionsEnabled}
                title={!createdRun
                  ? "Waiting for the new run to appear in the snapshot"
                  : !actionsEnabled
                    ? "Connect a live source to enable actions"
                    : undefined}
                onClick={() => void startNow()}
              >
                <Play className="size-3.5" /> {submitting ? "Starting…" : "Start now"}
              </Button>
            )}
          </div>
        </div>
      )}
    </Dialog>
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
  if (["waiting", "reviewing", "disconnected"].includes(status ?? "")) return "warn";
  if (status === "planning") return "info";
  return "idle";
}

function memberTone(status?: string | null): StatusTone {
  if (status === "running") return "running";
  if (status === "completed") return "good";
  if (["blocked", "failed"].includes(status ?? "")) return "bad";
  if (["waiting", "reviewing", "disconnected"].includes(status ?? "")) return "warn";
  return "idle";
}

function timestamp(value?: string | null): number {
  if (!value) return 0;
  if (value.startsWith("unix-ms:")) return Number(value.slice(8)) || 0;
  return Date.parse(value) || 0;
}

function teamLeadLabel(leadAgentId?: string | null): string {
  if (!leadAgentId || leadAgentId === "host") return "Current Host Agent";
  return leadAgentId;
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
