import { useEffect, useState, type ComponentProps } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Flag,
  Plus,
  Rocket,
  ShieldCheck,
  Waves,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DocProperties,
  DocSection,
  DocumentSurface,
  EmptyState,
  MonoId,
  StatusDot,
  type StatusTone,
} from "@/components/workbench/atoms";
import {
  Dialog,
  DialogFooter,
  Field,
  parseList,
  Select,
  TextArea,
  TextInput,
} from "@/components/workbench/OperatorForms";

import {
  createMission,
  createTeamRun,
  createWave,
  gateWave,
  type ActionDescriptor,
} from "../api/actions";
import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";
import type { Mission, TeamRun, Wave } from "../types";

interface MissionsProps {
  model: WorkbenchModel;
  missionId?: string;
  waveId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
}

interface MemberDraft {
  name: string;
  role: string;
  provider: "kimi";
  model: string;
  ownedPaths: string;
}

function dispatch(onAction: MissionsProps["onAction"], descriptor: ActionDescriptor): void {
  onAction?.(descriptor.path, descriptor.body);
}

function missionTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running":
      return "running";
    case "completed":
      return "good";
    case "blocked":
      return "bad";
    case "planned":
      return "info";
    default:
      return "idle";
  }
}

function waveTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running":
      return "running";
    case "completed":
      return "good";
    case "blocked":
    case "failed":
      return "bad";
    case "waiting":
      return "warn";
    case "planned":
      return "info";
    default:
      return "idle";
  }
}

function gateTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "accepted":
      return "good";
    case "blocked":
      return "bad";
    case "revise":
      return "warn";
    default:
      return "idle";
  }
}

function fmt(value?: string | null): string {
  if (!value) return "—";
  const epoch = value.startsWith("unix-ms:") ? Number(value.slice(8)) : Date.parse(value);
  return Number.isFinite(epoch) ? new Date(epoch).toLocaleString() : value;
}

function wavesFor(model: WorkbenchModel, missionId: string): Wave[] {
  return [...(model.snapshot.waves ?? [])]
    .filter((wave) => wave.mission_id === missionId)
    .sort((a, b) => a.index - b.index);
}

function runsForWave(model: WorkbenchModel, wave: Wave): TeamRun[] {
  return [...(model.snapshot.team_runs ?? [])]
    .filter((run) => run.mission_id === wave.mission_id && run.wave_id === wave.id)
    .sort((a, b) => (a.created_at ?? "").localeCompare(b.created_at ?? ""));
}

function blankMember(): MemberDraft {
  return { name: "", role: "", provider: "kimi", model: "", ownedPaths: "" };
}

