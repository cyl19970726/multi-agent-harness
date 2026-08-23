export const DSH_MEMBER_ROLE_ACTION_TOKEN = "DSH_FIRM_MEMBER_ROLE_ACTION_TOKEN";

export function registerMemberRoleActionEnvironment(ctx, environment = process.env) {
  const token = environment.FIRM_MEMBER_ROLE_ACTION_TOKEN;
  if (!token) throw new Error("DSH_MEMBER_ROLE_ACTION_TOKEN_MISSING");
  return ctx.shellEnv.register({
    name: "star-harness-member-role-action",
    variables: {
      [DSH_MEMBER_ROLE_ACTION_TOKEN]: {
        description: "Ephemeral Supervisor capability for the current Star Harness AgentMember Role Action.",
      },
    },
    resolve(execution) {
      return execution.agent ? { [DSH_MEMBER_ROLE_ACTION_TOKEN]: token } : {};
    },
  });
}
