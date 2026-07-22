import type { ReactNode } from "react";
import {
  CheckCircle2,
  ChevronRight,
  Code2,
  Compass,
  FileCheck2,
  FilePenLine,
  Flag,
  MessageSquareText,
  Search,
  ShieldCheck,
  SquareTerminal,
  Wrench,
} from "lucide-react";

import { Avatar } from "@/components/workbench/Avatar";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";
import { memberTone } from "@/components/workbench/tones";

type PhaseId = "briefing" | "exploration" | "implementation" | "verification" | "handoff";

interface Phase {
  id: PhaseId;
  label: string;
  description: string;
  items: WorkbenchActivityItem[];
}

const PHASE_META: Record<PhaseId, { label: string; description: string; icon: typeof Compass; tone: string }> = {
  briefing: { label: "Briefing", description: "Assignment, constraints, and working context", icon: MessageSquareText, tone: "border-[#d8d1f4] bg-[#f4f1ff] text-[#7055c6]" },
  exploration: { label: "Exploration", description: "Reading the workspace and locating the change surface", icon: Search, tone: "border-[#cce2ee] bg-[#edf8fc] text-[#287da6]" },
  implementation: { label: "Implementation", description: "Applying the selected change in the provider workspace", icon: Code2, tone: "border-[#d5dfec] bg-[#f2f6fb] text-[#466783]" },
  verification: { label: "Verification", description: "Checks, tests, and review evidence", icon: ShieldCheck, tone: "border-[#cce7d7] bg-[#eff9f2] text-[#2f8b59]" },
  handoff: { label: "Handoff", description: "Result returned to the Host with native provenance", icon: Flag, tone: "border-[#e0d0ee] bg-[#f8f1fb] text-[#8b55ad]" },
};

/**
 * Read-time editorial projection of one member's complete chronology.
 * It never writes phases or merged events back to Harness: every source row is
 * still available inside the disclosure attached to its visual group.
 */
export function MemberHistoryNarrative({
  items,
  memberName,
  memberRole,
  memberStatus,
  empty,
}: {
  items: WorkbenchActivityItem[];
  memberName: string;
  memberRole: string;
  memberStatus?: string | null;
  empty?: ReactNode;
}) {
  if (!items.length) return <div className="grid min-h-52 place-items-center">{empty}</div>;
  const phases = projectPhases(items);

  return (
    <ol className="member-history-narrative mx-auto max-w-[920px] pb-14 pt-2">
      {phases.map((phase) => (
        <li key={phase.id} className="member-history-phase grid grid-cols-[3.25rem_minmax(0,1fr)] gap-x-4 sm:grid-cols-[5rem_minmax(0,1fr)]">
          <PhaseMarker phase={phase} />
          <section aria-labelledby={`member-phase-${phase.id}`} className="min-w-0 py-2 last:border-b-0">
            <header className="mb-1.5">
              <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                <h3 id={`member-phase-${phase.id}`} className="text-[15px] font-semibold tracking-[-0.015em] text-foreground">{phase.label}</h3>
                <span className="text-[10px] font-medium tabular-nums text-muted-foreground">{formatPhaseTime(phase.items[0]?.occurredAt)}</span>
              </div>
            </header>
            <PhaseWorkCard
              phase={phase}
              memberName={memberName}
              memberRole={memberRole}
              memberStatus={memberStatus}
            />
          </section>
        </li>
      ))}
    </ol>
  );
}

