import type { ProjectionInvalidation } from "../api";

export const freshnessDomains = ["works", "docs", "organization", "runtime"] as const;

export type FreshnessDomain = (typeof freshnessDomains)[number];
export type FreshnessStatus = "live" | "reconnecting" | "stale" | "offline";
export type DomainFreshness = Record<FreshnessDomain, FreshnessStatus>;

export function uniformFreshness(status: FreshnessStatus): DomainFreshness {
  return Object.fromEntries(freshnessDomains.map((domain) => [domain, status])) as DomainFreshness;
}

export function updateFreshness(
  current: DomainFreshness,
  domains: readonly FreshnessDomain[],
  status: FreshnessStatus,
): DomainFreshness {
  if (domains.every((domain) => current[domain] === status)) return current;
  const next = { ...current };
  for (const domain of domains) next[domain] = status;
  return next;
}

/**
 * Does an Execution Space ledger carry Work?
 *
 * `agentfirm_trust_operations.jsonl` is the Work journal: since the W4 writer
 * cutover every Work transition is a `work` aggregate envelope in that file,
 * and `work_operations.jsonl` holds only the pre-cutover half. Its name does
 * not contain "work" and cannot be inferred from one, so it is named.
 *
 * That file also carries Messages, bindings, deliveries and sessions, so some
 * invalidations mark `works` stale when only a non-Work canonical row moved.
 * That is the safe direction and deliberately so: a freshness pill claiming
 * `live` while a Work write is in flight is a false truth claim, while an
 * extra `stale` costs one authoritative resync that this invalidation already
 * triggers for `runtime`.
 */
function executionSpaceLedgerBearsWork(ledger: string): boolean {
  const name = ledger.split("/").pop() ?? ledger;
  return name === "agentfirm_trust_operations.jsonl" || name.includes("work");
}

/**
 * Translate a Runtime invalidation ledger into the product domains whose
 * projection is no longer proven current. Runtime is always included because
 * it represents the browser read model converging with the selected stores.
 * Unknown or malformed Company ledgers fail stale across every Company domain.
 */
export function freshnessDomainsForInvalidation(
  invalidation: ProjectionInvalidation | null,
): readonly FreshnessDomain[] {
  if (!invalidation) return freshnessDomains;
  const ledger = invalidation.ledger.toLowerCase();
  // NodeDaemon and Team Supervisor heartbeats only rewrite ambient lease
  // files. That churn proves liveness; it never dirties product projection
  // truth, so a healthy SSE stream must not turn it into snapshot polling.
  if (["node_daemon_leases.jsonl", "team_supervisor_leases.jsonl"].includes(
    ledger.split("/").pop() ?? "",
  )) return [];
  if (invalidation.scope === "execution_space") {
    return executionSpaceLedgerBearsWork(ledger) ? ["works", "runtime"] : ["runtime"];
  }
  if (/work|assignment|commitment/.test(ledger)) return ["works", "runtime"];
  if (/document|block|relation|page|module|typed_record|view/.test(ledger)) {
    return ["docs", "runtime"];
  }
  if (/human|agent_membership|org_|membership|governance/.test(ledger)) {
    return ["organization", "runtime"];
  }
  return freshnessDomains;
}
