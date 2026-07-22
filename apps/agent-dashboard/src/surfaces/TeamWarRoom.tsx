import { useEffect, useState } from "react";
import {
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  ListFilter,
  MessageSquare,
  Play,
  Send,
  ShieldCheck,
  SquareArrowOutUpRight,
  Users,
  X,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import { ActivityStream, type WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";
import { ContextModule, ContextRail } from "@/components/workbench/context/ContextRail";
import { ReadinessMeter } from "@/components/workbench/execution/ExecutionPrimitives";
import { FocusHeader, FocusShell } from "@/components/workbench/layout/FocusShell";
import { EmptyState, StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { Select, TextArea } from "@/components/workbench/OperatorForms";

import {
  selectMemberAssignmentCorrelations,
  selectTeamRunContext,
  type StableTeamActivity,
} from "../model/teamSelectors";
import type { WorkbenchModel } from "../model/readModel";
import { acknowledgeTeamMessage, resolvePendingInteraction, sendTeamMessage, startTeamRun, transitionTeamRun } from "../api/actions";
import type { MemberRun, PendingInteraction, TeamMessage, Wave } from "../types";
import type { SelectionState } from "../app/selection";

export interface TeamWarRoomProps {
  model: WorkbenchModel;
  teamRunId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void | Promise<boolean>;
}

type StreamFilter = "all" | "messages" | "actions" | "decisions" | "evidence";
type ComposerTarget = "team" | string;

const FILTERS: Array<{ id: StreamFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "messages", label: "Messages" },
  { id: "actions", label: "Actions" },
  { id: "decisions", label: "Decisions" },
  { id: "evidence", label: "Evidence" },
];

/**
 * One operational view of one AgentTeamRun attempt.  Its durable input is the
 * native Mission → Wave → TeamRun hierarchy; it deliberately has no inferred dependency graph
 * projection and keeps Wave acceptance visibly separate from attempt status.
 */
export function TeamWarRoom({
  model,
  teamRunId,
  onSelectionChange,
  actionsEnabled = false,
  onAction,
}: TeamWarRoomProps) {
  const context = selectTeamRunContext(model.snapshot, teamRunId);
  const [filter, setFilter] = useState<StreamFilter>("all");
  const [selectedMemberId, setSelectedMemberId] = useState<string | undefined>();
  const [composerTarget, setComposerTarget] = useState<ComposerTarget>("team");
  const [draft, setDraft] = useState("");
  const [kind, setKind] = useState("broadcast");
  const [showAllMembers, setShowAllMembers] = useState(false);
  const [showFullActivity, setShowFullActivity] = useState(false);
  const [starting, setStarting] = useState(false);
  const runStatus = context?.run.status;

  useEffect(() => {
    if (runStatus !== "planning") setStarting(false);
  }, [runStatus]);

  if (!context) {
    return (
      <div className="mx-auto flex min-h-72 max-w-3xl items-center px-4 py-8">
        <div className="space-y-3 text-center">
          <EmptyState
            icon={Users}
            title="Team attempt not found"
            description="This Agent Team attempt is not present in the current project snapshot."
          />
          <Button variant="secondary" size="sm" onClick={() => onSelectionChange({ surface: "team", teamId: undefined })}>
            Back to Agent Teams
          </Button>
        </div>
      </div>
    );
  }

  const { run, mission, wave, attempts, members, memberById, messages, actions, delegations, events, liveActivityByMember, needsYou } = context;
  const orderedMembers = [...members].sort(
    (left, right) => memberPressureRank(left.status) - memberPressureRank(right.status),
  );
  const selectedMember =
    memberById.get(selectedMemberId ?? "") ??
    needsYou.blockedMembers[0] ??
    needsYou.waitingMembers[0] ??
    orderedMembers[0];
  const pendingInteractions = context.interactions
    .filter((interaction) => interaction.status === "pending")
    .sort((left, right) => timestamp(left.created_at) - timestamp(right.created_at));
  const activityItems = toActivityItems(context.activity, memberById).map((item) => {
    if (!item.id.startsWith("message:")) return item;
    const message = messages.find((candidate) => `message:${candidate.id}` === item.id);
    const hostDelivery = message?.deliveries?.find(
      (delivery) => delivery.member_id === "host" && delivery.status === "delivered",
    );
    if (!message || !hostDelivery) return item;
    return {
      ...item,
      action: (
        <Button
          size="sm"
          variant="secondary"
          disabled={!actionsEnabled}
          title={actionsEnabled ? "Acknowledge delivered message" : "Connect a live source to acknowledge"}
          onClick={() => dispatch(onAction, acknowledgeTeamMessage(run.id, message.id, "host"))}
        >
          ACK
        </Button>
      ),
    };
  });
  [...pendingInteractions].reverse().forEach((interaction) => {
    activityItems.unshift(toInteractionActivity(
      interaction,
      memberById,
      actionsEnabled,
      (optionId) => dispatch(onAction, resolvePendingInteraction(
        run.id,
        interaction.id,
        optionId,
        interaction.route === "human" ? "operator" : "host",
      )),
    ));
  });
  const pressureActivityIndex = selectedMember?.status === "blocked"
    ? activityItems.map((item) => item.kind).lastIndexOf("decision")
    : -1;
  if (pressureActivityIndex >= 0) {
    activityItems[pressureActivityIndex] = {
      ...activityItems[pressureActivityIndex],
      action: (
        <div className="min-w-32 rounded-lg border border-primary/25 bg-primary/[0.055] px-2.5 py-2 text-center">
          <p className="text-[10px] font-semibold leading-snug text-primary">QA approval required</p>
          <button
            type="button"
            onClick={() => messageMember(selectedMember)}
            className="mt-1.5 rounded-md border border-primary/30 bg-background px-2.5 py-1 text-[10px] font-semibold text-primary transition-colors hover:bg-primary/10"
          >
            Review request
          </button>
        </div>
      ),
    };
  }
  const filteredActivity = activityItems.filter((item) => matchesFilter(item, filter));
  const primaryActivity = filteredActivity.filter((item) => item.prominence === "primary");
  const latestPressure = [...filteredActivity].reverse().find((item) => item.prominence === "pressure");
  const keyActivity = latestPressure && !primaryActivity.some((item) => item.id === latestPressure.id)
    ? [...primaryActivity, latestPressure]
    : primaryActivity;
  const shownActivity = filter === "all" && !showFullActivity ? keyActivity : filteredActivity;
  const selectedAssignment = selectedMember
    ? selectMemberAssignmentCorrelations(messages, selectedMember.id)[0]?.assignment
    : undefined;
  const explicitRecipients = composerTarget === "team" ? members.map((member) => member.id) : [composerTarget];
  const canSend = actionsEnabled && Boolean(draft.trim()) && explicitRecipients.length > 0;
  const status = run.status ?? "planning";

  function openMember(member: MemberRun): void {
    onSelectionChange({
      surface: "team",
      teamId: run.id,
      memberRunId: member.id,
    });
  }

  function selectMember(member: MemberRun): void {
    setSelectedMemberId(member.id);
  }

  function messageMember(member?: MemberRun): void {
    if (member) {
      setSelectedMemberId(member.id);
      setComposerTarget(member.id);
    } else {
      setComposerTarget("team");
    }
    document.getElementById("team-war-room-composer")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }

  function submit(): void {
    if (!canSend) return;
    const descriptor = sendTeamMessage(run.id, {
      fromMemberId: "host",
      toMemberIds: explicitRecipients,
      kind,
      body: draft.trim(),
    });
    onAction?.(descriptor.path, descriptor.body);
    setDraft("");
  }

  return (
    <FocusShell
      headerClassName="min-h-[118px] bg-background py-3 sm:py-3"
      composerClassName="bg-background shadow-[0_-12px_30px_-28px_rgba(15,23,42,0.55)]"
      header={
        <FocusHeader
          breadcrumb={
            <button
              type="button"
              onClick={() => onSelectionChange(
                run.mission_id && run.wave_id
                  ? { surface: "missions", missionId: run.mission_id, waveId: run.wave_id, teamId: undefined }
                  : { surface: "team", teamId: undefined },
              )}
              className="inline-flex items-center gap-1 text-muted-foreground transition-colors hover:text-foreground"
            >
              <ChevronLeft className="size-3.5" />
              {mission?.title ?? "Mission"} <span className="text-border">/</span> {wave ? `Wave ${wave.index}` : "Agent Teams"}
            </button>
          }
          title={wave ? `${wave.title} · Agent Team` : "Agent Team attempt"}
          meta={
            <>
              <Badge tone={teamTone(status)}>{status}</Badge>
              <Badge tone="muted">attempt {attemptNumber(attempts, run.id)}</Badge>
              {wave && <Badge tone={gateTone(wave.gate_status)}>Wave gate: {wave.gate_status ?? "pending"}</Badge>}
            </>
          }
          actions={
            <>
              <span className="hidden items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground sm:inline-flex">
                <Users className="size-3.5" /> {members.length} members active
              </span>
              <AttemptActions
                status={status}
                actionsEnabled={actionsEnabled}
                starting={starting}
                onStart={() => {
                  setStarting(true);
                  const result = dispatch(onAction, startTeamRun(run.id));
                  if (result instanceof Promise) {
                    void result.then((ok) => {
                      if (!ok) setStarting(false);
                    });
                  } else {
                    setStarting(false);
                  }
                }}
                onCancel={() => dispatch(onAction, transitionTeamRun(run.id, "cancelled"))}
                onComplete={() => dispatch(onAction, transitionTeamRun(run.id, "completed"))}
              />
            </>
          }
        />
      }
      composer={
        <section id="team-war-room-composer" className="space-y-2">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="hidden text-[11px] font-medium text-muted-foreground sm:inline">Message</span>
            <Select
              aria-label="Message recipient"
              value={composerTarget}
              onChange={(event) => setComposerTarget(event.target.value)}
              className="h-8 w-44"
            >
              <option value="team">Team · all members</option>
              {members.map((member) => <option key={member.id} value={member.id}>{member.name ?? member.id}</option>)}
            </Select>
            <Select aria-label="Message kind" value={kind} onChange={(event) => setKind(event.target.value)} className="h-8 w-32">
              <option value="broadcast">Broadcast</option>
              <option value="question">Question</option>
              <option value="answer">Answer</option>
              <option value="progress">Progress</option>
              <option value="blocker">Blocker</option>
              <option value="review_request">Review request</option>
            </Select>
            <span className="hidden text-[11px] text-muted-foreground sm:inline">from Host / operator</span>
          </div>
          <div className="flex items-end gap-2">
            <TextArea
              aria-label="Team message"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder={composerTarget === "team" ? "Message team or @member…" : `Message ${memberById.get(composerTarget)?.name ?? "member"}…`}
              className="min-h-12 flex-1 resize-none"
              rows={2}
              disabled={!actionsEnabled}
            />
            <Button size="sm" onClick={submit} disabled={!canSend} title={actionsEnabled ? undefined : "Connect a live source to enable actions"}>
              <Send className="size-3.5" /> Send
            </Button>
          </div>
        </section>
      }
      context={
        <ContextRail quiet label="Team context">
          <WaveModule wave={wave} onOpen={() => wave && onSelectionChange({ surface: "missions", missionId: wave.mission_id, waveId: wave.id, teamId: undefined })} />
          <GateReadinessModule wave={wave} runStatus={status} needsYouCount={needsYou.total} />
          <AttemptModule runId={run.id} status={status} attempt={attemptNumber(attempts, run.id)} previousRunId={run.previous_run_id} hostSurface={run.host_surface} executionRoot={run.execution_root} createdAt={run.created_at} completedAt={run.completed_at} />
          <SelectedMemberModule
            member={selectedMember}
            assignment={selectedAssignment?.body}
            currentAction={latestActionTitle(actions, selectedMember?.id)}
            onMessage={() => messageMember(selectedMember)}
            onOpen={() => selectedMember && openMember(selectedMember)}
          />
          <ResourcesModule
            members={members}
            delegationCount={delegations.length}
            liveCount={liveActivityByMember.size}
          />
        </ContextRail>
      }
    >
      <div className="mx-auto flex w-full max-w-[1040px] flex-col px-4 py-3 sm:px-5">
        <div className="hidden items-center justify-end gap-1 text-[10px] font-medium text-muted-foreground sm:flex xl:hidden">
          Scroll members <ChevronRight className="size-3" />
        </div>
        <section aria-label="Team members" className="relative border-b border-border/70 pb-3">
          <div className="mb-2 flex items-center justify-between gap-3">
            <div>
              <h2 className="text-[12px] font-semibold text-foreground">Team presence</h2>
            </div>
            <span className="inline-flex items-center gap-1.5 text-[10px] font-medium text-status-running"><StatusDot tone="running" pulse /> Live</span>
          </div>
          <div className="grid grid-cols-1 divide-y divide-border/60 sm:flex sm:divide-x sm:divide-y-0 sm:overflow-x-auto xl:grid xl:grid-cols-4 xl:overflow-visible">
          {orderedMembers.map((member, index) => (
            <MemberControl
              key={member.id}
              member={member}
              className={index > 0 && !showAllMembers ? "hidden sm:block" : undefined}
              selected={member.id === selectedMember?.id}
              assignment={selectMemberAssignmentCorrelations(messages, member.id)[0]?.assignment?.body}
              currentAction={latestActionTitle(actions, member.id)}
              livePreview={liveActivityByMember.get(member.id)?.preview}
              terminal={["completed", "failed", "cancelled"].includes(status)}
              onSelect={() => selectMember(member)}
              onOpen={() => openMember(member)}
            />
          ))}
          {orderedMembers.length > 1 && (
            <button
              type="button"
              className="rounded-lg border border-dashed border-border px-3 py-2 text-[11px] font-medium text-muted-foreground hover:border-primary/35 hover:text-primary sm:hidden"
              onClick={() => setShowAllMembers((value) => !value)}
            >
              {showAllMembers ? "Show priority member only" : `View all ${orderedMembers.length} members`}
            </button>
          )}
          {members.length === 0 && <EmptyState icon={Users} title="No member runs" description="This attempt did not create any runnable member instances." />}
          </div>
        </section>

        <section className="min-h-[28rem] overflow-hidden bg-background">
          <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border/70 py-3">
            <div className="flex items-center gap-2">
              <h2 className="text-[13px] font-semibold text-foreground">Team activity</h2>
            </div>
            <div className="flex flex-wrap gap-1" role="group" aria-label="Activity filters">
              {FILTERS.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  aria-pressed={filter === entry.id}
                  onClick={() => setFilter(entry.id)}
                  className={cn(
                    "rounded-md border px-2 py-1 text-[11px] transition-colors",
                    filter === entry.id ? "border-primary/35 bg-primary/10 text-primary" : "border-transparent text-muted-foreground hover:bg-accent hover:text-foreground",
                  )}
                >
                  {entry.label}
                </button>
              ))}
              <button
                type="button"
                aria-label={showFullActivity ? "Show key activity" : "Show full durable record"}
                aria-pressed={showFullActivity}
                onClick={() => setShowFullActivity((value) => !value)}
                className={cn(
                  "grid size-7 place-items-center rounded-md border text-muted-foreground transition-colors hover:border-primary/25 hover:text-foreground",
                  showFullActivity ? "border-primary/30 bg-primary/10 text-primary" : "border-border/70",
                )}
                title={showFullActivity ? "Show key activity" : `Show all ${filteredActivity.length} durable records`}
              >
                <ListFilter className="size-3.5" />
              </button>
            </div>
          </header>
          <ActivityStream
            items={shownActivity}
            variant="timeline"
            empty={
              <div className="space-y-1">
                <p className="text-sm font-medium text-foreground">No {filter === "all" ? "team activity" : filter} yet</p>
                <p className="text-sm text-muted-foreground">Durable messages, actions, evidence, and decisions appear here as the attempt progresses.</p>
              </div>
            }
          />
          {events.length === 0 && context.activity.length === 0 && (
            <p className="border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground">Live provider previews remain transient and are not added to this record.</p>
          )}
        </section>
      </div>
    </FocusShell>
  );
}

