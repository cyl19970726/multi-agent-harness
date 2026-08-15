import { useEffect, useState, type ComponentProps, type ReactNode } from "react";
import { AlertTriangle, CheckCircle2, ChevronLeft, ChevronRight, FileClock, Flag, History, PencilLine, Play, Plus, Users } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DocSection, DocumentSurface, EmptyState, MonoId, StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { Dialog, DialogFooter, Field, Select, TextArea, TextInput } from "@/components/workbench/OperatorForms";
import { appendMissionLog, closeMission, createMission, createTeam, createTeamRun, updateMissionContext, type ActionDescriptor } from "../api/actions";
import type { SelectionState } from "../app/selection";
import type { WorkbenchModel } from "../model/readModel";
import type { AgentTeam, Mission, MissionLogEntry, MissionLogEntryKind, TeamRun, LegacyWave, Work } from "../types";

interface MissionsProps {
  model: WorkbenchModel;
  missionId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void;
}

function dispatch(onAction: MissionsProps["onAction"], descriptor: ActionDescriptor): void {
  onAction?.(descriptor.path, descriptor.body);
}

function missionTone(status?: string | null): StatusTone {
  if (status === "running") return "running";
  if (status === "completed") return "good";
  if (status === "blocked") return "bad";
  if (status === "planned") return "info";
  return "idle";
}

function runTone(status?: string | null): StatusTone {
  if (status === "running") return "running";
  if (status === "completed") return "good";
  if (["failed", "cancelled"].includes(status ?? "")) return "bad";
  if (["waiting", "reviewing", "disconnected"].includes(status ?? "")) return "warn";
  if (status === "planning") return "info";
  return "idle";
}

function fmt(value?: string | null): string {
  if (!value) return "—";
  const epoch = value.startsWith("unix-ms:") ? Number(value.slice(8)) : Date.parse(value);
  return Number.isFinite(epoch) ? new Date(epoch).toLocaleString() : value;
}

function missionLogFor(model: WorkbenchModel, missionId: string): MissionLogEntry[] {
  return [...(model.snapshot.mission_log ?? [])]
    .filter((entry) => entry.mission_id === missionId)
    .sort((left, right) => right.revision - left.revision);
}

function teamsFor(model: WorkbenchModel, missionId: string): AgentTeam[] {
  return (model.snapshot.teams ?? []).filter((team) => team.mission_id === missionId);
}

function runsFor(model: WorkbenchModel, teams: AgentTeam[]): TeamRun[] {
  const teamIds = new Set(teams.map((team) => team.id));
  return [...(model.snapshot.team_runs ?? [])]
    .filter((run) => teamIds.has(run.agent_team_id))
    .sort((left, right) => (right.created_at ?? "").localeCompare(left.created_at ?? ""));
}

function worksFor(model: WorkbenchModel, runs: TeamRun[]): Work[] {
  const runIds = new Set(runs.map((run) => run.id));
  return (model.snapshot.works ?? []).filter((work) => runIds.has(work.team_run_id));
}

function legacyWavesFor(model: WorkbenchModel, missionId: string): LegacyWave[] {
  return [...(model.snapshot.legacy_waves ?? [])]
    .filter((wave) => wave.mission_id === missionId)
    .sort((left, right) => left.index - right.index);
}

function MarkdownContext({ value, empty }: { value?: string | null; empty: string }) {
  if (!value?.trim()) return <p className="text-[12px] leading-relaxed text-muted-foreground">{empty}</p>;
  const content: ReactNode[] = [];
  value.split("\n").forEach((line, index) => {
    if (line.startsWith("### ")) content.push(<h4 key={index} className="pt-1 text-[11px] font-semibold uppercase tracking-wider">{line.slice(4)}</h4>);
    else if (line.startsWith("## ")) content.push(<h3 key={index} className="pt-1 text-sm font-semibold">{line.slice(3)}</h3>);
    else if (line.startsWith("# ")) content.push(<h2 key={index} className="text-base font-semibold tracking-tight">{line.slice(2)}</h2>);
    else if (/^[-*] /.test(line)) content.push(<p key={index} className="pl-3 before:mr-2 before:text-primary before:content-['•']">{line.slice(2)}</p>);
    else if (line.trim()) content.push(<p key={index} className="whitespace-pre-wrap">{line}</p>);
    else content.push(<span key={index} className="block h-1" aria-hidden />);
  });
  return <div className="space-y-2 text-[12px] leading-relaxed text-foreground">{content}</div>;
}

