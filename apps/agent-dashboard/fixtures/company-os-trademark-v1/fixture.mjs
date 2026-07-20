import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const fixtureRoot = dirname(fileURLToPath(import.meta.url));
export const repoRoot = resolve(fixtureRoot, "../../../..");

function sha256(text) {
  return createHash("sha256").update(text).digest("hex");
}

export async function loadCompanyOsFixture() {
  const manifestText = await readFile(resolve(fixtureRoot, "fixture-manifest.json"), "utf8");
  const manifest = JSON.parse(manifestText);
  const sourcePath = resolve(fixtureRoot, manifest.authoritative_source);
  const sourceText = await readFile(sourcePath, "utf8");
  const sourceSha256 = sha256(sourceText);
  if (sourceSha256 !== manifest.authoritative_sha256) {
    throw new Error(
      `authoritative Company OS fixture changed: expected ${manifest.authoritative_sha256}, got ${sourceSha256}. ` +
      "Review the canonical change, then update this manifest deliberately.",
    );
  }
  const fixture = JSON.parse(sourceText);
  if (fixture.fixture_id !== manifest.id) {
    throw new Error(`fixture id mismatch: manifest=${manifest.id}, source=${fixture.fixture_id}`);
  }
  return { manifest, fixture, sourcePath, sourceSha256 };
}

export function resolveContractRoute(route, routeTokens) {
  let resolved = route;
  for (const [token, value] of Object.entries(routeTokens)) {
    resolved = resolved.replaceAll(token, encodeURIComponent(value));
  }
  const unresolved = resolved.match(/<[^>]+>/g);
  if (unresolved) throw new Error(`unresolved route token(s): ${unresolved.join(", ")} in ${route}`);
  return resolved;
}

/**
 * Read-only API projection used by browser evidence. It deliberately embeds the
 * canonical fixture instead of translating it into unrelated legacy ledgers.
 */
export function companyOsApiProjection(manifest, fixture) {
  return {
    generated_at: manifest.capture_now,
    fixture_id: manifest.id,
    company_os: fixture,
  };
}

export function indexFixture(fixture) {
  const records = new Map();
  const add = (value) => {
    if (value && typeof value === "object" && typeof value.id === "string") records.set(value.id, value);
  };
  for (const value of Object.values(fixture)) {
    if (Array.isArray(value)) value.forEach(add);
  }
  fixture.organization?.org_units?.forEach(add);
  fixture.organization?.memberships?.forEach(add);
  fixture.organization?.explicitly_reported_statuses?.forEach(add);
  return records;
}
