import type {
  ActionCommandDispatcher,
  ActionCommandTransport,
  ActorIdentity,
  CommandDenialCode,
  CommandDispatchResult,
  CustomPageDefinition,
  HumanApprovalProof,
  PageAuditSink,
  PagePolicyContext,
} from "./types";

function hasHumanApproval(approval: HumanApprovalProof | undefined): boolean {
  return Boolean(
    approval && approval.status === "approved" && approval.decidedBy.kind === "human",
  );
}

function denied<T>(params: {
  audit: PageAuditSink;
  command: string;
  code: CommandDenialCode;
  message: string;
}): CommandDispatchResult<T> {
  params.audit.record({ kind: "action.denied", subject: params.command, code: params.code });
  return {
    status: "denied",
    command: params.command,
    code: params.code,
    message: params.message,
  };
}

export function createActionCommandDispatcher(params: {
  definition: CustomPageDefinition;
  transport: ActionCommandTransport;
  policy: PagePolicyContext;
  audit: PageAuditSink;
  grantedActionNames?: ReadonlySet<string>;
}): ActionCommandDispatcher {
  const declarations = new Map(params.definition.actions.map((action) => [action.name, action]));

  return Object.freeze({
    async dispatch<T>({ command, input = {}, actor, approval }: {
      command: string;
      input?: Readonly<Record<string, unknown>>;
      actor: ActorIdentity;
      approval?: HumanApprovalProof;
    }): Promise<CommandDispatchResult<T>> {
      const declaration = declarations.get(command);
      if (!declaration || (params.grantedActionNames && !params.grantedActionNames.has(command))) {
        return denied({
          audit: params.audit,
          command,
          code: "UNDECLARED_ACTION",
          message: `Action is not declared for this page: ${command}`,
        });
      }

      const allowed = await params.policy.canInvoke({
        actor,
        definition: params.definition,
        action: declaration,
      });
      if (!allowed) {
        return denied({
          audit: params.audit,
          command,
          code: "POLICY_DENIED",
          message: "The current actor is not permitted to invoke this action",
        });
      }

      if (declaration.humanApproval === "required" && !hasHumanApproval(approval)) {
        return denied({
          audit: params.audit,
          command,
          code: "HUMAN_APPROVAL_REQUIRED",
          message: "This sensitive action requires an approved Approval decided by a Human",
        });
      }

      const transportResult = await params.transport.dispatch<T>({
        command,
        input: Object.freeze({ ...input }),
        actor,
        approval,
        policy: {
          definitionId: params.definition.id,
          allowedEffectKinds: [...declaration.allowedEffectKinds],
        },
      });
      const unexpectedEffect = transportResult.effects.find(
        (effect) => !declaration.allowedEffectKinds.includes(effect.kind),
      );
      if (unexpectedEffect) {
        return denied({
          audit: params.audit,
          command,
          code: "INVALID_COMMAND_EFFECT",
          message: `Command returned an undeclared durable effect: ${unexpectedEffect.kind}`,
        });
      }

      params.audit.record({ kind: "action.accepted", subject: command });
      return {
        status: "accepted",
        command,
        data: transportResult.data,
        effects: transportResult.effects.map((effect) => ({ ...effect })),
      };
    },
  });
}