export function MissionsSurface({
  model,
  missionId,
  waveId,
  onSelectionChange,
  actionsEnabled = false,
  onAction,
}: MissionsProps) {
  const [createOpen, setCreateOpen] = useState(false);
  const missions = [...(model.snapshot.missions ?? [])]
    .filter((mission) => !mission.id.startsWith("compat-goal:"))
    .sort((a, b) =>
      (b.updated_at ?? b.created_at ?? "").localeCompare(a.updated_at ?? a.created_at ?? ""),
    );
  const selected = missions.find((mission) => mission.id === missionId);

  if (selected) {
    return (
      <MissionDetail
        model={model}
        mission={selected}
        selectedWaveId={waveId}
        onSelectionChange={onSelectionChange}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
      />
    );
  }

  return (
    <DocumentSurface className="max-w-[1180px]">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Flag className="size-3.5" /> Native control plane
          </div>
          <h1 className="text-2xl font-semibold tracking-tight">Missions</h1>
          <p className="text-sm text-muted-foreground">
            Durable intent, ordered Waves, and their executor attempts. Team Runs belong
            to a Wave; they are not the plan.
          </p>
        </div>
        <ActionButton enabled={actionsEnabled} onClick={() => setCreateOpen(true)}>
          <Plus className="size-3.5" /> New Mission
        </ActionButton>
      </header>

      <DocSection label={`${missions.length} ${missions.length === 1 ? "mission" : "missions"}`}>
        {missions.length === 0 ? (
          <EmptyState
            icon={Flag}
            title="No native Missions yet"
            description="Create a Mission, then add the small ordered Waves needed to reach its outcome."
          />
        ) : (
          <div className="overflow-hidden rounded-lg border border-border bg-card">
            {missions.map((mission) => {
              const waves = wavesFor(model, mission.id);
              return (
                <button
                  key={mission.id}
                  type="button"
                  onClick={() =>
                    onSelectionChange({
                      surface: "missions",
                      missionId: mission.id,
                      waveId: undefined,
                    })
                  }
                  className="flex w-full items-center gap-3 border-b border-border/60 px-4 py-3 text-left last:border-b-0 hover:bg-accent/40"
                >
                  <StatusDot tone={missionTone(mission.status)} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[14px] font-medium">{mission.title}</span>
                    <span className="block truncate text-[12px] text-muted-foreground">
                      {mission.objective}
                    </span>
                  </span>
                  <span className="hidden items-center gap-1.5 sm:flex">
                    <Badge tone="muted">{waves.length} waves</Badge>
                    <Badge tone={missionTone(mission.status)}>{mission.status ?? "planned"}</Badge>
                  </span>
                  <ChevronRight className="size-4 text-muted-foreground" />
                </button>
              );
            })}
          </div>
        )}
      </DocSection>

      <MissionDialog
        open={createOpen}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setCreateOpen(false)}
      />
    </DocumentSurface>
  );
}

function MissionDetail({
  model,
  mission,
  selectedWaveId,
  onSelectionChange,
  actionsEnabled = false,
  onAction,
}: MissionsProps & { mission: Mission; selectedWaveId?: string }) {
  const [waveOpen, setWaveOpen] = useState(false);
  const waves = wavesFor(model, mission.id);
  const selectedWave = waves.find((wave) => wave.id === selectedWaveId);

  return (
    <DocumentSurface className="max-w-[1180px]">
      <button
        type="button"
        onClick={() =>
          onSelectionChange({ surface: "missions", missionId: undefined, waveId: undefined })
        }
        className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground"
      >
        <ChevronLeft className="size-3.5" /> Missions
      </button>

      <header className="mt-4 flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1 space-y-1.5">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Flag className="size-3.5" /> Mission
          </div>
          <h1 className="text-xl font-semibold tracking-tight">{mission.title}</h1>
          <p className="max-w-3xl text-sm text-muted-foreground">{mission.objective}</p>
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge tone={missionTone(mission.status)}>{mission.status ?? "planned"}</Badge>
            <MonoId>{mission.id}</MonoId>
          </div>
        </div>
        <ActionButton enabled={actionsEnabled} onClick={() => setWaveOpen(true)}>
          <Plus className="size-3.5" /> Add Wave
        </ActionButton>
      </header>

      <DocProperties
        items={[
          { label: "Desired outcome", value: mission.desired_outcome || "—" },
          { label: "Outcome", value: mission.outcome_summary || "—" },
          { label: "Updated", value: fmt(mission.updated_at ?? mission.created_at) },
        ]}
      />

      <DocSection label="Ordered Waves">
        {waves.length === 0 ? (
          <EmptyState
            icon={Waves}
            title="No Waves yet"
            description="Add a small ordered unit, select its executor, and then create the needed attempt."
          />
        ) : (
          <div className="space-y-2">
            {waves.map((wave) => (
              <WaveCard
                key={wave.id}
                wave={wave}
                runs={runsForWave(model, wave)}
                expanded={selectedWave?.id === wave.id}
                onSelect={() =>
                  onSelectionChange({ surface: "missions", missionId: mission.id, waveId: wave.id })
                }
                onSelectionChange={onSelectionChange}
                actionsEnabled={actionsEnabled}
                onAction={onAction}
              />
            ))}
          </div>
        )}
      </DocSection>

      <WaveDialog
        open={waveOpen}
        mission={mission}
        nextIndex={(waves[waves.length - 1]?.index ?? 0) + 1}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setWaveOpen(false)}
      />
    </DocumentSurface>
  );
}

