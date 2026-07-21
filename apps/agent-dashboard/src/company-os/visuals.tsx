import { Bot, Landmark, LibraryBig, Scale, Sparkles } from "lucide-react";
import type { ReactNode } from "react";

import { portraitFor } from "@/components/workbench/identity/portraits";
import { cn } from "@/lib/utils";

export function ActorAvatar({
  identity,
  name,
  size = "md",
  ring = "neutral",
  className,
}: {
  identity: string;
  name: string;
  size?: "sm" | "md" | "lg" | "hero";
  ring?: "neutral" | "good" | "warm" | "external";
  className?: string;
}) {
  const src = portraitFor(`${identity} ${name}`);
  return (
    <span
      className={cn(
        "company-avatar relative grid shrink-0 place-items-center overflow-hidden rounded-full border bg-card text-xs font-semibold",
        size === "sm" && "size-8",
        size === "md" && "size-11",
        size === "lg" && "size-16",
        size === "hero" && "size-28 lg:size-36",
        ring === "good" && "border-status-good/55 ring-4 ring-status-good/10",
        ring === "warm" && "border-primary/55 ring-4 ring-primary/10",
        ring === "external" && "border-sky-500/45 ring-4 ring-sky-500/10",
        ring === "neutral" && "border-border ring-4 ring-background/70",
        className,
      )}
      aria-label={name}
    >
      {src ? <img src={src} alt="" className="size-full object-cover" /> : name.slice(0, 2).toUpperCase()}
    </span>
  );
}

export function ObjectEmblem({ kind, className }: { kind: "docs" | "module" | "work" | "approval" | "agent"; className?: string }) {
  const Icon = kind === "docs" ? LibraryBig : kind === "module" ? Scale : kind === "work" ? Landmark : kind === "agent" ? Bot : Sparkles;
  return <span className={cn("company-emblem grid size-10 place-items-center rounded-xl border border-primary/25 bg-primary/[0.07] text-primary", className)}><Icon className="size-5" /></span>;
}

export function EditorialTitle({ children, className }: { children: ReactNode; className?: string }) {
  return <h1 className={cn("company-editorial-title text-4xl leading-[0.98] tracking-[-0.035em] text-foreground sm:text-5xl", className)}>{children}</h1>;
}

export function ArtField({ className }: { className?: string }) {
  return <div aria-hidden className={cn("company-art-field pointer-events-none absolute inset-0 overflow-hidden", className)}><span /><span /><span /></div>;
}
