import type {
  CustomPageDefinition,
  CustomPagePackageManifest,
  PageAuditSink,
  RuntimeAuditEvent,
  RuntimeAuditMetadata,
} from "./types";

export interface AuditClock {
  now(): string;
}

export function createPageAuditSink(params: {
  runtimeId: string;
  definition: CustomPageDefinition;
  packageManifest: CustomPagePackageManifest;
  clock?: AuditClock;
}): PageAuditSink {
  const events: RuntimeAuditEvent[] = [];
  const clock = params.clock ?? { now: () => new Date().toISOString() };

  return {
    record(event) {
      events.push({
        ...event,
        sequence: events.length + 1,
        occurredAt: clock.now(),
      });
    },
    snapshot() {
      return {
        runtimeId: params.runtimeId,
        definitionId: params.definition.id,
        definitionVersion: params.definition.version,
        packageId: params.packageManifest.id,
        packageVersion: params.packageManifest.version,
        events: events.map((event) => ({ ...event })),
      } satisfies RuntimeAuditMetadata;
    },
  };
}