function toInteractionActivity(
  interaction: PendingInteraction,
  members: Map<string, MemberRun>,
  actionsEnabled: boolean,
  onResolve: (optionId: string) => void,
): WorkbenchActivityItem {
  return {
    id: `interaction:${interaction.id}`,
    kind: "decision",
    glyph: interaction.kind === "question" ? "message" : "decision",
    title: (
      <span className="inline-flex flex-wrap items-center gap-2">
        <span>{interaction.title}</span>
        <Badge tone="warn">{interaction.route} decision</Badge>
      </span>
    ),
    body: interaction.prompt,
    actor: memberLabel(members, interaction.member_run_id),
    timestamp: formatTime(interaction.created_at),
    tone: "warn",
    prominence: "pressure",
    action: (
      <div className="flex max-w-72 flex-wrap justify-end gap-1.5 rounded-lg border border-status-warn/25 bg-status-warn/[0.055] p-2">
        {interaction.options.map((option) => (
          <Button
            key={option.id}
            size="sm"
            variant={option.intent?.startsWith("reject") ? "secondary" : "default"}
            disabled={!actionsEnabled || interaction.route === "policy"}
            onClick={() => onResolve(option.id)}
          >
            {option.label}
          </Button>
        ))}
        {interaction.route === "policy" && (
          <span className="self-center text-[10px] text-muted-foreground">Awaiting governed policy decision</span>
        )}
        {interaction.options.length === 0 && (
          <span className="text-[10px] text-muted-foreground">No compatible response option</span>
        )}
      </div>
    ),
  };
}

