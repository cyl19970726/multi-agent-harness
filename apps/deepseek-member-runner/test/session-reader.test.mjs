import assert from "node:assert/strict";
import test from "node:test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Context } from "@deepseek-ai/cordis";
import SessionStore, { SESSION_FORMAT_VERSION, SessionId } from "@deepseek-ai/dsh-session";
import JsonlSessionPersistence from "@deepseek-ai/dsh-session-persistence-jsonl";
import { readOfficialSessionJsonl } from "../src/session-reader.mjs";

test("reads one exact official zstd Session without rewriting it", async () => {
  const root = await mkdtemp(join(tmpdir(), "star-dsh-session-reader-"));
  const id = "star-session-reader-fixture";
  const ctx = new Context();
  try {
    await ctx.plugin(SessionStore);
    await ctx.plugin(JsonlSessionPersistence, { root, compression: "zstd" });
    await ctx.sessionPersistence.create({ version: SESSION_FORMAT_VERSION, id: SessionId(id), createdAt: 1 });
    await ctx.sessionPersistence.append(SessionId(id), [
      { type: "turn/start", seq: 0, time: 1, data: { turn: 1 } },
      { type: "turn/end", seq: 1, time: 2, data: { turn: 1, reason: { kind: "completed" } } },
    ]);
    const content = await readOfficialSessionJsonl({ root, sessionId: id });
    assert.match(content, /"type":"turn\/end"/);
    await assert.rejects(() => readOfficialSessionJsonl({ root, sessionId: "star-missing" }), /NOT_FOUND/);
  } finally {
    await ctx.fiber.dispose();
    await rm(root, { recursive: true, force: true });
  }
});