function PhaseWorkCard({
  phase,
  memberName,
  memberRole,
  memberStatus,
}: {
  phase: Phase;
  memberName: string;
  memberRole: string;
  memberStatus?: string | null;
}) {
  const groups = groupPhaseItems(phase.items);
  const toolGroups = groups.filter((group) => group.kind === "tool" && String(group.items[0]?.title) !== "Harness coordination");
  const messageItems = groups.filter((group) => group.kind === "message").flatMap((group) => group.items);
  const preferred = phase.id === "briefing"
    ? messageItems.find((item) => item.glyph === "assignment") ?? messageItems[0]
    : phase.id === "handoff"
      ? [...messageItems].reverse().find((item) => item.glyph === "handoff") ?? messageItems[messageItems.length - 1]
      : [...messageItems].reverse().find((item) => item.source === "provider-native") ?? messageItems[messageItems.length - 1];
  const actor = preferred?.actorLabel ?? memberName;
  const handoff = phase.id === "handoff";
  return (
    <article className={cn(
      "overflow-hidden rounded-[10px] border bg-card",
      handoff ? "border-[#f2ad91] shadow-[0_18px_44px_-36px_rgba(233,104,73,.55)]" : "border-[#bdd8e9] shadow-[0_14px_36px_-34px_rgba(36,110,150,.65)]",
    )}>
      {preferred && (
        <div className="flex min-w-0 items-start gap-2.5 px-3 py-1.5">
          <Avatar name={actor} identity={memberRole} tone={memberTone(memberStatus)} size="sm" />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2 text-[10px]">
              <span className="font-semibold text-foreground">{actor}</span>
              <span className="text-muted-foreground">{preferred.timestamp}</span>
              {preferred.source === "provider-native" && <Badge tone="muted">native session</Badge>}
              {handoff && <Badge tone="decision">handoff</Badge>}
            </div>
            {handoff ? (
              <div className="member-handoff-summary mt-1.5 max-h-[152px] overflow-hidden text-[11px] leading-relaxed">{preferred.body ?? compactMessagePreview(preferred.rawText ?? String(preferred.title))}</div>
            ) : (
              <p className="mt-1 line-clamp-1 text-[11px] italic leading-relaxed text-muted-foreground">{compactMessagePreview(preferred.rawText ?? String(preferred.title))}</p>
            )}
          </div>
        </div>
      )}
      {toolGroups.length > 0 && (
        <div className="divide-y divide-border/55 border-t border-border/55 bg-[#fbfdfe] px-3">
          {toolGroups.map((group) => <ToolMiniRow key={group.items.map((item) => item.id).join(":")} items={group.items} />)}
        </div>
      )}
      <details className="border-t border-border/55 px-3 py-1.5 text-[9px] text-muted-foreground">
        <summary className="cursor-pointer select-none font-medium hover:text-foreground">{phase.items.length} source records · open complete phase history</summary>
        <div className="mt-2 max-h-72 space-y-2 overflow-auto border-l border-border pl-3">
          {phase.items.map((source) => (
            <div key={source.id} className="space-y-1">
              <div className="font-mono uppercase tracking-wider">{source.source ?? "unknown"} · {String(source.title)} · {source.statusLabel ?? "recorded"}</div>
              {source.body && <div className="text-[10px] leading-relaxed">{source.body}</div>}
            </div>
          ))}
        </div>
      </details>
    </article>
  );
}

function ToolMiniRow({ items }: { items: WorkbenchActivityItem[] }) {
  const item = items[0];
  const failed = items.some((source) => source.tone === "bad" || source.statusLabel === "failed");
  const Icon = toolIcon(item);
  const invocations = items.filter((source) => String(source.title).toLowerCase() !== "tool result").length;
  return (
    <details className="group/minitool">
      <summary className="flex h-8 cursor-pointer list-none items-center gap-2 text-[10px] marker:content-none">
        <Icon className="size-3.5 shrink-0 text-[#287da6]" aria-hidden />
        <span className="min-w-0 flex-1 truncate font-medium text-foreground">{String(item.title)}</span>
        {invocations > 1 && <span className="text-muted-foreground">×{invocations}</span>}
        <span className={failed ? "font-semibold text-status-bad" : "font-semibold text-status-good"}>{failed ? "Failed" : "Completed"}</span>
        <ChevronRight className="size-3 text-muted-foreground transition-transform group-open/minitool:rotate-90" />
      </summary>
      <div className="mb-2 rounded-md bg-muted/30 p-2 text-[9px] text-muted-foreground">{summaryForTool(items)} · {items.length} native records</div>
    </details>
  );
}

function PhaseMarker({ phase }: { phase: Phase }) {
  const meta = PHASE_META[phase.id];
  const Icon = meta.icon;
  return (
    <div className="relative flex justify-center pt-2">
      <span className={cn("relative z-[1] grid size-10 place-items-center rounded-[13px] border shadow-[0_10px_28px_-22px_currentColor] ring-[6px] ring-background", meta.tone)}>
        <Icon className="size-[17px]" strokeWidth={1.9} aria-hidden />
      </span>
    </div>
  );
}

function projectPhases(items: WorkbenchActivityItem[]): Phase[] {
  const handoffIndex = findFromEnd(items, (item) => item.glyph === "handoff" || /\bhandoff\b/i.test(item.statusLabel ?? ""));
  const mutationIndex = items.findIndex((item, index) => index >= firstNativeTool(items) && (
    /apply_patch|write_file|edit_file|filepen|\bpatch\b/i.test(searchable(item)) ||
    (item.source === "provider-native" && item.kind === "message" && /now landing|wired the|implement(?:ed|ing)|change is in place/i.test(searchable(item)))
  ));
  const verifyIndex = mutationIndex >= 0
    ? items.findIndex((item, index) => index > mutationIndex && item.source === "provider-native" && /cargo test|pnpm|npm test|typecheck|checks? (?:are )?running|suite is green|verify|lint|git diff/i.test(searchable(item)))
    : -1;
  const firstToolIndex = items.findIndex((item) => item.source === "provider-native" && item.kind === "action");

  const buckets = new Map<PhaseId, WorkbenchActivityItem[]>([
    ["briefing", []], ["exploration", []], ["implementation", []], ["verification", []], ["handoff", []],
  ]);
  items.forEach((item, index) => {
    let phase: PhaseId = "briefing";
    if (handoffIndex >= 0 && index >= handoffIndex) phase = "handoff";
    else if (verifyIndex >= 0 && index >= verifyIndex) phase = "verification";
    else if (mutationIndex >= 0 && index >= mutationIndex) phase = "implementation";
    else if (firstToolIndex >= 0 && index >= firstToolIndex) phase = "exploration";
    buckets.get(phase)?.push(item);
  });

  return (["briefing", "exploration", "implementation", "verification", "handoff"] as PhaseId[])
    .filter((id) => (buckets.get(id)?.length ?? 0) > 0)
    .map((id) => ({ id, ...PHASE_META[id], items: buckets.get(id) ?? [] }));
}

