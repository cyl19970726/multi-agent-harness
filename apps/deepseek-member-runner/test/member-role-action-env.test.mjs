import assert from "node:assert/strict";
import test from "node:test";
import { SENSITIVE_ENV_PATTERN } from "@deepseek-ai/dsh-subprocess";
import {
  DSH_MEMBER_ROLE_ACTION_TOKEN,
  registerMemberRoleActionEnvironment,
} from "../src/member-role-action-env.mjs";

test("contributes only the exact ephemeral member capability to agent tool executions", () => {
  let contributor;
  const ctx = { shellEnv: { register(value) { contributor = value; return () => {}; } } };
  registerMemberRoleActionEnvironment(ctx, {
    FIRM_MEMBER_ROLE_ACTION_TOKEN: "exact-live-capability",
    DEEPSEEK_API_KEY: "must-stay-scrubbed",
    OTHER_SECRET: "must-stay-scrubbed",
  });
  assert.deepEqual(Object.keys(contributor.variables), [DSH_MEMBER_ROLE_ACTION_TOKEN]);
  assert.deepEqual(contributor.resolve({ agent: { session: { id: "session-1" } } }), {
    [DSH_MEMBER_ROLE_ACTION_TOKEN]: "exact-live-capability",
  });
  assert.deepEqual(contributor.resolve({}), {});
  assert.equal(JSON.stringify(contributor.variables).includes("exact-live-capability"), false);
  for (const key of ["DEEPSEEK_API_KEY", "OTHER_SECRET", "DB_PASSWORD", "RANDOM_TOKEN"]) {
    assert.equal(SENSITIVE_ENV_PATTERN.test(key), true);
    assert.equal(Object.hasOwn(contributor.resolve({ agent: {} }), key), false);
  }
});

test("fails closed when the Supervisor did not issue a member capability", () => {
  const ctx = { shellEnv: { register() { throw new Error("must not register"); } } };
  assert.throws(
    () => registerMemberRoleActionEnvironment(ctx, {}),
    /DSH_MEMBER_ROLE_ACTION_TOKEN_MISSING/,
  );
});