function AttemptActions({ status, actionsEnabled, starting, onStart, onCancel, onComplete }: {
  status: string;
  actionsEnabled: boolean;
  starting: boolean;
  onStart: () => void;
  onCancel: () => void;
  onComplete: () => void;
}) {
  if (status === "planning") {
    return <Button size="sm" onClick={onStart} disabled={!actionsEnabled || starting} title={actionsEnabled ? undefined : "Connect a live source to enable actions"}><Play className="size-3.5" /> {starting ? "Starting…" : "Start attempt"}</Button>;
  }
  if (["planning", "waiting", "reviewing"].includes(status)) {
    return (
      <>
        {status === "reviewing" && <Button size="sm" onClick={onComplete} disabled={!actionsEnabled}><CheckCircle2 className="size-3.5" /> Complete attempt</Button>}
        <Button size="sm" variant="secondary" onClick={onCancel} disabled={!actionsEnabled}><X className="size-3.5" /> Stop attempt</Button>
      </>
    );
  }
  return null;
}

function MemberControl({ member, selected, assignment, currentAction, livePreview, terminal, className, onSelect, onOpen }: {
  member: MemberRun;
  selected: boolean;
  assignment?: string;
  currentAction?: string;
  livePreview?: string;
  terminal: boolean;
  className?: string;
  onSelect: () => void;
  onOpen: () => void;
}) {
  const tone = memberTone(member.status);
  const blocked = member.status === "blocked";
  return (
    <article className={cn(
      "group relative w-full shrink-0 px-3 py-2 sm:w-[15.5rem] xl:w-auto xl:min-w-0",
      selected && "bg-primary/[0.035]",
      blocked && "bg-status-bad/[0.035]",
      className,
    )}>
      <div className="flex min-w-0 items-start gap-2">
        <button type="button" onClick={onSelect} className="flex min-w-0 flex-1 items-start gap-2 text-left">
          <Avatar name={member.name ?? member.id} tone={tone} />
          <span className="min-w-0 flex-1">
            <span className="flex min-w-0 items-center gap-1.5"><span className="truncate text-[12px] font-semibold text-foreground">{member.name ?? member.id}</span><StatusDot tone={tone} pulse={tone === "running"} /></span>
            <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{member.role ?? "member"} · {member.provider ?? "provider"}{member.model ? ` · ${member.model}` : ""}<span className="sm:hidden"> · {member.status ?? "unknown"}</span></span>
          </span>
        </button>
        <button type="button" onClick={onOpen} aria-label={`Open ${member.name ?? member.id}`} className="absolute right-1.5 top-1.5 rounded bg-background/90 p-1 text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100"><SquareArrowOutUpRight className="size-3.5" /></button>
      </div>
      {blocked && (
        <button
          type="button"
          onClick={onSelect}
          aria-label={terminal ? "Unresolved history" : "QA approval required"}
          className="mt-1.5 w-full rounded-md border border-primary/25 bg-primary/[0.045] px-2 py-1 text-[10px] font-semibold text-primary transition-colors hover:bg-primary/10"
        >
          {terminal ? "Inspect unresolved history" : "Review request"}
        </button>
      )}
      <div className="mt-1.5 hidden space-y-1 border-t border-border/60 pt-1.5 text-[10px] sm:block">
        <p className="truncate text-foreground"><span className="text-muted-foreground">Now · </span>{currentAction ?? assignment ?? "No durable action yet"}</p>
        {livePreview && <p className="truncate text-status-info"><span className="font-semibold">Live · </span>{livePreview}</p>}
        {!blocked && <p className="truncate text-muted-foreground">{pressureLabel(member.status)} · {relativeTime(member.last_event_at ?? member.finished_at ?? member.started_at)}</p>}
      </div>
    </article>
  );
}

