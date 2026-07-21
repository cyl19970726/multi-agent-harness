import { useEffect, useState, type ComponentProps } from "react";
import {
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  CircleDashed,
  FileCheck2,
  Flag,
  PanelsTopLeft,
  Plus,
  Rocket,
  ShieldCheck,
  Users,
  Waves,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import {
  DocProperties,
  DocSection,
  DocumentSurface,
  EmptyState,
  MonoId,
  StatusDot,
  type StatusTone,
} from "@/components/workbench/atoms";
import { ContextModule, ContextRail } from "@/components/workbench/context/ContextRail";
import { DecisionAnchor, LiveTrace, ReadinessMeter } from "@/components/workbench/execution/ExecutionPrimitives";
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
  closeMission,
  createMission,
  createTeamRun,
  createWave,
  gateWave,
  type ActionDescriptor,
} from "../api/actions";
import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";
import { selectMemberPressureMessage } from "../model/teamSelectors";
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

function exitCriteriaFor(wave: Wave): string[] {
  return (wave.exit_criteria ?? "")
    .split(";")
    .map((criterion) => criterion.trim())
    .filter(Boolean);
}

function reportedGateReadiness(wave: Wave, total: number): number | undefined {
  if (!total) return undefined;
  if (wave.gate_status === "accepted") return total;
  const note = wave.gate_note?.toLowerCase() ?? "";
  const numeric = note.match(/\b(\d+)\s+(?:of\s+\d+\s+)?criteria?\b/);
  if (numeric) return Math.min(total, Number(numeric[1]));
  const words: Record<string, number> = { zero: 0, one: 1, two: 2, three: 3, four: 4, five: 5 };
  const spelled = note.match(/\b(zero|one|two|three|four|five)\s+(?:of\s+\w+\s+)?criteria?\b/);
  return spelled ? Math.min(total, words[spelled[1]]) : undefined;
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
  const [closeOpen, setCloseOpen] = useState(false);
  const waves = wavesFor(model, mission.id);
  const readyToClose =
    waves.length > 0 &&
    waves.every((wave) => wave.status === "completed" && wave.gate_status === "accepted");
  // A Mission always has one useful focal point: keep an explicit selection when
  // there is one, otherwise favour the active Wave and then the next planned
  // decision. This is presentation state only; it does not mutate Wave order.
  const selectedWave =
    waves.find((wave) => wave.id === selectedWaveId) ??
    waves.find((wave) => ["running", "waiting", "blocked"].includes(wave.status ?? "")) ??
    waves.find((wave) => wave.status === "planned") ??
    waves[0];
  const selectedRuns = selectedWave ? runsForWave(model, selectedWave) : [];
  const latestSelectedRun = selectedRuns[selectedRuns.length - 1];
  const selectedMembers = (model.snapshot.member_runs ?? []).filter(
    (member) => member.team_run_id === latestSelectedRun?.id,
  );
  const selectedMessages = (model.snapshot.team_messages ?? []).filter(
    (message) => message.team_run_id === latestSelectedRun?.id,
  );
  const pendingMembers = selectedMembers.filter((member) =>
    ["waiting", "reviewing", "blocked"].includes(member.status ?? ""),
  );
  const blockedMember = selectedMembers.find((member) => member.status === "blocked");
  const pressureMessage = selectMemberPressureMessage(selectedMessages, blockedMember);
  const gateCriteria = selectedWave ? exitCriteriaFor(selectedWave) : [];
  const evidencedCriteria = selectedWave
    ? reportedGateReadiness(selectedWave, gateCriteria.length)
    : undefined;
  const gateNeedsReview =
    Boolean(selectedWave) &&
    selectedWave?.gate_status !== "accepted" &&
    latestSelectedRun?.status === "completed";

  return (
    <DocumentSurface className="max-w-[1280px] space-y-0">
      <div className="grid min-w-0 gap-5 xl:grid-cols-[minmax(0,1fr)_21rem] xl:gap-0">
        <section className="min-w-0 xl:pr-6">
          <button
            type="button"
            onClick={() =>
              onSelectionChange({ surface: "missions", missionId: undefined, waveId: undefined })
            }
            className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground transition-colors hover:text-foreground"
          >
            <ChevronLeft className="size-3.5" /> Missions
          </button>

          <header className="mt-3 flex flex-col items-stretch gap-3 border-b border-border/70 pb-5 sm:flex-row sm:flex-wrap sm:items-start sm:justify-between">
            <div className="min-w-0 flex-1 space-y-1.5 sm:min-w-72">
              <h1 className="text-2xl font-semibold tracking-[-0.025em] text-foreground">{mission.title}</h1>
              <p className="line-clamp-2 max-w-3xl text-[13px] leading-relaxed text-muted-foreground">{mission.objective}</p>
              <div className="flex flex-wrap items-center gap-1.5">
                <Badge tone="muted">{waves.length} ordered waves</Badge>
                <Badge tone={missionTone(mission.status)}>{mission.status ?? "planned"}</Badge>
              </div>
            </div>
            <div className="flex w-full flex-wrap items-center gap-2 sm:w-auto sm:justify-end">
              <Button
                type="button"
                variant="secondary"
                size="sm"
                className="xl:hidden"
                onClick={() => document.getElementById("mission-context")?.scrollIntoView({ behavior: "smooth", block: "start" })}
              >
                <PanelsTopLeft className="size-3.5" /> Context
              </Button>
              <ActionButton
                enabled={actionsEnabled}
                disabled={mission.status === "completed" || mission.status === "cancelled"}
                onClick={() => setWaveOpen(true)}
              >
                <Plus className="size-3.5" /> Add Wave
              </ActionButton>
              {mission.status !== "completed" && (
                <ActionButton
                  enabled={actionsEnabled}
                  disabled={!readyToClose}
                  variant={readyToClose ? "default" : "secondary"}
                  onClick={() => setCloseOpen(true)}
                  title={readyToClose ? "Record the Mission outcome" : "Every Wave must be accepted first"}
                >
                  <CheckCircle2 className="size-3.5" /> Close Mission
                </ActionButton>
              )}
            </div>
          </header>

          {waves.length === 0 ? (
            <EmptyState
              icon={Waves}
              title="Define the first Wave"
              description="Start with one small ordered unit, its executor, and a clear exit criterion."
            />
          ) : (
            <div className="mt-5">
              {waves.map((wave, index) => {
                const selected = selectedWave?.id === wave.id;
                const accepted = wave.gate_status === "accepted" || wave.status === "completed";
                return (
                <div key={wave.id} className="relative grid grid-cols-[2.5rem_minmax(0,1fr)] gap-3">
                  <div className="relative flex justify-center">
                    {index < waves.length - 1 && (
                      <span className="absolute bottom-0 top-8 w-px bg-border/90">
                        {wave.status === "running" && <LiveTrace axis="vertical" className="absolute inset-x-0 top-0 h-full" />}
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={() => onSelectionChange({ surface: "missions", missionId: mission.id, waveId: wave.id })}
                      aria-label={`Open Wave ${wave.index}: ${wave.title}`}
                      aria-current={selected ? "step" : undefined}
                      className={`relative z-[1] mt-0.5 grid size-8 place-items-center rounded-full border text-[11px] font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                        accepted
                          ? "border-status-good bg-status-good text-white"
                          : selected
                            ? "border-status-running bg-status-running text-white shadow-sm"
                            : "border-border bg-background text-muted-foreground hover:border-status-running/40"
                      }`}
                    >
                      {accepted ? <CheckCircle2 className="size-4" /> : wave.index}
                    </button>
                  </div>
                  <div className={index < waves.length - 1 ? "pb-5" : "pb-1"}>
                  {selected ? (
                    <WaveCanvasCard
                      wave={wave}
                      runs={runsForWave(model, wave)}
                      members={(model.snapshot.member_runs ?? []).filter((member) =>
                        runsForWave(model, wave).some((run) => run.id === member.team_run_id),
                      )}
                      onSelect={() =>
                        onSelectionChange({ surface: "missions", missionId: mission.id, waveId: wave.id })
                      }
                      onSelectionChange={onSelectionChange}
                      actionsEnabled={actionsEnabled}
                      onAction={onAction}
                    />
                  ) : (
                    <WaveJourneyCompact
                      wave={wave}
                      onOpen={() =>
                        onSelectionChange({ surface: "missions", missionId: mission.id, waveId: wave.id })
                      }
                    />
                  )}
                  {wave.plan_note && index < waves.length - 1 && (
                    <div className="mt-3 border-l-2 border-status-decision/70 bg-status-decision/5 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
                      <span className="mr-1 font-semibold uppercase tracking-wider text-status-decision">Re-plan</span>
                      {wave.plan_note}
                    </div>
                  )}
                  </div>
                </div>
              )})}
            </div>
          )}
        </section>

        <div id="mission-context" className="scroll-mt-3 xl:sticky xl:top-0 xl:self-start">
        <ContextRail quiet label="Mission context" className="h-fit" contentClassName="flex flex-col space-y-0">
          <ContextModule className="order-5 xl:order-1" title="Mission brief" kicker="Durable intent" icon={<Flag className="size-3.5" />}>
            <dl className="space-y-2 text-[11px] leading-relaxed">
              <ContextFact label="Objective" value={mission.objective} />
              <ContextFact label="Desired" value={mission.desired_outcome || "Not declared"} />
              {mission.outcome_summary && <ContextFact label="Closeout" value={mission.outcome_summary} />}
              <ContextFact label="Updated" value={fmt(mission.updated_at ?? mission.created_at)} />
            </dl>
          </ContextModule>

          {(pendingMembers.length > 0 || gateNeedsReview || selectedWave?.gate_status === "blocked") && (
            <ContextModule
              className="order-1 xl:order-2"
              title="Needs you"
              kicker="Decision queue"
              icon={<CircleAlert className="size-3.5" />}
              tone={selectedWave?.gate_status === "blocked" ? "bad" : "warn"}
              pinned
            >
              <div className="space-y-2 text-[11px] leading-relaxed text-muted-foreground">
                {gateNeedsReview && <p>Completed attempt is available for an explicit Wave gate decision.</p>}
                {selectedWave?.gate_status === "blocked" && <p>Wave is blocked; record the next decision or a revised attempt.</p>}
                {blockedMember ? (
                  <div className="space-y-1">
                    <p className="font-medium text-foreground">{blockedMember.name ?? blockedMember.id} is blocked.</p>
                    <p>{pressureMessage?.body ?? "Open the member activity and provide unblock direction."}</p>
                  </div>
                ) : pendingMembers.length > 0 ? (
                  <p>{pendingMembers.length} member{pendingMembers.length === 1 ? "" : "s"} need review or a response.</p>
                ) : null}
                {blockedMember && latestSelectedRun && (
                  <button
                    type="button"
                    onClick={() =>
                      onSelectionChange({
                        surface: "team",
                        teamId: latestSelectedRun.id,
                        memberRunId: blockedMember.id,
                        missionId: selectedWave?.mission_id,
                        waveId: selectedWave?.id,
                      })
                    }
                    className="inline-flex min-h-8 items-center gap-1 rounded-md border border-status-warn/30 bg-status-warn/10 px-2.5 font-medium text-foreground transition-colors hover:bg-status-warn/15"
                  >
                    Open {blockedMember.name ?? "blocked member"} <ChevronRight className="size-3.5" />
                  </button>
                )}
              </div>
            </ContextModule>
          )}

          {selectedWave && (
            <ContextModule
              className="order-3 xl:order-3"
              title={`Wave ${selectedWave.index} · ${selectedWave.title}`}
              kicker="Selected wave"
              icon={<Waves className="size-3.5" />}
              tone={waveTone(selectedWave.status)}
            >
              <dl className="space-y-2 text-[11px] leading-relaxed">
                <ContextFact label="Objective" value={selectedWave.objective} />
                <ContextFact label="Executor" value={executorLabel(selectedWave.executor_kind)} />
                <ContextFact label="Exit" value={selectedWave.exit_criteria || "Not declared"} />
                {selectedWave.outcome_summary && <ContextFact label="Outcome" value={selectedWave.outcome_summary} />}
              </dl>
            </ContextModule>
          )}

          {selectedWave?.executor_kind === "agent_team" && (
            <ContextModule
              className="order-4 xl:order-4"
              title="Agent Team"
              kicker="Executor compact"
              icon={<Users className="size-3.5" />}
              tone={latestSelectedRun ? waveTone(latestSelectedRun.status) : "idle"}
              live={latestSelectedRun?.status === "running"}
            >
              <dl className="space-y-2 text-[11px] leading-relaxed">
                <ContextFact label="Attempt" value={latestSelectedRun ? `Attempt ${selectedRuns.length} · ${latestSelectedRun.status ?? "planning"}` : "Not yet started"} />
                <ContextFact label="Members" value={selectedMembers.length ? `${selectedMembers.length} linked members` : "No members yet"} />
                <ContextFact label="Lineage" value={`${selectedRuns.length} preserved attempt${selectedRuns.length === 1 ? "" : "s"}`} />
              </dl>
              {latestSelectedRun && (
                <button
                  type="button"
                  onClick={() =>
                    onSelectionChange({
                      surface: "team",
                      teamId: latestSelectedRun.id,
                      missionId: selectedWave.mission_id,
                      waveId: selectedWave.id,
                    })
                  }
                  className="mt-3 inline-flex items-center gap-1 text-[11px] font-medium text-primary hover:underline"
                >
                  Open team attempt <ChevronRight className="size-3.5" />
                </button>
              )}
            </ContextModule>
          )}

          {selectedWave && (
            <ContextModule
              className="order-2 xl:order-5"
              title="Gate & outcome"
              kicker="Explicit host decision"
              icon={<ShieldCheck className="size-3.5" />}
              tone={gateTone(selectedWave.gate_status)}
            >
              <div className="space-y-3 text-[11px] leading-relaxed">
                {gateCriteria.length > 0 && (
                  <div className="rounded-md bg-muted/55 p-2.5">
                    <div className="flex items-end justify-between gap-3">
                      <div>
                        <p className="text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Readiness</p>
                        <p className="mt-0.5 text-xs font-medium text-foreground">
                          {evidencedCriteria == null
                            ? `${gateCriteria.length} declared criteria`
                            : `${evidencedCriteria} of ${gateCriteria.length} evidenced`}
                        </p>
                      </div>
                      {evidencedCriteria != null && (
                        <strong className="text-xl font-semibold tracking-tight text-foreground">
                          {evidencedCriteria}/{gateCriteria.length}
                        </strong>
                      )}
                    </div>
                    {evidencedCriteria != null && (
                      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-border" aria-label={`${evidencedCriteria} of ${gateCriteria.length} criteria evidenced`}>
                        <div
                          className="h-full rounded-full bg-status-good"
                          style={{ width: `${Math.round((evidencedCriteria / gateCriteria.length) * 100)}%` }}
                        />
                      </div>
                    )}
                    <ol className="mt-2.5 space-y-1.5">
                      {gateCriteria.map((criterion, index) => (
                        <li key={criterion} className="flex items-start gap-2 text-foreground">
                          {selectedWave.gate_status === "accepted" ? (
                            <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-status-good" />
                          ) : (
                            <CircleDashed className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                          )}
                          <span><span className="sr-only">Criterion {index + 1}: </span>{criterion}</span>
                        </li>
                      ))}
                    </ol>
                    {selectedWave.gate_status !== "accepted" && evidencedCriteria != null && (
                      <p className="mt-2 border-t border-border/70 pt-2 text-[10px] text-muted-foreground">
                        Criterion-level evidence mapping is not recorded; individual statuses remain unassigned.
                      </p>
                    )}
                  </div>
                )}
                <dl className="space-y-2">
                <ContextFact label="Gate" value={selectedWave.gate_status ?? "pending review"} />
                <ContextFact label="Candidate" value={latestSelectedRun ? latestSelectedRun.status === "completed" ? `Attempt ${selectedRuns.length} is eligible` : `Attempt ${selectedRuns.length} is ${latestSelectedRun.status ?? "planning"}` : "No attempt yet"} />
                <ContextFact label="Evidence" value={selectedWave.artifact_refs?.length ? `${selectedWave.artifact_refs.length} linked artifact${selectedWave.artifact_refs.length === 1 ? "" : "s"}` : "No linked artifacts"} />
                {selectedWave.gate_note && <ContextFact label="Note" value={selectedWave.gate_note} />}
                </dl>
              </div>
            </ContextModule>
          )}
        </ContextRail>
        </div>
      </div>

      <WaveDialog
        open={waveOpen}
        mission={mission}
        nextIndex={(waves[waves.length - 1]?.index ?? 0) + 1}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setWaveOpen(false)}
      />
      <MissionCloseDialog
        open={closeOpen}
        mission={mission}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setCloseOpen(false)}
      />
    </DocumentSurface>
  );
}

