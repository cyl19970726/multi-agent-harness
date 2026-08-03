import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  Activity,
  ArrowRight,
  BadgeCheck,
  BrainCircuit,
  CheckCircle2,
  ChevronLeft,
  CircleCheckBig,
  CircleHelp,
  Columns3,
  ExternalLink,
  FileCheck2,
  Handshake,
  Inbox,
  ListFilter,
  ListTodo,
  Mail,
  Megaphone,
  MessageCircleWarning,
  MessageSquare,
  MessageSquareReply,
  OctagonAlert,
  Play,
  ScanSearch,
  Search,
  Send,
  SendHorizontal,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  SquareArrowOutUpRight,
  Users,
  Wrench,
  X,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import type { WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";
import { ContextModule, ContextRail } from "@/components/workbench/context/ContextRail";
import { ReadinessMeter } from "@/components/workbench/execution/ExecutionPrimitives";
import { FocusHeader, FocusShell } from "@/components/workbench/layout/FocusShell";
import { Markdown } from "@/components/workbench/Markdown";
import { EmptyState, StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { Select, TextArea } from "@/components/workbench/OperatorForms";

import {
  canMemberAcceptWork,
  selectFilteredTeamWorks,
  selectTeamRunContext,
  selectWorkOwnerMember,
  type TeamWorksAttentionFilter,
  type TeamWorksOwnerFilter,
  type StableTeamActivity,
} from "../model/teamSelectors";
import type { WorkbenchModel } from "../model/readModel";
import { acknowledgeTeamMessage, assignTeamWork, createTeamWork, resolvePendingInteraction, reviewTeamWork, sendTeamMessage, startTeamRun, transitionTeamRun, type ActionDescriptor } from "../api/actions";
import type { MemberRun, PendingInteraction, TeamMessage, Wave, Work, WorkDelivery, WorkEvent } from "../types";
import { effectiveTeamMessageResponseIntent } from "../types";
import type { SelectionState } from "../app/selection";

export interface TeamWarRoomProps {
  model: WorkbenchModel;
  teamRunId?: string;
  /** Optional navigation context. A Mission-scoped TeamRun is not owned by it. */
  missionId?: string;
  waveId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void | Promise<boolean>;
}

type StreamFilter = "all" | "messages" | "actions" | "decisions" | "evidence";
type ComposerTarget = "team" | string;
type TeamActivityItem = WorkbenchActivityItem & { workId?: string };

const FILTERS: Array<{ id: StreamFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "messages", label: "Messages" },
  { id: "actions", label: "Actions" },
  { id: "decisions", label: "Decisions" },
  { id: "evidence", label: "Evidence" },
];

const KEY_ACTIVITY_MESSAGE_KINDS = new Set([
  "message",
  "plan_request",
  "plan_proposal",
  "plan_feedback",
  "plan_approval",
  "question",
  "answer",
  "handoff",
]);

/**
 * One operational view of one AgentTeamRun. New runs are independent or
 * Mission-scoped and may span Host-plan Waves; a selected Wave is navigation
 * context only. Legacy direct-Wave attempts remain readable without inventing
 * a dependency graph.
 */
export function TeamWarRoom({
  model,
  teamRunId,
  missionId,
  waveId,
  onSelectionChange,
  actionsEnabled = false,
  onAction,
}: TeamWarRoomProps) {
  const context = selectTeamRunContext(model.snapshot, teamRunId);
  const [filter, setFilter] = useState<StreamFilter>("all");
  const [participantFilter, setParticipantFilter] = useState<string>("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedMemberId, setSelectedMemberId] = useState<string | undefined>();
  const [composerTarget, setComposerTarget] = useState<ComposerTarget>("team");
  const [draft, setDraft] = useState("");
  const [kind, setKind] = useState("message");
  const [replyAnchor, setReplyAnchor] = useState<TeamMessage>();
  const [composerWorkId, setComposerWorkId] = useState("");
  const [showAllMembers, setShowAllMembers] = useState(false);
  const [showFullActivity, setShowFullActivity] = useState(false);
  const [teamView, setTeamView] = useState<"works" | "activity" | "members">("works");
  const [selectedWorkId, setSelectedWorkId] = useState<string | undefined>();
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

  const { run, mission, wave, attempts, members, memberById, messages, actions, delegations, events, works, workEvents, workDeliveries, liveActivityByMember, needsYou } = context;
  const navigationMission = mission ?? model.snapshot.missions?.find((item) => item.id === missionId);
  const navigationWave = wave ?? model.snapshot.waves?.find(
    (item) =>
      item.id === waveId &&
      (!navigationMission || item.mission_id === navigationMission.id),
  );
  const stableTeam = model.snapshot.teams?.find((item) => item.id === run.agent_team_id);
  const supervisor = model.snapshot.team_supervisor_leases?.find(
    (lease) => lease.team_run_id === run.id,
  );
  const supervisorCurrent = Boolean(
    supervisor
    && supervisor.status === "active"
    && supervisor.expires_unix_ms > Date.now(),
  );
  const pendingCloseCount = model.snapshot.team_member_close_requests?.filter(
    (request) => request.team_run_id === run.id && request.status === "pending",
  ).length ?? 0;
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
  const leadInboxMessages = messages
    .filter((message) =>
      message.from_member_id !== "host"
      && message.to_member_ids?.includes("host")
      && message.kind !== "handoff",
    )
    .sort((left, right) => timestamp(right.created_at) - timestamp(left.created_at));
  const activityItems = toActivityItems(context.activity, memberById, openMember).map((item) => {
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
  const normalizedSearch = searchQuery.trim().toLowerCase();
  const filteredActivity = activityItems.filter((item) =>
    matchesFilter(item, filter)
    && (participantFilter === "all" || item.relatedMemberIds?.includes(participantFilter))
    && (!normalizedSearch || `${item.rawText ?? ""} ${item.actorLabel ?? ""}`.toLowerCase().includes(normalizedSearch)),
  );
  const primaryActivity = filteredActivity.filter((item) => item.prominence === "primary");
  const latestPressure = [...filteredActivity].reverse().find((item) => item.prominence === "pressure");
  const keyActivity = latestPressure && !primaryActivity.some((item) => item.id === latestPressure.id)
    ? [...primaryActivity, latestPressure]
    : primaryActivity;
  const shownActivity = filter === "all" && !showFullActivity ? keyActivity : filteredActivity;
  const selectedMemberWork = selectedMember
    ? works.find((work) => work.active_member_run_id === selectedMember.id && !["done", "cancelled"].includes(work.status))
    : undefined;
  const explicitRecipients = composerTarget === "team" ? members.map((member) => member.id) : [composerTarget];
  const canSend = actionsEnabled
    && Boolean(draft.trim())
    && explicitRecipients.length > 0
    && (!replyAnchor || Boolean(replyAnchor.correlation_id));
  const status = run.status ?? "planning";
  const activeMemberCount = members.filter((member) =>
    ["queued", "starting", "running", "waiting", "reviewing"].includes(member.status ?? ""),
  ).length;

  function openMember(member: MemberRun): void {
    onSelectionChange({
      surface: "team",
      teamId: run.id,
      memberRunId: member.id,
      missionId: navigationMission?.id,
      waveId: navigationWave?.id,
    });
  }

  function selectMember(member: MemberRun): void {
    setSelectedMemberId(member.id);
  }

  function messageMember(member?: MemberRun): void {
    setReplyAnchor(undefined);
    if (member) {
      setSelectedMemberId(member.id);
      setComposerTarget(member.id);
      const memberWork = works.find((work) => work.active_member_run_id === member.id && !["done", "cancelled"].includes(work.status));
      setComposerWorkId(memberWork?.id ?? "");
    } else {
      setComposerTarget("team");
      setComposerWorkId("");
    }
    document.getElementById("team-war-room-composer")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }

  function discussWork(work: Work): void {
    setSelectedWorkId(undefined);
    setReplyAnchor(undefined);
    setComposerWorkId(work.id);
    setKind("message");
    document.getElementById("team-war-room-composer")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }

  function submit(): void {
    void submitMessage();
  }

  async function submitMessage(): Promise<void> {
    if (!canSend) return;
    const descriptor = sendTeamMessage(run.id, {
      fromMemberId: "host",
      senderKind: "operator",
      senderId: "operator",
      senderName: "Operator",
      toMemberIds: explicitRecipients,
      kind,
      body: draft.trim(),
      workId: (replyAnchor?.work_id ?? composerWorkId) || undefined,
      correlationId: replyAnchor?.correlation_id ?? undefined,
      causationId: replyAnchor?.id,
      originWaveId: navigationWave?.id,
    });
    const result = onAction?.(descriptor.path, descriptor.body);
    const accepted = result instanceof Promise ? await result : true;
    if (!accepted) return;
    if (replyAnchor && hostDeliveryStatus(replyAnchor) === "delivered") {
      const ack = acknowledgeTeamMessage(run.id, replyAnchor.id, "host");
      await onAction?.(ack.path, ack.body);
    }
    setDraft("");
    setReplyAnchor(undefined);
  }

  return (
    <FocusShell
      className="xl:grid-cols-[minmax(0,1fr)_20rem]"
      headerClassName="min-h-[84px] bg-background py-1.5 sm:py-1.5"
      composerClassName="bg-background py-2 shadow-[0_-12px_30px_-28px_rgba(15,23,42,0.55)]"
      header={
        <FocusHeader
          breadcrumb={
            <button
              type="button"
              onClick={() => onSelectionChange(
                navigationMission
                  ? { surface: "missions", missionId: navigationMission.id, waveId: navigationWave?.id, teamId: undefined }
                  : { surface: "team", teamId: undefined },
              )}
              className="inline-flex items-center gap-1 text-muted-foreground transition-colors hover:text-foreground"
            >
              <ChevronLeft className="size-3.5" />
              {navigationMission?.title ?? "Agent Teams"} <span className="text-border">/</span> {navigationWave ? `Wave ${navigationWave.index}` : "Team"}
            </button>
          }
          title={stableTeam?.name ?? "Agent Team"}
          description={stableTeam?.name ?? "Agent Team attempt"}
          meta={
            <>
              <Badge tone={teamTone(status)}>{status}</Badge>
              <Badge tone="muted">attempt {attemptNumber(attempts, run.id)}</Badge>
              <Badge tone="muted">Lead · {teamLeadLabel(stableTeam?.owner_agent_id)}</Badge>
              <Badge tone={supervisorCurrent ? "good" : status === "running" ? "bad" : "muted"}>
                Supervisor · {supervisorCurrent ? `live g${supervisor?.generation}` : "offline"}
              </Badge>
              {pendingCloseCount > 0 && <Badge tone="warn">Close pending · {pendingCloseCount}</Badge>}
              {navigationWave && <Badge tone={gateTone(navigationWave.gate_status)}>Host plan: Wave {navigationWave.index}</Badge>}
            </>
          }
          actions={
            <>
              <span className="hidden items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground sm:inline-flex">
                <Users className="size-3.5" /> {activeMemberCount} active · {members.length} {members.length === 1 ? "member" : "members"}
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
        <section id="team-war-room-composer" className="space-y-1.5">
          {replyAnchor && (
            <div className="flex min-w-0 items-center gap-2 text-[10px]">
              <Badge tone="decision">Reply in conversation</Badge>
              <span className="truncate text-muted-foreground">{memberLabel(memberById, replyAnchor.from_member_id ?? "")} · {shortId(replyAnchor.correlation_id ?? "")}</span>
              <button type="button" onClick={() => setReplyAnchor(undefined)} className="text-primary hover:underline">New message</button>
            </div>
          )}
          {!replyAnchor && composerWorkId && (
            <div className="flex min-w-0 items-center gap-2 text-[10px]">
              <Badge tone="info">Discussing Work</Badge>
              <span className="truncate text-muted-foreground">{works.find((work) => work.id === composerWorkId)?.title ?? shortId(composerWorkId)}</span>
              <button type="button" onClick={() => setComposerWorkId("")} className="text-primary hover:underline">Detach</button>
            </div>
          )}
          <div className="grid min-w-0 gap-2 sm:grid-cols-2 lg:grid-cols-[9rem_7rem_11rem_minmax(12rem,1fr)_auto]">
            <Select
              aria-label="Message recipient"
              value={composerTarget}
              onChange={(event) => setComposerTarget(event.target.value)}
              className="h-9 w-full"
              disabled={Boolean(replyAnchor)}
            >
              <option value="team">Team · all members</option>
              {members.map((member) => <option key={member.id} value={member.id}>{member.name ?? member.id}</option>)}
            </Select>
            <Select aria-label="Message kind" value={kind} onChange={(event) => setKind(event.target.value)} className="h-9 w-full" disabled={Boolean(replyAnchor)}>
              <option value="message">Message</option>
              <option value="handoff">Handoff</option>
            </Select>
            <Select
              aria-label="Related Work"
              value={replyAnchor?.work_id ?? composerWorkId}
              onChange={(event) => setComposerWorkId(event.target.value)}
              className="h-9 w-full"
              disabled={Boolean(replyAnchor)}
            >
              <option value="">No related Work</option>
              {works.map((work) => <option key={work.id} value={work.id}>{work.title}</option>)}
            </Select>
            <TextArea
              aria-label="Team message"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder={replyAnchor
                ? `Reply to ${memberLabel(memberById, replyAnchor.from_member_id ?? "")} in this conversation…`
                : composerTarget === "team" ? "Message team or @member…" : `Message ${memberById.get(composerTarget)?.name ?? "member"}…`}
              className="min-h-9 resize-none py-2"
              rows={1}
              disabled={!actionsEnabled}
            />
            <Button size="sm" className="h-9" onClick={submit} disabled={!canSend} title={actionsEnabled ? undefined : "Connect a live source to enable actions"}>
              <Send className="size-3.5" /> Send
            </Button>
          </div>
          <p className="text-center text-[9px] text-muted-foreground">Host coordination only · Member-originated messages come from their provider session.</p>
        </section>
      }
      context={
        <ContextRail label="Team context" className="bg-[#fbfaf8]">
          <MissionTeamModule
            missionTitle={navigationMission?.title}
            teamName={stableTeam?.name}
            leadAgentId={stableTeam?.owner_agent_id}
            missionScoped={Boolean(run.mission_id && !run.wave_id)}
            members={orderedMembers}
            onOpenMember={openMember}
            onOpen={() => navigationMission && onSelectionChange({ surface: "missions", missionId: navigationMission.id, waveId: navigationWave?.id, teamId: undefined })}
          />
          <WaveModule
            wave={navigationWave}
            directExecutor={Boolean(wave && wave.id === navigationWave?.id)}
            onOpen={() => navigationWave && onSelectionChange({ surface: "missions", missionId: navigationWave.mission_id, waveId: navigationWave.id, teamId: undefined })}
          />
          {wave && <GateReadinessModule wave={wave} runStatus={status} needsYouCount={needsYou.total} />}
          <AttemptModule runId={run.id} status={status} attempt={attemptNumber(attempts, run.id)} previousRunId={run.previous_run_id} hostSurface={run.host_surface} hostThreadId={run.host_thread_id} executionRoot={run.execution_root} createdAt={run.created_at} completedAt={run.completed_at} />
          <SelectedMemberModule
            member={selectedMember}
            work={selectedMemberWork?.title}
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
      <div className="mx-auto flex w-full max-w-[1180px] flex-col px-4 py-2 sm:px-5">
        {teamView === "activity" ? (
          <>
            <TeamMailboxStrip
              members={orderedMembers}
              messages={messages}
              selectedId={participantFilter}
              selectedMemberId={selectedMember?.id}
              showAllMembers={showAllMembers}
              onToggleAllMembers={() => setShowAllMembers((value) => !value)}
              onSelect={(id) => {
                setParticipantFilter(id);
                if (id !== "all" && id !== "host") {
                  const member = memberById.get(id);
                  if (member) selectMember(member);
                }
              }}
              onOpenMember={openMember}
            />
            <LeadInbox
              messages={leadInboxMessages}
              members={memberById}
              actionsEnabled={actionsEnabled}
              onAnswer={(message) => {
                if (!message.from_member_id || message.from_member_id === "host") return;
                setReplyAnchor(message);
                setComposerWorkId(message.work_id ?? "");
                setComposerTarget(message.from_member_id);
                setKind("message");
                document.getElementById("team-war-room-composer")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
              }}
              onAcknowledge={(message) => dispatch(onAction, acknowledgeTeamMessage(run.id, message.id, "host"))}
            />
          </>
        ) : (
          <TeamCoordinationPressure
            members={members}
            messages={leadInboxMessages}
            pendingInteractions={pendingInteractions.length}
            onOpenActivity={() => setTeamView("activity")}
          />
        )}

        {["completed", "cancelled"].includes(status ?? "") && needsYou.unfinishedWorks.length > 0 && (
          <section
            role="alert"
            data-testid="terminal-work-integrity-anomaly"
            className="mt-2 flex flex-wrap items-start gap-x-3 gap-y-1 rounded-xl border border-status-bad/30 bg-status-bad/[0.045] px-3 py-2"
          >
            <ShieldAlert className="mt-0.5 size-3.5 shrink-0 text-status-bad" />
            <div className="min-w-0 flex-1">
              <p className="text-[11px] font-semibold text-status-bad">Integrity anomaly · terminal TeamRun has unfinished Work</p>
              <p className="mt-0.5 text-[10px] leading-relaxed text-muted-foreground">
                {needsYou.unfinishedWorks.length} Work{needsYou.unfinishedWorks.length === 1 ? "" : "s"} remain non-terminal. Historical state is shown honestly; reconcile or cancel each Work before treating this attempt as clean.
              </p>
            </div>
            <Button size="sm" variant="secondary" onClick={() => setTeamView("works")}>Inspect Works</Button>
          </section>
        )}

        <nav aria-label="Team workspace" className="mt-2 flex items-center gap-1 border-b border-border/70">
          {([
            { id: "works", label: "Works", count: works.filter((item) => !["done", "cancelled"].includes(item.status)).length, icon: Columns3 },
            { id: "activity", label: "Activity", count: activityItems.length, icon: MessageSquare },
            { id: "members", label: "Members", count: members.length, icon: Users },
          ] as const).map((entry) => {
            const Icon = entry.icon;
            return (
              <button
                key={entry.id}
                type="button"
                aria-current={teamView === entry.id ? "page" : undefined}
                onClick={() => setTeamView(entry.id)}
                className={cn(
                  "relative flex h-10 items-center gap-2 px-3 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground",
                  teamView === entry.id && "text-foreground after:absolute after:inset-x-2 after:bottom-0 after:h-0.5 after:rounded-full after:bg-primary",
                )}
              >
                <Icon className="size-3.5" />
                {entry.label}
                <span className="rounded-full bg-muted px-1.5 py-0.5 text-[9px] tabular-nums">{entry.count}</span>
              </button>
            );
          })}
        </nav>

        {teamView === "works" && (
          <TeamWorksBoard
            works={works}
            members={orderedMembers}
            selectedWorkId={selectedWorkId}
            actionsEnabled={actionsEnabled}
            onSelectWork={setSelectedWorkId}
            onOpenMember={openMember}
            onAction={(descriptor) => dispatch(onAction, descriptor)}
            teamRunId={run.id}
            workEvents={workEvents}
            workDeliveries={workDeliveries}
            onDiscuss={discussWork}
          />
        )}

        {teamView === "activity" && <section className="min-h-[28rem] overflow-hidden bg-background" data-testid="team-conversation">
          <header className="sticky top-0 z-10 border-b border-border/70 bg-background/95 py-2 backdrop-blur">
            <div className="flex min-w-max items-center gap-1 overflow-x-auto pb-0.5" role="group" aria-label="Activity filters">
              <label className="flex h-8 min-w-[13rem] items-center gap-2 rounded-lg border border-border/75 bg-card px-2.5 text-muted-foreground focus-within:border-primary/45">
                <Search className="size-3.5" />
                <input
                  value={searchQuery}
                  onChange={(event) => setSearchQuery(event.target.value)}
                  placeholder="Search team activity"
                  className="min-w-0 flex-1 bg-transparent text-[11px] text-foreground outline-none placeholder:text-muted-foreground"
                  aria-label="Search team activity"
                />
                {searchQuery && <button type="button" onClick={() => setSearchQuery("")} aria-label="Clear search"><X className="size-3" /></button>}
              </label>
              {FILTERS.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  data-testid={`activity-filter-${entry.id}`}
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
              <span className="mx-1 h-4 w-px bg-border" />
              <button
                type="button"
                aria-pressed={participantFilter === "all"}
                onClick={() => setParticipantFilter("all")}
                className={cn(
                  "rounded-md border px-2 py-1 text-[11px] transition-colors",
                  participantFilter === "all" ? "border-primary/35 bg-primary/10 text-primary" : "border-transparent text-muted-foreground hover:bg-accent",
                )}
              >
                Everyone
              </button>
              {orderedMembers.map((member) => (
                <button
                  key={member.id}
                  type="button"
                  aria-pressed={participantFilter === member.id}
                  onClick={() => setParticipantFilter(member.id)}
                  className={cn(
                    "rounded-md border px-2 py-1 text-[11px] transition-colors",
                    participantFilter === member.id ? "border-primary/35 bg-primary/10 text-primary" : "border-transparent text-muted-foreground hover:bg-accent",
                  )}
                >
                  {member.name ?? member.id}
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
          <TeamConversationStream
            items={shownActivity}
            onOpenWork={(workId) => {
              setSelectedWorkId(workId);
              setTeamView("works");
            }}
            empty={
              <div className="space-y-1">
                <p className="text-sm font-medium text-foreground">No activity matches these filters</p>
                <p className="text-sm text-muted-foreground">Clear the participant, type, or search filter to return to the full Team conversation.</p>
                <Button size="sm" variant="secondary" onClick={() => { setFilter("all"); setParticipantFilter("all"); setSearchQuery(""); }}>Reset filters</Button>
              </div>
            }
          />
          {events.length === 0 && context.activity.length === 0 && (
            <p className="border-t border-border/60 px-4 py-2 text-[11px] text-muted-foreground">Live provider previews remain transient and are not added to this record.</p>
          )}
        </section>}

        {teamView === "members" && (
          <section className="grid gap-3 py-4 sm:grid-cols-2 xl:grid-cols-3" aria-label="Team members">
            {orderedMembers.map((member) => (
              <MemberControl
                key={member.id}
                member={member}
                selected={selectedMember?.id === member.id}
                work={works.find((work) => work.active_member_run_id === member.id && !["done", "cancelled"].includes(work.status))?.title}
                currentAction={latestActionTitle(actions, member.id)}
                livePreview={liveActivityByMember.get(member.id)?.preview}
                terminal={["failed", "stopped"].includes(member.status ?? "")}
                onSelect={() => selectMember(member)}
                onOpen={() => openMember(member)}
              />
            ))}
          </section>
        )}
      </div>
    </FocusShell>
  );
}

type WorkLane = "open" | "assigned" | "doing" | "review" | "done";

const WORK_LANES: Array<{ id: WorkLane; label: string; statuses: string[]; tone: StatusTone }> = [
  { id: "open", label: "Open", statuses: ["open"], tone: "idle" },
  { id: "assigned", label: "Assigned", statuses: ["open"], tone: "info" },
  { id: "doing", label: "In progress", statuses: ["in_progress", "blocked"], tone: "running" },
  { id: "review", label: "Review", statuses: ["review"], tone: "warn" },
  { id: "done", label: "Done", statuses: ["done", "cancelled"], tone: "good" },
];

function TeamWorksBoard({
  works,
  workEvents,
  workDeliveries,
  members,
  selectedWorkId,
  actionsEnabled,
  teamRunId,
  onSelectWork,
  onOpenMember,
  onDiscuss,
  onAction,
}: {
  works: Work[];
  workEvents: WorkEvent[];
  workDeliveries: WorkDelivery[];
  members: MemberRun[];
  selectedWorkId?: string;
  actionsEnabled: boolean;
  teamRunId: string;
  onSelectWork: (id: string | undefined) => void;
  onOpenMember: (member: MemberRun) => void;
  onDiscuss: (work: Work) => void;
  onAction: (descriptor: ActionDescriptor) => void;
}) {
  const [creating, setCreating] = useState(false);
  const [title, setTitle] = useState("");
  const [criteria, setCriteria] = useState("");
  const [ownerId, setOwnerId] = useState("");
  const [reviewNote, setReviewNote] = useState("");
  const [ownerFilter, setOwnerFilter] = useState<TeamWorksOwnerFilter>("all");
  const [attentionFilter, setAttentionFilter] = useState<TeamWorksAttentionFilter>("all");
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const selected = works.find((work) => work.id === selectedWorkId);
  const active = works.filter((work) => !["done", "cancelled"].includes(work.status)).length;
  const assignableMembers = members.filter(canMemberAcceptWork);
  const ownerFor = (work: Work) => selectWorkOwnerMember(work, members);
  const visibleWorks = selectFilteredTeamWorks(works, members, ownerFilter, attentionFilter);
  const ownerCount = (filter: TeamWorksOwnerFilter) =>
    selectFilteredTeamWorks(works, members, filter, attentionFilter).length;
  const attentionCount = (filter: TeamWorksAttentionFilter) =>
    selectFilteredTeamWorks(works, members, ownerFilter, filter).length;
  useEffect(() => {
    if (!selected) return undefined;
    closeButtonRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onSelectWork(undefined);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [selected?.id, onSelectWork]);
  const create = () => {
    if (!title.trim() || !criteria.trim()) return;
    onAction(createTeamWork(teamRunId, {
      title: title.trim(),
      contextMarkdown: "Created from the Team Works board.",
      completionCriteriaMarkdown: criteria.trim(),
      activeMemberRunId: ownerId || undefined,
      claimMode: ownerId ? "host_assign" : "team_claim",
    }));
    setTitle("");
    setCriteria("");
    setOwnerId("");
    setCreating(false);
  };
  const laneWorksFor = (lane: (typeof WORK_LANES)[number]) => visibleWorks.filter((work) =>
    lane.statuses.includes(work.status)
    && (lane.id === "open"
      ? !work.owner_member_id && !work.active_member_run_id
      : lane.id === "assigned"
        ? Boolean(work.owner_member_id || work.active_member_run_id)
        : true),
  );
  const workCard = (work: Work) => {
    const owner = ownerFor(work);
    return (
      <button key={work.id} type="button" onClick={() => onSelectWork(work.id)} className={cn("w-full rounded-lg border bg-card p-2.5 text-left shadow-[0_12px_26px_-26px_rgba(15,23,42,.75)] transition hover:-translate-y-px hover:border-primary/30 hover:shadow-md", selectedWorkId === work.id ? "border-primary/40 ring-1 ring-primary/15" : "border-border/75")}>
        <div className="flex items-start justify-between gap-2"><Badge tone={workTone(work.status)}>{work.status.replace(/_/g, " ")}</Badge><span className="text-[9px] uppercase tracking-wider text-muted-foreground">{work.priority}</span></div>
        <h3 className="mt-2 line-clamp-2 text-[12px] font-semibold leading-snug text-foreground">{work.title}</h3>
        <p className="mt-1 line-clamp-2 text-[10px] leading-relaxed text-muted-foreground">{work.completion_criteria_markdown || "No completion criteria"}</p>
        <div className="mt-2 flex items-center gap-1.5 border-t border-border/55 pt-2">{owner ? <><Avatar name={owner.name ?? owner.id} tone={memberTone(owner.status)} /><span className="min-w-0 flex-1 truncate text-[10px] text-foreground">{owner.name ?? owner.id}</span></> : <><span className="grid size-6 place-items-center rounded-full border border-dashed border-border text-muted-foreground"><Users className="size-3" /></span><span className="text-[10px] text-muted-foreground">Unassigned</span></>}<span className="ml-auto font-mono text-[9px] text-muted-foreground">v{work.version}</span></div>
      </button>
    );
  };
  return (
    <section className="min-h-[30rem] py-3" aria-label="Shared team Works board" data-testid="team-works-board">
      <header className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2"><ListTodo className="size-4 text-primary" /><h2 className="text-sm font-semibold text-foreground">Shared Works</h2><Badge tone="info">{active} active</Badge></div>
          <p className="mt-1 text-[11px] text-muted-foreground">Durable ownership lives here. Messages discuss Work; they never create ownership.</p>
        </div>
        <Button size="sm" disabled={!actionsEnabled} onClick={() => setCreating((value) => !value)}>
          <ListTodo className="size-3.5" /> New Work
        </Button>
      </header>

      {creating && (
        <div className="mb-3 grid gap-2 rounded-xl border border-primary/20 bg-primary/[0.025] p-3 sm:grid-cols-[1.2fr_1.2fr_.8fr_auto] sm:items-end">
          <label className="space-y-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Title<input className="mt-1 h-9 w-full rounded-md border border-border bg-background px-2.5 text-[12px] font-normal normal-case tracking-normal text-foreground outline-none focus:border-primary/50" value={title} onChange={(event) => setTitle(event.target.value)} placeholder="What outcome is needed?" /></label>
          <label className="space-y-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Completion criteria<input className="mt-1 h-9 w-full rounded-md border border-border bg-background px-2.5 text-[12px] font-normal normal-case tracking-normal text-foreground outline-none focus:border-primary/50" value={criteria} onChange={(event) => setCriteria(event.target.value)} placeholder="Evidence required for acceptance" /></label>
          <label className="space-y-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Owner<Select className="mt-1 text-[12px] font-normal normal-case tracking-normal" value={ownerId} onChange={(event) => setOwnerId(event.target.value)}><option value="">Unassigned pool</option>{assignableMembers.map((member) => <option key={member.id} value={member.id}>{member.name ?? member.id}</option>)}</Select></label>
          <div className="flex gap-1"><Button size="sm" onClick={create} disabled={!title.trim() || !criteria.trim()}>Create</Button><Button size="sm" variant="secondary" onClick={() => setCreating(false)}>Cancel</Button></div>
        </div>
      )}

      <div className="mb-3 space-y-2 rounded-xl border border-border/65 bg-muted/[0.12] p-2.5" aria-label="Filter Works board">
        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter Works by owner">
          <span className="mr-1 text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Owner</span>
          <WorkFilterChip
            active={ownerFilter === "all"}
            count={ownerCount("all")}
            label="All"
            onClick={() => setOwnerFilter("all")}
          />
          <WorkFilterChip
            active={ownerFilter === "unassigned"}
            count={ownerCount("unassigned")}
            label="Unassigned"
            icon={<Users className="size-3" />}
            onClick={() => setOwnerFilter("unassigned")}
          />
          {members.map((member) => {
            const filter: TeamWorksOwnerFilter = `member:${member.id}`;
            return (
              <WorkFilterChip
                key={member.id}
                active={ownerFilter === filter}
                count={ownerCount(filter)}
                label={member.name ?? member.id}
                icon={<Avatar name={member.name ?? member.id} tone={memberTone(member.status)} size="xs" />}
                onClick={() => setOwnerFilter(filter)}
              />
            );
          })}
        </div>
        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter Works by attention state">
          <span className="mr-1 text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">Attention</span>
          <WorkFilterChip active={attentionFilter === "all"} count={attentionCount("all")} label="All statuses" onClick={() => setAttentionFilter("all")} />
          <WorkFilterChip active={attentionFilter === "review"} count={attentionCount("review")} label="Needs review" onClick={() => setAttentionFilter("review")} />
          <WorkFilterChip active={attentionFilter === "blocked"} count={attentionCount("blocked")} label="Blocked" onClick={() => setAttentionFilter("blocked")} />
        </div>
        <p className="text-[10px] leading-relaxed text-muted-foreground" aria-live="polite">
          Showing {visibleWorks.length} of {works.length} Works. Responsibility comes from Work ownership; activity messages do not assign it.
        </p>
      </div>

      <div className="hidden grid-cols-5 gap-2 pb-2 lg:grid">
        {WORK_LANES.map((lane) => {
          const laneWorks = laneWorksFor(lane);
          return (
            <section key={lane.id} className="min-h-[23rem] rounded-xl border border-border/70 bg-muted/[0.18] p-2" aria-label={`${lane.label} Works`}>
              <header className="mb-2 flex items-center justify-between px-1"><span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground"><StatusDot tone={lane.tone} />{lane.label}</span><span className="text-[10px] tabular-nums text-muted-foreground">{laneWorks.length}</span></header>
              <div className="space-y-2">
                {laneWorks.map(workCard)}
                {laneWorks.length === 0 && <div className="grid min-h-20 place-items-center rounded-lg border border-dashed border-border/75 px-2 text-center text-[10px] text-muted-foreground">No Works</div>}
              </div>
            </section>
          );
        })}
      </div>

      <div className="space-y-3 lg:hidden" aria-label="Works grouped by status">
        {WORK_LANES.map((lane) => {
          const laneWorks = laneWorksFor(lane);
          return (
            <section key={lane.id} className="rounded-xl border border-border/70 bg-muted/[0.16] p-2.5" aria-label={`${lane.label} Works`}>
              <header className="mb-2 flex items-center justify-between px-0.5">
                <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground"><StatusDot tone={lane.tone} />{lane.label}</span>
                <Badge tone="muted">{laneWorks.length}</Badge>
              </header>
              <div className="grid gap-2 sm:grid-cols-2">
                {laneWorks.map(workCard)}
                {laneWorks.length === 0 && <p className="rounded-lg border border-dashed border-border/70 px-3 py-4 text-center text-[10px] text-muted-foreground sm:col-span-2">No Works</p>}
              </div>
            </section>
          );
        })}
      </div>

      {selected && (
        <div className="fixed inset-0 z-50 bg-foreground/15 backdrop-blur-[1px]" onMouseDown={(event) => { if (event.target === event.currentTarget) onSelectWork(undefined); }}>
          <aside
            role="dialog"
            aria-modal="true"
            aria-labelledby="selected-work-title"
            className="absolute inset-x-0 bottom-0 max-h-[86vh] overflow-y-auto rounded-t-2xl border border-border bg-background p-4 shadow-2xl lg:inset-y-0 lg:left-auto lg:right-0 lg:max-h-none lg:w-[34rem] lg:rounded-none lg:border-y-0 lg:border-r-0 lg:p-5"
          >
            <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-border lg:hidden" />
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2"><Badge tone={workTone(selected.status)}>{selected.status.replace(/_/g, " ")}</Badge><span className="font-mono text-[9px] text-muted-foreground">{selected.id}</span></div>
                <h3 id="selected-work-title" className="mt-2 text-lg font-semibold text-foreground">{selected.title}</h3>
              </div>
              <button ref={closeButtonRef} type="button" className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground" onClick={() => onSelectWork(undefined)} aria-label="Close Work details"><X className="size-4" /></button>
            </div>

            <div className="mt-4 space-y-4">
              <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Context</p>{selected.context_markdown ? <Markdown source={selected.context_markdown} compact /> : <p className="text-[11px] text-muted-foreground">No context recorded.</p>}</section>
              <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Completion criteria</p><Markdown source={selected.completion_criteria_markdown || "Not declared"} compact /></section>
              {selected.blocker_reason && <section className="rounded-lg border border-status-bad/25 bg-status-bad/[0.045] p-3"><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-status-bad">Blocker</p><Markdown source={selected.blocker_reason} compact /></section>}
              {selected.result_summary && <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Result</p><Markdown source={selected.result_summary} compact /></section>}

              <div className="grid grid-cols-2 gap-2 text-[10px] sm:grid-cols-3">
                <WorkFact label="Owner" value={ownerFor(selected)?.name ?? "Unassigned"} />
                <WorkFact label="Claim mode" value={selected.claim_mode} />
                <WorkFact label="Priority" value={selected.priority} />
                <WorkFact label="Parent" value={selected.parent_work_id ? shortId(selected.parent_work_id) : "None"} />
                <WorkFact label="Source" value={selected.source_work_item_ref ? shortId(selected.source_work_item_ref) : "None"} />
                <WorkFact label="Prerequisites" value={selected.prerequisite_work_ids?.length ? String(selected.prerequisite_work_ids.length) : "None"} />
                <WorkFact label="Artifacts" value={String(selected.artifact_refs?.length ?? 0)} />
                <WorkFact label="Checks" value={String(selected.check_refs?.length ?? 0)} />
                <WorkFact label="Version" value={`v${selected.version}`} />
              </div>

              {(selected.prerequisite_work_ids?.length ?? 0) > 0 && (
                <section><p className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-muted-foreground">Prerequisite Works</p><div className="flex flex-wrap gap-1.5">{selected.prerequisite_work_ids?.map((id) => <button type="button" key={id} onClick={() => onSelectWork(id)} className="rounded-md border border-border px-2 py-1 font-mono text-[9px] text-primary hover:bg-accent">{shortId(id)}</button>)}</div></section>
              )}

              <section className="space-y-2 rounded-xl border border-border/70 bg-muted/[0.16] p-3">
                <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Lead controls</p>
                {selected.status === "open" && !selected.owner_member_id && !selected.active_member_run_id && <label className="block text-[10px] text-muted-foreground">Assign owner<Select className="mt-1" value="" disabled={!actionsEnabled || assignableMembers.length === 0} onChange={(event) => { if (event.target.value) onAction(assignTeamWork(teamRunId, selected.id, event.target.value, selected.version)); }}><option value="">{assignableMembers.length ? "Choose member…" : "No active members available"}</option>{assignableMembers.map((member) => <option key={member.id} value={member.id}>{member.name ?? member.id}</option>)}</Select></label>}
                {selected.status === "review" && <><TextArea value={reviewNote} onChange={(event) => setReviewNote(event.target.value)} placeholder="Optional review note" className="min-h-16" /><div className="flex flex-wrap gap-2"><Button size="sm" disabled={!actionsEnabled} onClick={() => onAction(reviewTeamWork(teamRunId, selected.id, selected.version, "accept", reviewNote))}><CheckCircle2 className="size-3.5" />Accept</Button><Button size="sm" variant="secondary" disabled={!actionsEnabled || !reviewNote.trim()} onClick={() => onAction(reviewTeamWork(teamRunId, selected.id, selected.version, "request-changes", reviewNote))}>Request changes</Button></div></>}
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" variant="secondary" onClick={() => onDiscuss(selected)}><MessageSquare className="size-3.5" /> Discuss Work</Button>
                  {ownerFor(selected) && <Button size="sm" variant="secondary" onClick={() => onOpenMember(ownerFor(selected)!)}>Open member</Button>}
                  {!['done', 'cancelled'].includes(selected.status) && <Button size="sm" variant="secondary" disabled={!actionsEnabled} onClick={() => onAction({ method: "POST", path: `/v1/team-runs/${encodeURIComponent(teamRunId)}/works/${encodeURIComponent(selected.id)}/cancel`, body: { expected_version: selected.version, reason: "Cancelled by Host" } })}>Cancel Work</Button>}
                </div>
              </section>

              <WorkRecordHistory
                events={workEvents.filter((event) => event.work_id === selected.id)}
                deliveries={workDeliveries.filter((delivery) => delivery.work_id === selected.id)}
              />
            </div>
          </aside>
        </div>
      )}
    </section>
  );
}

function WorkFilterChip({
  active,
  count,
  label,
  icon,
  onClick,
}: {
  active: boolean;
  count: number;
  label: string;
  icon?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "flex max-w-full items-center gap-1.5 rounded-full border px-2 py-1 text-[10px] font-medium transition-colors",
        active
          ? "border-primary/35 bg-primary/[0.08] text-primary"
          : "border-border/70 bg-background text-muted-foreground hover:border-primary/25 hover:text-foreground",
      )}
    >
      {icon}
      <span className="max-w-36 truncate" title={label}>{label}</span>
      <span className="tabular-nums opacity-70">{count}</span>
    </button>
  );
}

function WorkFact({ label, value }: { label: string; value: string }) {
  return <div className="rounded-md bg-muted/45 px-2 py-1.5"><span className="block text-[8px] uppercase tracking-wider text-muted-foreground">{label}</span><span className="mt-0.5 block truncate text-foreground">{value}</span></div>;
}

function WorkRecordHistory({ events, deliveries }: { events: WorkEvent[]; deliveries: WorkDelivery[] }) {
  const orderedEvents = [...events].sort((left, right) => left.sequence - right.sequence);
  return (
    <section>
      <div className="mb-2 flex items-center justify-between gap-2">
        <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Durable history</p>
        <span className="text-[9px] text-muted-foreground">{events.length} events · {deliveries.length} deliveries</span>
      </div>
      {orderedEvents.length === 0 && deliveries.length === 0 ? (
        <p className="rounded-lg border border-dashed border-border px-3 py-3 text-[10px] text-muted-foreground">No Work events or deliveries are present in this snapshot.</p>
      ) : (
        <div className="divide-y divide-border/60 overflow-hidden rounded-lg border border-border/70 bg-card">
          {orderedEvents.slice(-8).map((event) => (
            <div key={event.id} className="flex items-start gap-2 px-3 py-2 text-[10px]">
              <StatusDot tone={event.kind.includes("block") || event.kind.includes("cancel") ? "bad" : event.kind.includes("accept") || event.kind.includes("complete") ? "good" : "info"} />
              <div className="min-w-0 flex-1"><p className="font-medium text-foreground">{event.kind.replace(/_/g, " ")}</p><p className="mt-0.5 text-muted-foreground">sequence {event.sequence} · v{event.expected_version} → v{event.resulting_version} · {formatTime(event.created_at)}</p></div>
            </div>
          ))}
          {deliveries.slice(-6).map((delivery) => (
            <div key={delivery.id} className="flex items-start gap-2 px-3 py-2 text-[10px]">
              <SendHorizontal className="mt-0.5 size-3 shrink-0 text-primary" />
              <div className="min-w-0 flex-1"><p className="font-medium text-foreground">Delivery · {delivery.status.replace(/_/g, " ")}</p><p className="mt-0.5 truncate text-muted-foreground">to {shortId(delivery.recipient_member_run_id)} · Work v{delivery.work_version} · attempt {delivery.attempt}</p></div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function TeamConversationStream({ items, empty, onOpenWork }: { items: TeamActivityItem[]; empty: ReactNode; onOpenWork: (workId: string) => void }) {
  if (!items.length) return <div className="grid min-h-48 place-items-center px-6 py-10 text-center">{empty}</div>;
  return (
    <ol className="relative py-1 before:absolute before:bottom-5 before:left-[1.05rem] before:top-5 before:w-px before:bg-border/80">
      {items.map((item) => <li key={item.id}><TeamConversationRow item={item} onOpenWork={onOpenWork} /></li>)}
    </ol>
  );
}

function TeamConversationRow({ item, onOpenWork }: { item: TeamActivityItem; onOpenWork: (workId: string) => void }) {
  const kind = item.messageKind ?? item.kind;
  const tone = item.tone ?? "idle";
  const plan = kind === "plan_proposal";
  const pressure = ["blocker", "review_request", "plan_feedback"].includes(kind);
  const accepted = ["plan_approval", "review_result"].includes(kind);
  const handoff = kind === "handoff";
  const execution = item.kind === "action" || item.kind === "evidence";
  return (
    <article className="relative grid grid-cols-[2.25rem_4.25rem_minmax(0,1fr)] gap-x-2.5 py-1.5">
      <ConversationNode kind={kind} tone={tone} avatarName={item.actorAvatarName} avatarTone={item.actorTone} onActorClick={item.onActorClick} />
      <time className="pt-1 text-right text-[10px] font-medium text-muted-foreground">{item.timestamp}</time>
      <div className="min-w-0">
        <ConversationMeta item={item} label={conversationLabel(kind)} />
        <div className={cn(
          "mt-1 overflow-hidden rounded-lg border bg-card/75 shadow-[0_18px_42px_-38px_rgba(15,23,42,.75)]",
          plan && "border-[#8b5cf6]/30 bg-[linear-gradient(145deg,hsl(var(--card)),rgba(139,92,246,.035))]",
          pressure && "border-status-warn/35 bg-[linear-gradient(145deg,hsl(var(--card)),hsl(var(--status-warn)/.045))]",
          accepted && "border-status-good/30 bg-[linear-gradient(145deg,hsl(var(--card)),hsl(var(--status-good)/.04))]",
          handoff && "border-primary/30 bg-[linear-gradient(145deg,hsl(var(--card)),hsl(var(--primary)/.035))]",
          execution && "border-status-info/25",
        )}>
          <div className="flex min-w-0 items-center gap-2 border-b border-border/55 px-2.5 py-1.5">
            <div className="min-w-0 flex-1">
              {item.messageKind ? <ConversationRoute item={item} /> : <div className="text-[11px] font-semibold text-foreground">{item.title}</div>}
            </div>
            {item.workId && <button type="button" onClick={() => onOpenWork(item.workId!)} className="shrink-0 rounded-full border border-primary/20 bg-primary/[0.055] px-2 py-1 font-mono text-[9px] text-primary hover:bg-primary/10">Work · {shortId(item.workId)}</button>}
            {item.action && <div className="shrink-0">{item.action}</div>}
          </div>
          <div className="px-2.5 py-2">
            {plan && item.bodySource
              ? <PlanProposalBody source={item.bodySource} />
              : <div className="text-[11px] leading-relaxed text-foreground/85">{item.body}</div>}
            {(item.evidenceRefs?.length ?? 0) > 0 && (
              <div className="mt-2 grid gap-1 border-t border-border/55 pt-2 sm:grid-cols-2">
                {item.evidenceRefs?.map((ref) => (
                  <span key={ref} className="inline-flex min-w-0 items-center gap-1.5 rounded-md bg-muted/45 px-2 py-1 text-[9px] text-muted-foreground">
                    <FileCheck2 className="size-3 shrink-0 text-status-good" />
                    <span className="truncate">{ref}</span>
                  </span>
                ))}
              </div>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border/50 bg-muted/20 px-2.5 py-1 text-[9px] text-muted-foreground">
            {item.actor && <span>{item.actor}</span>}
            {item.source === "provider-native" && <span>native session</span>}
            {item.statusLabel && <span className={accepted ? "text-status-good" : undefined}>{item.statusLabel}</span>}
            <span className="ml-auto">{item.kind === "message" || item.kind === "blocker" || item.kind === "decision" ? "coordination record" : "Harness evidence"}</span>
          </div>
        </div>
      </div>
    </article>
  );
}

function ConversationMeta({ item, label }: { item: WorkbenchActivityItem; label: string }) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[9px] uppercase tracking-[0.11em] text-muted-foreground">
      <span className="font-semibold text-foreground/75">{label}</span>
      {item.actorLabel && <span className="normal-case tracking-normal">{item.actorLabel}</span>}
    </div>
  );
}

function ConversationRoute({ item }: { item: WorkbenchActivityItem }) {
  const presentation = messagePresentation(item.messageKind);
  const TypeIcon = presentation.icon;
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
      <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-foreground">
        <span className={cn("size-1.5 rounded-full", presentation.dotClass)} />
        {item.actorLabel ?? "Host"}
      </span>
      <ArrowRight className="size-3 shrink-0 text-muted-foreground" />
      <span className="flex min-w-0 items-center gap-1">
        {(item.recipientLabels ?? []).slice(0, 3).map((recipient) => (
          <span key={recipient} className="inline-flex items-center gap-1 rounded-full bg-muted/45 py-0.5 pl-0.5 pr-1.5">
            <Avatar name={recipient} tone="idle" size="xs" />
            <span className="max-w-28 truncate text-[10px] font-medium text-foreground/80">{recipient}</span>
          </span>
        ))}
        {(item.recipientLabels?.length ?? 0) > 3 && <span className="text-[9px] text-muted-foreground">+{item.recipientLabels!.length - 3}</span>}
      </span>
      <Badge tone={messageTone(item.messageKind)}>
        <TypeIcon className="mr-1 size-3" />
        {presentation.label}
      </Badge>
    </div>
  );
}

function ConversationNode({ kind, tone, avatarName, avatarTone, onActorClick }: {
  kind: string;
  tone: StatusTone;
  avatarName?: string;
  avatarTone?: StatusTone;
  onActorClick?: () => void;
}) {
  if (avatarName) {
    // The timeline identity is always the sender portrait; the corner mark
    // communicates message type without replacing or obscuring authorship.
    const presentation = messagePresentation(kind);
    const TypeIcon = presentation.icon;
    return (
      <button type="button" disabled={!onActorClick} onClick={onActorClick} className="relative z-[1] rounded-full ring-4 ring-background focus-visible:outline-none focus-visible:ring-primary">
        <Avatar name={avatarName} tone={avatarTone ?? tone} />
        <span className={cn(
          "absolute -bottom-1 -right-1 grid size-4 place-items-center rounded-full border border-background text-white shadow-sm",
          presentation.iconClass,
        )}>
          <TypeIcon className="size-2.5" strokeWidth={2.4} />
        </span>
      </button>
    );
  }
  const Icon = kind === "plan_proposal" ? BrainCircuit
      : kind === "plan_feedback" || kind === "blocker" ? ShieldAlert
        : kind === "plan_approval" || kind === "review_result" ? CheckCircle2
          : kind === "handoff" ? ArrowRight
            : kind === "evidence" ? FileCheck2
              : kind === "action" ? Wrench
                : MessageSquare;
  return (
    <span className={cn(
      "relative z-[1] grid size-8 place-items-center rounded-xl border ring-4 ring-background",
      tone === "bad" && "border-status-bad/25 bg-status-bad/8 text-status-bad",
      tone === "warn" && "border-status-warn/25 bg-status-warn/8 text-status-warn",
      tone === "good" && "border-status-good/25 bg-status-good/8 text-status-good",
      tone === "decision" && "border-[#8b5cf6]/25 bg-[#8b5cf6]/[0.08] text-[#7653c6]",
      ["info", "running"].includes(tone) && "border-status-info/25 bg-status-info/8 text-status-info",
      tone === "idle" && "border-border bg-card text-muted-foreground",
    )}>
      <Icon className="size-3.5" />
    </span>
  );
}

function PlanProposalBody({ source }: { source: string }) {
  const sections = source.split(/^##\s+/m).map((value) => value.trim()).filter(Boolean).map((value) => {
    const [heading, ...lines] = value.split("\n");
    return { heading, body: lines.join("\n").trim() };
  });
  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {sections.map((section) => (
        <section key={section.heading} className="rounded-lg border border-[#8b5cf6]/15 bg-background/75 px-2.5 py-2">
          <h3 className="mb-1 text-[9px] font-semibold uppercase tracking-[0.12em] text-[#7653c6]">{section.heading}</h3>
          <Markdown source={section.body} compact />
        </section>
      ))}
    </div>
  );
}

function conversationLabel(kind: string): string {
  if (kind === "plan_request") return "Plan request";
  if (kind === "plan_proposal") return "Plan proposal";
  if (kind === "plan_feedback") return "Host challenge";
  if (kind === "plan_approval") return "Plan approved";
  if (kind === "review_request") return "Lead inbox";
  if (kind === "review_result") return "Review decision";
  if (kind === "handoff") return "Handoff";
  if (kind === "blocker") return "Blocker";
  if (kind === "evidence") return "Evidence";
  if (kind === "action") return "Execution";
  return kind.replace(/_/g, " ");
}

function messagePresentation(kind?: string | null): {
  label: string;
  icon: typeof MessageSquare;
  iconClass: string;
  dotClass: string;
} {
  const normalized = kind ?? "message";
  if (normalized === "message") return { label: "Message", icon: MessageSquare, iconClass: "bg-[#64748b]", dotClass: "bg-[#64748b]" };
  if (normalized === "broadcast") return { label: "Broadcast", icon: Megaphone, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  if (normalized === "question") return { label: "Question", icon: CircleHelp, iconClass: "bg-[#7c5bd6]", dotClass: "bg-[#7c5bd6]" };
  if (normalized === "answer") return { label: "Answer", icon: MessageSquareReply, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "progress") return { label: "Progress", icon: Activity, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  if (normalized === "blocker") return { label: "Blocker", icon: OctagonAlert, iconClass: "bg-status-bad", dotClass: "bg-status-bad" };
  if (normalized === "review_request") return { label: "Lead inbox", icon: ScanSearch, iconClass: "bg-status-warn", dotClass: "bg-status-warn" };
  if (normalized === "review_result") return { label: "Review decision", icon: BadgeCheck, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "plan_request") return { label: "Plan request", icon: ListTodo, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  if (normalized === "plan_proposal") return { label: "Plan proposal", icon: BrainCircuit, iconClass: "bg-[#7c5bd6]", dotClass: "bg-[#7c5bd6]" };
  if (normalized === "plan_feedback") return { label: "Host challenge", icon: MessageCircleWarning, iconClass: "bg-status-warn", dotClass: "bg-status-warn" };
  if (normalized === "plan_approval") return { label: "Plan approved", icon: CircleCheckBig, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "handoff") return { label: "Handoff", icon: Handshake, iconClass: "bg-[#ff725e]", dotClass: "bg-[#ff725e]" };
  if (normalized === "evidence") return { label: "Evidence", icon: FileCheck2, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "action") return { label: "Tool activity", icon: Wrench, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  return { label: conversationLabel(normalized), icon: MessageSquare, iconClass: "bg-muted-foreground", dotClass: "bg-muted-foreground" };
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

function TeamCoordinationPressure({
  members,
  messages,
  pendingInteractions,
  onOpenActivity,
}: {
  members: MemberRun[];
  messages: TeamMessage[];
  pendingInteractions: number;
  onOpenActivity: () => void;
}) {
  const unread = messages.filter((message) => hostDeliveryStatus(message) === "delivered").length;
  const blocked = members.filter((member) => member.status === "blocked").length;
  return (
    <button
      type="button"
      onClick={onOpenActivity}
      className="mt-1 flex w-full flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-border/70 bg-card/70 px-3 py-2 text-left transition-colors hover:border-primary/25 hover:bg-primary/[0.025]"
      aria-label="Open complete Team mailboxes and Lead Inbox"
    >
      <span className="inline-flex items-center gap-2 text-[11px] font-semibold text-foreground"><Inbox className="size-3.5 text-primary" /> Coordination pressure</span>
      <span className={cn("text-[10px]", unread ? "text-status-warn" : "text-muted-foreground")}>{unread} unread for Lead</span>
      <span className={cn("text-[10px]", pendingInteractions ? "text-status-warn" : "text-muted-foreground")}>{pendingInteractions} pending interaction{pendingInteractions === 1 ? "" : "s"}</span>
      <span className={cn("text-[10px]", blocked ? "text-status-bad" : "text-muted-foreground")}>{blocked} blocked member{blocked === 1 ? "" : "s"}</span>
      <span className="ml-auto inline-flex items-center gap-1 text-[10px] font-medium text-primary">Open Activity <ArrowRight className="size-3" /></span>
    </button>
  );
}

function TeamMailboxStrip({
  members,
  messages,
  selectedId,
  selectedMemberId,
  showAllMembers,
  onToggleAllMembers,
  onSelect,
  onOpenMember,
}: {
  members: MemberRun[];
  messages: TeamMessage[];
  selectedId: string;
  selectedMemberId?: string;
  showAllMembers: boolean;
  onToggleAllMembers: () => void;
  onSelect: (id: string) => void;
  onOpenMember: (member: MemberRun) => void;
}) {
  const participants = [
    { id: "host", name: "Host Lead", role: "Team lead", tone: "info" as StatusTone, member: undefined },
    ...members.map((member) => ({
      id: member.id,
      name: member.name ?? member.id,
      role: member.role ?? "member",
      tone: memberTone(member.status),
      member,
    })),
  ];
  return (
    <section aria-label="Participant mailboxes" className="border-b border-border/70 pb-1.5">
      <header className="mb-1 flex items-end justify-between gap-3">
        <div>
          <h2 className="text-[13px] font-semibold text-foreground">Team mailboxes</h2>
          <p className="text-[10px] text-muted-foreground">Inbox and Outbox are live projections of coordination delivery, not duplicate stored objects.</p>
        </div>
        <button type="button" onClick={() => onSelect("all")} className="text-[11px] font-medium text-primary hover:underline">
          All activity
        </button>
      </header>
      <div className="flex snap-x gap-2 overflow-x-auto pb-1 xl:grid xl:grid-cols-5 xl:overflow-visible" data-testid="team-mailbox-strip">
        {participants.map((participant, index) => {
          const inbox = messages.filter((message) => (message.to_member_ids ?? []).includes(participant.id));
          const outbox = messages.filter(
            (message) => messageSenderParticipantId(message) === participant.id,
          );
          const awaiting = inbox.filter((message) => message.deliveries?.some(
            (delivery) => delivery.member_id === participant.id && ["queued", "delivered"].includes(delivery.status ?? ""),
          )).length;
          return (
            <article
              key={participant.id}
              className={cn(
                "relative min-w-[14.5rem] snap-start rounded-lg border bg-card/65 p-1.5 shadow-[0_12px_32px_-30px_rgba(15,23,42,.75)] transition-colors xl:min-w-0",
                selectedId === participant.id ? "border-primary/45 bg-primary/[0.045]" : "border-border/75 hover:border-primary/25",
                index > 1 && !showAllMembers ? "hidden sm:block" : undefined,
              )}
            >
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  data-testid={`mailbox-${participant.id}`}
                  onClick={() => onSelect(participant.id)}
                  className="flex min-w-0 flex-1 items-center gap-2 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                >
                  <Avatar name={participant.name} tone={participant.tone} />
                  <span className="min-w-0">
                    <span className="block truncate text-[11px] font-semibold text-foreground">{participant.name}</span>
                    <span className="block truncate text-[9px] uppercase tracking-wider text-muted-foreground">{participant.role}</span>
                  </span>
                </button>
                {participant.member && (
                  <button
                    type="button"
                    data-testid={`mailbox-open-${participant.id}`}
                    onClick={() => onOpenMember(participant.member!)}
                    className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
                    aria-label={`Open ${participant.name}`}
                  >
                    <SquareArrowOutUpRight className="size-3.5" />
                  </button>
                )}
              </div>
              <div className="mt-1 grid grid-cols-2 divide-x divide-border/70 rounded-md border border-border/60 bg-background/60">
                <button type="button" onClick={() => onSelect(participant.id)} className="px-2 py-1 text-left hover:bg-accent/50">
                  <span className="flex items-center gap-1 text-[9px] uppercase tracking-wider text-muted-foreground"><Mail className="size-3" /> Inbox</span>
                  <span className="mt-0.5 flex items-end gap-1.5"><strong className="text-base leading-none">{inbox.length}</strong>{awaiting > 0 && <span className="text-[8px] text-status-warn">{awaiting} needs attention</span>}</span>
                </button>
                <button type="button" onClick={() => onSelect(participant.id)} className="px-2 py-1 text-left hover:bg-accent/50">
                  <span className="flex items-center gap-1 text-[9px] uppercase tracking-wider text-muted-foreground"><SendHorizontal className="size-3" /> Outbox</span>
                  <span className="mt-0.5 flex items-end gap-1.5"><strong className="text-base leading-none">{outbox.length}</strong><span className="text-[8px] text-muted-foreground">sent</span></span>
                </button>
              </div>
              {participant.member && participant.id === selectedMemberId && <span className="absolute right-2 top-2 size-1.5 rounded-full bg-primary" />}
            </article>
          );
        })}
      </div>
      {participants.length > 2 && (
        <button type="button" className="mt-2 text-[10px] font-medium text-primary sm:hidden" onClick={onToggleAllMembers}>
          {showAllMembers ? "Show priority mailboxes" : `Show all ${participants.length} mailboxes`}
        </button>
      )}
    </section>
  );
}

function LeadInbox({
  messages,
  members,
  actionsEnabled,
  onAnswer,
  onAcknowledge,
}: {
  messages: TeamMessage[];
  members: Map<string, MemberRun>;
  actionsEnabled: boolean;
  onAnswer: (message: TeamMessage) => void;
  onAcknowledge: (message: TeamMessage) => void;
}) {
  return (
    <section aria-label="Lead Inbox" className="border-b border-border/70 py-3">
      <header className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className="grid size-7 place-items-center rounded-lg border border-primary/20 bg-primary/[0.06] text-primary">
            <Inbox className="size-3.5" />
          </span>
          <div>
            <h2 className="text-[12px] font-semibold text-foreground">Lead Inbox</h2>
            <p className="text-[10px] text-muted-foreground">Every Member message addressed to the Host, preserving its conversation thread and delivery state.</p>
          </div>
        </div>
        <Badge tone={messages.some((message) => hostDeliveryStatus(message) === "delivered") ? "warn" : "muted"}>
          {messages.filter((message) => hostDeliveryStatus(message) === "delivered").length} unread
        </Badge>
      </header>
      {messages.length === 0 ? (
        <p className="rounded-lg border border-dashed border-border px-3 py-3 text-[11px] text-muted-foreground">Inbox is clear. Provider pauses remain in PendingInteraction, not this coordination queue.</p>
      ) : (
        <div className="divide-y divide-border/60 overflow-hidden rounded-lg border border-border/70 bg-card">
          {messages.slice(0, 8).map((message) => {
            const delivery = hostDelivery(message);
            const canAnswer = Boolean(message.correlation_id && message.from_member_id && message.from_member_id !== "host");
            return (
              <article key={message.id} className="grid gap-2 px-3 py-2.5 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge>
                    <span className="text-[11px] font-semibold text-foreground">{teamMessageActorLabel(message, members)}</span>
                    <span className="text-[10px] text-muted-foreground">{formatTime(message.created_at)}</span>
                  </div>
                  <p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-foreground/85">{message.body || "No message body"}</p>
                  <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-[9px] uppercase tracking-wider text-muted-foreground">
                    <span>correlation · {message.correlation_id ? shortId(message.correlation_id) : "missing"}</span>
                    <span>caused by · {message.causation_id ? shortId(message.causation_id) : "root message"}</span>
                    <span>delivery · {delivery?.policy ?? "unknown"} / {delivery?.status ?? "unknown"}</span>
                    {effectiveTeamMessageResponseIntent(message) === "response_required" ? (
                      <span className="text-status-warn">intent · response required</span>
                    ) : (
                      <span>intent · informational</span>
                    )}
                    {delivery?.provider_receipt_id && <span className="text-status-good">provider receipt · {shortId(delivery.provider_receipt_id)}</span>}
                    {delivery?.status === "acknowledged" && <span className="text-status-good">ACK</span>}
                  </div>
                </div>
                <div className="flex items-center justify-end gap-1.5">
                  {delivery?.status === "delivered" && (
                    <Button size="sm" variant="secondary" disabled={!actionsEnabled} onClick={() => onAcknowledge(message)}>ACK</Button>
                  )}
                  <Button
                    size="sm"
                    disabled={!actionsEnabled || !canAnswer}
                    title={canAnswer ? "Reply in this correlated conversation" : "Cannot answer until this message has a conversation correlation"}
                    onClick={() => onAnswer(message)}
                  >
                    <MessageSquare className="size-3.5" /> Answer
                  </Button>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
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

function MemberControl({ member, selected, work, currentAction, livePreview, terminal, className, onSelect, onOpen }: {
  member: MemberRun;
  selected: boolean;
  work?: string;
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
        <p className="truncate text-foreground"><span className="text-muted-foreground">Now · </span>{currentAction ?? work ?? "No durable action yet"}</p>
        {livePreview && <p className="truncate text-status-info"><span className="font-semibold">Live · </span>{livePreview}</p>}
        {!blocked && <p className="truncate text-muted-foreground">{pressureLabel(member.status)} · {relativeTime(member.last_event_at ?? member.finished_at ?? member.started_at)}</p>}
      </div>
    </article>
  );
}

function memberPressureRank(status?: string | null): number {
  if (["blocked", "failed"].includes(status ?? "")) return 0;
  if (status === "disconnected") return 1;
  if (["waiting", "reviewing"].includes(status ?? "")) return 1;
  if (status === "running") return 2;
  if (status === "idle") return 3;
  if (status === "completed") return 4;
  return 5;
}

function MissionTeamModule({ missionTitle, teamName, leadAgentId, missionScoped, members, onOpenMember, onOpen }: {
  missionTitle?: string;
  teamName?: string;
  leadAgentId?: string;
  missionScoped: boolean;
  members: MemberRun[];
  onOpenMember: (member: MemberRun) => void;
  onOpen: () => void;
}) {
  return (
    <ContextModule
      title={teamName ?? "Independent Agent Team"}
      kicker={missionScoped ? "Mission-linked team" : "Agent Team"}
      tone="good"
      action={missionTitle ? <button type="button" onClick={onOpen} className="text-[11px] font-medium text-primary hover:underline">Open Mission</button> : undefined}
    >
      <div className="space-y-1.5">
        <div className="flex items-center gap-2 rounded-lg border border-border/60 bg-background/65 px-2 py-1.5">
          <Avatar name="Host Lead" tone="info" size="xs" />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[11px] font-semibold text-foreground">{teamLeadLabel(leadAgentId)}</span>
            <span className="block text-[9px] uppercase tracking-wider text-muted-foreground">Lead · outside MemberRuns</span>
          </span>
        </div>
        {members.slice(0, 4).map((member) => (
          <button
            key={member.id}
            type="button"
            onClick={() => onOpenMember(member)}
            className="flex w-full items-center gap-2 rounded-lg border border-transparent px-2 py-1 text-left transition-colors hover:border-border/70 hover:bg-background/75"
          >
            <Avatar name={member.name ?? member.id} tone={memberTone(member.status)} size="xs" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[11px] font-medium text-foreground">{member.name ?? member.id}</span>
              <span className="block truncate text-[9px] text-muted-foreground">{member.role ?? "member"}</span>
            </span>
            <span className={cn("text-[9px] font-medium", member.status === "completed" ? "text-status-good" : member.status === "running" ? "text-status-info" : "text-muted-foreground")}>{member.status ?? "unknown"}</span>
          </button>
        ))}
        {members.length > 4 && <p className="px-2 text-[9px] text-muted-foreground">+{members.length - 4} more members</p>}
      </div>
      <p className="mt-2 line-clamp-2 text-[10px] leading-relaxed text-muted-foreground">
        {missionTitle
          ? `${missionScoped ? "Mission-scoped" : "Linked"} · ${missionTitle}. Sessions may continue across Host-plan Waves.`
          : "Independent Team. Link it to a Mission when useful."}
      </p>
    </ContextModule>
  );
}

function teamLeadLabel(leadAgentId?: string | null): string {
  if (!leadAgentId || leadAgentId === "host") return "Current Host Agent";
  return leadAgentId;
}

function WaveModule({ wave, directExecutor, onOpen }: { wave?: Wave; directExecutor: boolean; onOpen: () => void }) {
  if (!wave) return <ContextModule title="No Wave selected" kicker="Host plan"><p className="text-[11px] text-muted-foreground">Open this Team from a Mission Wave to carry that planning context into the console.</p></ContextModule>;
  return (
    <ContextModule title={`Wave ${wave.index} · ${wave.title}`} kicker={directExecutor ? "Legacy direct executor" : "Current Host plan"} tone={waveTone(wave.status)} action={<button type="button" onClick={onOpen} className="text-[11px] font-medium text-primary hover:underline">Open</button>}>
      <p className="text-[12px] leading-relaxed text-foreground">{wave.objective}</p>
      <div className="mt-2 flex flex-wrap gap-1"><Badge tone={gateTone(wave.gate_status)}>decision {wave.gate_status ?? "pending"}</Badge><Badge tone="muted">revision {wave.revision ?? 1}</Badge></div>
      {!directExecutor && <p className="mt-2 text-[11px] text-muted-foreground">Navigation context only: Works own execution and messages carry coordination; the Wave does not own this long-lived runtime.</p>}
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

function AttemptModule({ runId, status, attempt, previousRunId, hostSurface, hostThreadId, executionRoot, createdAt, completedAt }: { runId: string; status: string; attempt: number; previousRunId?: string | null; hostSurface?: string | null; hostThreadId?: string | null; executionRoot?: string | null; createdAt?: string; completedAt?: string | null }) {
  const hostBinding = hostSurface && hostThreadId ? `${hostSurface} · ${shortId(hostThreadId)}` : hostSurface ? `${hostSurface} · unbound` : "Not recorded";
  return <ContextModule title={`Attempt ${attempt}`} kicker="Attempt" tone={teamTone(status)}><div className="space-y-1.5 text-[11px]"><Fact label="Status" value={status} /><Fact label="Run" value={shortId(runId)} mono /><Fact label="Host binding" value={hostBinding} mono /><Fact label="Execution root" value={executionRoot ?? "Not recorded (legacy run)"} mono /><Fact label="Started" value={formatDate(createdAt)} />{previousRunId && <Fact label="Retry of" value={shortId(previousRunId)} mono />}{completedAt && <Fact label="Completed" value={formatDate(completedAt)} />}</div></ContextModule>;
}

function SelectedMemberModule({ member, work, currentAction, onMessage, onOpen }: { member?: MemberRun; work?: string; currentAction?: string; onMessage: () => void; onOpen: () => void }) {
  if (!member) return <ContextModule title="No member selected" kicker="Selected member"><p className="text-[11px] text-muted-foreground">Choose a member control to inspect its attempt-scoped context.</p></ContextModule>;
  return (
    <ContextModule title={member.name ?? member.id} kicker="Selected member" tone={memberTone(member.status)}>
      <div className="flex items-center gap-2"><Avatar name={member.name ?? member.id} tone={memberTone(member.status)} /><p className="min-w-0 truncate text-[11px] text-muted-foreground">{member.role ?? "member"} · {member.provider ?? "provider"}</p></div>
      <div className="mt-2 space-y-1.5 text-[11px]"><Fact label="Current Work" value={work ?? "No Work owned"} /><Fact label="Now" value={currentAction ?? "No durable action"} /><Fact label="Worktree override" value={member.worktree_ref ?? "None"} mono /><Fact label="Actual cwd" value={member.workspace_snapshot?.cwd ?? "Not captured (legacy run)"} mono /><Fact label="Native session" value={member.native_session?.native_session_id ?? "Not recorded"} mono /></div>
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

function toActivityItems(
  items: StableTeamActivity[],
  members: Map<string, MemberRun>,
  onOpenMember: (member: MemberRun) => void,
): TeamActivityItem[] {
  return items.map((item) => {
    const actor = item.sourceMemberRunId ? memberLabel(members, item.sourceMemberRunId) : "Host";
    if (item.kind === "message") {
      const message = item.message;
      const messageActor = teamMessageActorLabel(message, members);
      const recipients = (message.to_member_ids ?? []).map((id) => memberLabel(members, id)).join(", ") || "team";
      const evidenceRefs = message.evidence_refs ?? [];
      const actorMember = message.sender?.kind === "member_run" || !message.sender
        ? (message.from_member_id ? members.get(message.from_member_id) : undefined)
        : undefined;
      const deliverySummary = summarizeDeliveries(message, members);
      return {
        id: item.id,
        kind: message.kind === "blocker" ? "blocker" : message.kind === "review_result" ? "decision" : evidenceRefs.length ? "evidence" : "message",
        glyph: teamMessageGlyph(message.kind, evidenceRefs.length > 0),
        title: (
          <span className="flex flex-wrap items-center gap-2">
            <span>{messageActor} <span className="font-normal text-muted-foreground">to {recipients}</span></span>
            <Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge>
            {deliverySummary && <span className="text-[10px] font-normal text-muted-foreground">{deliverySummary}</span>}
          </span>
        ),
        body: message.body ? <Markdown source={message.body} compact /> : undefined,
        actor: message.correlation_id
          ? `work chain ${shortId(message.correlation_id)}${message.causation_id ? ` · reply to ${shortId(message.causation_id)}` : ""}`
          : undefined,
        timestamp: formatTime(message.created_at),
        evidenceRefs,
        tone: messageTone(message.kind),
        actorAvatarName: messageActor,
        actorTone: actorMember ? memberTone(actorMember.status) : "info",
        onActorClick: actorMember ? () => onOpenMember(actorMember) : undefined,
        relatedMemberIds: [
          ...(message.from_member_id ? [message.from_member_id] : []),
          ...(message.to_member_ids ?? []),
        ],
        rawText: `${message.kind ?? ""} ${message.body ?? ""} ${messageActor} ${recipients} ${message.correlation_id ?? ""} ${message.causation_id ?? ""}`,
        actorLabel: messageActor,
        statusLabel: deliverySummary,
        messageKind: message.kind ?? "message",
        bodySource: message.body ?? undefined,
        recipientLabels: (message.to_member_ids ?? []).map((id) => memberLabel(members, id)),
        workId: message.work_id ?? undefined,
        prominence: KEY_ACTIVITY_MESSAGE_KINDS.has(message.kind ?? "")
          ? "primary"
          : ["blocker", "review_request", "review_result"].includes(message.kind ?? "")
            ? "pressure"
            : "detail",
      };
    }
    if (item.kind === "action") {
      const action = item.action;
      const evidenceRefs = action.evidence_refs ?? [];
      const status = [action.provider_status, action.semantic_status].filter(Boolean).join(" · ");
      return { id: item.id, kind: evidenceRefs.length ? "evidence" : "action", glyph: evidenceRefs.length ? "artifact" : "runtime", title: action.title ?? action.action_type ?? "Member action", body: status ? <><span>{action.summary}</span><span className="mt-1 block text-[10px] text-muted-foreground">Harness action · provider {action.provider_status ?? "unknown"} · semantic {action.semantic_status ?? "not classified"}</span></> : action.summary, actor, timestamp: formatTime(action.started_at ?? action.completed_at), evidenceRefs, tone: action.status === "failed" ? "bad" : action.status === "succeeded" ? "good" : "running", prominence: "detail", relatedMemberIds: item.sourceMemberRunId ? [item.sourceMemberRunId] : [], rawText: `${action.title ?? ""} ${action.summary ?? ""} ${actor}`, actorLabel: actor };
    }
    if (item.kind === "work_event") {
      const event = item.workEvent;
      const pressure = event.kind.includes("block")
        || event.kind.includes("cancel")
        || event.kind === "changes_requested";
      const accepted = event.kind.includes("accept") || event.kind.includes("complete");
      return {
        id: item.id,
        kind: pressure ? "blocker" : accepted ? "decision" : "action",
        glyph: accepted ? "complete" : pressure ? "review" : "runtime",
        title: `Work ${event.kind.replace(/_/g, " ")}`,
        body: `Version ${event.expected_version} → ${event.resulting_version}`,
        actor,
        actorLabel: actor,
        timestamp: formatTime(event.created_at),
        tone: pressure ? "bad" : accepted ? "good" : "info",
        prominence: pressure ? "pressure" : "detail",
        relatedMemberIds: item.sourceMemberRunId ? [item.sourceMemberRunId] : [],
        rawText: `${event.kind} ${event.work_id} ${actor}`,
        workId: event.work_id,
      };
    }
    if (item.kind === "work_delivery") {
      const delivery = item.workDelivery;
      const recipient = memberLabel(members, delivery.recipient_member_run_id);
      const pressure = delivery.status === "failed";
      const deliveryBody = pressure && delivery.failure_reason
        ? `Work v${delivery.work_version} · attempt ${delivery.attempt} · ${delivery.failure_reason}`
        : `Work v${delivery.work_version} · attempt ${delivery.attempt}`;
      return {
        id: item.id,
        kind: pressure ? "blocker" : "action",
        glyph: pressure ? "review" : delivery.status === "provider_received" ? "complete" : "queued",
        title: `Work delivery ${delivery.status.replace(/_/g, " ")}`,
        body: deliveryBody,
        actor: `to ${recipient}`,
        actorLabel: recipient,
        timestamp: formatTime(delivery.updated_at),
        tone: pressure ? "bad" : delivery.status === "provider_received" ? "good" : "info",
        prominence: pressure ? "pressure" : "detail",
        relatedMemberIds: [delivery.recipient_member_run_id],
        rawText: `${delivery.status} ${delivery.work_id} ${recipient} ${delivery.failure_reason ?? ""}`,
        workId: delivery.work_id,
      };
    }
    const event = item.event;
    const decision = event.entity_type === "wave" || event.operation === "completed" || /gate|decision/i.test(event.summary ?? "");
    return { id: item.id, kind: decision ? "decision" : "action", glyph: decision ? "decision" : "runtime", title: event.summary ?? `${event.entity_type ?? "Team"} ${event.operation ?? "updated"}`, actor, timestamp: formatTime(event.occurred_at), tone: decision ? "decision" : "info", prominence: "detail", relatedMemberIds: item.sourceMemberRunId ? [item.sourceMemberRunId] : [], rawText: `${event.summary ?? ""} ${actor}`, actorLabel: actor };
  });
}

function summarizeDeliveries(message: TeamMessage, members: Map<string, MemberRun>): string | undefined {
  const deliveries = message.deliveries ?? [];
  if (!deliveries.length) return undefined;
  const acknowledged = deliveries.filter((delivery) => delivery.status === "acknowledged").length;
  const delivered = deliveries.filter((delivery) => delivery.status === "delivered").length;
  const queued = deliveries.filter((delivery) => delivery.status === "queued").length;
  const claimed = deliveries.filter((delivery) => delivery.status === "claimed").length;
  const nextRoundBatched = deliveries.filter((delivery) =>
    delivery.status === "queued"
    && delivery.member_id
    && members.get(delivery.member_id)?.provider_profile?.ordinary_message_boundary === "next_round_batched"
  ).length;
  if (acknowledged === deliveries.length) return `ACK ${acknowledged}/${deliveries.length}`;
  if (claimed) return `${claimed} provider receipt pending${queued ? ` · ${queued} queued` : ""}`;
  if (queued) {
    // Informational mail never starts a provider round on its own (ADR 0046
    // §4): show that it waits for the next response-required trigger instead
    // of implying an imminent delivery.
    if (effectiveTeamMessageResponseIntent(message) === "informational") {
      return `${queued} informational · batched on next response round${delivered || acknowledged ? ` · ${delivered + acknowledged} received` : ""}`;
    }
    const queuedLabel = nextRoundBatched === queued
      ? `${queued} response required · next provider round`
      : nextRoundBatched
        ? `${queued} response required · ${nextRoundBatched} next-round`
        : `${queued} response required · queued`;
    return `${queuedLabel}${delivered || acknowledged ? ` · ${delivered + acknowledged} received` : ""}`;
  }
  return `${delivered} delivered${acknowledged ? ` · ${acknowledged} ACK` : ""}`;
}

function teamMessageGlyph(kind?: string | null, hasEvidence = false): WorkbenchActivityItem["glyph"] {
  if (hasEvidence) return "artifact";
  switch (kind) {
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
function teamMessageActorLabel(message: TeamMessage, members: Map<string, MemberRun>): string {
  if (message.sender?.display_name) return message.sender.display_name;
  if (message.sender?.kind === "operator") return "Operator";
  if (message.sender?.kind === "service") return `Service · ${message.sender.id}`;
  if (message.sender?.kind === "agent_member") return `Agent · ${message.sender.id}`;
  return memberLabel(members, message.from_member_id ?? message.sender?.id ?? "host");
}
function messageSenderParticipantId(message: TeamMessage): string | undefined {
  if (message.sender?.kind === "operator" || message.sender?.kind === "service") return undefined;
  if (message.sender?.kind === "host") return "host";
  return message.from_member_id;
}
function shortId(value: string): string { return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-5)}` : value; }
function hostDelivery(message: TeamMessage) { return message.deliveries?.find((delivery) => delivery.member_id === "host"); }
function hostDeliveryStatus(message: TeamMessage): string | undefined { return hostDelivery(message)?.status; }
function timestamp(value?: string | null): number { if (!value) return 0; return value.startsWith("unix-ms:") ? Number(value.slice(8)) || 0 : Date.parse(value) || 0; }
function formatDate(value?: string | null): string { if (!value) return "Not recorded"; const ms = timestamp(value); return ms ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(ms) : value; }
function formatTime(value?: string | null): string { if (!value) return "—"; const ms = timestamp(value); return ms ? new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(ms) : value; }
function relativeTime(value?: string | null): string { const ms = timestamp(value); if (!ms) return "no update"; const delta = Math.max(0, Date.now() - ms); if (delta < 60_000) return "just now"; if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`; if (delta < 86_400_000) return `${Math.floor(delta / 3_600_000)}h ago`; return `${Math.floor(delta / 86_400_000)}d ago`; }
function pressureLabel(status?: string | null): string { if (["blocked", "failed"].includes(status ?? "")) return "blocked"; if (["waiting", "reviewing"].includes(status ?? "")) return "waiting"; if (status === "running") return "active"; return status ?? "idle"; }
function teamTone(status?: string | null): StatusTone { if (status === "running") return "running"; if (status === "completed") return "good"; if (["failed", "cancelled"].includes(status ?? "")) return "bad"; if (["waiting", "reviewing"].includes(status ?? "")) return "warn"; if (status === "planning") return "info"; return "idle"; }
function memberTone(status?: string | null): StatusTone { if (status === "running") return "running"; if (status === "completed") return "good"; if (["blocked", "failed", "stopped"].includes(status ?? "")) return "bad"; if (["waiting", "reviewing", "disconnected"].includes(status ?? "")) return "warn"; if (["queued", "starting"].includes(status ?? "")) return "info"; return "idle"; }
function workTone(status?: string | null): StatusTone { if (status === "done") return "good"; if (status === "cancelled") return "bad"; if (status === "blocked") return "warn"; if (status === "in_progress") return "running"; if (status === "review") return "info"; return "idle"; }
function waveTone(status?: string | null): StatusTone { if (status === "completed") return "good"; if (["blocked", "failed", "cancelled"].includes(status ?? "")) return "bad"; if (["waiting"].includes(status ?? "")) return "warn"; if (status === "running") return "running"; return "info"; }
function gateTone(status?: string | null): StatusTone { if (status === "accepted") return "good"; if (status === "blocked") return "bad"; if (status === "revise") return "warn"; return "decision"; }
function messageTone(kind?: string | null): StatusTone { if (kind === "blocker") return "bad"; if (["review_request", "plan_feedback"].includes(kind ?? "")) return "warn"; if (["review_result", "answer", "plan_approval"].includes(kind ?? "")) return "good"; if (["handoff", "question", "plan_proposal"].includes(kind ?? "")) return "decision"; if (kind === "progress") return "running"; if (["broadcast", "plan_request"].includes(kind ?? "")) return "info"; return "idle"; }