export function MissionsSurface({ model, missionId, onSelectionChange, actionsEnabled = false, onAction }: MissionsProps) {
  const [createOpen, setCreateOpen] = useState(false);
  const missions = [...(model.snapshot.missions ?? [])].sort((left, right) =>
    (right.updated_at ?? right.created_at ?? "").localeCompare(left.updated_at ?? left.created_at ?? ""),
  );
  const selected = missions.find((mission) => mission.id === missionId);
  if (selected) {
    return <MissionDetail model={model} mission={selected} onSelectionChange={onSelectionChange} actionsEnabled={actionsEnabled} onAction={onAction} />;
  }

  return (
    <DocumentSurface className="max-w-[1180px]">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div className="space-y-1">
          <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground"><Flag className="size-3.5" /> Native control plane</div>
          <h1 className="text-2xl font-semibold tracking-tight">Missions</h1>
          <p className="text-sm text-muted-foreground">Durable intent with one flat AgentTeam and an append-only Mission Log.</p>
        </div>
        <ActionButton enabled={actionsEnabled} onClick={() => setCreateOpen(true)}><Plus className="size-3.5" /> New Mission</ActionButton>
      </header>
      <DocSection label={`${missions.length} ${missions.length === 1 ? "mission" : "missions"}`}>
        {missions.length === 0 ? (
          <EmptyState icon={Flag} title="No native Missions yet" description="Create durable intent, bind its one AgentTeam, then record Host judgments in the Mission Log." />
        ) : (
          <div className="overflow-hidden rounded-lg border border-border bg-card">
            {missions.map((mission) => {
              const teams = teamsFor(model, mission.id);
              const logs = missionLogFor(model, mission.id);
              const runs = runsFor(model, teams);
              return (
                <button key={mission.id} type="button" onClick={() => onSelectionChange({ surface: "missions", missionId: mission.id })} className="flex w-full items-center gap-3 border-b border-border/60 px-4 py-3 text-left last:border-b-0 hover:bg-accent/40">
                  <StatusDot tone={missionTone(mission.status)} />
                  <span className="min-w-0 flex-1"><span className="block truncate text-[14px] font-medium">{mission.title}</span><span className="block truncate text-[12px] text-muted-foreground">{mission.objective}</span></span>
                  <span className="hidden items-center gap-1.5 sm:flex"><Badge tone="muted">{teams.length} team</Badge><Badge tone="muted">{runs.length} runs</Badge><Badge tone="muted">log r{logs[0]?.revision ?? 0}</Badge><Badge tone={missionTone(mission.status)}>{mission.status ?? "planned"}</Badge></span>
                  <ChevronRight className="size-4 text-muted-foreground" />
                </button>
              );
            })}
          </div>
        )}
      </DocSection>
      <MissionDialog open={createOpen} actionsEnabled={actionsEnabled} onAction={onAction} onClose={() => setCreateOpen(false)} />
    </DocumentSurface>
  );
}

