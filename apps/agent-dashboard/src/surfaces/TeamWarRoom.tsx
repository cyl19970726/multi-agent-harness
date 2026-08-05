import { useEffect, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Columns3,
  ExternalLink,
  Inbox,
  ListFilter,
  MessageSquare,
  Play,
  Plus,
  Search,
  Send,
  ShieldAlert,
  ShieldCheck,
  Users,
  X,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { providerDisplayName, providerStackLine, memberModelLabel, TEAM_MEMBER_PROVIDER_MODES } from "@/lib/provider";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Avatar } from "@/components/workbench/Avatar";
import { ContextModule, ContextRail } from "@/components/workbench/context/ContextRail";
import { ReadinessMeter } from "@/components/workbench/execution/ExecutionPrimitives";
import { FocusHeader, FocusShell } from "@/components/workbench/layout/FocusShell";
import { EmptyState } from "@/components/workbench/atoms";
import { Dialog, DialogFooter, Field, Select, TextArea, TextInput } from "@/components/workbench/OperatorForms";

import { TeamCapacityStrip } from "@/components/workbench/team/TeamCapacityStrip";
import { TeamConversationStream } from "@/components/workbench/team/TeamConversation";
import { LeadInbox, TeamCoordinationPressure, TeamMailboxStrip } from "@/components/workbench/team/TeamMailboxes";
import { TeamMembersCapacity } from "@/components/workbench/team/TeamMembersCapacity";
import { TeamWorksBoard } from "@/components/workbench/team/TeamWorksBoard";
import {
  FILTERS,
  matchesFilter,
  toActivityItems,
  toInteractionActivity,
  type StreamFilter,
  type TeamActivityItem,
} from "@/components/workbench/team/teamActivityItems";
import {
  attemptNumber,
  formatDate,
  gateTone,
  hostDeliveryStatus,
  memberPressureRank,
  memberTone,
  shortId,
  teamLeadLabel,
  teamTone,
  timestamp,
  waveTone,
} from "@/components/workbench/team/teamFormat";

import {
  selectTeamCapacity,
  selectTeamRunContext,
} from "../model/teamSelectors";
import { buildAgentTeamOrgModel, orgTeamPath } from "../model/orgSelectors";
import type { WorkbenchModel } from "../model/readModel";
import { acknowledgeTeamMessage, addTeamMember, resolvePendingInteraction, sendTeamMessage, startTeamRun, transitionTeamRun, type ActionDescriptor } from "../api/actions";
import type { MemberRun, TeamMessage, TeamMessageResponseIntent, Wave, Work } from "../types";
import type { SelectionState } from "../app/selection";

export interface TeamWarRoomProps {
  model: WorkbenchModel;
  teamRunId?: string;
  /** Optional deep-linked Team Work row opened from the global aggregate. */
  workId?: string;
  /** Optional navigation context. A Mission-scoped TeamRun is not owned by it. */
  missionId?: string;
  waveId?: string;
  onSelectionChange: (selection: Partial<SelectionState>) => void;
  actionsEnabled?: boolean;
  onAction?: (path: string, body?: unknown) => void | Promise<boolean>;
}

type ComposerTarget = "team" | string;
type TeamView = "works" | "activity" | "members";

const TEAM_VIEWS = [
  { id: "works", label: "Works", icon: Columns3 },
  { id: "activity", label: "Activity", icon: MessageSquare },
  { id: "members", label: "Members", icon: Users },
] as const;

/**
 * One operational view of one AgentTeamRun. New runs are independent or
 * Mission-scoped and may span Host-plan Waves; a selected Wave is navigation
 * context only. Legacy direct-Wave attempts remain readable without inventing
 * a dependency graph.
 */
