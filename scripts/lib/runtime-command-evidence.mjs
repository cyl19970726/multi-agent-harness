// Read-only evidence compatibility. No fallback from an explicit unknown or
// conflicting phase, and neither phase nor legacy status proves effect alone.
export function isAppliedRuntimeCommand(command) {
  if (!command || command.effect_certainty !== "applied") return false;
  if (command.phase === undefined) return command.status === "applied";
  return command.phase === "settled"
    && (command.status === undefined || command.status === "applied");
}