interface WaveCardProps {
  wave: Wave;
  runs: TeamRun[];
  expanded: boolean;
  onSelect: () => void;
  onSelectionChange: MissionsProps["onSelectionChange"];
  actionsEnabled: boolean;
  onAction: MissionsProps["onAction"];
}

function WaveCard({
  wave,
  runs,
  expanded,
  onSelect,
  onSelectionChange,
  actionsEnabled,
  onAction,
}: WaveCardProps) {
  const [attemptOpen, setAttemptOpen] = useState(false);
  const [gateOpen, setGateOpen] = useState(false);
  const latest = runs[runs.length - 1];
  const canTeamRun = wave.executor_kind === "agent_team";
  const hasActiveAttempt = runs.some((run) =>
    ["planning", "running", "waiting", "reviewing"].includes(run.status ?? ""),
  );
  const waveAccepted = wave.gate_status === "accepted" || wave.status === "completed";

  return (
    <section className="overflow-hidden rounded-lg border border-border bg-card">
      <button
        type="button"
        onClick={onSelect}
        className="flex w-full items-center gap-3 px-3.5 py-3 text-left hover:bg-accent/30"
      >
        <StatusDot tone={waveTone(wave.status)} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] font-medium">
            <span className="mr-2 text-muted-foreground">{wave.index}.</span>
            {wave.title}
          </span>
          <span className="block truncate text-[12px] text-muted-foreground">{wave.objective}</span>
        </span>
        <Badge tone="muted">{wave.executor_kind}</Badge>
        <Badge tone={gateTone(wave.gate_status)}>{wave.gate_status ?? "pending"}</Badge>
        <Badge tone="muted">{runs.length} attempts</Badge>
        <ChevronRight
          className={
            expanded ? "size-4 rotate-90 text-muted-foreground" : "size-4 text-muted-foreground"
          }
        />
      </button>

      {expanded && (
        <div className="space-y-3 border-t border-border/60 px-3.5 py-3">
          <div className="grid gap-2 text-[12px] text-muted-foreground sm:grid-cols-2">
            <p>
              <span className="font-medium text-foreground">Exit:</span>{" "}
              {wave.exit_criteria || "Not specified"}
            </p>
            <p>
              <span className="font-medium text-foreground">Outcome:</span>{" "}
              {wave.outcome_summary || "Pending"}
            </p>
          </div>

          <div className="flex flex-wrap gap-2">
            {canTeamRun && (
              <ActionButton
                enabled={actionsEnabled}
                disabled={hasActiveAttempt || waveAccepted}
                size="sm"
                onClick={() => setAttemptOpen(true)}
              >
                <Rocket className="size-3.5" />
                {latest ? "Retry / new attempt" : "Create Agent Team"}
              </ActionButton>
            )}
            {canTeamRun && (
              <ActionButton
                enabled={actionsEnabled}
                disabled={hasActiveAttempt || waveAccepted}
                size="sm"
                variant="secondary"
                onClick={() => setGateOpen(true)}
              >
                <ShieldCheck className="size-3.5" /> Gate Wave
              </ActionButton>
            )}
            {hasActiveAttempt && (
              <span className="self-center text-[11px] text-muted-foreground">
                Settle the active attempt before retry or gate.
              </span>
            )}
          </div>

          {runs.length === 0 ? (
            <p className="text-[12px] text-muted-foreground">
              {canTeamRun
                ? "No Agent Team attempt yet."
                : "This executor is a read-only architecture seam in the current Console."}
            </p>
          ) : (
            <div className="overflow-hidden rounded-md border border-border bg-background/30">
              {runs.map((run, index) => (
                <button
                  key={run.id}
                  type="button"
                  onClick={() =>
                    onSelectionChange({
                      surface: "team",
                      teamId: run.id,
                      missionId: wave.mission_id,
                      waveId: wave.id,
                    })
                  }
                  className="flex w-full items-center gap-2 border-b border-border/60 px-3 py-2 text-left text-[12px] last:border-b-0 hover:bg-accent/40"
                >
                  <StatusDot tone={waveTone(run.status)} />
                  <span className="font-medium text-foreground">Attempt {index + 1}</span>
                  <Badge tone={waveTone(run.status)}>{run.status ?? "planning"}</Badge>
                  <MonoId>{run.id}</MonoId>
                  {run.id === wave.accepted_run_id && <Badge tone="good">accepted</Badge>}
                  <span className="ml-auto text-muted-foreground">{fmt(run.created_at)}</span>
                </button>
              ))}
            </div>
          )}

          <AttemptDialog
            open={attemptOpen}
            wave={wave}
            latestRun={latest}
            actionsEnabled={actionsEnabled}
            onAction={onAction}
            onClose={() => setAttemptOpen(false)}
          />
          {canTeamRun && (
            <GateDialog
              open={gateOpen}
              wave={wave}
              runs={runs}
              actionsEnabled={actionsEnabled}
              onAction={onAction}
              onClose={() => setGateOpen(false)}
            />
          )}
        </div>
      )}
    </section>
  );
}

