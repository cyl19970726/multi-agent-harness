#!/usr/bin/env node
import { homedir } from "node:os";
import { resolve } from "node:path";
import { readOfficialSessionJsonl } from "../src/session-reader.mjs";
const sessionId = process.argv[2]?.trim();
if (!sessionId || !sessionId.startsWith("star-") || sessionId.includes("/") || sessionId.includes("\\")) {
  throw new Error("DEEPSEEK_SESSION_READER_INVALID_ID");
}
const root = resolve(process.env.DSH_SESSION_ROOT ?? `${homedir()}/.dsh/sessions/star-harness`);
process.stdout.write(await readOfficialSessionJsonl({ root, sessionId }));