function memberPressureRank(status?: string | null): number {
  if (["blocked", "failed"].includes(status ?? "")) return 0;
  if (["waiting", "reviewing"].includes(status ?? "")) return 1;
  if (status === "running") return 2;
  if (status === "idle") return 3;
  if (status === "completed") return 4;
  return 5;
}

function WaveModule({ wave, onOpen }: { wave?: Wave; onOpen: () => void }) {
  if (!wave) return <ContextModule title="Wave unavailable" kicker="Wave"><p className="text-[11px] text-muted-foreground">This attempt has no resolved parent Wave in the snapshot.</p></ContextModule>;
  return (
    <ContextModule title={`Wave ${wave.index} · ${wave.title}`} kicker="Wave" tone={waveTone(wave.status)} action={<button type="button" onClick={onOpen} className="text-[11px] font-medium text-primary hover:underline">Open</button>}>
      <p className="text-[12px] leading-relaxed text-foreground">{wave.objective}</p>
      <div className="mt-2 flex flex-wrap gap-1"><Badge tone="muted">{wave.executor_kind}</Badge><Badge tone={gateTone(wave.gate_status)}>gate {wave.gate_status ?? "pending"}</Badge></div>
      {wave.exit_criteria && <p className="mt-2 text-[11px] text-muted-foreground">Exit: {wave.exit_criteria}</p>}
    </ContextModule>
  );
}

