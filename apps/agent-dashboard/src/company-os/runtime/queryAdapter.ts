import { PageContractError } from "./contract";
import type {
  CustomPageDefinition,
  PageAuditSink,
  ScopedQueryAdapter,
  ScopedViewSource,
} from "./types";

function readonlyParameters(
  parameters: Readonly<Record<string, unknown>>,
): Readonly<Record<string, unknown>> {
  return Object.freeze({ ...parameters });
}

export function createScopedQueryAdapter(params: {
  definition: CustomPageDefinition;
  source: ScopedViewSource;
  audit: PageAuditSink;
  grantedQueryNames?: ReadonlySet<string>;
}): ScopedQueryAdapter {
  const declarations = new Map(params.definition.queries.map((query) => [query.name, query]));

  return Object.freeze({
    async query<T>(name: string, parameters: Readonly<Record<string, unknown>> = {}): Promise<T> {
      const declaration = declarations.get(name);
      if (!declaration || (params.grantedQueryNames && !params.grantedQueryNames.has(name))) {
        params.audit.record({ kind: "query.denied", subject: name, code: "UNDECLARED_QUERY" });
        throw new PageContractError("UNDECLARED_QUERY", `Query is not declared for this page: ${name}`);
      }

      const result = await params.source.read<T>({
        viewId: declaration.viewId,
        recordTypes: [...declaration.recordTypes],
        relationPaths: [...(declaration.relationPaths ?? [])],
        parameters: readonlyParameters(parameters),
      });
      params.audit.record({ kind: "query.completed", subject: name });
      return result;
    },
  });
}
