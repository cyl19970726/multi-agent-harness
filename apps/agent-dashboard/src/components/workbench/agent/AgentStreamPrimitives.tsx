import type { ReactNode } from "react";
import {
  BrainCircuit,
  BriefcaseBusiness,
  CheckCheck,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleDot,
  FileCheck2,
  MessageSquareText,
  PackageCheck,
  RadioTower,
  TerminalSquare,
  Wrench,
  type LucideIcon,
} from "lucide-react";

export type EventFamily =
  | "authored"
  | "reasoning"
  | "tool"
  | "work"
  | "review"
  | "delivery"
  | "runtime"
  | "result"
  | "error"
  | "fact";

export type EventTone = "neutral" | "accent" | "info" | "positive" | "warning" | "danger";

export interface EventPresentation {
  family:EventFamily;
  icon:LucideIcon;
  tone:EventTone;
  label:string;
}

function normalize(value:string|null|undefined) {
  return (value ?? "").trim().toLowerCase().replace(/[\s-]+/g,"_");
}

function readableToken(value:string) {
  if(!value)return "Event";
  return value
    .split("_")
    .filter(Boolean)
    .map(part=>part.charAt(0).toUpperCase()+part.slice(1))
    .join(" ");
}

function statusTone(status:string):EventTone|null {
  if(/error|fail|fatal|cancel|reject/.test(status))return "danger";
  if(/block|interrupt|pause|stale|timeout|warn/.test(status))return "warning";
  if(/complete|success|accept|approve|deliver|resolve|pass|closed/.test(status))return "positive";
  if(/running|active|start|progress|queued|pending|review/.test(status))return "info";
  return null;
}

/**
 * Maps canonical event vocabulary into a deliberately small presentation
 * language. Unknown kinds stay truthful: they become a neutral Fact rather
 * than being guessed into a more specific product concept.
 */
export function eventPresentation(kind:string|null|undefined,status?:string|null):EventPresentation {
  const canonicalKind=normalize(kind);
  const canonicalStatus=normalize(status);
  const statusOverride=statusTone(canonicalStatus);

  const exceptional=statusOverride==="danger"||statusOverride==="warning"?statusOverride:null;
  if(/message|authored|reply|turn/.test(canonicalKind))return {family:"authored",icon:MessageSquareText,tone:exceptional??"accent",label:"Authored message"};
  if(/thinking|reasoning|analysis|plan/.test(canonicalKind))return {family:"reasoning",icon:BrainCircuit,tone:statusOverride??"neutral",label:"Reasoning"};
  if(/tool|command|shell|terminal/.test(canonicalKind))return {family:"tool",icon:Wrench,tone:exceptional??"info",label:"Tool activity"};
  if(/work|assignment|responsibility/.test(canonicalKind))return {family:"work",icon:BriefcaseBusiness,tone:exceptional??"accent",label:"Work update"};
  if(/gate|review|approval|finding/.test(canonicalKind))return {family:"review",icon:FileCheck2,tone:statusOverride??"warning",label:"Review gate"};
  if(/delivery|artifact|evidence|report/.test(canonicalKind))return {family:"delivery",icon:PackageCheck,tone:statusOverride??"positive",label:"Delivery"};
  if(/runtime|session|provider|supervisor|reconnect/.test(canonicalKind))return {family:"runtime",icon:RadioTower,tone:exceptional??"info",label:"Runtime"};
  if(/result|outcome|complete|success/.test(canonicalKind))return {family:"result",icon:CheckCheck,tone:statusOverride??"positive",label:"Result"};
  if(/error|failure|exception|fatal/.test(canonicalKind))return {family:"error",icon:CircleAlert,tone:"danger",label:"Error"};
  return {family:"fact",icon:canonicalKind?TerminalSquare:CircleDot,tone:statusOverride??"neutral",label:canonicalKind?readableToken(canonicalKind):"Operational fact"};
}