function GateReadinessModule({ wave, runStatus, needsYouCount }: { wave?: Wave; runStatus: string; needsYouCount: number }) {
  const gate = wave?.gate_status ?? "pending";
  const criteria = (wave?.exit_criteria ?? "").split(";").map((item) => item.trim()).filter(Boolean);
  const readiness = teamGateReadiness(wave, criteria.length);
  return (
    <ContextModule title="Gate readiness" kicker="Wave gate" tone={gateTone(gate)} icon={<ShieldCheck className="size-3.5" />}>
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        Attempt is <span className="font-medium text-foreground">{runStatus}</span>. The Host records the parent Wave decision separately.
      </p>
      <div className="mt-2 space-y-1 text-[11px]">
        <Fact label="Gate" value={gate} />
        <Fact label="Open signals" value={String(needsYouCount)} />
        {wave?.accepted_run_id && <Fact label="Accepted attempt" value={shortId(wave.accepted_run_id)} mono />}
      </div>
      {criteria.length > 0 && readiness != null && <ReadinessMeter className="mt-3" value={readiness} total={criteria.length} />}
      {wave?.gate_note && <p className="mt-2 border-t border-border/60 pt-2 text-[11px] text-muted-foreground">{wave.gate_note}</p>}
      <p className="mt-2 text-[10px] font-medium text-status-warn">This page cannot accept the Wave.</p>
    </ContextModule>
  );
}

