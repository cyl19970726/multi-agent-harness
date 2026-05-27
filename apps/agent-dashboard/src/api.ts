import type { DashboardSnapshot } from "./types";

export interface ActionResponse {
  ok: boolean;
  result?: unknown;
  snapshot?: DashboardSnapshot;
  error?: string;
}

export async function fetchSnapshot(baseUrl: string): Promise<DashboardSnapshot> {
  const normalized = baseUrl.trim().replace(/\/$/, "");
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}/v1/snapshot`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return (await response.json()) as DashboardSnapshot;
}

export async function postAction(baseUrl: string, path: string, body: unknown = {}): Promise<ActionResponse> {
  const normalized = baseUrl.trim().replace(/\/$/, "");
  if (!normalized) {
    throw new Error("Harness API URL is required");
  }
  const response = await fetch(`${normalized}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const payload = (await response.json()) as ActionResponse;
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || `HTTP ${response.status}`);
  }
  return payload;
}
