import { describe, expect, it } from "vitest";

import type { ProjectionInvalidation } from "../api";
import { freshnessDomainsForInvalidation } from "./freshness";

function invalidation(ledger: string): ProjectionInvalidation {
  return {
    scope: "execution_space",
    scope_id: "space-1",
    ledger,
    revision: 2,
    reason: "append",
    stream_epoch: "epoch-1",
  };
}

describe("freshnessDomainsForInvalidation", () => {
  it("does not refresh product projections for lease heartbeat-only ledgers", () => {
    expect(freshnessDomainsForInvalidation(invalidation("node_daemon_leases.jsonl")))
      .toEqual([]);
    expect(freshnessDomainsForInvalidation(invalidation("team_supervisor_leases.jsonl")))
      .toEqual([]);
  });

  it("marks Works stale for a canonical trust write, which is where Work lives", () => {
    // Every Work transition is a `work` envelope in this file since the W4
    // writer cutover. Its name contains no "work", so a substring rule alone
    // would leave the Works pill claiming `live` through a Work write.
    expect(freshnessDomainsForInvalidation(invalidation("agentfirm_trust_operations.jsonl")))
      .toEqual(["works", "runtime"]);
    expect(
      freshnessDomainsForInvalidation({
        ...invalidation("agentfirm_trust_operations.jsonl"),
        reason: "replace",
      }),
    ).toEqual(["works", "runtime"]);
  });

  it("still marks Works stale for the pre-cutover Work ledger", () => {
    expect(freshnessDomainsForInvalidation(invalidation("work_operations.jsonl")))
      .toEqual(["works", "runtime"]);
    expect(freshnessDomainsForInvalidation(invalidation("work_delegation_operations.jsonl")))
      .toEqual(["works", "runtime"]);
  });

  it("leaves Works alone for an Execution Space ledger that carries no Work", () => {
    for (const ledger of ["member_runs.jsonl", "team_runs.jsonl", "messages.jsonl"]) {
      expect(freshnessDomainsForInvalidation(invalidation(ledger))).toEqual(["runtime"]);
    }
  });
});