export function TeamWarRoom({
  model,
  teamRunId,
  workId,
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
  const [composerOpen, setComposerOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [kind, setKind] = useState("message");
  const [responseIntent, setResponseIntent] = useState<TeamMessageResponseIntent>("response_required");
  const [addMemberOpen, setAddMemberOpen] = useState(false);
  const [replyAnchor, setReplyAnchor] = useState<TeamMessage>();
  const [composerWorkId, setComposerWorkId] = useState("");
  const [showAllMembers, setShowAllMembers] = useState(false);
  const [showFullActivity, setShowFullActivity] = useState(false);
  const [teamView, setTeamView] = useState<TeamView>("works");
  const [selectedWorkId, setSelectedWorkId] = useState<string | undefined>(workId);
  const [starting, setStarting] = useState(false);
  const runStatus = context?.run.status;

  useEffect(() => {
    if (runStatus !== "planning") setStarting(false);
  }, [runStatus]);

  useEffect(() => {
    if (workId) {
      setSelectedWorkId(workId);
      setTeamView("works");
    }
  }, [workId]);

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
  const organization = buildAgentTeamOrgModel(model.snapshot);
  const organizationNode = stableTeam ? organization.nodesById.get(stableTeam.id) : undefined;
  const organizationPath = organizationNode ? orgTeamPath(organization, organizationNode.team.id) : [];
  const childTeams = organizationNode?.childTeamIds
    .map((id) => organization.nodesById.get(id))
    .filter((node): node is NonNullable<typeof node> => Boolean(node)) ?? [];
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
  const leadInboxUnread = leadInboxMessages.filter((message) => hostDeliveryStatus(message) === "delivered").length;
  const capacityTiles = selectTeamCapacity(members, works);
  const activeTurns = members.filter((member) => member.status === "running").length;
  const activityItems: TeamActivityItem[] = toActivityItems(context.activity, memberById, openMember).map((item) => {
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
          className="min-h-11 sm:min-h-0"
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

  function revealComposer(): void {
    setComposerOpen(true);
    document.getElementById("team-war-room-composer")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
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
    revealComposer();
  }

  function discussWork(work: Work): void {
    setSelectedWorkId(undefined);
    setReplyAnchor(undefined);
    setComposerWorkId(work.id);
    setKind("message");
    revealComposer();
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
      responseIntent,
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
      headerClassName="bg-background py-1.5 sm:py-1.5"
      composerClassName="bg-background py-2 shadow-[0_-12px_30px_-28px_rgba(15,23,42,0.55)] max-sm:px-2 max-sm:py-0"
      responsiveContextVariant="sheet"
      splitMobileToolbar
      header={
        <FocusHeader
          breadcrumb={
            <button
              type="button"
              onClick={() => onSelectionChange(organizationNode
                ? { surface: "organization", orgView: "agent-teams", orgTeamId: organizationNode.team.id, teamId: undefined, memberRunId: undefined, teamWorkId: undefined }
                : navigationMission
                  ? { surface: "missions", missionId: navigationMission.id, waveId: navigationWave?.id, teamId: undefined }
                  : { surface: "team", teamId: undefined })}
              className="inline-flex items-center gap-1 text-muted-foreground transition-colors hover:text-foreground"
            >
              {organizationPath.length > 0
                ? <>Organization <span className="text-border">/</span> {organizationPath.map((entry) => entry.team.name ?? entry.team.id).join(" / ")}</>
                : <>{navigationMission?.title ?? "Agent Teams"} <span className="text-border">/</span> {navigationWave ? `Wave ${navigationWave.index}` : "Team"}</>}
            </button>
          }
          title={stableTeam?.name ?? "Agent Team"}
          meta={
            // One scrollable row on narrow screens: wrapping these chips cost
            // three header rows at 320px and pushed real Work below the fold.
            <div className="flex w-full min-w-0 items-center gap-2 overflow-x-auto pb-0.5 [&>*]:shrink-0 [&>*]:whitespace-nowrap sm:flex-wrap sm:overflow-visible sm:[&>*]:whitespace-normal">
              <Badge tone={teamTone(status)}>{status}</Badge>
              <Badge tone="muted">attempt {attemptNumber(attempts, run.id)}</Badge>
              <Badge tone="muted">Lead · {teamLeadLabel(stableTeam?.owner_agent_id)}</Badge>
              <Badge
                tone={supervisorCurrent ? "good" : status === "running" ? "bad" : "muted"}
                title={supervisorCurrent
                  ? "A live Team Supervisor lease owns this run's control handles."
                  : status === "running"
                    ? "The run reports running, but no current Team Supervisor lease was observed. Cross-process control may be unavailable until a supervisor reattaches."
                    : "No supervisor lease is expected while the run is not active."}
              >
                Supervisor · {supervisorCurrent ? `live g${supervisor?.generation}` : "offline"}
              </Badge>
              {pendingCloseCount > 0 && <Badge tone="warn">Close pending · {pendingCloseCount}</Badge>}
              {navigationWave && <Badge tone={gateTone(navigationWave.gate_status)}>Host plan: Wave {navigationWave.index}</Badge>}
            </div>
          }
          actions={
            <>
              <span className="hidden items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground sm:inline-flex">
                <Users className="size-3.5" /> {activeTurns} running · {members.length} {members.length === 1 ? "member" : "members"}
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
          {/* The full composer costs 264px at 390px wide, which is most of the
              mobile work surface. It collapses to one reachable control until
              the operator actually wants to write. */}
          <div className={cn("sm:hidden", composerOpen && "hidden")}>
            <Button className="h-11 w-full rounded-none border-0 bg-transparent text-foreground shadow-none hover:bg-accent" variant="secondary" onClick={() => setComposerOpen(true)}>
              <MessageSquare className="size-3.5 text-primary" /> Message team
            </Button>
          </div>
          {/* One row from the tablet breakpoint up. The 2x2 fallback only
              applies between 640px and 768px, where five columns genuinely do
              not fit. */}
          <div
            data-composer-controls="true"
            className={cn(
              "grid min-w-0 gap-2 sm:grid-cols-2 md:grid-cols-[7rem_6rem_7rem_8rem_minmax(8rem,1fr)_auto] lg:grid-cols-[9rem_7rem_8rem_11rem_minmax(12rem,1fr)_auto]",
              !composerOpen && "hidden sm:grid",
            )}
          >
            <Select
              aria-label="Message recipient"
              value={composerTarget}
              onChange={(event) => setComposerTarget(event.target.value)}
              className="h-11 w-full sm:h-9"
              disabled={Boolean(replyAnchor)}
            >
              <option value="team">Team · all members</option>
              {members.map((member) => <option key={member.id} value={member.id}>{member.name ?? member.id}</option>)}
            </Select>
            <Select aria-label="Message kind" value={kind} onChange={(event) => setKind(event.target.value)} className="h-11 w-full sm:h-9" disabled={Boolean(replyAnchor)}>
              <option value="message">Message</option>
              <option value="handoff">Handoff</option>
            </Select>
            <Select
              aria-label="Response intent"
              value={responseIntent}
              onChange={(event) => setResponseIntent(event.target.value as TeamMessageResponseIntent)}
              className="h-11 w-full sm:h-9"
            >
              <option value="response_required">Needs reply</option>
              <option value="informational">Informational</option>
            </Select>
            <Select
              aria-label="Related Work"
              value={replyAnchor?.work_id ?? composerWorkId}
              onChange={(event) => setComposerWorkId(event.target.value)}
              className="h-11 w-full sm:h-9"
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
              className="min-h-11 resize-none py-2 sm:min-h-9"
              rows={1}
              disabled={!actionsEnabled}
            />
            <div className="flex gap-2">
              <Button size="sm" className="h-11 flex-1 sm:h-9" onClick={submit} disabled={!canSend} title={actionsEnabled ? undefined : "Connect a live source to enable actions"}>
                <Send className="size-3.5" /> Send
              </Button>
              <Button size="sm" variant="secondary" className="h-11 sm:hidden" aria-label="Hide composer" onClick={() => setComposerOpen(false)}>
                <X className="size-3.5" />
              </Button>
            </div>
          </div>
          <p className="hidden text-center text-[9px] text-muted-foreground sm:block">Host coordination only · Member-originated messages come from their provider session.</p>
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
        <TeamCapacityStrip tiles={capacityTiles} />

        {/* The pressure row is the affordance that opens Activity, so it is
            redundant once Activity is the visible panel — and at 320px it cost
            the conversation its place in the first viewport. */}
        {teamView !== "activity" && (
          <TeamCoordinationPressure
            className="mt-2"
            members={members}
            messages={leadInboxMessages}
            pendingInteractions={pendingInteractions.length}
            onOpenActivity={() => {
              setTeamView("activity");
              setFilter("all");
              setParticipantFilter("all");
            }}
          />
        )}

        {childTeams.length > 0 && (
          <section className="mt-2 rounded-xl border border-border bg-card/65 px-3 py-2" aria-label="Child Agent Teams" data-team-child-count={childTeams.length}>
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Child Teams</span>
              {childTeams.map((child) => (
                <button
                  key={child.team.id}
                  type="button"
                  disabled={!child.latestRunId}
                  onClick={() => child.latestRunId && onSelectionChange({ surface: "team", teamId: child.latestRunId, memberRunId: undefined, teamWorkId: undefined })}
                  className="inline-flex min-h-11 items-center gap-1 rounded-lg border border-border bg-background px-3 text-xs font-medium text-foreground disabled:text-muted-foreground sm:min-h-8"
                  title={child.latestRunId ? "Open child Team War Room" : "No TeamRun exists for this child Team"}
                >
                  {child.team.name ?? child.team.id}<ChevronRight className="size-3" />
                </button>
              ))}
            </div>
          </section>
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

        {/* Semantic tabs come first so switching views never displaces the tab
            row, and so the selected panel starts in the first viewport. */}
        <Tabs value={teamView} onValueChange={(value) => setTeamView(value as TeamView)} className="mt-2">
          <TabsList
            aria-label="Team workspace"
            className="h-auto w-full justify-start gap-1 rounded-none border-0 border-b border-border/70 bg-transparent p-0"
          >
            {TEAM_VIEWS.map((entry) => {
              const Icon = entry.icon;
              const count = entry.id === "works"
                ? works.filter((item) => !["done", "cancelled"].includes(item.status)).length
                : entry.id === "activity"
                  ? activityItems.length
                  : members.length;
              return (
                <TabsTrigger
                  key={entry.id}
                  value={entry.id}
                  className="relative h-11 rounded-none px-3 text-xs font-medium text-muted-foreground shadow-none data-[state=active]:bg-transparent data-[state=active]:text-foreground data-[state=active]:shadow-none data-[state=active]:after:absolute data-[state=active]:after:inset-x-2 data-[state=active]:after:bottom-0 data-[state=active]:after:h-0.5 data-[state=active]:after:rounded-full data-[state=active]:after:bg-primary sm:h-10"
                >
                  <Icon className="size-3.5" />
                  {entry.label}
                  <span className="rounded-full bg-muted px-1.5 py-0.5 text-[9px] tabular-nums">{count}</span>
                </TabsTrigger>
              );
            })}
          </TabsList>

          <TabsContent value="works" className="mt-0">
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
          </TabsContent>

          <TabsContent value="activity" className="mt-0">
            <section className="overflow-hidden bg-background" data-testid="team-conversation">
              {/* Mailbox tiles are a desktop affordance; on mobile the compact
                  pressure row above already carries the same counts. */}
              <div className="hidden sm:block">
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
              </div>

              {/* The standalone Lead Inbox measured 644-1351px and pushed the
                  conversation out of every first viewport. It stays one click
                  away instead of owning the fold. */}
              <details className="group border-b border-border/70">
                <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 py-2 text-[12px] font-semibold text-foreground marker:content-none">
                  <span className="grid size-7 place-items-center rounded-lg border border-primary/20 bg-primary/[0.06] text-primary">
                    <Inbox className="size-3.5" />
                  </span>
                  Lead Inbox
                  <Badge tone={leadInboxUnread ? "warn" : "muted"}>{leadInboxUnread} unread</Badge>
                  <ChevronDown className="ml-auto size-3.5 text-muted-foreground transition-transform group-open:rotate-180" />
                </summary>
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
                    revealComposer();
                  }}
                  onAcknowledge={(message) => dispatch(onAction, acknowledgeTeamMessage(run.id, message.id, "host"))}
                />
              </details>

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
            </section>
          </TabsContent>

          <TabsContent value="members" className="mt-0">
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border/60 px-1 py-2">
              <p className="text-[11px] text-muted-foreground">
                Attempt-scoped roster; new members queue for the live Supervisor.
              </p>
              <Button
                size="sm"
                variant="secondary"
                disabled={!actionsEnabled}
                title={actionsEnabled ? undefined : "Connect a live source to enable actions"}
                onClick={() => setAddMemberOpen(true)}
              >
                <Plus className="size-3.5" /> Add member
              </Button>
            </div>
            <TeamMembersCapacity
              members={orderedMembers}
              works={works}
              selectedMemberId={selectedMember?.id}
              liveActivityByMember={liveActivityByMember}
              currentActionFor={(memberId) => latestActionTitle(actions, memberId)}
              onSelect={selectMember}
              onOpen={openMember}
            />
            <AddMemberDialog
              open={addMemberOpen}
              teamRunId={run.id}
              actionsEnabled={actionsEnabled}
              onAction={onAction}
              onClose={() => setAddMemberOpen(false)}
            />
          </TabsContent>
        </Tabs>
      </div>
    </FocusShell>
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
    return <Button size="sm" className="min-h-11 sm:min-h-0" onClick={onStart} disabled={!actionsEnabled || starting} title={actionsEnabled ? undefined : "Connect a live source to enable actions"}><Play className="size-3.5" /> {starting ? "Starting…" : "Start attempt"}</Button>;
  }
  if (["planning", "waiting", "reviewing"].includes(status)) {
    return (
      <>
        {status === "reviewing" && <Button size="sm" className="min-h-11 sm:min-h-0" onClick={onComplete} disabled={!actionsEnabled}><CheckCircle2 className="size-3.5" /> Complete attempt</Button>}
        <Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" onClick={onCancel} disabled={!actionsEnabled}><X className="size-3.5" /> Stop attempt</Button>
      </>
    );
  }
  return null;
}

/**
 * Add one member to this TeamRun (POST /v1/team-runs/{id}/members). Selecting
 * a provider auto-fills its registered execution mode; creation only queues the
 * member — a live Supervisor picks it up, otherwise it waits until the run is
 * (re)started or the member is reopened.
 */
function AddMemberDialog({
  open,
  teamRunId,
  actionsEnabled,
  onAction,
  onClose,
}: {
  open: boolean;
  teamRunId: string;
  actionsEnabled: boolean;
  onAction: TeamWarRoomProps["onAction"];
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [role, setRole] = useState("");
  const [provider, setProvider] = useState(TEAM_MEMBER_PROVIDER_MODES[0].provider);
  const [model, setModel] = useState("");
  const [resumeSessionId, setResumeSessionId] = useState("");
  const [initialWork, setInitialWork] = useState("");

  useEffect(() => {
    if (open) {
      setName("");
      setRole("");
      setProvider(TEAM_MEMBER_PROVIDER_MODES[0].provider);
      setModel("");
      setResumeSessionId("");
      setInitialWork("");
    }
  }, [open]);

  const selectedMode = TEAM_MEMBER_PROVIDER_MODES.find((entry) => entry.provider === provider)?.mode;
  const valid = Boolean(name.trim() && role.trim() && provider);
  const submit = () => {
    if (!valid) return;
    const descriptor = addTeamMember({
      teamRunId,
      name: name.trim(),
      role: role.trim(),
      provider,
      model: model.trim() || undefined,
      executionMode: selectedMode,
      resumeNativeSessionId: resumeSessionId.trim() || undefined,
      initialWork: initialWork.trim() || undefined,
    });
    void onAction?.(descriptor.path, descriptor.body);
    onClose();
  };

  return (
    <Dialog
      open={open}
      title="Add member to this run"
      description="Adds one MemberRun to this attempt's roster."
      onClose={onClose}
    >
      <form
        className="space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label="Name" required>
          {(id) => <TextInput id={id} value={name} onChange={(event) => setName(event.target.value)} />}
        </Field>
        <Field label="Role" required>
          {(id) => (
            <TextInput
              id={id}
              value={role}
              onChange={(event) => setRole(event.target.value)}
              placeholder="implementer, reviewer, researcher…"
            />
          )}
        </Field>
        <Field
          label="Provider"
          required
          hint={selectedMode ? `Execution mode auto-fills as ${selectedMode}.` : undefined}
        >
          {(id) => (
            <Select id={id} value={provider} onChange={(event) => setProvider(event.target.value)}>
              {TEAM_MEMBER_PROVIDER_MODES.map((entry) => (
                <option key={entry.provider} value={entry.provider}>
                  {entry.label} · {entry.mode}
                </option>
              ))}
            </Select>
          )}
        </Field>
        <Field label="Model" hint="Optional model override requested for this member.">
          {(id) => <TextInput id={id} value={model} onChange={(event) => setModel(event.target.value)} />}
        </Field>
        <Field label="Resume native session id" hint="Optional provider-native session to resume instead of starting fresh.">
          {(id) => (
            <TextInput
              id={id}
              value={resumeSessionId}
              onChange={(event) => setResumeSessionId(event.target.value)}
              className="font-mono text-[12px]"
            />
          )}
        </Field>
        <Field label="Initial work" hint="Optional first assignment; omit to create an idle, addressable member.">
          {(id) => <TextArea id={id} value={initialWork} onChange={(event) => setInitialWork(event.target.value)} />}
        </Field>
        <p className="rounded-md border border-border bg-muted/35 px-3 py-2 text-[11px] text-muted-foreground">
          A live Supervisor picks the queued member up; without one it waits until the run is (re)started or the member is reopened.
        </p>
        <DialogFooter
          submitLabel="Add member"
          actionsEnabled={actionsEnabled}
          canSubmit={valid}
          onCancel={onClose}
          onSubmit={submit}
        />
      </form>
    </Dialog>
  );
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
              <span className="block truncate text-[9px] text-muted-foreground">{member.role ?? "member"} · {providerDisplayName(member.provider)}</span>
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
      <div className="flex items-center gap-2"><Avatar name={member.name ?? member.id} tone={memberTone(member.status)} /><p className="min-w-0 truncate text-[11px] text-muted-foreground" title={`${member.role ?? "member"} · ${providerStackLine(member.provider, member.provider_profile?.execution_mode ?? member.native_session?.execution_mode, memberModelLabel(member))}`}>{member.role ?? "member"} · {providerStackLine(member.provider, member.provider_profile?.execution_mode ?? member.native_session?.execution_mode, memberModelLabel(member))}</p></div>
      <div className="mt-2 space-y-1.5 text-[11px]"><Fact label="Current Work" value={work ?? "No Work owned"} /><Fact label="Now" value={currentAction ?? "No durable action"} /><Fact label="Worktree override" value={member.worktree_ref ?? "None"} mono /><Fact label="Actual cwd" value={member.workspace_snapshot?.cwd ?? "Not captured (legacy run)"} mono title="Runs started before workspace capture was introduced did not record their cwd. Reopen the member to capture it." /><Fact label="Native session" value={member.native_session?.native_session_id ?? "Not recorded"} mono /><Fact label="Provider account" value={member.provider_capacity ? `${member.provider_capacity.state} · ${member.provider_capacity.evidence_source}` : "Not observed"} /></div>
      <div className="mt-3 flex gap-2"><Button size="sm" variant="secondary" onClick={onMessage}><MessageSquare className="size-3.5" /> Message</Button><Button size="sm" variant="secondary" onClick={onOpen}><ExternalLink className="size-3.5" /> Open member</Button></div>
    </ContextModule>
  );
}

function ResourcesModule({ members, delegationCount, liveCount }: { members: MemberRun[]; delegationCount: number; liveCount: number }) {
  const sessions = members.filter((member) => member.native_session).length;
  const worktrees = members.filter((member) => member.worktree_ref).length;
  const observedCapacity = members.filter((member) => member.provider_capacity).length;
  return <ContextModule title="Resources" kicker="Observed runtime"><div className="space-y-1.5 text-[11px]"><Fact label="Sessions" value={`${sessions} / ${members.length}`} /><Fact label="Worktrees" value={String(worktrees)} /><Fact label="Delegations" value={String(delegationCount)} /><Fact label="Live previews" value={String(liveCount)} /><Fact label="Capacity observed" value={`${observedCapacity} / ${members.length}`} /></div><p className="mt-2 text-[10px] text-muted-foreground">Observed resources only; no termination control is implied.</p></ContextModule>;
}

function Fact({ label, value, mono = false, title }: { label: string; value: string; mono?: boolean; title?: string }) {
  return <div className="grid grid-cols-[5.25rem_1fr] gap-2" title={title}><span className="text-muted-foreground">{label}</span><span className={cn("min-w-0 break-words text-foreground", mono && "font-mono text-[10px]")}>{value}</span></div>;
}

function dispatch(onAction: TeamWarRoomProps["onAction"], action: { path: string; body: unknown }): void | Promise<boolean> | undefined { return onAction?.(action.path, action.body); }
function memberLabel(members: Map<string, MemberRun>, id: string): string { return id === "host" ? "Host" : members.get(id)?.name ?? id; }
function latestActionTitle(actions: Array<{ member_run_id?: string; title?: string; action_type?: string; started_at?: string; completed_at?: string | null }>, memberId?: string): string | undefined {
  if (!memberId) return undefined;
  const ordered = actions
    .filter((action) => action.member_run_id === memberId)
    .sort((left, right) => timestamp(right.started_at ?? right.completed_at) - timestamp(left.started_at ?? left.completed_at));
  return ordered[0]?.title ?? ordered[0]?.action_type;
}
