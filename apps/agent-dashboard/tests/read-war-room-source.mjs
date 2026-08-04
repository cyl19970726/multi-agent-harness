import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

/**
 * Read the Team War Room composition as one source string.
 *
 * The surface was split into `components/workbench/team/**` so the Works,
 * Activity, Members and capacity units could each be tested and changed
 * independently. The existing source assertions are about the composition as a
 * whole ("the War Room contains X"), not about which file X lives in, so they
 * read the surface plus every extracted Team component together.
 *
 * Behavioural guarantees that depend on rendered geometry, focus, or roles
 * belong in `team-war-room-first-viewport-check.mjs`, not here.
 */
export async function readWarRoomSource(dashboardRoot) {
  const teamDir = join(dashboardRoot, "src/components/workbench/team");
  const entries = (await readdir(teamDir)).filter((name) => /\.tsx?$/.test(name)).sort();
  const sources = await Promise.all([
    readFile(join(dashboardRoot, "src/surfaces/TeamWarRoom.tsx"), "utf8"),
    ...entries.map((name) => readFile(join(teamDir, name), "utf8")),
  ]);
  return sources.join("\n");
}