export function StreamKindMark({
  presentation,
  label,
}: {
  presentation:EventPresentation;
  label?:string;
}) {
  const Icon=presentation.icon;
  return <span className="aw-stream-kind" data-family={presentation.family} data-tone={presentation.tone}>
    <Icon aria-hidden="true"/>
    <span>{label??presentation.label}</span>
  </span>;
}

/** A quiet grouping boundary for one canonical turn or operational episode. */
export function StreamEpisode({
  label,
  detail,
  timestamp,
  children,
}: {
  label:string;
  detail?:string|null;
  timestamp?:ReactNode;
  children:ReactNode;
}) {
  return <section className="aw-stream-episode">
    <header className="aw-stream-episode__header">
      <span>{label}</span>
      {detail&&<small>{detail}</small>}
      {timestamp&&<span className="aw-stream-episode__time">{timestamp}</span>}
    </header>
    <div className="aw-stream-episode__body">{children}</div>
  </section>;
}

/**
 * Compact display for non-authored operational events. The caller owns the
 * canonical data, expansion state and selection; this component only renders
 * their presentation and never fabricates fallback records.
 */
export function OperationalFactRow({
  kind,
  status,
  title,
  summary,
  timestamp,
  expanded=false,
  selected=false,
  onToggle,
  onSelect,
  children,
}: {
  kind:string;
  status?:string|null;
  title:string;
  summary?:ReactNode;
  timestamp?:ReactNode;
  expanded?:boolean;
  selected?:boolean;
  onToggle?:()=>void;
  onSelect?:()=>void;
  children?:ReactNode;
}) {
  const presentation=eventPresentation(kind,status);
  const toggle=()=>{onToggle?.();onSelect?.();};
  const content=<>
    {onToggle&&(expanded?<ChevronDown className="aw-stream-fact__disclosure" aria-hidden="true"/>:<ChevronRight className="aw-stream-fact__disclosure" aria-hidden="true"/>)}
    <StreamKindMark presentation={presentation}/>
    <span className="aw-stream-fact__content">
      <span className="aw-stream-fact__heading">
        <strong>{title}</strong>
        {status&&<span className="aw-stream-fact__status">{readableToken(normalize(status))}</span>}
      </span>
      {!expanded&&summary&&<span className="aw-stream-fact__summary">{summary}</span>}
    </span>
    {timestamp&&<span className="aw-stream-fact__time">{timestamp}</span>}
  </>;

  return <article className="aw-stream-fact" data-family={presentation.family} data-tone={presentation.tone} data-expanded={expanded||undefined} data-selected={selected||undefined}>
    {onToggle
      ? <button type="button" className="aw-stream-fact__trigger" aria-expanded={expanded} onClick={toggle}>{content}</button>
      : onSelect
        ? <button type="button" className="aw-stream-fact__trigger" onClick={onSelect}>{content}</button>
        : <div className="aw-stream-fact__trigger">{content}</div>}
    {expanded&&children&&<div className="aw-stream-fact__detail">{children}</div>}
  </article>;
}

/** A dedicated authored turn wrapper so dialogue remains visually primary. */
export function AuthoredStreamTurn({
  actor,
  avatar,
  timestamp,
  meta,
  selected=false,
  onSelect,
  children,
}: {
  actor:string;
  avatar?:ReactNode;
  timestamp?:ReactNode;
  meta?:ReactNode;
  selected?:boolean;
  onSelect?:()=>void;
  children:ReactNode;
}) {
  const body=<>{avatar}<span className="aw-authored-stream-turn__content"><header><strong>{actor}</strong><span>Authored message</span>{timestamp&&<time>{timestamp}</time>}</header><div className="aw-authored-stream-turn__body">{children}</div>{meta&&<footer>{meta}</footer>}</span></>;
  return onSelect
    ? <button type="button" className="aw-authored-stream-turn" data-selected={selected||undefined} onClick={onSelect}>{body}</button>
    : <article className="aw-authored-stream-turn" data-selected={selected||undefined}>{body}</article>;
}
