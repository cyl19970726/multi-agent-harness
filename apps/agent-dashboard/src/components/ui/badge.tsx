import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium leading-none transition-colors",
  {
    variants: {
      tone: {
        muted: "border-border bg-muted text-muted-foreground",
        running:
          "border-status-running/30 bg-status-running/12 text-status-running",
        good: "border-status-good/30 bg-status-good/12 text-status-good",
        warn: "border-status-warn/30 bg-status-warn/12 text-status-warn",
        bad: "border-status-bad/30 bg-status-bad/12 text-status-bad",
        info: "border-status-info/30 bg-status-info/12 text-status-info",
        decision:
          "border-status-decision/30 bg-status-decision/12 text-status-decision",
        idle: "border-status-idle/30 bg-status-idle/12 text-status-idle",
      },
    },
    defaultVariants: {
      tone: "muted",
    },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, tone, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

export { Badge, badgeVariants };