function MissionDetail({ model, mission, onSelectionChange, actionsEnabled = false, onAction }: MissionsProps & { mission: Mission }) {
  const [logOpen, setLogOpen] = useState(false);
  const [teamOpen, setTeamOpen] = useState(false);
  const [runTeam, setRunTeam] = useState<AgentTeam>();
  const [closeOpen, setCloseOpen] = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  const teams = teamsFor(model, mission.id);
  const runs = runsFor(model, teams);
  const works = worksFor(model, runs);
  const missionLog = missionLogFor(model, mission.id);
  const legacyWaves = legacyWavesFor(model, mission.id);
  const activeWorks = works.filter((work) => work.phase !== "closed");
  const blockedWorks = works.filter((work) => work.condition === "blocked");

  return (
    <DocumentSurface className="h-full max-w-[1280px] overflow-y-auto px-3 py-5 sm:px-5 xl:px-0" data-mission-scroll-owner="true">
      <button type="button" onClick={() => onSelectionChange({ surface: "missions", missionId: undefined })} className="inline-flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground hover:text-foreground"><ChevronLeft className="size-3.5" /> Missions</button>
      <header className="mt-3 flex flex-wrap items-start justify-between gap-3 border-b border-border/70 pb-5">
        <div className="min-w-0 flex-1 space-y-1.5">
          <h1 className="text-2xl font-semibold tracking-[-0.025em]">{mission.title}</h1>
          <p className="max-w-3xl text-[13px] leading-relaxed text-muted-foreground">{mission.objective}</p>
          <div className="flex flex-wrap gap-1.5"><Badge tone={missionTone(mission.status)}>{mission.status ?? "planned"}</Badge><Badge tone="muted">{teams.length} owning team</Badge><Badge tone="muted">{runs.length} runs</Badge><Badge tone={blockedWorks.length ? "bad" : "muted"}>{activeWorks.length} active work</Badge><Badge tone="muted">Mission Log r{missionLog[0]?.revision ?? 0}</Badge></div>
        </div>
        <div className="flex flex-wrap gap-2">
          <ActionButton enabled={actionsEnabled} disabled={["completed", "cancelled"].includes(mission.status ?? "")} onClick={() => setLogOpen(true)}><Plus className="size-3.5" /> Append judgment</ActionButton>
          {teams.length === 0 && <ActionButton enabled={actionsEnabled} variant="secondary" onClick={() => setTeamOpen(true)}><Users className="size-3.5" /> Create AgentTeam</ActionButton>}
          {mission.status !== "completed" && <ActionButton enabled={actionsEnabled} variant="secondary" onClick={() => setCloseOpen(true)}><CheckCircle2 className="size-3.5" /> Close Mission</ActionButton>}
        </div>
      </header>

      {teams.length > 1 && <div role="alert" className="mt-4 flex gap-2 rounded-lg border border-status-bad/35 bg-status-bad/10 p-3 text-xs text-status-bad"><AlertTriangle className="mt-0.5 size-4 shrink-0" />Integrity anomaly: this Mission resolves to {teams.length} AgentTeams. Current product identity requires exactly one.</div>}

      <div className="grid gap-5 py-5 xl:grid-cols-[minmax(0,1.5fr)_minmax(18rem,.75fr)]">
        <div className="space-y-5">
          <section className="rounded-lg border border-border bg-card p-4">
            <div className="flex items-center justify-between gap-2"><h2 className="text-sm font-semibold">Mission context</h2><ActionButton enabled={actionsEnabled} size="sm" variant="ghost" onClick={() => setContextOpen(true)}><PencilLine className="size-3.5" /> Edit</ActionButton></div>
            <div className="mt-3 border-t border-border/60 pt-3"><MarkdownContext value={mission.context} empty="No durable Mission context has been recorded." /></div>
            {mission.desired_outcome && <p className="mt-3 border-t border-border/60 pt-3 text-[12px]"><span className="font-semibold">Desired outcome:</span> {mission.desired_outcome}</p>}
          </section>

          <section className="rounded-lg border border-border bg-card p-4">
            <h2 className="text-sm font-semibold">AgentTeam and runs</h2>
            <p className="mt-1 text-[11px] text-muted-foreground">The Team belongs to this Mission; each TeamRun is created from the Team identity.</p>
            {teams.length === 0 ? <div className="mt-3"><EmptyState icon={Users} title="No AgentTeam" description="Bind this Mission to its one flat AgentTeam before starting a run." /></div> : (
              <div className="mt-3 space-y-3">{teams.map((team) => {
                const teamRuns = runs.filter((run) => run.agent_team_id === team.id);
                return <div key={team.id} className="rounded-lg border border-border/70 bg-background/60 p-3">
                  <div className="flex flex-wrap items-start justify-between gap-2"><div><p className="text-[13px] font-semibold">{team.name ?? team.id}</p><p className="mt-1 text-[11px] text-muted-foreground">{team.description ?? "No team description"}</p><div className="mt-2 flex flex-wrap gap-1.5"><Badge tone="muted">host {team.host_agent_id}</Badge><Badge tone="muted">node {team.node_id}</Badge><Badge tone="muted">{team.member_ids?.length ?? 0} members</Badge><Badge tone="muted">{teamRuns.length} runs</Badge></div></div><ActionButton enabled={actionsEnabled} size="sm" onClick={() => setRunTeam(team)}><Play className="size-3.5" /> New run</ActionButton></div>
                  {teamRuns.length > 0 && <div className="mt-3 divide-y divide-border/50 border-t border-border/60">{teamRuns.map((run) => <button key={run.id} type="button" onClick={() => onSelectionChange({ surface: "team", teamId: run.id, missionId: mission.id })} className="flex w-full items-center gap-2 py-2 text-left hover:text-primary"><StatusDot tone={runTone(run.status)} /><span className="min-w-0 flex-1 truncate text-[12px]">{run.objective || run.id}</span><Badge tone={runTone(run.status)}>{run.status ?? "planning"}</Badge><ChevronRight className="size-3.5 text-muted-foreground" /></button>)}</div>}
                </div>;
              })}</div>
            )}
          </section>

          <section className="rounded-lg border border-border bg-card p-4">
            <h2 className="text-sm font-semibold">Mission work</h2><p className="mt-1 text-[11px] text-muted-foreground">Work remains authoritative on its owning TeamRun.</p>
            {works.length === 0 ? <p className="mt-3 text-[12px] text-muted-foreground">No Work has been created on this Mission's TeamRuns.</p> : <div className="mt-3 divide-y divide-border/50 border-t border-border/60">{works.map((work) => <div key={work.id} className="flex items-center gap-2 py-2"><StatusDot tone={work.condition === "blocked" ? "bad" : work.phase === "closed" ? "good" : work.phase === "active" ? "running" : "idle"} /><span className="min-w-0 flex-1 truncate text-[12px]">{work.title}</span><Badge tone={work.condition === "blocked" ? "bad" : "muted"}>{work.condition === "normal" ? work.phase : work.condition}</Badge></div>)}</div>}
          </section>
        </div>

        <aside className="space-y-5">
          <section className="rounded-lg border border-border bg-card p-4">
            <div className="flex items-center justify-between gap-2"><h2 className="flex items-center gap-2 text-sm font-semibold"><FileClock className="size-4" /> Mission Log</h2><Badge tone="muted">append-only</Badge></div>
            {missionLog.length === 0 ? <p className="mt-3 text-[12px] leading-relaxed text-muted-foreground">No Host judgment has been recorded yet.</p> : <ol className="mt-3 space-y-3">{missionLog.map((entry) => <li key={entry.id} className="border-l border-border pl-3"><div className="flex flex-wrap items-center gap-1.5"><Badge tone="muted">r{entry.revision}</Badge><Badge tone="info">{entry.kind}</Badge><span className="text-[10px] text-muted-foreground">{fmt(entry.created_at)}</span></div><p className="mt-2 whitespace-pre-wrap text-[11px] leading-relaxed">{entry.body}</p><p className="mt-1 text-[10px] text-muted-foreground">{entry.actor}</p></li>)}</ol>}
          </section>
          <section className="rounded-lg border border-border bg-card p-4"><h2 className="text-sm font-semibold">Mission facts</h2><dl className="mt-3 space-y-2 text-[11px]"><Fact label="Mission ID"><MonoId>{mission.id}</MonoId></Fact><Fact label="Created">{fmt(mission.created_at)}</Fact><Fact label="Updated">{fmt(mission.updated_at)}</Fact><Fact label="Completed">{fmt(mission.completed_at)}</Fact></dl></section>
          {legacyWaves.length > 0 && <details className="rounded-lg border border-border bg-muted/20 p-4" data-legacy-wave-history="true"><summary className="flex cursor-pointer list-none items-center gap-2 text-[12px] font-semibold"><History className="size-4" /> Legacy Wave history <Badge tone="muted">read-only · {legacyWaves.length}</Badge></summary><p className="mt-2 text-[10px] leading-relaxed text-muted-foreground">ADR 0051 pre-cutover rows are preserved for historical reading only. They do not control Mission status, closeout, TeamRun creation, or navigation.</p><ol className="mt-3 space-y-2 border-t border-border/60 pt-3">{legacyWaves.map((wave) => <li key={wave.id} className="rounded-md border border-border/60 bg-background/50 p-2.5"><div className="flex items-center justify-between gap-2"><span className="text-[11px] font-medium">Legacy Wave {wave.index} · {wave.title}</span><Badge tone="muted">{wave.status ?? "historical"}</Badge></div><p className="mt-1 text-[10px] leading-relaxed text-muted-foreground">{wave.objective}</p></li>)}</ol></details>}
        </aside>
      </div>

      <EditContextDialog open={contextOpen} mission={mission} actionsEnabled={actionsEnabled} onAction={onAction} onClose={() => setContextOpen(false)} />
      <MissionLogDialog open={logOpen} missionId={mission.id} actionsEnabled={actionsEnabled} onAction={onAction} onClose={() => setLogOpen(false)} />
      <MissionTeamDialog open={teamOpen} mission={mission} actionsEnabled={actionsEnabled} onAction={onAction} onClose={() => setTeamOpen(false)} />
      {runTeam && <MissionRunDialog team={runTeam} mission={mission} actionsEnabled={actionsEnabled} onAction={onAction} onClose={() => setRunTeam(undefined)} />}
      <MissionCloseDialog open={closeOpen} mission={mission} actionsEnabled={actionsEnabled} onAction={onAction} onClose={() => setCloseOpen(false)} />
    </DocumentSurface>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return <div className="flex items-center justify-between gap-3"><dt className="text-muted-foreground">{label}</dt><dd className="min-w-0 text-right">{children}</dd></div>;
}

function EditContextDialog({ open, mission, actionsEnabled, onAction, onClose }: { open: boolean; mission: Mission; actionsEnabled: boolean; onAction: MissionsProps["onAction"]; onClose: () => void }) {
  const [context, setContext] = useState("");
  useEffect(() => { if (open) setContext(mission.context ?? ""); }, [open, mission.context]);
  const submit = () => { dispatch(onAction, updateMissionContext(mission.id, context)); onClose(); };
  return <Dialog open={open} title="Edit Mission context" description="Update the durable Mission brief. Historical Host judgments stay in the Mission Log." onClose={onClose}><form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}><Field label="Context" hint="Markdown: intent, constraints, and decision boundaries.">{(id) => <TextArea id={id} value={context} onChange={(event) => setContext(event.target.value)} rows={10} />}</Field><DialogFooter submitLabel="Save context" actionsEnabled={actionsEnabled} canSubmit onCancel={onClose} onSubmit={submit} /></form></Dialog>;
}

function MissionLogDialog({ open, missionId, actionsEnabled, onAction, onClose }: { open: boolean; missionId: string; actionsEnabled: boolean; onAction: MissionsProps["onAction"]; onClose: () => void }) {
  const [kind, setKind] = useState<MissionLogEntryKind>("judgment");
  const [body, setBody] = useState("");
  useEffect(() => { if (open) { setKind("judgment"); setBody(""); } }, [open]);
  const valid = Boolean(body.trim());
  const submit = () => { if (!valid) return; dispatch(onAction, appendMissionLog({ missionId, kind, body: body.trim(), actor: "operator" })); onClose(); };
  return <Dialog open={open} title="Append Mission Log entry" description="Record one immutable Host judgment, replan, recovery, or closeout-evidence entry." onClose={onClose}><form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}><Field label="Kind" required>{(id) => <Select id={id} value={kind} onChange={(event) => setKind(event.target.value as MissionLogEntryKind)}><option value="judgment">Judgment</option><option value="replan">Replan</option><option value="recovery">Recovery</option><option value="closeout_evidence">Closeout evidence</option></Select>}</Field><Field label="Entry" required hint="Markdown: judgment, responsibilities, carry-over, blockers, and next decision.">{(id) => <TextArea id={id} value={body} onChange={(event) => setBody(event.target.value)} className="min-h-40 font-mono text-[12px]" />}</Field><DialogFooter submitLabel="Append log entry" actionsEnabled={actionsEnabled} canSubmit={valid} onCancel={onClose} onSubmit={submit} /></form></Dialog>;
}

