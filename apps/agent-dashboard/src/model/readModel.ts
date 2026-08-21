import type { SelectionState } from "../app/selection";
import type {
  DashboardSnapshot,
  Evidence,
  ProviderLaunchProfile,
} from "../types";

/**
 * Dashboard read model after the superseded coordination-stack retirement.
 * Native Mission and Agent Team selectors read directly from `snapshot`;
 * this projection only keeps shared lookup state needed by execution surfaces.
 */
export interface WorkbenchModel {
  snapshot: DashboardSnapshot;
  generatedAt?: string;
  selectedMember?: ProviderLaunchProfile;
  evidence: Evidence[];
}

export function buildWorkbenchModel(
  snapshot: DashboardSnapshot,
  selection: SelectionState,
): WorkbenchModel {
  const members = snapshot.members ?? [];
  const selectedMember = selection.memberId
    ? members.find((member) => member.id === selection.memberId)
    : undefined;
  return {
    snapshot,
    generatedAt: snapshot.generated_at,
    selectedMember,
    evidence: snapshot.evidence ?? [],
  };
}

export function parseTs(value?: string | null): number {
  if (!value) return 0;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function formatDuration(start?: string | null, end?: string | null): string | undefined {
  const startMs = parseTs(start);
  if (!startMs) return undefined;
  const endMs = end ? parseTs(end) : Date.now();
  const seconds = Math.max(0, Math.round((endMs - startMs) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  if (minutes < 60) return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const minuteRemainder = minutes % 60;
  return minuteRemainder ? `${hours}h ${minuteRemainder}m` : `${hours}h`;
}
