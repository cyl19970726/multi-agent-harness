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
});