function ContextFact({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words text-foreground">{value}</dd>
    </div>
  );
}

function executorLabel(executor?: string | null): string {
  if (executor === "agent_team") return "Agent Team";
  if (executor === "dynamic_workflow") return "Dynamic Workflow";
  if (executor === "host") return "Host";
  return executor || "Not selected";
}

interface WaveCanvasCardProps {
  wave: Wave;
  runs: TeamRun[];
  members: { id: string; team_run_id?: string; name?: string | null; role?: string | null; status?: string | null }[];
  onSelect: () => void;
  onSelectionChange: MissionsProps["onSelectionChange"];
  actionsEnabled: boolean;
  onAction: MissionsProps["onAction"];
}

function WaveJourneyCompact({ wave, onOpen }: { wave: Wave; onOpen: () => void }) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className="group flex w-full items-start gap-4 border-b border-border/70 px-1 pb-5 text-left transition-colors hover:border-status-running/35 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <span className="min-w-0 flex-1">
        <span className="flex flex-wrap items-center gap-2">
          <span className="text-[15px] font-semibold tracking-tight text-foreground">Wave {wave.index} · {wave.title}</span>
          <Badge tone={waveTone(wave.status)}>{wave.status ?? "planned"}</Badge>
        </span>
        <span className="mt-1.5 line-clamp-2 block text-[12px] leading-relaxed text-muted-foreground">{wave.objective}</span>
        <span className="mt-2.5 flex flex-wrap items-center gap-x-5 gap-y-1 text-[10px] text-muted-foreground">
          <span><span className="font-semibold uppercase tracking-wider">Executor</span> · {executorLabel(wave.executor_kind)}</span>
          <span><span className="font-semibold uppercase tracking-wider">Gate</span> · {wave.gate_status ?? "pending"}</span>
          <span><span className="font-semibold uppercase tracking-wider">Artifacts</span> · {wave.artifact_refs?.length ?? 0}</span>
        </span>
      </span>
      <ChevronRight className="mt-1 size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground motion-reduce:transform-none" />
    </button>
  );
}