function MissionDialog({ open, actionsEnabled, onAction, onClose }: Pick<MissionsProps, "actionsEnabled" | "onAction"> & { open: boolean; onClose: () => void }) {
  const [title, setTitle] = useState(""); const [objective, setObjective] = useState(""); const [outcome, setOutcome] = useState(""); const [context, setContext] = useState("");
  useEffect(() => { if (open) { setTitle(""); setObjective(""); setOutcome(""); setContext(""); } }, [open]);
  const valid = Boolean(title.trim() && objective.trim());
  const submit = () => { if (!valid) return; dispatch(onAction, createMission({ title: title.trim(), objective: objective.trim(), desiredOutcome: outcome.trim() || undefined, context: context.trim() || undefined })); onClose(); };
  return <Dialog open={open} title="New Mission" description="Create durable intent. Bind its one AgentTeam separately and record plan changes in the Mission Log." onClose={onClose}><form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}><Field label="Title" required>{(id) => <TextInput id={id} value={title} onChange={(event) => setTitle(event.target.value)} />}</Field><Field label="Objective" required>{(id) => <TextArea id={id} value={objective} onChange={(event) => setObjective(event.target.value)} />}</Field><Field label="Desired outcome">{(id) => <TextArea id={id} value={outcome} onChange={(event) => setOutcome(event.target.value)} />}</Field><Field label="Mission context" hint="Markdown shared by the Mission's Team and runs.">{(id) => <TextArea id={id} value={context} onChange={(event) => setContext(event.target.value)} />}</Field><DialogFooter submitLabel="Create Mission" actionsEnabled={Boolean(actionsEnabled)} canSubmit={valid} onCancel={onClose} onSubmit={submit} /></form></Dialog>;
}

