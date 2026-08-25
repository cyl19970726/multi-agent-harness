import { Context } from "@deepseek-ai/cordis";
import SessionStore, { SessionId } from "@deepseek-ai/dsh-session";
import JsonlSessionPersistence from "@deepseek-ai/dsh-session-persistence-jsonl";

const MAX_BYTES = 16 * 1024 * 1024;

export async function readOfficialSessionJsonl({ root, sessionId }) {
  const ctx = new Context();
  try {
    await ctx.plugin(SessionStore);
    await ctx.plugin(JsonlSessionPersistence, { root, compression: "zstd" });
    const raw = await ctx.sessionPersistence.readRaw(SessionId(sessionId));
    if (!raw || raw.meta.id !== sessionId) throw new Error("DEEPSEEK_SESSION_READER_NOT_FOUND");
    if (Buffer.byteLength(raw.content, "utf8") > MAX_BYTES) {
      throw new Error("DEEPSEEK_SESSION_READER_SOURCE_TOO_LARGE");
    }
    return raw.content;
  } finally {
    await ctx.fiber.dispose();
  }
}