function MissionDialog({
  open,
  actionsEnabled,
  onAction,
  onClose,
}: Pick<MissionsProps, "actionsEnabled" | "onAction"> & { open: boolean; onClose: () => void }) {
  const [title, setTitle] = useState("");
  const [objective, setObjective] = useState("");
  const [outcome, setOutcome] = useState("");

  useEffect(() => {
    if (open) {
      setTitle("");
      setObjective("");
      setOutcome("");
    }
  }, [open]);

  const submit = () => {
    if (!title.trim() || !objective.trim()) return;
    dispatch(
      onAction,
      createMission({
        title: title.trim(),
        objective: objective.trim(),
        desiredOutcome: outcome.trim() || undefined,
      }),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="New Mission"
      description="Create durable intent before deciding its ordered executor Waves."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Title" required>
          {(id) => <TextInput id={id} value={title} onChange={(event) => setTitle(event.target.value)} />}
        </Field>
        <Field label="Objective" required>
          {(id) => <TextArea id={id} value={objective} onChange={(event) => setObjective(event.target.value)} />}
        </Field>
        <Field label="Desired outcome" hint="Optional success description.">
          {(id) => <TextArea id={id} value={outcome} onChange={(event) => setOutcome(event.target.value)} />}
        </Field>
        <DialogFooter
          submitLabel="Create Mission"
          actionsEnabled={Boolean(actionsEnabled)}
          canSubmit={Boolean(title.trim() && objective.trim())}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

function WaveDialog({
  open,
  mission,
  nextIndex,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  mission: Mission;
  nextIndex: number;
  actionsEnabled: boolean;
  onAction: MissionsProps["onAction"];
  onClose: () => void;
}) {
  const [title, setTitle] = useState("");
  const [objective, setObjective] = useState("");
  const [executor, setExecutor] = useState<"agent_team" | "dynamic_workflow" | "host">("agent_team");
  const [exit, setExit] = useState("");

  useEffect(() => {
    if (open) {
      setTitle("");
      setObjective("");
      setExecutor("agent_team");
      setExit("");
    }
  }, [open]);

  const submit = () => {
    if (!title.trim() || !objective.trim()) return;
    dispatch(
      onAction,
      createWave({
        missionId: mission.id,
        index: nextIndex,
        title: title.trim(),
        objective: objective.trim(),
        executorKind: executor,
        exitCriteria: exit.trim() || undefined,
      }),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      title={`Add Wave ${nextIndex}`}
      description="A Wave is a small ordered unit. Its executor owns its internal plan."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Title" required>
          {(id) => <TextInput id={id} value={title} onChange={(event) => setTitle(event.target.value)} />}
        </Field>
        <Field label="Objective" required>
          {(id) => <TextArea id={id} value={objective} onChange={(event) => setObjective(event.target.value)} />}
        </Field>
        <Field
          label="Executor"
          required
          hint="Agent Team is executable here. Dynamic Workflow and Host are visible seams, not active Console controls yet."
        >
          {(id) => (
            <Select
              id={id}
              value={executor}
              onChange={(event) => setExecutor(event.target.value as typeof executor)}
            >
              <option value="agent_team">Agent Team</option>
              <option value="dynamic_workflow" disabled>Dynamic Workflow (coming later)</option>
              <option value="host" disabled>Host (coming later)</option>
            </Select>
          )}
        </Field>
        <Field label="Exit criteria">
          {(id) => <TextInput id={id} value={exit} onChange={(event) => setExit(event.target.value)} />}
        </Field>
        <DialogFooter
          submitLabel="Add Wave"
          actionsEnabled={actionsEnabled}
          canSubmit={Boolean(title.trim() && objective.trim())}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

function AttemptDialog({
  open,
  wave,
  latestRun,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  wave: Wave;
  latestRun?: TeamRun;
  actionsEnabled: boolean;
  onAction: MissionsProps["onAction"];
  onClose: () => void;
}) {
  const [objective, setObjective] = useState("");
  const [members, setMembers] = useState<MemberDraft[]>([blankMember()]);

  useEffect(() => {
    if (open) {
      setObjective(wave.objective);
      setMembers([blankMember()]);
    }
  }, [open, wave.objective]);

  const valid = Boolean(objective.trim()) && members.every((member) => member.name.trim() && member.role.trim());
  const updateMember = (index: number, patch: Partial<MemberDraft>) => {
    setMembers((current) =>
      current.map((member, memberIndex) => (memberIndex === index ? { ...member, ...patch } : member)),
    );
  };
  const submit = () => {
    if (!valid) return;
    dispatch(
      onAction,
      createTeamRun({
        objective: objective.trim(),
        missionId: wave.mission_id,
        waveId: wave.id,
        previousRunId: latestRun?.id,
        members: members.map((member) => ({
          name: member.name.trim(),
          role: member.role.trim(),
          provider: member.provider,
          model: member.model.trim() || undefined,
          ownedPaths: parseList(member.ownedPaths),
        })),
      }),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      title={latestRun ? "Create retry attempt" : "Create Agent Team attempt"}
      description={
        latestRun
          ? "This becomes the next attempt of the same Wave and preserves the prior attempt."
          : "This Agent Team Run is linked to this Mission and Wave."
      }
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Objective" required>
          {(id) => <TextArea id={id} value={objective} onChange={(event) => setObjective(event.target.value)} />}
        </Field>
        <div className="flex items-center justify-between">
          <p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Members</p>
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={() => setMembers((current) => [...current, blankMember()])}
          >
            <Plus className="size-3.5" /> Add member
          </Button>
        </div>

        {members.map((member, index) => (
          <div key={index} className="space-y-2 rounded-lg border border-border p-2.5">
            <div className="grid grid-cols-2 gap-2">
              <Field label="Name" required>
                {(id) => (
                  <TextInput
                    id={id}
                    value={member.name}
                    onChange={(event) => updateMember(index, { name: event.target.value })}
                  />
                )}
              </Field>
              <Field label="Role" required>
                {(id) => (
                  <TextInput
                    id={id}
                    value={member.role}
                    onChange={(event) => updateMember(index, { role: event.target.value })}
                  />
                )}
              </Field>
              <Field label="Provider" required hint="Kimi is currently the executable provider.">
                {(id) => (
                  <Select id={id} value={member.provider} onChange={() => undefined}>
                    <option value="kimi">Kimi</option>
                    <option disabled>Codex (coming later)</option>
                    <option disabled>Claude (coming later)</option>
                  </Select>
                )}
              </Field>
              <Field label="Model">
                {(id) => (
                  <TextInput
                    id={id}
                    value={member.model}
                    onChange={(event) => updateMember(index, { model: event.target.value })}
                  />
                )}
              </Field>
            </div>
            <Field label="Owned paths">
              {(id) => (
                <TextInput
                  id={id}
                  value={member.ownedPaths}
                  onChange={(event) => updateMember(index, { ownedPaths: event.target.value })}
                  placeholder="src/, docs/"
                />
              )}
            </Field>
            {members.length > 1 && (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => setMembers((current) => current.filter((_, memberIndex) => memberIndex !== index))}
              >
                Remove member
              </Button>
            )}
          </div>
        ))}

        <DialogFooter
          submitLabel={latestRun ? "Create retry" : "Create attempt"}
          actionsEnabled={actionsEnabled}
          canSubmit={valid}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

function GateDialog({
  open,
  wave,
  runs,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  wave: Wave;
  runs: TeamRun[];
  actionsEnabled: boolean;
  onAction: MissionsProps["onAction"];
  onClose: () => void;
}) {
  const [status, setStatus] = useState<"accepted" | "revise" | "blocked">("accepted");
  const [runId, setRunId] = useState("");
  const [outcome, setOutcome] = useState("");
  const [note, setNote] = useState("");
  const [artifacts, setArtifacts] = useState("");
  const completedRuns = runs.filter((run) => run.status === "completed");
  const latestCompletedRunId = completedRuns[completedRuns.length - 1]?.id ?? "";
  const artifactValues = (wave.artifact_refs ?? []).join(", ");

  useEffect(() => {
    if (open) {
      setStatus("accepted");
      setRunId(wave.accepted_run_id ?? latestCompletedRunId);
      setOutcome(wave.outcome_summary ?? "");
      setNote("");
      setArtifacts(artifactValues);
    }
  }, [artifactValues, latestCompletedRunId, open, wave.accepted_run_id, wave.outcome_summary]);

  const valid = status !== "accepted" || Boolean(runId && outcome.trim());
  const selectableRuns = status === "accepted" ? completedRuns : runs;
  const submit = () => {
    if (!valid) return;
    dispatch(
      onAction,
      gateWave({
        waveId: wave.id,
        status,
        runId: runId || undefined,
        acceptedBy: "host",
        outcome: outcome.trim() || undefined,
        note: note.trim() || undefined,
        artifactRefs: parseList(artifacts),
      }),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="Gate Wave"
      description="The Host records accepted, revise, or blocked without deleting any attempt."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Gate result" required>
          {(id) => (
            <Select
              id={id}
              value={status}
              onChange={(event) => setStatus(event.target.value as typeof status)}
            >
              <option value="accepted">Accepted</option>
              <option value="revise">Revise</option>
              <option value="blocked">Blocked</option>
            </Select>
          )}
        </Field>
        <Field label="Attempt" hint="Accepted must name a completed Agent Team attempt.">
          {(id) => (
            <Select id={id} value={runId} onChange={(event) => setRunId(event.target.value)}>
              <option value="">No attempt selected</option>
              {selectableRuns.map((run) => (
                <option key={run.id} value={run.id}>
                  {run.id} · {run.status ?? "planning"}
                </option>
              ))}
            </Select>
          )}
        </Field>
        <Field label="Outcome" required={status === "accepted"}>
          {(id) => <TextArea id={id} value={outcome} onChange={(event) => setOutcome(event.target.value)} />}
        </Field>
        <Field label="Gate note">
          {(id) => <TextArea id={id} value={note} onChange={(event) => setNote(event.target.value)} />}
        </Field>
        <Field label="Artifacts" hint="Comma-separated references.">
          {(id) => <TextInput id={id} value={artifacts} onChange={(event) => setArtifacts(event.target.value)} />}
        </Field>
        <DialogFooter
          submitLabel="Record gate"
          actionsEnabled={actionsEnabled}
          canSubmit={valid}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

function ActionButton({
  enabled,
  children,
  ...props
}: ComponentProps<typeof Button> & { enabled: boolean }) {
  return (
    <Button {...props} disabled={!enabled || props.disabled}>
      {children}
    </Button>
  );
}