function MissionTeamDialog({ open, mission, actionsEnabled, onAction, onClose }: { open: boolean; mission: Mission; actionsEnabled: boolean; onAction: MissionsProps["onAction"]; onClose: () => void }) {
  const [name, setName] = useState(""); const [description, setDescription] = useState(""); const [nodeId, setNodeId] = useState("");
  useEffect(() => { if (open) { setName(""); setDescription(""); setNodeId(""); } }, [open]);
  const valid = Boolean(name.trim() && description.trim() && nodeId.trim());
  const submit = () => { if (!valid) return; dispatch(onAction, createTeam({ missionId: mission.id, name: name.trim(), description: description.trim(), hostAgentId: "host", nodeId: nodeId.trim() })); onClose(); };
  return <Dialog open={open} title="Create Mission AgentTeam" description="Create the Mission's one flat AgentTeam with immutable Node placement." onClose={onClose}><form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}><Field label="Team name" required>{(id) => <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} />}</Field><Field label="Purpose" required>{(id) => <TextArea id={id} value={description} onChange={(event) => setDescription(event.target.value)} />}</Field><Field label="Node ID" required hint="Stable UUID of the machine that owns this Team.">{(id) => <TextInput id={id} value={nodeId} onChange={(event) => setNodeId(event.target.value)} />}</Field><DialogFooter submitLabel="Create team" actionsEnabled={actionsEnabled} canSubmit={valid} onCancel={onClose} onSubmit={submit} /></form></Dialog>;
}