function teamGateReadiness(wave: Wave | undefined, total: number): number | undefined {
  if (!wave || !total) return undefined;
  if (wave.gate_status === "accepted") return total;
  const note = wave.gate_note?.toLowerCase() ?? "";
  const numeric = note.match(/\b(\d+)\s+(?:of\s+\d+\s+)?criteria?\b/);
  if (numeric) return Math.min(total, Number(numeric[1]));
  const words: Record<string, number> = { zero: 0, one: 1, two: 2, three: 3, four: 4, five: 5 };
  const spelled = note.match(/\b(zero|one|two|three|four|five)\s+(?:of\s+\w+\s+)?criteria?\b/);
  return spelled ? Math.min(total, words[spelled[1]]) : undefined;
}

function AttemptModule({ runId, status, attempt, previousRunId, hostSurface, executionRoot, createdAt, completedAt }: { runId: string; status: string; attempt: number; previousRunId?: string | null; hostSurface?: string | null; executionRoot?: string | null; createdAt?: string; completedAt?: string | null }) {
  return <ContextModule title={`Attempt ${attempt}`} kicker="Attempt" tone={teamTone(status)}><div className="space-y-1.5 text-[11px]"><Fact label="Status" value={status} /><Fact label="Run" value={shortId(runId)} mono /><Fact label="Execution root" value={executionRoot ?? "Not recorded (legacy run)"} mono /><Fact label="Started" value={formatDate(createdAt)} />{previousRunId && <Fact label="Retry of" value={shortId(previousRunId)} mono />}{hostSurface && <Fact label="Host" value={hostSurface} />}{completedAt && <Fact label="Completed" value={formatDate(completedAt)} />}</div></ContextModule>;
}

