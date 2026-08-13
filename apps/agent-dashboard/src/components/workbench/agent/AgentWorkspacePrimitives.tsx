import { ArrowDownRight, Circle, LockKeyhole, SlidersHorizontal } from "lucide-react";

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
    <strong title={canonicalValue??value}>{value}{canonicalValue&&canonicalValue!==value&&<small>{canonicalValue}</small>}</strong>
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

export function WorkspaceActionIndex({
  actions,
  label,
}:{
  actions:Array<{key:string;label:string;disabledReason?:string|null}>;
  label:string;
}) {
  return <div className="aw-action-index" aria-label={label}>
    <p><SlidersHorizontal aria-hidden="true"/>Choose in composer</p>
    <ul>{actions.map(action=><li key={action.key} className="aw-action-row" data-disabled={Boolean(action.disabledReason)||undefined} title={action.disabledReason??undefined}>
      <Circle aria-hidden="true"/>
      <span>{action.label}</span>
    </li>)}</ul>
  </div>;
}
