import {
  ArrowDownRight,
  Ban,
  BriefcaseBusiness,
  Circle,
  CircleStop,
  LockKeyhole,
  MessageSquareText,
  RefreshCw,
  RotateCcw,
  ShieldCheck,
  SlidersHorizontal,
} from "lucide-react";

function ActionIcon({kind}:{kind:string}) {
  const normalized=kind.toLowerCase();
  const Icon=/message/.test(normalized)?MessageSquareText
    :/assign|reassign|work/.test(normalized)?BriefcaseBusiness
    :/review|gate/.test(normalized)?ShieldCheck
    :/retry|reconcile/.test(normalized)?RefreshCw
    :/resume|reopen/.test(normalized)?RotateCcw
    :/interrupt/.test(normalized)?Ban
    :/stop|close|retire|cancel/.test(normalized)?CircleStop
    :ArrowDownRight;
  return <Icon aria-hidden="true"/>;
}

export function WorkspaceSection({
  title,
  hint,
  primary=false,
  children,
}:{
  title:string;
  hint?:string;
  primary?:boolean;
  children:React.ReactNode;
}) {
  return <section className="aw-context-section" data-primary={primary || undefined}>
    <header className="aw-context-section__header">
      <h2>{title}</h2>
      {hint&&<span>{hint}</span>}
    </header>
    {children}
  </section>;
}

export function WorkspaceFact({label,value,canonicalValue}:{label:string;value:string;canonicalValue?:string}) {
  return <div className="aw-fact-row">
    <span>{label}</span>
    <strong title={canonicalValue&&canonicalValue!==value?`Canonical: ${canonicalValue}`:value}>{value}</strong>
  </div>;
}

export function WorkspaceState({label,tone="muted"}:{label:string;tone?:"muted"|"good"|"running"|"warn"|"bad"}) {
  return <span className="aw-state-mark" data-tone={tone}><Circle aria-hidden="true"/>{label}</span>;
}

export function WorkspaceCanvasIntro({
  eyebrow,
  title,
  detail,
  privacy=false,
  compact=false,
  facts,
}:{
  eyebrow:string;
  title:string;
  detail:string;
  privacy?:boolean;
  compact?:boolean;
  facts:Array<string|null|undefined>;
}) {
  return <header className="aw-canvas-intro" data-compact={compact || undefined}>
    <div>
      <p className="aw-canvas-intro__eyebrow">{privacy?<LockKeyhole aria-hidden="true"/>:<ArrowDownRight aria-hidden="true"/>}{eyebrow}</p>
      <h2>{title}</h2>
      <p>{detail}</p>
    </div>
    <div className="aw-canvas-intro__facts">{facts.filter(Boolean).map(fact=><span key={fact}>{fact}</span>)}</div>
  </header>;
}

const ACTION_DETAIL:Record<string,string>={
  send_message:"Send guidance to the next agent turn",
  close_member_run:"Close the current runtime",
  reopen_member_run:"Reopen a closed runtime",
  assign_work:"Assign guidance for the next agent turn",
  rebind_work:"Assign guidance for the next agent turn",
  request_gate_evaluation:"Request a gate review",
};

function actionDetail(key:string) {
  return ACTION_DETAIL[key]??key.split(/[_-]+/).filter(Boolean).map(part=>part.charAt(0).toUpperCase()+part.slice(1)).join(" ");
}

export function WorkspaceActionIndex({
  actions,
  label,
}:{
  actions:Array<{key:string;label:string;disabledReason?:string|null}>;
  label:string;
}) {
  return <div className="aw-action-index" aria-label={label}>
    <p><SlidersHorizontal aria-hidden="true"/>Choose in composer</p>
    <ul>{actions.map(action=><li key={action.key} className="aw-action-row" data-danger={/close|retire|cancel|interrupt|stop/i.test(action.label)||undefined} data-disabled={Boolean(action.disabledReason)||undefined} title={action.disabledReason??undefined}>
      <ActionIcon kind={action.key}/>
      <span className="aw-action-row__text"><span>{action.label}</span><small>{actionDetail(action.key)}</small></span>
    </li>)}</ul>
  </div>;
}
