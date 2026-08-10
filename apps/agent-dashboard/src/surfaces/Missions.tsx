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
  PencilLine,
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
  appendMissionLog,
  closeMission,
  createTeam,
  createMission,
  createTeamRun,
  updateMissionContext,
  type ActionDescriptor,
  type TeamRunMemberSpec,
} from "../api/actions";
import { TEAM_MEMBER_PROVIDER_MODES } from "@/lib/provider";
import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";
import type { Mission, MissionLogEntry, MissionLogEntryKind, TeamRun, Wave } from "../types";

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
  provider: string;
  executionMode: string;
  model: string;
  effort: string;
  serviceTier: string;
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

/** Mission Log entries for one Mission, newest revision first (ADR 0051). */
function missionLogFor(model: WorkbenchModel, missionId: string): MissionLogEntry[] {
  return [...(model.snapshot.mission_log ?? [])]
    .filter((entry) => entry.mission_id === missionId)
    .sort((a, b) => b.revision - a.revision);
}

function runsForWave(model: WorkbenchModel, wave: Wave): TeamRun[] {
  const teamIds = new Set((model.snapshot.teams ?? [])
    .filter((team) => team.mission_id === wave.mission_id)
    .map((team) => team.id));
  return [...(model.snapshot.team_runs ?? [])]
    .filter((run) => teamIds.has(run.agent_team_id))
    .sort((a, b) => (a.created_at ?? "").localeCompare(b.created_at ?? ""));
}

function runsForMission(model: WorkbenchModel, mission: Mission): TeamRun[] {
  const teamIds = new Set((model.snapshot.teams ?? [])
    .filter((team) => team.mission_id === mission.id)
    .map((team) => team.id));
  return [...(model.snapshot.team_runs ?? [])]
    .filter((run) => teamIds.has(run.agent_team_id))
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
  const codex = TEAM_MEMBER_PROVIDER_MODES.find((entry) => entry.provider === "codex")
    ?? TEAM_MEMBER_PROVIDER_MODES[0];
  return {
    name: "",
    role: "",
    provider: codex.provider,
    executionMode: codex.mode,
    model: "",
    effort: "high",
    serviceTier: "",
    ownedPaths: "",
  };
}

