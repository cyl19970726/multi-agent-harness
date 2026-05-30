import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

interface ActionButtonProps {
  children: ReactNode;
  icon?: LucideIcon;
  tone?: "primary" | "neutral" | "danger";
  disabled?: boolean;
  title?: string;
  onClick?: () => void;
}

export function ActionButton({ children, icon: Icon, tone = "neutral", disabled, title, onClick }: ActionButtonProps) {
  return (
    <button className={`actionButton ${tone}`} type="button" disabled={disabled} title={title} onClick={onClick}>
      {Icon && <Icon aria-hidden="true" size={16} />}
      <span>{children}</span>
    </button>
  );
}

interface IconButtonProps {
  label: string;
  icon: LucideIcon;
  active?: boolean;
  onClick?: () => void;
}

export function IconButton({ label, icon: Icon, active, onClick }: IconButtonProps) {
  return (
    <button className={`iconButton${active ? " active" : ""}`} type="button" title={label} aria-label={label} onClick={onClick}>
      <Icon aria-hidden="true" size={18} />
      <span className="iconButtonLabel">{label}</span>
    </button>
  );
}

interface StatusBadgeProps {
  children: ReactNode;
  tone?: "good" | "warn" | "bad" | "info" | "muted";
}

export function StatusBadge({ children, tone = "muted" }: StatusBadgeProps) {
  return <span className={`statusBadge ${tone}`}>{children}</span>;
}

interface SectionPanelProps {
  title: string;
  kicker?: string;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function SectionPanel({ title, kicker, action, children, className = "" }: SectionPanelProps) {
  return (
    <section className={`sectionPanel ${className}`.trim()}>
      <header className="sectionHeader">
        <div>
          {kicker && <p className="sectionKicker">{kicker}</p>}
          <h2>{title}</h2>
        </div>
        {action && <div className="sectionAction">{action}</div>}
      </header>
      {children}
    </section>
  );
}

interface SegmentedControlProps<T extends string> {
  label: string;
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}

export function SegmentedControl<T extends string>({ label, options, value, onChange }: SegmentedControlProps<T>) {
  return (
    <div className="segmentedControl" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={option.value === value ? "active" : ""}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

interface TimelineRowProps {
  kind: string;
  title: string;
  meta: string;
  body?: string;
  severity?: "high" | "medium" | "low";
  onClick?: () => void;
}

export function TimelineRow({ kind, title, meta, body, severity, onClick }: TimelineRowProps) {
  return (
    <button className={`timelineRow ${severity ?? ""}`.trim()} type="button" onClick={onClick}>
      <span className="timelineKind">{kind}</span>
      <span className="timelineText">
        <strong>{title}</strong>
        <small>{meta}</small>
        {body && <span>{body}</span>}
      </span>
    </button>
  );
}

export function EmptyState({ title, body }: { title: string; body: string }) {
  return (
    <div className="emptyState">
      <strong>{title}</strong>
      <span>{body}</span>
    </div>
  );
}
