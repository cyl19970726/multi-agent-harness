import { ArrowUpRight, FileText, Landmark, ListTodo, Network, ShieldCheck, UserRound } from "lucide-react";

import { cn } from "@/lib/utils";

import type { CompanyOsLink } from "./types";

const iconFor = {
  document: FileText,
  record: Network,
  work: ListTodo,
  approval: ShieldCheck,
  finance: Landmark,
  module: Network,
  actor: UserRound,
};

export function RelationChips({
  links,
  emptyLabel = "No linked records",
  className,
}: {
  links?: CompanyOsLink[];
  emptyLabel?: string;
  className?: string;
}) {
  if (!links?.length) {
    return <p className="text-xs text-muted-foreground">{emptyLabel}</p>;
  }

  return (
    <div className={cn("flex flex-wrap gap-1.5", className)} aria-label="Linked records">
      {links.map((link) => {
        const Icon = iconFor[link.kind ?? "record"];
        const body = (
          <>
            <Icon className="size-3 shrink-0" aria-hidden />
            <span className="min-w-0 break-words">{link.label}</span>
            {link.meta && <span className="text-muted-foreground">{link.meta}</span>}
            {link.href && <ArrowUpRight className="size-3 shrink-0 opacity-70" aria-hidden />}
          </>
        );
        const className = "inline-flex max-w-full items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-xs text-foreground hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
        return link.href ? (
          <a key={link.id} href={link.href} data-company-os-ref={link.id} data-actor-type={link.actorType} data-financial-record-type={link.financialRecordType} className={className}>
            {body}
          </a>
        ) : (
          <span key={link.id} data-company-os-ref={link.id} data-actor-type={link.actorType} data-financial-record-type={link.financialRecordType} className={className}>
            {body}
          </span>
        );
      })}
    </div>
  );
}
