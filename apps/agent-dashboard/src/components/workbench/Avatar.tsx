import { cn } from "@/lib/utils";
import { StatusDot, type StatusTone } from "./atoms";

export function initials(value: string): string {
  return (
    value
      .split(/[-_\s]/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0]?.toUpperCase() ?? "")
      .join("") || "?"
  );
}

/** Square monogram avatar with a live status dot. */
export function Avatar({
  name,
  tone = "idle",
  size = "sm",
}: {
  name: string;
  tone?: StatusTone;
  size?: "sm" | "lg";
}) {
  return (
    <span
      className={cn(
        "relative grid shrink-0 place-items-center rounded-md bg-secondary font-mono font-semibold text-foreground/80 ring-1 ring-border",
        size === "lg" ? "size-12 text-sm" : "size-8 text-[11px]",
      )}
    >
      {initials(name)}
      <StatusDot
        tone={tone}
        pulse={tone === "running"}
        className="absolute -bottom-0.5 -right-0.5 ring-2 ring-card"
      />
    </span>
  );
}
