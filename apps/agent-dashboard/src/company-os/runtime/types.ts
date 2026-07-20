export type ActorKind = "human" | "standing_agent" | "external" | "service";

export interface ActorIdentity {
  id: string;
  kind: ActorKind;
}

export interface HumanApprovalProof {
  id: string;
  status: "approved" | "rejected" | "pending" | "withdrawn";
  decidedBy: ActorIdentity;
  decidedAt?: string;
}

export interface ScopedQueryDeclaration {
  name: string;
  viewId: string;
  recordTypes: readonly string[];
  relationPaths?: readonly string[];
}

export interface ActionCommandDeclaration {
  name: string;
  sensitive: boolean;
  humanApproval: "required" | "not_required";
  allowedEffectKinds: readonly string[];
}

export interface StandardViewFallback {
  title: string;
  owningDocumentId: string;
  viewIds: readonly string[];
  nextActions: readonly string[];
}

export interface CustomPagePackageRef {
  id: string;
  version: string;
}

export interface CustomPageDefinition {
  id: string;
  version: string;
  purpose: string;
  primaryQuestion: string;
  ownerActorId: string;
  moduleId: string;
  fixtureId: string;
  componentVersion: string;
  package: CustomPagePackageRef;
  queries: readonly ScopedQueryDeclaration[];
  actions: readonly ActionCommandDeclaration[];
  fallback: StandardViewFallback;
}

export interface CustomPagePackageManifest {
  id: string;
  definitionId: string;
  version: string;
  format: "react-component" | "html-template";
  entryPoint: string;
  integrity: string;
  capabilities: {
    queries: readonly string[];
    actions: readonly string[];
    components: readonly string[];
  };
}

export interface ScopedQueryRequest {
  viewId: string;
  recordTypes: readonly string[];
  relationPaths: readonly string[];
  parameters: Readonly<Record<string, unknown>>;
}

/**
 * The page runtime accepts a read-only view source, never a store client. This
 * is deliberately too small to expose persistence or arbitrary record scans.
 */
export interface ScopedViewSource {
  read<T>(request: ScopedQueryRequest): Promise<T>;
}

export interface ScopedQueryAdapter {
  query<T>(name: string, parameters?: Readonly<Record<string, unknown>>): Promise<T>;
}

export interface CanonicalEffect {
  kind: string;
  recordId: string;
}

export interface ActionTransportResult<T = unknown> {
  data: T;
  effects: readonly CanonicalEffect[];
}

export interface ActionCommandEnvelope {
  command: string;
  input: Readonly<Record<string, unknown>>;
  actor: ActorIdentity;
  approval?: HumanApprovalProof;
  policy: {
    definitionId: string;
    allowedEffectKinds: readonly string[];
  };
}

/** The backend command seam. It is not a direct store mutation API. */
export interface ActionCommandTransport {
  dispatch<T>(envelope: ActionCommandEnvelope): Promise<ActionTransportResult<T>>;
}

export interface PagePolicyContext {
  canInvoke(params: {
    actor: ActorIdentity;
    definition: CustomPageDefinition;
    action: ActionCommandDeclaration;
  }): boolean | Promise<boolean>;
}

export type CommandDenialCode =
  | "UNDECLARED_ACTION"
  | "POLICY_DENIED"
  | "HUMAN_APPROVAL_REQUIRED"
  | "INVALID_COMMAND_EFFECT";

export type CommandDispatchResult<T = unknown> =
  | {
      status: "accepted";
      command: string;
      data: T;
      effects: readonly CanonicalEffect[];
    }
  | {
      status: "denied";
      command: string;
      code: CommandDenialCode;
      message: string;
    };

export interface ActionCommandDispatcher {
  dispatch<T>(params: {
    command: string;
    input?: Readonly<Record<string, unknown>>;
    actor: ActorIdentity;
    approval?: HumanApprovalProof;
  }): Promise<CommandDispatchResult<T>>;
}

export type RuntimeAuditEventKind =
  | "runtime.loading"
  | "runtime.ready"
  | "runtime.render_failed"
  | "runtime.fallback_ready"
  | "runtime.fallback_failed"
  | "query.completed"
  | "query.denied"
  | "action.accepted"
  | "action.denied";

export interface RuntimeAuditEvent {
  sequence: number;
  kind: RuntimeAuditEventKind;
  occurredAt: string;
  subject?: string;
  code?: string;
}

export interface RuntimeAuditMetadata {
  runtimeId: string;
  definitionId: string;
  definitionVersion: string;
  packageId: string;
  packageVersion: string;
  events: readonly RuntimeAuditEvent[];
}

export interface PageAuditSink {
  record(event: Omit<RuntimeAuditEvent, "sequence" | "occurredAt">): void;
  snapshot(): RuntimeAuditMetadata;
}

export interface PageRuntimeCapabilities {
  readonly queries: ScopedQueryAdapter;
  readonly actions: ActionCommandDispatcher;
  readonly audit: RuntimeAuditMetadata;
}

export interface CustomPageRenderer<Props, Output> {
  render(props: Readonly<Props>, capabilities: PageRuntimeCapabilities): Promise<Output>;
}

export interface StandardViewAdapter<FallbackOutput> {
  render(
    fallback: StandardViewFallback,
    context: { definitionId: string; renderError: string },
  ): Promise<FallbackOutput>;
}

export type CustomPageRenderState<Output, FallbackOutput> =
  | { status: "loading"; audit: RuntimeAuditMetadata }
  | { status: "ready"; content: Output; audit: RuntimeAuditMetadata }
  | {
      status: "fallback";
      content: FallbackOutput;
      fallback: StandardViewFallback;
      renderError: string;
      audit: RuntimeAuditMetadata;
    }
  | {
      status: "error";
      fallback: StandardViewFallback;
      renderError: string;
      fallbackError: string;
      audit: RuntimeAuditMetadata;
    };