function MissionRunDialog({ team, mission, actionsEnabled, onAction, onClose }: { team: AgentTeam; mission: Mission; actionsEnabled: boolean; onAction: MissionsProps["onAction"]; onClose: () => void }) {
  const [objective, setObjective] = useState(""); const [executionRoot, setExecutionRoot] = useState(""); const [budget, setBudget] = useState("");
  useEffect(() => { setObjective(mission.objective); setExecutionRoot(""); setBudget(""); }, [mission.objective, team.id]);
  const valid = Boolean(objective.trim());
  const submit = () => { if (!valid) return; const budgetValue = Number(budget); dispatch(onAction, createTeamRun({ objective: objective.trim(), agentTeamId: team.id, executionRoot: executionRoot.trim() || undefined, budgetLimitUsd: Number.isFinite(budgetValue) && budgetValue > 0 ? budgetValue : undefined, members: [] })); onClose(); };
  return <Dialog open title={`New run · ${team.name ?? team.id}`} description="Create one TeamRun from this AgentTeam. Mission identity is inherited from the Team; no Legacy Wave or Mission field is sent." onClose={onClose}><form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}><Field label="Objective" required>{(id) => <TextArea id={id} value={objective} onChange={(event) => setObjective(event.target.value)} />}</Field><Field label="Execution root">{(id) => <TextInput id={id} value={executionRoot} onChange={(event) => setExecutionRoot(event.target.value)} />}</Field><Field label="Budget (USD)">{(id) => <TextInput id={id} type="number" min="0" step="0.01" value={budget} onChange={(event) => setBudget(event.target.value)} />}</Field><DialogFooter submitLabel="Create run" actionsEnabled={actionsEnabled} canSubmit={valid} onCancel={onClose} onSubmit={submit} /></form></Dialog>;
}

