import { createActionCommandDispatcher } from "./actionDispatcher";
import { createPageAuditSink, type AuditClock } from "./audit";
import { validateCustomPageContract } from "./contract";
import { createScopedQueryAdapter } from "./queryAdapter";
import type {
  ActionCommandTransport,
  CustomPageDefinition,
  CustomPagePackageManifest,
  CustomPageRenderer,
  CustomPageRenderState,
  PagePolicyContext,
  PageRuntimeCapabilities,
  ScopedViewSource,
  StandardViewAdapter,
} from "./types";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export interface CustomPageRuntime<Props, Output, FallbackOutput> {
  loading(): CustomPageRenderState<Output, FallbackOutput>;
  render(props: Readonly<Props>): Promise<CustomPageRenderState<Output, FallbackOutput>>;
}

export function createCustomPageRuntime<Props, Output, FallbackOutput>(params: {
  runtimeId: string;
  definition: CustomPageDefinition;
  packageManifest: CustomPagePackageManifest;
  renderer: CustomPageRenderer<Props, Output>;
  source: ScopedViewSource;
  transport: ActionCommandTransport;
  policy: PagePolicyContext;
  fallback: StandardViewAdapter<FallbackOutput>;
  clock?: AuditClock;
}): CustomPageRuntime<Props, Output, FallbackOutput> {
  validateCustomPageContract(params.definition, params.packageManifest);
  const audit = createPageAuditSink({
    runtimeId: params.runtimeId,
    definition: params.definition,
    packageManifest: params.packageManifest,
    clock: params.clock,
  });
  const queries = createScopedQueryAdapter({
    definition: params.definition,
    source: params.source,
    audit,
    grantedQueryNames: new Set(params.packageManifest.capabilities.queries),
  });
  const actions = createActionCommandDispatcher({
    definition: params.definition,
    transport: params.transport,
    policy: params.policy,
    audit,
    grantedActionNames: new Set(params.packageManifest.capabilities.actions),
  });

  const capabilities = (): PageRuntimeCapabilities =>
    Object.freeze({
      queries,
      actions,
      audit: audit.snapshot(),
    });

  return Object.freeze({
    loading(): CustomPageRenderState<Output, FallbackOutput> {
      audit.record({ kind: "runtime.loading" });
      return { status: "loading", audit: audit.snapshot() };
    },

    async render(props: Readonly<Props>): Promise<CustomPageRenderState<Output, FallbackOutput>> {
      audit.record({ kind: "runtime.loading" });
      try {
        const content = await params.renderer.render(Object.freeze({ ...props }), capabilities());
        audit.record({ kind: "runtime.ready" });
        return { status: "ready", content, audit: audit.snapshot() };
      } catch (error) {
        const renderError = errorMessage(error);
        audit.record({ kind: "runtime.render_failed", code: "CUSTOM_RENDER_FAILED" });
        try {
          const content = await params.fallback.render(params.definition.fallback, {
            definitionId: params.definition.id,
            renderError,
          });
          audit.record({ kind: "runtime.fallback_ready" });
          return {
            status: "fallback",
            content,
            fallback: params.definition.fallback,
            renderError,
            audit: audit.snapshot(),
          };
        } catch (fallbackError) {
          audit.record({ kind: "runtime.fallback_failed", code: "STANDARD_VIEW_FAILED" });
          return {
            status: "error",
            fallback: params.definition.fallback,
            renderError,
            fallbackError: errorMessage(fallbackError),
            audit: audit.snapshot(),
          };
        }
      }
    },
  });
}