function WaveCanvasCard({
  wave,
  runs,
  members,
  onSelect,
  onSelectionChange,
  actionsEnabled,
  onAction,
}: WaveCanvasCardProps) {
  const [attemptOpen, setAttemptOpen] = useState(false);
  const [gateOpen, setGateOpen] = useState(false);
  const latest = runs[runs.length - 1];
  const canTeamRun = wave.executor_kind === "agent_team";
  const hasActiveAttempt = runs.some((run) =>
    ["planning", "running", "waiting", "reviewing"].includes(run.status ?? ""),
  );
  const waveAccepted = wave.gate_status === "accepted" || wave.status === "completed";
  const activeMembers = latest ? members.filter((member) => member.team_run_id === latest.id) : [];
  const blockedMember = activeMembers.find((member) => member.status === "blocked");
  const criteria = exitCriteriaFor(wave);
  const readyCriteria = reportedGateReadiness(wave, criteria.length);

  return (
    <section className="relative min-w-0 border-b border-border/80 bg-background">
      <button
        type="button"
        onClick={onSelect}
        className="flex w-full items-start gap-3 px-1 pb-4 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span className="min-w-0 flex-1">
          <span className="flex flex-wrap items-center gap-1.5">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Current wave</span>
            <Badge tone={waveTone(wave.status)}>{wave.status ?? "planned"}</Badge>
            <Badge tone={gateTone(wave.gate_status)}>gate {wave.gate_status ?? "pending"}</Badge>
          </span>
          <span className="mt-1.5 block text-lg font-semibold tracking-tight text-foreground">Wave {wave.index} · {wave.title}</span>
          <span className="mt-1 block max-w-3xl text-[12px] leading-relaxed text-muted-foreground">{wave.objective}</span>
        </span>
      </button>

      {wave.status === "running" && <LiveTrace className="mb-4" />}

      <div className="space-y-4 px-1 pb-5">
        <div className="flex flex-wrap gap-x-7 gap-y-2 text-[11px]">
          <p><span className="mr-2 font-semibold uppercase tracking-wider text-muted-foreground">Executor</span><span className="font-medium text-foreground">{executorLabel(wave.executor_kind)}</span></p>
          <p className="min-w-0 flex-1"><span className="mr-2 font-semibold uppercase tracking-wider text-muted-foreground">Exit</span><span className="text-foreground">{wave.exit_criteria || "Not declared"}</span></p>
        </div>

        {canTeamRun ? (
          <section className="border-y border-border/70 py-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="flex items-center gap-2 text-[12px] font-semibold text-foreground"><Users className="size-3.5 text-muted-foreground" /> Agent Team</span>
              <span className="flex items-center gap-1.5">
                <Badge tone="muted">{runs.length} attempt{runs.length === 1 ? "" : "s"}</Badge>
                {latest && <Badge tone={waveTone(latest.status)}>{latest.status ?? "planning"}</Badge>}
              </span>
            </div>
            <div className="mt-3 space-y-3">
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
              </div>
              {runs.length === 0 ? (
                <p className="text-[12px] text-muted-foreground">No Agent Team attempt yet. Create one when this Wave is ready to execute.</p>
              ) : (
                <div className="overflow-hidden rounded-md bg-muted/35">
              {latest && (
                <button
                  key={latest.id}
                  type="button"
                  onClick={() =>
                    onSelectionChange({
                      surface: "team",
                      teamId: latest.id,
                      missionId: wave.mission_id,
                      waveId: wave.id,
                    })
                  }
                  className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] hover:bg-accent/40"
                >
                  <StatusDot tone={waveTone(latest.status)} />
                  <span className="font-medium text-foreground">Attempt {runs.length}</span>
                  <Badge tone={waveTone(latest.status)}>{latest.status ?? "planning"}</Badge>
                  {latest.previous_run_id && <span className="truncate text-[11px] text-muted-foreground">retry of Attempt {Math.max(1, runs.length - 1)}</span>}
                  <MonoId>{latest.id}</MonoId>
                  {latest.id === wave.accepted_run_id && <Badge tone="good">accepted</Badge>}
                </button>
              )}
                </div>
              )}
              {activeMembers.length > 0 && (
                <div className="flex flex-wrap gap-4">
                  {activeMembers.map((member) => (
                    <span key={`${member.team_run_id}:${member.name}:${member.role}`} className="inline-flex min-w-0 items-center gap-2">
                      <Avatar name={member.name || member.role || "Member"} tone={waveTone(member.status)} size="sm" />
                      <span className="min-w-0">
                        <span className="block max-w-28 truncate text-[10px] font-medium text-foreground">{member.name || "Member"}</span>
                        <span className="block text-[9px] text-muted-foreground">{member.status || "unknown"}</span>
                      </span>
                    </span>
                  ))}
                </div>
              )}
              {blockedMember && latest && (
                <DecisionAnchor
                  compact
                  title="QA approval required"
                  detail={`${blockedMember.name ?? "A member"} is blocked`}
                  actionLabel="Review request"
                  onAction={() => onSelectionChange({ surface: "team", teamId: latest.id, memberRunId: blockedMember.id, missionId: wave.mission_id, waveId: wave.id })}
                />
              )}
            </div>
          </section>
        ) : (
          <p className="rounded-md border border-border bg-background/35 px-3 py-3 text-[12px] text-muted-foreground">
            {executorLabel(wave.executor_kind)} remains a distinct executor surface. This canvas retains its declared outcome and gate rather than inventing an Agent Team attempt.
          </p>
        )}

        <section className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_10rem] sm:items-end">
          <div>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="flex items-center gap-2 text-[12px] font-semibold text-foreground"><FileCheck2 className="size-3.5 text-muted-foreground" /> Evidence & gate</span>
            <Badge tone={gateTone(wave.gate_status)}>gate {wave.gate_status ?? "pending"}</Badge>
          </div>
          <p className="mt-1.5 text-[11px] leading-relaxed text-muted-foreground">
            {wave.artifact_refs?.length
              ? `${wave.artifact_refs.length} linked artifact${wave.artifact_refs.length === 1 ? "" : "s"} · ${wave.artifact_refs.join(", ")}`
              : "No linked artifacts yet. Gate remains an explicit host decision."}
          </p>
          {wave.outcome_summary && <p className="mt-1.5 text-[11px] leading-relaxed text-foreground">{wave.outcome_summary}</p>}
          <div className="mt-2 flex flex-wrap gap-2">
            <ActionButton
              enabled={actionsEnabled}
              disabled={hasActiveAttempt || waveAccepted}
              size="sm"
              variant="secondary"
              onClick={() => setGateOpen(true)}
            >
              <ShieldCheck className="size-3.5" /> Gate Wave
            </ActionButton>
          </div>
          </div>
          {criteria.length > 0 && readyCriteria != null && <ReadinessMeter value={readyCriteria} total={criteria.length} />}
        </section>
      </div>

      <AttemptDialog
        open={attemptOpen}
        wave={wave}
        latestRun={latest}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setAttemptOpen(false)}
      />
      <GateDialog
        open={gateOpen}
        wave={wave}
        runs={runs}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setGateOpen(false)}
      />
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

