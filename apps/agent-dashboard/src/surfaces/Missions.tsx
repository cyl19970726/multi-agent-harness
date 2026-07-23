import { useEffect, useState, type ComponentProps, type ReactNode } from "react";
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
  advanceWave,
  closeMission,
  createMissionTeam,
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
  provider: "codex" | "kimi";
  executionMode: "codex_exec" | "codex_app_server" | "kimi_acp";
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

function runsForMission(model: WorkbenchModel, mission: Mission): TeamRun[] {
  const teamIds = new Set(mission.agent_team_ids ?? []);
  return [...(model.snapshot.team_runs ?? [])]
    .filter((run) =>
      run.mission_id === mission.id
      && (!run.agent_team_id || teamIds.size === 0 || teamIds.has(run.agent_team_id)),
    )
    .sort((a, b) => (a.created_at ?? "").localeCompare(b.created_at ?? ""));
}

function MarkdownContext({ value, empty }: { value?: string | null; empty: string }) {
  if (!value?.trim()) {
    return <p className="text-[12px] leading-relaxed text-muted-foreground">{empty}</p>;
  }
  const lines = value.split("\n");
  const content: ReactNode[] = [];
  const cells = (line: string) => line.slice(1, -1).split("|").map((cell) => cell.trim());
  for (let index = 0; index < lines.length;) {
    const line = lines[index];
    if (/^\|.*\|$/.test(line)) {
      const tableLines: string[] = [];
      while (index < lines.length && /^\|.*\|$/.test(lines[index])) {
        tableLines.push(lines[index]);
        index += 1;
      }
      const rows = tableLines
        .map(cells)
        .filter((row) => !row.every((cell) => /^:?-{3,}:?$/.test(cell)));
      if (rows.length > 0) {
        const [head, ...body] = rows;
        content.push(
          <div key={`table-${index}`} className="overflow-x-auto rounded-lg border border-border/70 bg-background/70">
            <table className="w-full min-w-[34rem] border-collapse text-left text-[11px]">
              <thead className="bg-muted/55 text-[9px] uppercase tracking-[0.1em] text-muted-foreground">
                <tr>{head.map((cell, cellIndex) => <th key={cellIndex} className="border-b border-border/70 px-3 py-2 font-semibold">{cell}</th>)}</tr>
              </thead>
              <tbody>
                {body.map((row, rowIndex) => (
                  <tr key={rowIndex} className="border-b border-border/45 last:border-b-0">
                    {row.map((cell, cellIndex) => <td key={cellIndex} className="px-3 py-2 align-top text-foreground/85">{cell}</td>)}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>,
        );
      }
      continue;
    }
    if (line.startsWith("### ")) content.push(<h4 key={index} className="pt-1 text-[11px] font-semibold uppercase tracking-wider">{line.slice(4)}</h4>);
    else if (line.startsWith("## ")) content.push(<h3 key={index} className="pt-1 text-sm font-semibold">{line.slice(3)}</h3>);
    else if (line.startsWith("# ")) content.push(<h2 key={index} className="text-base font-semibold tracking-tight">{line.slice(2)}</h2>);
    else if (/^[-*] /.test(line)) content.push(<p key={index} className="pl-3 before:mr-2 before:text-primary before:content-['•']">{line.slice(2)}</p>);
    else if (line.trim()) content.push(<p key={index} className="whitespace-pre-wrap">{line}</p>);
    else content.push(<span key={index} className="block h-1" aria-hidden="true" />);
    index += 1;
  }
  return (
    <div className="space-y-2 text-[12px] leading-relaxed text-foreground">
      {content}
    </div>
  );
}

function blankMember(): MemberDraft {
  return {
    name: "",
    role: "",
    provider: "codex",
    executionMode: "codex_app_server",
    model: "",
    ownedPaths: "",
  };
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
            Durable intent, Host plan revisions, and independent long-lived Agent Teams.
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
  const [teamOpen, setTeamOpen] = useState(false);
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
  const missionRuns = runsForMission(model, mission);
  const latestMissionRun = missionRuns[missionRuns.length - 1];
  const linkedMissionTeams = (model.snapshot.teams ?? []).filter((team) =>
    (mission.agent_team_ids ?? []).includes(team.id),
  );
  const latestMissionTeam = latestMissionRun?.agent_team_id
    ? linkedMissionTeams.find((team) => team.id === latestMissionRun.agent_team_id)
    : linkedMissionTeams[0];
  const missionRunIds = new Set(missionRuns.map((run) => run.id));
  const selectedMembers = (model.snapshot.member_runs ?? []).filter(
    (member) => member.team_run_id && missionRunIds.has(member.team_run_id),
  );
  const selectedMessages = (model.snapshot.team_messages ?? []).filter(
    (message) => message.team_run_id && missionRunIds.has(message.team_run_id),
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
    <DocumentSurface
      className="h-full max-w-[1280px] space-y-0 overflow-y-auto overscroll-contain px-3 py-5 sm:px-5 xl:px-0"
      data-mission-scroll-owner="true"
      role="region"
      aria-label="Mission detail"
      tabIndex={0}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        const page = Math.max(event.currentTarget.clientHeight * 0.85, 240);
        if (event.key === "PageDown" || event.key === "PageUp") {
          event.preventDefault();
          event.currentTarget.scrollBy({ top: event.key === "PageDown" ? page : -page, behavior: "auto" });
        } else if (event.key === "Home" || event.key === "End") {
          event.preventDefault();
          event.currentTarget.scrollTo({ top: event.key === "Home" ? 0 : event.currentTarget.scrollHeight, behavior: "auto" });
        }
      }}
    >
      <div className="grid min-w-0 gap-5 xl:grid-cols-[minmax(0,1fr)_21rem] xl:gap-0">
        <section className="min-w-0 xl:pl-5 xl:pr-6">
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
                <Badge tone="muted">{mission.agent_team_ids?.length ?? 0} linked teams</Badge>
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
              <ActionButton
                enabled={actionsEnabled}
                disabled={mission.status === "completed" || mission.status === "cancelled"}
                variant="secondary"
                onClick={() => setTeamOpen(true)}
              >
                <Users className="size-3.5" /> New Team
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

          <section className="border-b border-border/70 py-5">
            <div className="mb-3 flex items-center justify-between gap-3">
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Mission context</p>
                <p className="mt-1 text-[11px] text-muted-foreground">Durable brief used by the Host across every Wave.</p>
              </div>
              <Badge tone="muted">Markdown</Badge>
            </div>
            <MarkdownContext value={mission.context} empty="No Mission context has been recorded yet." />
          </section>

          {waves.length === 0 ? (
            <EmptyState
              icon={Waves}
              title="Define the first Wave"
              description="Record the Host's first operational memo and the plan decision that should come next."
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
              <ContextFact label="Teams" value={`${mission.agent_team_ids?.length ?? 0} linked`} />
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
                {blockedMember && latestMissionRun && (
                  <button
                    type="button"
                    onClick={() =>
                      onSelectionChange({
                        surface: "team",
                        teamId: latestMissionRun.id,
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
                <ContextFact label="Revision" value={`r${selectedWave.revision ?? 0} · ${selectedWave.updated_by ?? "legacy"}`} />
                <ContextFact label="Decision" value={selectedWave.exit_criteria || "Host judgment"} />
                {selectedWave.outcome_summary && <ContextFact label="Outcome" value={selectedWave.outcome_summary} />}
              </dl>
            </ContextModule>
          )}

          {(mission.agent_team_ids?.length ?? 0) > 0 && (
            <ContextModule
              className="order-5 xl:order-5"
              title="Mission Agent Teams"
              kicker="Independent relation"
              icon={<Users className="size-3.5" />}
              tone={latestMissionRun ? waveTone(latestMissionRun.status) : "idle"}
              live={latestMissionRun?.status === "running"}
            >
              <dl className="space-y-2 text-[11px] leading-relaxed">
                <ContextFact label="Linked" value={`${mission.agent_team_ids?.length ?? 0} reusable team${mission.agent_team_ids?.length === 1 ? "" : "s"}`} />
                <ContextFact label="Team Lead" value={!latestMissionTeam?.owner_agent_id || latestMissionTeam.owner_agent_id === "host" ? "Current Host Agent" : latestMissionTeam.owner_agent_id} />
                <ContextFact label="Run" value={latestMissionRun ? `${latestMissionRun.status ?? "planning"} · ${latestMissionRun.objective ?? latestMissionRun.id}` : "Not yet started"} />
                <ContextFact label="Members" value={selectedMembers.length ? `${selectedMembers.length} linked members` : "No members yet"} />
                <ContextFact label="Lifetime" value="Continues across Waves" />
              </dl>
              <p className="mt-2 text-[10px] leading-relaxed text-muted-foreground">
                The current Host is Team Lead; it is not counted as a MemberRun unless explicitly added to execute a lane.
              </p>
              {latestMissionRun && (
                <button
                  type="button"
                  onClick={() =>
                    onSelectionChange({
                      surface: "team",
                      teamId: latestMissionRun.id,
                      missionId: mission.id,
                      waveId: selectedWave?.id,
                    })
                  }
                  className="mt-3 inline-flex items-center gap-1 text-[11px] font-medium text-primary hover:underline"
                >
                  Open Mission team <ChevronRight className="size-3.5" />
                </button>
              )}
            </ContextModule>
          )}

          {selectedWave && (
            <ContextModule
              className="order-2 xl:order-4"
              title="Gate readiness"
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
      <MissionTeamDialog
        open={teamOpen}
        mission={mission}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setTeamOpen(false)}
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
        <section className="rounded-lg border border-border/70 bg-muted/20 p-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Host plan context</span>
            <Badge tone="muted">r{wave.revision ?? 0}</Badge>
          </div>
          <MarkdownContext value={wave.context} empty="No detailed Host plan has been recorded for this Wave." />
        </section>
        <div className="flex flex-wrap gap-x-7 gap-y-2 text-[11px]">
          <p><span className="mr-2 font-semibold uppercase tracking-wider text-muted-foreground">Updated by</span><span className="font-medium text-foreground">{wave.updated_by || "legacy row"}</span></p>
          <p className="min-w-0 flex-1"><span className="mr-2 font-semibold uppercase tracking-wider text-muted-foreground">Advance when</span><span className="text-foreground">{wave.exit_criteria || "Host judgment changes materially"}</span></p>
        </div>

        {canTeamRun ? (
          <section className="border-y border-border/70 py-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="flex items-center gap-2 text-[12px] font-semibold text-foreground"><Users className="size-3.5 text-muted-foreground" /> Agent Team{runs.length > 0 ? ` · Attempt ${runs.length}` : ""}</span>
              <span className="flex flex-wrap items-center justify-end gap-1.5">
                <Badge tone="muted">{runs.length} attempt{runs.length === 1 ? "" : "s"}</Badge>
                {latest && <Badge tone={waveTone(latest.status)}>{latest.status ?? "planning"}</Badge>}
                <ActionButton
                  enabled={actionsEnabled}
                  disabled={hasActiveAttempt || waveAccepted}
                  size="sm"
                  variant="secondary"
                  onClick={() => setAttemptOpen(true)}
                >
                  <Rocket className="size-3.5" />
                  {latest ? "Retry / new attempt" : "Create Agent Team"}
                </ActionButton>
              </span>
            </div>
            <div className="mt-3 space-y-3">
              {runs.length === 0 ? (
                <p className="text-[12px] text-muted-foreground">No Agent Team attempt yet. Create one when this Wave is ready to execute.</p>
              ) : latest ? (
                <button
                  type="button"
                  onClick={() => onSelectionChange({ surface: "team", teamId: latest.id, missionId: wave.mission_id, waveId: wave.id })}
                  className="sr-only"
                >
                  Open Attempt {runs.length}<MonoId>{latest.id}</MonoId>
                </button>
              ) : null}
              {activeMembers.length > 0 && (
                <div className="flex flex-wrap gap-4">
                  {activeMembers.map((member) => (
                    <button
                      key={`${member.team_run_id}:${member.id}`}
                      type="button"
                      onClick={() => onSelectionChange({
                        surface: "team",
                        teamId: latest.id,
                        memberRunId: member.id,
                        missionId: wave.mission_id,
                        waveId: wave.id,
                      })}
                      aria-label={`Open member ${member.name || member.role || member.id}`}
                      className="group inline-flex min-w-0 items-center gap-2 rounded-md px-1.5 py-1 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      <Avatar
                        name={member.name || member.role || "Member"}
                        identity={`${member.role ?? "member"} ${member.id}`}
                        tone={waveTone(member.status)}
                        size="sm"
                      />
                      <span className="min-w-0">
                        <span className="block max-w-28 truncate text-[10px] font-medium text-foreground group-hover:text-primary">{member.name || "Member"}</span>
                        <span className="block text-[9px] text-muted-foreground">{member.status || "unknown"}</span>
                      </span>
                      <ChevronRight className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
                    </button>
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
              <ShieldCheck className="size-3.5" /> {wave.executor_kind === "host" ? "Advance Wave" : "Gate Wave"}
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
  const [context, setContext] = useState("");

  useEffect(() => {
    if (open) {
      setTitle("");
      setObjective("");
      setOutcome("");
      setContext("");
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
        context: context.trim() || undefined,
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
        <Field label="Mission context" hint="Markdown brief shared across all Waves and linked teams.">
          {(id) => <TextArea id={id} value={context} onChange={(event) => setContext(event.target.value)} />}
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
  const [exit, setExit] = useState("");
  const [context, setContext] = useState("");

  useEffect(() => {
    if (open) {
      setTitle("");
      setObjective("");
      setExit("");
      setContext("");
    }
  }, [open]);

  const submit = () => {
    if (!title.trim() || !objective.trim() || !context.trim()) return;
    dispatch(
      onAction,
      createWave({
        missionId: mission.id,
        index: nextIndex,
        title: title.trim(),
        objective: objective.trim(),
        executorKind: "host",
        exitCriteria: exit.trim() || undefined,
        context: context.trim() || undefined,
      }),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      title={`Add Wave ${nextIndex}`}
      description="A Wave is the Host's versioned operational memo, not a runtime container."
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
        <Field label="Wave context" required hint="Markdown plan, member responsibilities, carry-over, questions, and Host judgment.">
          {(id) => <TextArea id={id} value={context} onChange={(event) => setContext(event.target.value)} />}
        </Field>
        <Field label="Advance when">
          {(id) => <TextInput id={id} value={exit} onChange={(event) => setExit(event.target.value)} />}
        </Field>
        <DialogFooter
          submitLabel="Add Wave"
          actionsEnabled={actionsEnabled}
          canSubmit={Boolean(title.trim() && objective.trim() && context.trim())}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

function MissionTeamDialog({
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
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setDescription("");
    }
  }, [open]);

  const valid = Boolean(name.trim() && description.trim());
  const submit = () => {
    if (!valid) return;
    dispatch(onAction, createMissionTeam({
      missionId: mission.id,
      name: name.trim(),
      description: description.trim(),
    }));
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="Create Mission Agent Team"
      description="Creates an independent reusable team and links it to this Mission. Closing the Mission will not close the team."
      onClose={onClose}
    >
      <form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}>
        <Field label="Team name" required>
          {(id) => <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} />}
        </Field>
        <Field label="Purpose" required>
          {(id) => <TextArea id={id} value={description} onChange={(event) => setDescription(event.target.value)} />}
        </Field>
        <DialogFooter
          submitLabel="Create and link team"
          actionsEnabled={actionsEnabled}
          canSubmit={valid}
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
          executionMode: member.executionMode,
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
              <Field label="Provider" required hint="Choose the provider; control capability is execution-mode specific.">
                {(id) => (
                  <Select
                    id={id}
                    value={member.provider}
                    onChange={(event) => {
                      const provider = event.target.value as MemberDraft["provider"];
                      updateMember(index, {
                        provider,
                        executionMode: provider === "codex" ? "codex_app_server" : "kimi_acp",
                        model: provider === "kimi" ? "k2.5" : "",
                      });
                    }}
                  >
                    <option value="codex">Codex</option>
                    <option value="kimi">Kimi</option>
                    <option disabled>Claude (coming later)</option>
                  </Select>
                )}
              </Field>
              <Field
                label="Execution mode"
                hint={member.executionMode === "codex_app_server"
                  ? "Interactive: same-turn steer and cooperative interrupt."
                  : member.executionMode === "codex_exec"
                    ? "Batch: messages queue for the next round."
                    : "ACP: provider questions resume in-turn; chat queues to the next round."}
              >
                {(id) => (
                  <Select
                    id={id}
                    value={member.executionMode}
                    onChange={(event) => updateMember(index, {
                      executionMode: event.target.value as MemberDraft["executionMode"],
                    })}
                  >
                    {member.provider === "codex" ? (
                      <>
                        <option value="codex_app_server">Interactive app-server</option>
                        <option value="codex_exec">Batch exec</option>
                      </>
                    ) : (
                      <option value="kimi_acp">Kimi ACP</option>
                    )}
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
    dispatch(onAction, wave.executor_kind === "host" && status === "accepted"
      ? advanceWave({
          waveId: wave.id,
          outcome: outcome.trim(),
          advancedBy: "host",
          artifactRefs: parseList(artifacts),
        })
      : gateWave({
          waveId: wave.id,
          status,
          runId: runId || undefined,
          acceptedBy: "host",
          outcome: outcome.trim() || undefined,
          note: note.trim() || undefined,
          artifactRefs: parseList(artifacts),
        }));
    onClose();
  };

  return (
    <Dialog
      open={open}
      title={wave.executor_kind === "host" ? "Advance Host plan" : "Gate legacy executor Wave"}
      description={wave.executor_kind === "host"
        ? "Record why the Host is advancing. Active assignments and provider sessions continue unchanged."
        : "The Host records accepted, revise, or blocked without deleting any attempt."}
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
