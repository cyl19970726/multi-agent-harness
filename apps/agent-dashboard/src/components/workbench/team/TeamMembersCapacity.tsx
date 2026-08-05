import { SquareArrowOutUpRight } from "lucide-react";

import { cn } from "@/lib/utils";
import { memberModelLabel, providerStackLine } from "@/lib/provider";
import { Avatar } from "@/components/workbench/Avatar";
import { StatusDot } from "@/components/workbench/atoms";

import { canMemberAcceptWork, selectWorkOwnerMember } from "../../../model/teamSelectors";
import type { MemberRun, ProviderCapacitySnapshot, Work } from "../../../types";
import { formatAbsolute, memberTone, pressureLabel, relativeTime } from "./teamFormat";

/**
 * Members as a factual capacity list.
 *
 * Capacity means addressability plus real Work counts, and provider-account
 * capacity is labelled separately from Harness runtime state. There is no
 * utilisation percentage anywhere: none of the providers reports a limit the
 * Workbench could divide by, so none is displayed.
 */
export function TeamMembersCapacity({
  members,
  works,
  selectedMemberId,
  liveActivityByMember,
  currentActionFor,
  onSelect,
  onOpen,
}: {
  members: MemberRun[];
  works: Work[];
  selectedMemberId?: string;
  liveActivityByMember: Map<string, { preview?: string }>;
  currentActionFor: (memberId: string) => string | undefined;
  onSelect: (member: MemberRun) => void;
  onOpen: (member: MemberRun) => void;
}) {
  return (
    <section className="py-3" aria-label="Team members" data-testid="team-members-capacity">
      <header className="mb-2">
        <h2 className="text-sm font-semibold text-foreground">Member capacity</h2>
        <p className="mt-0.5 text-[11px] text-muted-foreground">
          Addressability and Work counts come from durable MemberRun and Work rows. Provider-account
          capacity is a separate, separately-labelled observation.
        </p>
      </header>
      <ul className="divide-y divide-border/60 overflow-hidden rounded-xl border border-border/70 bg-card">
        {members.map((member) => (
          <li key={member.id}>
            <MemberCapacityRow
              member={member}
              works={works}
              selected={selectedMemberId === member.id}
              livePreview={liveActivityByMember.get(member.id)?.preview}
              currentAction={currentActionFor(member.id)}
              onSelect={() => onSelect(member)}
              onOpen={() => onOpen(member)}
            />
          </li>
        ))}
        {members.length === 0 && (
          <li className="px-3 py-6 text-center text-[11px] text-muted-foreground">
            This attempt has no MemberRuns. Check whether the stable team definition is empty or run
            materialization failed.
          </li>
        )}
      </ul>
    </section>
  );
}

