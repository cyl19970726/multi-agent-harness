export type CompanyOsDataMode =
  | "store-live"
  | "capture-fixture"
  | "snapshot-prototype"
  | "prototype-fixture";

export interface ResolvedCompanyOsData {
  value: unknown;
  mode: CompanyOsDataMode;
  source?: AuthoritativeStoreSource;
}

export interface AuthoritativeStoreSource {
  kind: "harness_store";
  authoritative: true;
  project_id: string;
  store_root: string;
  schema: "company-os/v1";
  revision: string;
  projection: "latest_row_wins";
}

type JsonRecord = Record<string, unknown>;

function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function nonemptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function absolutePath(value: unknown): value is string {
  return nonemptyString(value) && (/^\//.test(value) || /^[A-Za-z]:[\\/]/.test(value));
}

/**
 * Fail-closed recognition of the server's authoritative Company OS read model.
 * `projection_kind` or object presence alone are deliberately insufficient.
 */
export function authoritativeStoreSource(value: unknown): AuthoritativeStoreSource | undefined {
  if (!isRecord(value)) return undefined;
  if (value.snapshot_contract !== "company-os-v1" || value.projection_kind !== "live_company_os") return undefined;
  const source = value.source;
  if (!isRecord(source)) return undefined;
  if (
    source.kind !== "harness_store" ||
    source.authoritative !== true ||
    !nonemptyString(source.project_id) ||
    !absolutePath(source.store_root) ||
    source.schema !== "company-os/v1" ||
    !nonemptyString(source.revision) ||
    !/^fnv1a64:[0-9a-f]{16}$/.test(source.revision) ||
    source.projection !== "latest_row_wins"
  ) return undefined;

  return source as unknown as AuthoritativeStoreSource;
}

/** Capture injection always wins and always remains a fixture, even if a live
 * projection is present behind it. Unverified snapshot data may render for
 * diagnostics, but it remains visibly Prototype. */
export function resolveCompanyOsData({
  injected,
  snapshotProjection,
  fallback,
}: {
  injected?: unknown;
  snapshotProjection?: unknown;
  fallback: unknown;
}): ResolvedCompanyOsData {
  if (isRecord(injected)) return { value: injected, mode: "capture-fixture" };

  if (isRecord(snapshotProjection)) {
    const source = authoritativeStoreSource(snapshotProjection);
    return source
      ? { value: snapshotProjection, mode: "store-live", source }
      : { value: snapshotProjection, mode: "snapshot-prototype" };
  }

  return { value: fallback, mode: "prototype-fixture" };
}