function groupPhaseItems(items: WorkbenchActivityItem[]): Array<{ kind: "message" | "tool"; items: WorkbenchActivityItem[] }> {
  const groups: Array<{ kind: "message" | "tool"; items: WorkbenchActivityItem[] }> = [];
  for (const item of items) {
    const tool = item.source === "provider-native" && item.kind === "action";
    const harnessRuntime = item.source === "harness" && item.kind !== "message" && item.kind !== "blocker" && item.glyph !== "handoff" && item.glyph !== "assignment";
    const last = groups[groups.length - 1];
    if (tool) {
      if (String(item.title).toLowerCase() === "tool result" && last?.kind === "tool") {
        last.items.push(item);
      } else {
        const existing = groups.find((group) => group.kind === "tool" && sameToolFamily(group.items[0], item));
        if (existing) existing.items.push(item);
        else groups.push({ kind: "tool", items: [item] });
      }
    } else if (harnessRuntime) {
      const existing = groups.find((group) => group.kind === "tool" && group.items[0]?.id.startsWith("narrative:harness:"));
      const projected = { ...item, id: `narrative:harness:${item.id}`, title: "Harness coordination", rawText: "Durable coordination records linked to this member run." };
      if (existing) existing.items.push(projected);
      else groups.push({ kind: "tool", items: [projected] });
    } else if (item.source === "provider-native") {
      const existing = groups.find((group) => group.kind === "message" && group.items[0]?.source === "provider-native");
      if (existing) existing.items.push(item);
      else groups.push({ kind: "message", items: [item] });
    } else if (last?.kind === "message" && isDuplicateHandoff(last.items[last.items.length - 1], item)) {
      last.items.push(item);
    } else groups.push({ kind: "message", items: [item] });
  }
  return groups;
}

function sameToolFamily(left: WorkbenchActivityItem, right: WorkbenchActivityItem): boolean {
  const l = String(left.title).toLowerCase();
  const r = String(right.title).toLowerCase();
  return l === r || r === "tool result" || l === "tool result";
}

function isDuplicateHandoff(left: WorkbenchActivityItem, right: WorkbenchActivityItem): boolean {
  return (left.glyph === "handoff" || right.glyph === "handoff") && left.actorLabel === right.actorLabel;
}

function searchable(item: WorkbenchActivityItem): string {
  return `${String(item.title)} ${item.rawText ?? ""} ${item.glyph ?? ""}`.toLowerCase();
}

function compactMessagePreview(value: string): string {
  const plain = value
    .replace(/```[\s\S]*?```/g, " code block ")
    .replace(/[#*_`>[\]]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return plain.length > 220 ? `${plain.slice(0, 217)}…` : plain;
}

function findFromEnd(items: WorkbenchActivityItem[], predicate: (item: WorkbenchActivityItem) => boolean): number {
  for (let index = items.length - 1; index >= 0; index -= 1) if (predicate(items[index])) return index;
  return -1;
}

function summaryForTool(items: WorkbenchActivityItem[]): string | undefined {
  const title = String(items[0]?.title ?? "").toLowerCase();
  if (title === "spawn_agent") return "Started a delegated agent for a bounded lane.";
  if (title === "send_message") return "Sent a coordination update to a collaborating agent.";
  if (title === "wait") return "Waited for a running command or delegated lane to report progress.";
  if (title === "tool result") return undefined;
  return "Provider-native tool activity. Open the source records for details.";
}

function firstNativeTool(items: WorkbenchActivityItem[]): number {
  const index = items.findIndex((item) => item.source === "provider-native" && item.kind === "action");
  return index < 0 ? 0 : index;
}

function toolIcon(item: WorkbenchActivityItem) {
  if (item.glyph === "edit") return FilePenLine;
  if (item.glyph === "search") return Search;
  if (item.glyph === "artifact" || item.glyph === "review") return FileCheck2;
  if (/bash|shell|command/i.test(String(item.title))) return SquareTerminal;
  if (/test|check|verify/i.test(searchable(item))) return CheckCircle2;
  return Wrench;
}

function formatPhaseTime(value?: string | null): string {
  if (!value) return "";
  const parsed = /^unix-ms:(\d+)$/.exec(value)?.[1];
  const date = new Date(parsed ? Number(parsed) : value);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