function MissionCloseDialog({
  open,
  mission,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  mission: Mission;
  actionsEnabled: boolean;
  onAction: MissionsProps["onAction"];
  onClose: () => void;
}) {
  const [outcome, setOutcome] = useState("");
  const [completedBy, setCompletedBy] = useState("host");

  useEffect(() => {
    if (open) {
      setOutcome(mission.outcome_summary ?? "");
      setCompletedBy(mission.completed_by ?? "host");
    }
  }, [mission.completed_by, mission.outcome_summary, open]);

  const valid = Boolean(outcome.trim() && completedBy.trim());
  const submit = () => {
    if (!valid) return;
    dispatch(
      onAction,
      closeMission({
        missionId: mission.id,
        outcome: outcome.trim(),
        completedBy: completedBy.trim(),
      }),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="Close Mission"
      description="Record the durable Mission outcome after every ordered Wave has been accepted. This closeout is immutable."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Mission outcome" required>
          {(id) => <TextArea id={id} value={outcome} onChange={(event) => setOutcome(event.target.value)} />}
        </Field>
        <Field label="Completed by" required>
          {(id) => <TextInput id={id} value={completedBy} onChange={(event) => setCompletedBy(event.target.value)} />}
        </Field>
        <DialogFooter
          submitLabel="Complete Mission"
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
  const requiresRun = wave.executor_kind !== "host";
  const nonTeamRunIds = wave.executor_kind === "agent_team" ? [] : (wave.executor_run_ids ?? []);
  const defaultRunId = wave.accepted_run_id
    ?? (wave.executor_kind === "agent_team" ? latestCompletedRunId : nonTeamRunIds[nonTeamRunIds.length - 1])
    ?? "";
  const artifactValues = (wave.artifact_refs ?? []).join(", ");

  useEffect(() => {
    if (open) {
      setStatus("accepted");
      setRunId(defaultRunId);
      setOutcome(wave.outcome_summary ?? "");
      setNote("");
      setArtifacts(artifactValues);
    }
  }, [artifactValues, defaultRunId, open, wave.outcome_summary]);

  const valid = status !== "accepted" || Boolean(outcome.trim() && (!requiresRun || runId));
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
        {requiresRun && <Field
          label="Attempt"
          hint={wave.executor_kind === "agent_team" ? "Accepted must name a completed Agent Team attempt." : "Accepted must name an executor run registered to this Wave."}
        >
          {(id) => (
            <Select id={id} value={runId} onChange={(event) => setRunId(event.target.value)}>
              <option value="">No attempt selected</option>
              {selectableRuns.map((run) => (
                <option key={run.id} value={run.id}>
                  {run.id} · {run.status ?? "planning"}
                </option>
              ))}
              {nonTeamRunIds.map((id) => (
                <option key={id} value={id}>{id}</option>
              ))}
            </Select>
          )}
        </Field>}
        {!requiresRun && (
          <p className="rounded-md border border-border bg-muted/35 px-3 py-2 text-[11px] text-muted-foreground">
            Host execution records its direct outcome and artifacts without inventing an executor run.
          </p>
        )}
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