function MemberCapacityRow({
  member,
  works,
  selected,
  livePreview,
  currentAction,
  onSelect,
  onOpen,
}: {
  member: MemberRun;
  works: Work[];
  selected: boolean;
  livePreview?: string;
  currentAction?: string;
  onSelect: () => void;
  onOpen: () => void;
}) {
  const tone = memberTone(member.status);
  const owned = works.filter((work) => selectWorkOwnerMember(work, [member])?.id === member.id);
  const active = owned.filter((work) => work.status === "in_progress").length;
  const queued = owned.filter((work) => work.status === "open").length;
  const blocked = owned.filter((work) => work.status === "blocked").length;
  const review = owned.filter((work) => work.status === "review").length;
  const addressable = canMemberAcceptWork(member);
  return (
    <article
      className={cn(
        "grid gap-2 px-3 py-2.5 lg:grid-cols-[minmax(0,15rem)_minmax(0,1fr)_auto] lg:items-center",
        selected && "bg-primary/[0.035]",
        member.status === "blocked" && "bg-status-bad/[0.035]",
      )}
    >
      <button type="button" onClick={onSelect} className="flex min-w-0 items-center gap-2 text-left">
        <Avatar name={member.name ?? member.id} tone={tone} />
        <span className="min-w-0">
          <span className="flex min-w-0 items-center gap-1.5">
            <span className="truncate text-[12px] font-semibold text-foreground">{member.name ?? member.id}</span>
            <StatusDot tone={tone} pulse={tone === "running"} />
          </span>
          <span className="mt-0.5 block truncate text-[10px] text-muted-foreground" title={providerStackLine(member.provider, member.provider_profile?.execution_mode ?? member.native_session?.execution_mode, memberModelLabel(member))}>
            {member.role ?? "member"} · {providerStackLine(member.provider, member.provider_profile?.execution_mode ?? member.native_session?.execution_mode, memberModelLabel(member))}
          </span>
        </span>
      </button>

      <div className="grid grid-cols-2 gap-x-3 gap-y-1 sm:grid-cols-4">
        <CapacityFact label="Addressable" value={addressable ? "Yes" : "No"} tone={addressable ? undefined : "warn"} />
        <CapacityFact label="Active" value={String(active)} />
        <CapacityFact label="Queued" value={String(queued)} />
        <CapacityFact label="Review" value={String(review)} tone={review ? "warn" : undefined} />
        <CapacityFact label="Blocked" value={String(blocked)} tone={blocked ? "bad" : undefined} />
        <CapacityFact
          label="Runtime"
          value={`${pressureLabel(member.status)} · ${relativeTime(member.last_event_at ?? member.finished_at ?? member.started_at)}`}
        />
        <CapacityFact label="Session" value={member.native_session?.availability ?? "Not recorded"} />
        <ProviderAccountCapacity capacity={member.provider_capacity} />
      </div>

      <div className="flex items-center justify-end">
        <button
          type="button"
          onClick={onOpen}
          aria-label={`Open ${member.name ?? member.id}`}
          className="grid size-11 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground sm:size-8"
        >
          <SquareArrowOutUpRight className="size-3.5" />
        </button>
      </div>

      {(currentAction || livePreview) && (
        <div className="min-w-0 space-y-0.5 text-[10px] lg:col-span-3">
          {currentAction && <p className="truncate text-foreground"><span className="text-muted-foreground">Now · </span>{currentAction}</p>}
          {livePreview && <p className="truncate text-status-info"><span className="font-semibold">Live · </span>{livePreview}</p>}
        </div>
      )}
    </article>
  );
}

/**
 * Provider-account capacity, rendered without ever upgrading silence into
 * health. An absent snapshot is "Not observed"; an `unknown` state stays
 * `unknown` and carries the evidence source that produced it.
 */
function ProviderAccountCapacity({ capacity }: { capacity?: ProviderCapacitySnapshot | null }) {
  if (!capacity) {
    return (
      <CapacityFact
        label="Provider account"
        value="Not observed"
        title="No provider capacity snapshot exists for this member. Absent never means available."
      />
    );
  }
  const tone = capacity.state === "exhausted" || capacity.state === "unauthorized"
    ? "bad"
    : capacity.state === "limited"
      ? "warn"
      : capacity.state === "available"
        ? undefined
        : "muted";
  return (
    <CapacityFact
      label="Provider account"
      value={capacity.state}
      tone={tone === "muted" ? undefined : tone}
      title={[
        `evidence: ${capacity.evidence_source}`,
        `confidence: ${capacity.confidence}`,
        `observed: ${formatAbsolute(capacity.observed_at)}`,
        capacity.reset_at ? `resets: ${formatAbsolute(capacity.reset_at)}` : undefined,
        capacity.diagnosis ?? undefined,
      ].filter(Boolean).join(" · ")}
      note={`${capacity.evidence_source} · ${capacity.confidence}`}
    />
  );
}

function CapacityFact({ label, value, tone, title, note }: {
  label: string;
  value: string;
  tone?: "warn" | "bad";
  title?: string;
  note?: string;
}) {
  return (
    <div className="min-w-0" title={title ?? label}>
      <span className="block truncate text-[9px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{label}</span>
      <span className={cn(
        "block truncate text-[11px] font-medium text-foreground",
        tone === "warn" && "text-status-warn",
        tone === "bad" && "text-status-bad",
      )}>{value}</span>
      {note && <span className="block truncate text-[9px] text-muted-foreground">{note}</span>}
    </div>
  );
}

/** Compact member control retained for the context rail and pressure surfaces. */
export function MemberControl({ member, selected, work, currentAction, livePreview, terminal, className, onSelect, onOpen }: {
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
            <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{member.role ?? "member"} · {providerStackLine(member.provider, member.provider_profile?.execution_mode ?? member.native_session?.execution_mode, memberModelLabel(member))}<span className="sm:hidden"> · {member.status ?? "unknown"}</span></span>
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
