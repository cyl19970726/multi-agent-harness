/**
 * Lightweight route metadata for Company OS surfaces. WorkbenchShell needs
 * these synchronously for chrome and routing decisions, while the heavy
 * CompanyOsRouter page tree is lazy-loaded. Keep this module cheap: anything
 * page-sized belongs behind the lazy boundary.
 */
import canonicalFixture from "../../../../docs/design/company-os-v1/fixtures/company-os-trademark-v1.json";

import type { SurfaceId } from "@/app/selection";
import type { WorkbenchModel } from "@/model/readModel";

import { resolveCompanyOsData, type ResolvedCompanyOsData } from "./sourceTruth";

declare global {
  interface Window {
    /** Deterministic visual fixture injected by the Company OS capture runner. */
    __COMPANY_OS_FIXTURE__?: unknown;
  }
}

const COMPANY_OS_SURFACES = new Set<SurfaceId>([
  "home",
  "docs",
  "docs-v2",
  "organization",
  "work",
  "approvals",
  "finance",
  "providers",
  "plugins",
  "settings",
]);

export function isCompanyOsSurface(surface: SurfaceId): boolean {
  return COMPANY_OS_SURFACES.has(surface);
}

function companyOsProjection(model: WorkbenchModel): unknown {
  const snapshot = model.snapshot as unknown as Record<string, unknown>;
  return snapshot.company_os;
}

export function resolveCompanyOsRouteData(model: WorkbenchModel): ResolvedCompanyOsData {
  const injected = typeof window === "undefined" ? undefined : window.__COMPANY_OS_FIXTURE__;
  return resolveCompanyOsData({
    injected,
    snapshotProjection: companyOsProjection(model),
    fallback: canonicalFixture,
  });
}
