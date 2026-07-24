import { cn } from "@/lib/utils";
import { portraitFor } from "@/components/workbench/identity/portraits";
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

/** Shared portrait or monogram fallback with a live status dot. */
export function Avatar({
  name,
  identity,
  tone = "idle",
  size = "sm",
}: {
  name: string;
  identity?: string;
  tone?: StatusTone;
  size?: "sm" | "lg" | "xl";
}) {
  const portrait = portraitFor(`${identity ?? ""} ${name}`);
  return (
    <span
      className={cn(
        "relative grid shrink-0 place-items-center overflow-hidden rounded-full bg-secondary font-mono font-semibold text-foreground/80 ring-1 ring-border",
        size === "xl" ? "size-16 text-base" : size === "lg" ? "size-12 text-sm" : "size-8 text-[11px]",
      )}
      aria-label={name}
    >
      {portrait ? (
        <img src={portrait} alt="" className="size-full object-cover saturate-[.88] contrast-[.98]" />
      ) : initials(name)}
      <StatusDot
        tone={tone}
        pulse={tone === "running"}
        className="absolute -bottom-0.5 -right-0.5 ring-2 ring-card"
      />
    </span>
  );
}