function SelectedMemberModule({ member, assignment, currentAction, onMessage, onOpen }: { member?: MemberRun; assignment?: string; currentAction?: string; onMessage: () => void; onOpen: () => void }) {
  if (!member) return <ContextModule title="No member selected" kicker="Selected member"><p className="text-[11px] text-muted-foreground">Choose a member control to inspect its attempt-scoped context.</p></ContextModule>;
  return (
    <ContextModule title={member.name ?? member.id} kicker="Selected member" tone={memberTone(member.status)}>
      <div className="flex items-center gap-2"><Avatar name={member.name ?? member.id} tone={memberTone(member.status)} /><p className="min-w-0 truncate text-[11px] text-muted-foreground">{member.role ?? "member"} · {member.provider ?? "provider"}</p></div>
      <div className="mt-2 space-y-1.5 text-[11px]"><Fact label="Assignment" value={assignment ?? "No assignment recorded"} /><Fact label="Now" value={currentAction ?? "No durable action"} /><Fact label="Worktree override" value={member.worktree_ref ?? "None"} mono /><Fact label="Actual cwd" value={member.workspace_snapshot?.cwd ?? "Not captured (legacy run)"} mono /><Fact label="Native session" value={member.native_session?.native_session_id ?? "Not recorded"} mono /></div>
      <div className="mt-3 flex gap-2"><Button size="sm" variant="secondary" onClick={onMessage}><MessageSquare className="size-3.5" /> Message</Button><Button size="sm" variant="secondary" onClick={onOpen}><ExternalLink className="size-3.5" /> Open member</Button></div>
    </ContextModule>
  );
}

function ResourcesModule({ members, delegationCount, liveCount }: { members: MemberRun[]; delegationCount: number; liveCount: number }) {
  const sessions = members.filter((member) => member.native_session).length;
  const worktrees = members.filter((member) => member.worktree_ref).length;
  return <ContextModule title="Resources" kicker="Observed runtime"><div className="space-y-1.5 text-[11px]"><Fact label="Sessions" value={`${sessions} / ${members.length}`} /><Fact label="Worktrees" value={String(worktrees)} /><Fact label="Delegations" value={String(delegationCount)} /><Fact label="Live previews" value={String(liveCount)} /></div><p className="mt-2 text-[10px] text-muted-foreground">Observed resources only; no termination control is implied.</p></ContextModule>;
}