function MissionCloseDialog({ open, mission, actionsEnabled, onAction, onClose }: { open: boolean; mission: Mission; actionsEnabled: boolean; onAction: MissionsProps["onAction"]; onClose: () => void }) {
  const [outcome, setOutcome] = useState(""); const [completedBy, setCompletedBy] = useState("host");
  useEffect(() => { if (open) { setOutcome(mission.outcome_summary ?? ""); setCompletedBy(mission.completed_by ?? "host"); } }, [mission.completed_by, mission.outcome_summary, open]);
  const valid = Boolean(outcome.trim() && completedBy.trim());
  const submit = () => { if (!valid) return; dispatch(onAction, closeMission({ missionId: mission.id, outcome: outcome.trim(), completedBy: completedBy.trim() })); onClose(); };
  return <Dialog open={open} title="Close Mission" description="Record the durable Mission outcome. Closeout is explicit and does not depend on Legacy Wave rows." onClose={onClose}><form className="space-y-3" onSubmit={(event) => { event.preventDefault(); submit(); }}><Field label="Mission outcome" required>{(id) => <TextArea id={id} value={outcome} onChange={(event) => setOutcome(event.target.value)} />}</Field><Field label="Completed by" required>{(id) => <TextInput id={id} value={completedBy} onChange={(event) => setCompletedBy(event.target.value)} />}</Field><DialogFooter submitLabel="Complete Mission" actionsEnabled={actionsEnabled} canSubmit={valid} onCancel={onClose} onSubmit={submit} /></form></Dialog>;
}

function ActionButton({ enabled, children, ...props }: ComponentProps<typeof Button> & { enabled: boolean }) {
  const disabled = !enabled || props.disabled;
  return <Button {...props} disabled={disabled} title={disabled ? props.title ?? "Connect a live source to enable these actions" : props.title}>{children}</Button>;
}