/** One registered mode per provider, so the mode auto-fills from the provider. */
const EXECUTION_MODE_HINTS: Record<string, string> = {
  codex_app_server: "Interactive app-server is the only Codex Agent Team mode; one-shot exec belongs to Dynamic Workflow.",
  kimi_acp: "ACP: provider questions resume in-turn; chat queues to the next round.",
  claude_agent_sdk: "Agent SDK streaming session is the only Claude Agent Team mode; claude -p belongs to Dynamic Workflow.",
  pi_rpc: "RPC is Pi's persistent bidirectional mode.",
};

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
  const missions = [...(model.snapshot.missions ?? [])].sort((a, b) =>
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
  const [logOpen, setLogOpen] = useState(false);
  const [teamOpen, setTeamOpen] = useState(false);
  const [closeOpen, setCloseOpen] = useState(false);
  const [editContextOpen, setEditContextOpen] = useState(false);
  const waves = wavesFor(model, mission.id);
  const missionLog = missionLogFor(model, mission.id);
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
    team.mission_id === mission.id,
  );
  const linkedTeamSummaries = linkedMissionTeams.map((linkedTeam) => {
    const teamId = linkedTeam.id;
    const team = linkedMissionTeams.find((candidate) => candidate.id === teamId);
    const runs = missionRuns.filter((run) => run.agent_team_id === teamId);
    const latestRun = runs[runs.length - 1];
    const members = latestRun
      ? (model.snapshot.member_runs ?? []).filter((member) => member.team_run_id === latestRun.id)
      : [];
    return { teamId, team, latestRun, members, attemptCount: runs.length };
  });
  const missionRunIds = new Set(missionRuns.map((run) => run.id));
  const selectedMembers = (model.snapshot.member_runs ?? []).filter(
    (member) => member.team_run_id && missionRunIds.has(member.team_run_id),
  );
  const missionWorks = (model.snapshot.works ?? []).filter(
    (work) => missionRunIds.has(work.team_run_id),
  );
  const pendingMembers = selectedMembers.filter((member) =>
    ["waiting", "reviewing"].includes(member.status ?? ""),
  );
  const blockedWork = missionWorks.find((work) => work.condition === "blocked");
  const reviewWorkCount = missionWorks.filter((work) => work.phase === "review").length;
  const blockedMember = blockedWork
    ? selectedMembers.find((member) =>
      member.id === blockedWork.active_member_run_id
      || member.agent_member_id === blockedWork.owner_member_id
      || member.slot_id === blockedWork.owner_member_id)
    : undefined;
  const blockedRun = blockedWork
    ? missionRuns.find((run) => run.id === blockedWork.team_run_id)
    : undefined;
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
                <Badge tone="muted">{waves.length} ordered {waves.length === 1 ? "wave" : "waves"}</Badge>
                <Badge tone="muted">{linkedMissionTeams.length} owning {linkedMissionTeams.length === 1 ? "team" : "teams"}</Badge>
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
                onClick={() => setLogOpen(true)}
              >
                <Plus className="size-3.5" /> Append Host judgment
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
                <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Mission brief</p>
                <p className="mt-1 text-[11px] text-muted-foreground">Durable context used by the Host across every Wave.</p>
              </div>
              <span className="flex items-center gap-2">
                <Badge tone="muted">Markdown</Badge>
                <ActionButton
                  enabled={actionsEnabled}
                  variant="outline"
                  size="sm"
                  onClick={() => setEditContextOpen(true)}
                >
                  Edit context
                </ActionButton>
              </span>
            </div>
            <MarkdownContext value={mission.context} empty="No Mission context has been recorded yet." />
          </section>

          <section className="border-b border-border/70 py-5">
            <div className="mb-3 flex items-center justify-between gap-3">
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Linked teams</p>
                <p className="mt-1 text-[11px] text-muted-foreground">The one flat AgentTeam whose immutable mission_id points here.</p>
              </div>
              <Badge tone="muted">{linkedMissionTeams.length} linked</Badge>
            </div>
            {linkedMissionTeams.length === 0 ? (
              <p className="text-[12px] text-muted-foreground">No AgentTeam has been created for this Mission.</p>
            ) : (
              <ul className="space-y-2">
                {linkedMissionTeams.map((team) => (
                  <li key={team.id} className="flex items-center gap-2 rounded-lg border border-border/70 bg-background/70 px-3 py-2">
                    <span className="min-w-0 flex-1 truncate text-[12px] font-medium text-foreground">{team.name ?? team.id}</span>
                    <Badge tone="info">Mission owner</Badge>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="border-b border-border/70 py-5">
            <div className="mb-3 flex items-center justify-between gap-3">
              <div>
                <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Mission Log</p>
                <p className="mt-1 text-[11px] text-muted-foreground">Append-only Host judgment, newest first.</p>
              </div>
              <Badge tone="muted">{missionLog.length} {missionLog.length === 1 ? "entry" : "entries"}</Badge>
            </div>
            {missionLog.length === 0 ? (
              <p className="text-[12px] text-muted-foreground">No mission log yet.</p>
            ) : (
              <ul className="space-y-3">
                {missionLog.map((entry) => (
                  <li
                    key={entry.id}
                    className="border-l-2 border-status-decision/70 bg-status-decision/5 px-3 py-2"
                  >
                    <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
                      <span className="font-semibold text-foreground">#{entry.revision}</span>
                      <Badge tone="muted">{entry.kind}</Badge>
                      <span>{entry.actor}</span>
                      <span>{fmt(entry.created_at)}</span>
                    </div>
                    <p className="mt-1 whitespace-pre-wrap text-[12px] leading-relaxed text-foreground">
                      {entry.body}
                    </p>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <div className="mb-3 mt-5 flex items-center justify-between gap-3">
            <div>
              <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Wave canvas</p>
              <p className="mt-1 text-[11px] text-muted-foreground">
                Wave rows are append-only history; Host plan revisions are recorded as Mission Log entries (ADR 0051).
              </p>
            </div>
            <Badge tone="muted">Read-only</Badge>
          </div>

          {waves.length === 0 ? (
            <EmptyState
              icon={Waves}
              title="No historical Waves"
              description="This Mission has no Wave rows from before the Mission Log cutover."
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
        <ContextRail quiet label="Mission facts" className="h-fit" contentClassName="flex flex-col space-y-0">
          <ContextModule className="order-5 xl:order-1" title="Mission brief" kicker="Durable intent" icon={<Flag className="size-3.5" />}>
            <dl className="space-y-2 text-[11px] leading-relaxed">
              <ContextFact label="Objective" value={mission.objective} />
              <ContextFact label="Desired" value={mission.desired_outcome || "Not declared"} />
              <ContextFact label="Team" value={linkedMissionTeams[0]?.name ?? linkedMissionTeams[0]?.id ?? "Not created"} />
              {mission.outcome_summary && <ContextFact label="Closeout" value={mission.outcome_summary} />}
              <ContextFact label="Updated" value={fmt(mission.updated_at ?? mission.created_at)} />
            </dl>
          </ContextModule>

          {(pendingMembers.length > 0 || blockedWork || reviewWorkCount > 0 || gateNeedsReview || selectedWave?.gate_status === "blocked") && (
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
                {blockedWork ? (
                  <div className="space-y-1">
                    <p className="font-medium text-foreground">{blockedMember?.name ?? blockedWork.owner_member_id ?? "Assigned member"} is blocked on {blockedWork.title}.</p>
                    <p>{blockedWork.blocker_reason ?? "Open the Work and provide unblock direction."}</p>
                  </div>
                ) : reviewWorkCount > 0 ? (
                  <p>{reviewWorkCount} Work{reviewWorkCount === 1 ? "" : "s"} await Host acceptance.</p>
                ) : pendingMembers.length > 0 ? (
                  <p>{pendingMembers.length} member{pendingMembers.length === 1 ? "" : "s"} need review or a response.</p>
                ) : null}
                {blockedWork && blockedMember && blockedRun && (
                  <button
                    type="button"
                    onClick={() =>
                      onSelectionChange({
                        surface: "team",
                        teamId: blockedRun.id,
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

          {linkedMissionTeams.length > 0 && (
            <ContextModule
              className="order-5 xl:order-5"
              title="Mission Agent Teams"
              kicker="Independent relation"
              icon={<Users className="size-3.5" />}
              tone={latestMissionRun ? waveTone(latestMissionRun.status) : "idle"}
              live={latestMissionRun?.status === "running"}
            >
              <div className="space-y-2">
                {linkedTeamSummaries.map(({ teamId, team, latestRun, members, attemptCount }) => (
                  <button
                    key={teamId}
                    type="button"
                    disabled={!latestRun}
                    onClick={() => latestRun && onSelectionChange({
                      surface: "team",
                      teamId: latestRun.id,
                      missionId: mission.id,
                      waveId: selectedWave?.id,
                    })}
                    className="w-full rounded-lg border border-border/70 bg-background/70 p-2.5 text-left transition-colors enabled:hover:border-primary/30 enabled:hover:bg-primary/[0.035] disabled:cursor-default"
                  >
                    <span className="flex items-start justify-between gap-2">
                      <span className="min-w-0">
                        <span className="block truncate text-[11px] font-semibold text-foreground">{team?.name ?? teamId}</span>
                        <span className="mt-0.5 block text-[10px] leading-relaxed text-muted-foreground">
                          {team?.host_agent_id ?? "Host Agent unavailable"}
                          {" · "}{members.length} member{members.length === 1 ? "" : "s"}
                          {" · "}{attemptCount} attempt{attemptCount === 1 ? "" : "s"}
                        </span>
                      </span>
                      <Badge tone={latestRun ? waveTone(latestRun.status) : "idle"}>{latestRun?.status ?? "not started"}</Badge>
                    </span>
                    <span className="mt-2 block line-clamp-2 text-[10px] leading-relaxed text-muted-foreground">
                      {latestRun?.objective ?? "Mission-owned Team; no TeamRun has started yet."}
                    </span>
                  </button>
                ))}
                <ContextFact label="Lifetime" value="Continues across Waves" />
              </div>
              <p className="mt-2 text-[10px] leading-relaxed text-muted-foreground">
                The current Host is Team Lead; it is not counted as a MemberRun unless explicitly added to execute a lane.
              </p>
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

      <MissionLogDialog
        open={logOpen}
        missionId={mission.id}
        initialKind="judgment"
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setLogOpen(false)}
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
      <EditContextDialog
        open={editContextOpen}
        mission={mission}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setEditContextOpen(false)}
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
  const [logOpen, setLogOpen] = useState(false);
  const [logKind, setLogKind] = useState<MissionLogEntryKind>("judgment");
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
            <span className="flex items-center gap-2">
              <Badge tone="muted">r{wave.revision ?? 0}</Badge>
              <ActionButton
                enabled={actionsEnabled}
                disabled={wave.status === "completed"}
                size="sm"
                variant="secondary"
                onClick={() => {
                  setLogKind("replan");
                  setLogOpen(true);
                }}
              >
                <PencilLine className="size-3.5" /> Update plan
              </ActionButton>
            </span>
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
          <div className="mt-2 space-y-2">
            <p className="rounded-md border border-border bg-muted/35 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
              Host decisions are recorded as Mission Log entries (ADR 0051); the Wave gate/advance write routes are retired.
            </p>
            <ActionButton
              enabled={actionsEnabled}
              size="sm"
              variant="secondary"
              onClick={() => {
                setLogKind("judgment");
                setLogOpen(true);
              }}
            >
              <PencilLine className="size-3.5" /> Record judgment
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
      <MissionLogDialog
        open={logOpen}
        missionId={wave.mission_id}
        initialKind={logKind}
        actionsEnabled={actionsEnabled}
        onAction={onAction}
        onClose={() => setLogOpen(false)}
      />
    </section>
  );
}

/**
 * Append one Mission Log entry (ADR 0051) — the replacement for the retired
 * Wave write routes. Plan revisions post kind `replan`; gate/advance decisions
 * post kind `judgment`. Entries are append-only and never advance a Wave.
 */
function EditContextDialog({
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
  const [context, setContext] = useState("");

  useEffect(() => {
    if (open) setContext(mission.context ?? "");
  }, [open, mission.context]);

  const submit = () => {
    dispatch(onAction, updateMissionContext(mission.id, context));
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="Edit Mission context"
      description="The durable brief every Host Wave and linked team reads. Rewriting it does not rewrite history."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Context" hint="Markdown. Keep it durable: intent, constraints, and decision boundaries.">
          {(id) => <TextArea id={id} value={context} onChange={(event) => setContext(event.target.value)} rows={10} />}
        </Field>
        <DialogFooter
          submitLabel="Save context"
          actionsEnabled={actionsEnabled}
          canSubmit
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
}

function MissionLogDialog({
  open,
  missionId,
  initialKind,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  missionId: string;
  initialKind: MissionLogEntryKind;
  actionsEnabled: boolean;
  onAction: MissionsProps["onAction"];
  onClose: () => void;
}) {
  const [kind, setKind] = useState<MissionLogEntryKind>(initialKind);
  const [body, setBody] = useState("");

  useEffect(() => {
    if (open) {
      setKind(initialKind);
      setBody("");
    }
  }, [open, initialKind]);

  const valid = Boolean(body.trim());
  const submit = () => {
    if (!valid) return;
    dispatch(
      onAction,
      appendMissionLog({
        missionId,
        kind,
        body: body.trim(),
        actor: "operator",
      }),
    );
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="Append Mission Log entry"
      description="Append-only Host judgment, newest first. Recording an entry never advances a Wave."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Kind" required>
          {(id) => (
            <Select
              id={id}
              value={kind}
              onChange={(event) => setKind(event.target.value as MissionLogEntryKind)}
            >
              <option value="judgment">Judgment · Host decision or advance rationale</option>
              <option value="replan">Replan · revised plan for the current Wave</option>
              <option value="recovery">Recovery · how a blocker or failure was handled</option>
              <option value="closeout_evidence">Closeout evidence · acceptance support</option>
            </Select>
          )}
        </Field>
        <Field
          label="Entry"
          required
          hint="Markdown: current judgment, member responsibilities, carry-over, blockers, and the next decision."
        >
          {(id) => (
            <TextArea
              id={id}
              value={body}
              onChange={(event) => setBody(event.target.value)}
              className="min-h-40 font-mono text-[12px]"
            />
          )}
        </Field>
        <DialogFooter
          submitLabel="Append log entry"
          actionsEnabled={actionsEnabled}
          canSubmit={valid}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
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
  const [nodeId, setNodeId] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setDescription("");
      setNodeId("");
    }
  }, [open]);

  const valid = Boolean(name.trim() && description.trim() && nodeId.trim());
  const submit = () => {
    if (!valid) return;
    dispatch(onAction, createTeam({
      missionId: mission.id,
      name: name.trim(),
      description: description.trim(),
      hostAgentId: "host",
      nodeId: nodeId.trim(),
    }));
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="Create Mission Agent Team"
      description="Creates the Mission's one flat AgentTeam with immutable Node placement."
      onClose={onClose}
    >
      <form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}>
        <Field label="Team name" required>
          {(id) => <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} />}
        </Field>
        <Field label="Purpose" required>
          {(id) => <TextArea id={id} value={description} onChange={(event) => setDescription(event.target.value)} />}
        </Field>
        <Field label="Node ID" required hint="Stable UUID of the machine that owns this Team.">
          {(id) => <TextInput id={id} value={nodeId} onChange={(event) => setNodeId(event.target.value)} />}
        </Field>
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
          executionMode: member.executionMode as TeamRunMemberSpec["executionMode"],
          model: member.model.trim() || undefined,
          effort: member.effort.trim() || undefined,
          serviceTier: member.serviceTier.trim() || undefined,
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
                      const entry = TEAM_MEMBER_PROVIDER_MODES.find(
                        (candidate) => candidate.provider === event.target.value,
                      );
                      if (!entry) return;
                      updateMember(index, {
                        provider: entry.provider,
                        executionMode: entry.mode,
                        model: entry.provider === "kimi" ? "kimi-code/k3" : "",
                        serviceTier: entry.provider === "codex" ? member.serviceTier : "",
                      });
                    }}
                  >
                    {TEAM_MEMBER_PROVIDER_MODES.map((entry) => (
                      <option key={entry.provider} value={entry.provider}>{entry.label}</option>
                    ))}
                  </Select>
                )}
              </Field>
              <Field
                label="Execution mode"
                hint={EXECUTION_MODE_HINTS[member.executionMode]
                  ?? "Registered persistent bidirectional mode, auto-filled from the provider."}
              >
                {(id) => (
                  // One registered mode per provider: changing the provider
                  // above is the only mode change this form allows.
                  <Select
                    id={id}
                    value={member.executionMode}
                    onChange={() => undefined}
                  >
                    <option value={member.executionMode}>{member.executionMode}</option>
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
              <Field label="Reasoning effort" hint="Provider-neutral intent. The run records the effective receipt separately.">
                {(id) => (
                  <TextInput
                    id={id}
                    value={member.effort}
                    onChange={(event) => updateMember(index, { effort: event.target.value })}
                    placeholder="low, medium, high, max"
                  />
                )}
              </Field>
              <Field label="Service / latency tier" hint="Optional. Unsupported providers report unsupported; this is not a universal fast switch.">
                {(id) => (
                  <TextInput
                    id={id}
                    value={member.serviceTier}
                    onChange={(event) => updateMember(index, { serviceTier: event.target.value })}
                    placeholder={member.provider === "codex" ? "priority" : "Not reviewed for this mode"}
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

function ActionButton({
  enabled,
  children,
  ...props
}: ComponentProps<typeof Button> & { enabled: boolean }) {
  const disabled = !enabled || props.disabled;
  return (
    <Button
      {...props}
      disabled={disabled}
      title={disabled ? props.title ?? "Connect a live source to enable these actions" : props.title}
    >
      {children}
    </Button>
  );
}
