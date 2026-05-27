import type { ReactNode } from "react";

interface PillProps {
  children: ReactNode;
  tone?: "default" | "good" | "warn" | "bad";
}

export function Pill({ children, tone = "default" }: PillProps) {
  return <span className={`pill ${tone}`}>{children}</span>;
}
