import type { CustomPageDefinition, CustomPagePackageManifest } from "./types";

export class PageContractError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "PageContractError";
  }
}

function assertUnique(values: readonly string[], field: string): void {
  if (new Set(values).size !== values.length) {
    throw new PageContractError("DUPLICATE_CAPABILITY", `${field} contains duplicate names`);
  }
}

export function validateCustomPageContract(
  definition: CustomPageDefinition,
  packageManifest: CustomPagePackageManifest,
): void {
  if (definition.package.id !== packageManifest.id || definition.package.version !== packageManifest.version) {
    throw new PageContractError(
      "PACKAGE_REFERENCE_MISMATCH",
      "The package id/version does not match the registered CustomPageDefinition",
    );
  }
  if (packageManifest.definitionId !== definition.id) {
    throw new PageContractError(
      "DEFINITION_REFERENCE_MISMATCH",
      "The package is registered for a different CustomPageDefinition",
    );
  }
  if (!packageManifest.integrity.trim()) {
    throw new PageContractError("MISSING_INTEGRITY", "A custom page package requires an integrity value");
  }
  if (!definition.fallback.owningDocumentId || definition.fallback.viewIds.length === 0) {
    throw new PageContractError(
      "MISSING_STANDARD_FALLBACK",
      "A custom page requires an owning Document and at least one standard View",
    );
  }

  const declaredQueries = definition.queries.map((query) => query.name);
  const declaredActions = definition.actions.map((action) => action.name);
  assertUnique(declaredQueries, "definition.queries");
  assertUnique(declaredActions, "definition.actions");
  assertUnique(packageManifest.capabilities.queries, "package.capabilities.queries");
  assertUnique(packageManifest.capabilities.actions, "package.capabilities.actions");
  for (const action of definition.actions) {
    assertUnique(action.allowedEffectKinds, `definition.actions.${action.name}.allowedEffectKinds`);
    if (action.sensitive && action.humanApproval !== "required") {
      throw new PageContractError(
        "SENSITIVE_ACTION_REQUIRES_APPROVAL",
        `Sensitive action must require Human Approval: ${action.name}`,
      );
    }
  }

  for (const query of packageManifest.capabilities.queries) {
    if (!declaredQueries.includes(query)) {
      throw new PageContractError(
        "UNDECLARED_PACKAGE_QUERY",
        `Package requests undeclared query: ${query}`,
      );
    }
  }
  for (const action of packageManifest.capabilities.actions) {
    if (!declaredActions.includes(action)) {
      throw new PageContractError(
        "UNDECLARED_PACKAGE_ACTION",
        `Package requests undeclared action: ${action}`,
      );
    }
  }
}
