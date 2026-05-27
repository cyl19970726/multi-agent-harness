import { existsSync, readFileSync } from "node:fs";

const registryPath = "docs/registry.json";
const requiredCoreDocs = [
  "README.md",
  "docs/README.md",
  "docs/prd.md",
  "docs/design-basis.md",
  "docs/architecture.md",
  "docs/operations.md",
  "docs/schemas.md",
  "docs/decisions/README.md"
];
const allowedStatuses = new Set(["idea", "planned", "stable", "deprecated", "archival"]);
const allowedLifecycles = new Set(["volatile", "stable", "archival"]);
const requiredFields = [
  "path",
  "ownerRole",
  "status",
  "lifecycle",
  "canonicalFor",
  "dependsOn",
  "machineConsumers",
  "reviewAfter",
  "lastVerifiedWith",
  "reorgTrigger"
];

const failures = [];
const warnings = [];

function nonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function nonEmptyStringArray(value) {
  return Array.isArray(value) && value.length > 0 && value.every(nonEmptyString);
}

function stringArray(value) {
  return Array.isArray(value) && value.every(nonEmptyString);
}

function parseDateOnly(value) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value ?? "")) return null;
  const date = new Date(`${value}T00:00:00Z`);
  return Number.isNaN(date.getTime()) ? null : date;
}

if (!existsSync(registryPath)) {
  failures.push(`${registryPath}: missing docs governance registry`);
} else {
  const registry = JSON.parse(readFileSync(registryPath, "utf8"));

  if (registry.schema !== "agent_harness.docs_registry.v1") {
    failures.push(`${registryPath}: schema must be agent_harness.docs_registry.v1`);
  }
  if (!Array.isArray(registry.documents)) {
    failures.push(`${registryPath}: documents must be an array`);
  } else {
    const seen = new Set();
    const today = new Date();
    today.setUTCHours(0, 0, 0, 0);

    for (const [index, doc] of registry.documents.entries()) {
      const label = `${registryPath}: documents[${index}]`;

      for (const field of requiredFields) {
        if (!(field in doc)) {
          failures.push(`${label}: missing ${field}`);
        }
      }
      if (!nonEmptyString(doc.path)) {
        failures.push(`${label}: path must be a non-empty string`);
        continue;
      }
      if (seen.has(doc.path)) {
        failures.push(`${label}: duplicate path ${doc.path}`);
      }
      seen.add(doc.path);

      if (!existsSync(doc.path)) {
        failures.push(`${label}: registered path does not exist: ${doc.path}`);
      }
      if (!nonEmptyString(doc.ownerRole)) {
        failures.push(`${label}: ownerRole must be a non-empty string`);
      }
      if (!allowedStatuses.has(doc.status)) {
        failures.push(`${label}: invalid status ${doc.status}`);
      }
      if (!allowedLifecycles.has(doc.lifecycle)) {
        failures.push(`${label}: invalid lifecycle ${doc.lifecycle}`);
      }
      if (!nonEmptyStringArray(doc.canonicalFor)) {
        failures.push(`${label}: canonicalFor must be a non-empty string array`);
      }
      if (!stringArray(doc.dependsOn)) {
        failures.push(`${label}: dependsOn must be a string array`);
      } else {
        for (const dependency of doc.dependsOn) {
          if (!existsSync(dependency)) {
            failures.push(`${label}: dependency does not exist: ${dependency}`);
          }
        }
      }
      if (!nonEmptyStringArray(doc.machineConsumers)) {
        failures.push(`${label}: machineConsumers must be a non-empty string array`);
      }
      if (!nonEmptyStringArray(doc.lastVerifiedWith)) {
        failures.push(`${label}: lastVerifiedWith must be a non-empty string array`);
      }
      if (!nonEmptyString(doc.reorgTrigger)) {
        failures.push(`${label}: reorgTrigger must be a non-empty string`);
      }

      const reviewAfter = parseDateOnly(doc.reviewAfter);
      if (!reviewAfter) {
        failures.push(`${label}: reviewAfter must be YYYY-MM-DD`);
      } else if (reviewAfter < today) {
        warnings.push(`${label}: reviewAfter is stale: ${doc.reviewAfter}`);
      }
    }

    for (const coreDoc of requiredCoreDocs) {
      if (!seen.has(coreDoc)) {
        failures.push(`${registryPath}: missing core doc ${coreDoc}`);
      }
    }
  }
}

if (warnings.length) {
  console.warn(warnings.join("\n"));
}
if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`checked docs governance registry: ${registryPath}`);