function Fact({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return <div className="grid grid-cols-[5.25rem_1fr] gap-2"><span className="text-muted-foreground">{label}</span><span className={cn("min-w-0 break-words text-foreground", mono && "font-mono text-[10px]")}>{value}</span></div>;
}

function toActivityItems(items: StableTeamActivity[], members: Map<string, MemberRun>): WorkbenchActivityItem[] {
  return items.map((item) => {
    const actor = item.sourceMemberRunId ? memberLabel(members, item.sourceMemberRunId) : "Host";
    if (item.kind === "message") {
      const message = item.message;
      const recipients = (message.to_member_ids ?? []).map((id) => memberLabel(members, id)).join(", ") || "team";
      const evidenceRefs = message.evidence_refs ?? [];
      return {
        id: item.id,
        kind: message.kind === "blocker" ? "blocker" : message.kind === "review_result" ? "decision" : evidenceRefs.length ? "evidence" : "message",
        glyph: teamMessageGlyph(message.kind, evidenceRefs.length > 0),
        title: <span><Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge><span className="ml-2">{actor} → {recipients}</span></span>,
        body: message.body,
        actor: message.correlation_id ? `correlation ${shortId(message.correlation_id)}` : undefined,
        timestamp: formatTime(message.created_at),
        evidenceRefs,
        tone: messageTone(message.kind),
        prominence: message.kind === "assignment" ? "primary" : ["blocker", "review_request", "review_result"].includes(message.kind ?? "") ? "pressure" : "detail",
      };
    }
    if (item.kind === "action") {
      const action = item.action;
      const evidenceRefs = action.evidence_refs ?? [];
      const status = [action.provider_status, action.semantic_status].filter(Boolean).join(" · ");
      return { id: item.id, kind: evidenceRefs.length ? "evidence" : "action", glyph: evidenceRefs.length ? "artifact" : "runtime", title: action.title ?? action.action_type ?? "Member action", body: status ? <><span>{action.summary}</span><span className="mt-1 block text-[10px] text-muted-foreground">provider {action.provider_status ?? "unknown"} · semantic {action.semantic_status ?? "not classified"}</span></> : action.summary, actor, timestamp: formatTime(action.started_at ?? action.completed_at), evidenceRefs, tone: action.status === "failed" ? "bad" : action.status === "succeeded" ? "good" : "running", prominence: "detail" };
    }
    const event = item.event;
    const decision = event.entity_type === "wave" || event.operation === "completed" || /gate|decision/i.test(event.summary ?? "");
    return { id: item.id, kind: decision ? "decision" : "action", glyph: decision ? "decision" : "runtime", title: event.summary ?? `${event.entity_type ?? "Team"} ${event.operation ?? "updated"}`, actor, timestamp: formatTime(event.occurred_at), tone: decision ? "decision" : "info", prominence: event.entity_type === "team_run" && event.operation === "created" ? "primary" : "detail" };
  });
}

function teamMessageGlyph(kind?: string | null, hasEvidence = false): WorkbenchActivityItem["glyph"] {
  if (hasEvidence) return "artifact";
  switch (kind) {
    case "assignment": return "assignment";
    case "handoff": return "handoff";
    case "review_request": return "review";
    case "review_result": return "decision";
    default: return "message";
  }
}

function matchesFilter(item: WorkbenchActivityItem, filter: StreamFilter): boolean {
  if (filter === "all") return true;
  if (filter === "messages") return item.kind === "message" || item.kind === "blocker";
  if (filter === "actions") return item.kind === "action";
  if (filter === "decisions") return item.kind === "decision";
  return item.kind === "evidence" || Boolean(item.evidenceRefs?.length);
}

function latestActionTitle(actions: Array<{ member_run_id?: string; title?: string; action_type?: string; started_at?: string; completed_at?: string | null }>, memberId?: string): string | undefined {
  if (!memberId) return undefined;
  return actions.filter((action) => action.member_run_id === memberId).sort((left, right) => timestamp(right.started_at ?? right.completed_at) - timestamp(left.started_at ?? left.completed_at))[0]?.title ?? actions.filter((action) => action.member_run_id === memberId).sort((left, right) => timestamp(right.started_at ?? right.completed_at) - timestamp(left.started_at ?? left.completed_at))[0]?.action_type;
}

function dispatch(onAction: TeamWarRoomProps["onAction"], action: { path: string; body: unknown }): void | Promise<boolean> | undefined { return onAction?.(action.path, action.body); }
function attemptNumber(attempts: Array<{ id: string }>, id: string): number { return Math.max(1, attempts.findIndex((attempt) => attempt.id === id) + 1); }
function memberLabel(members: Map<string, MemberRun>, id: string): string { return id === "host" ? "Host" : members.get(id)?.name ?? id; }
function shortId(value: string): string { return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-5)}` : value; }
function timestamp(value?: string | null): number { if (!value) return 0; return value.startsWith("unix-ms:") ? Number(value.slice(8)) || 0 : Date.parse(value) || 0; }
function formatDate(value?: string | null): string { if (!value) return "Not recorded"; const ms = timestamp(value); return ms ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(ms) : value; }
function formatTime(value?: string | null): string { if (!value) return "—"; const ms = timestamp(value); return ms ? new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(ms) : value; }
function relativeTime(value?: string | null): string { const ms = timestamp(value); if (!ms) return "no update"; const delta = Math.max(0, Date.now() - ms); if (delta < 60_000) return "just now"; if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`; if (delta < 86_400_000) return `${Math.floor(delta / 3_600_000)}h ago`; return `${Math.floor(delta / 86_400_000)}d ago`; }
function pressureLabel(status?: string | null): string { if (["blocked", "failed"].includes(status ?? "")) return "blocked"; if (["waiting", "reviewing"].includes(status ?? "")) return "waiting"; if (status === "running") return "active"; return status ?? "idle"; }
function teamTone(status?: string | null): StatusTone { if (status === "running") return "running"; if (status === "completed") return "good"; if (["failed", "cancelled"].includes(status ?? "")) return "bad"; if (["waiting", "reviewing"].includes(status ?? "")) return "warn"; if (status === "planning") return "info"; return "idle"; }
function memberTone(status?: string | null): StatusTone { if (status === "running") return "running"; if (status === "completed") return "good"; if (["blocked", "failed", "stopped"].includes(status ?? "")) return "bad"; if (["waiting", "reviewing"].includes(status ?? "")) return "warn"; if (["queued", "starting"].includes(status ?? "")) return "info"; return "idle"; }
function waveTone(status?: string | null): StatusTone { if (status === "completed") return "good"; if (["blocked", "failed", "cancelled"].includes(status ?? "")) return "bad"; if (["waiting"].includes(status ?? "")) return "warn"; if (status === "running") return "running"; return "info"; }
function gateTone(status?: string | null): StatusTone { if (status === "accepted") return "good"; if (status === "blocked") return "bad"; if (status === "revise") return "warn"; return "decision"; }
function messageTone(kind?: string | null): StatusTone { if (kind === "blocker") return "bad"; if (["review_request"].includes(kind ?? "")) return "warn"; if (["review_result", "answer"].includes(kind ?? "")) return "good"; if (["handoff", "question"].includes(kind ?? "")) return "decision"; if (kind === "progress") return "running"; if (["assignment", "broadcast"].includes(kind ?? "")) return "info"; return "idle"; }
