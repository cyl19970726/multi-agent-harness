import { OperationalFactRow } from "./AgentStreamPrimitives";
import {
  resolveContentReference,
  type ToolEpisode,
  type ToolEpisodeContentReference,
} from "../../../model/providerEventTimeline";

function readable(value:string|null|undefined,fallback="Unavailable") {
  if(!value)return fallback;
  return value.split(/[_-]+/).filter(Boolean).map(part=>part.charAt(0).toUpperCase()+part.slice(1)).join(" ");
}

function displayValue(value:unknown) {
  return typeof value==="string"?value:JSON.stringify(value,null,2);
}

function ContentSection({label,content}:{label:string;content:ToolEpisodeContentReference|null}) {
  if(!content)return <section className="aw-tool-content" data-available="false"><h4>{label}</h4><p>Not projected by this provider record.</p></section>;
  const resolved=resolveContentReference(content.record,content.reference);
  return <section className="aw-tool-content" data-available={resolved.available||undefined}>
    <h4>{label}</h4>
    {resolved.available
      ? <pre>{displayValue(resolved.value)}</pre>
      : <p>{readable(resolved.reason)}</p>}
  </section>;
}

function EpisodeFacts({episode}:{episode:ToolEpisode}) {
  const first=episode.occurrences[0]!.record;
  return <dl className="aw-tool-facts">
    <div><dt>Call ID</dt><dd title={episode.call_id}>{episode.call_id}</dd></div>
    {!episode.tool_name&&<div><dt>Tool name</dt><dd>{readable(episode.tool_name_unavailable_reason,"Unavailable")}</dd></div>}
    <div><dt>Parent call</dt><dd title={episode.parent_call_id??undefined}>{episode.parent_call_id??"No parent recorded"}</dd></div>
    <div><dt>Category</dt><dd>{readable(episode.operation_category)}</dd></div>
    <div><dt>Provider</dt><dd>{readable(episode.provider)}</dd></div>
    <div><dt>Session generation</dt><dd>{first.agent_session_generation}</dd></div>
    <div><dt>Source order</dt><dd>{episode.occurrences.map(({record})=>`${record.ordering_key.kind}:${record.ordering_key.value}`).join(" → ")}</dd></div>
  </dl>;
}

function RawEvidence({episode}:{episode:ToolEpisode}) {
  return <details className="aw-tool-raw">
    <summary>Original provider-native records ({episode.occurrences.length})</summary>
    <div>{episode.occurrences.map(({record,fragment})=><section key={`${record.record_id}:${fragment.fragment_id}`}>
      <p>{readable(fragment.semantic_kind)} · {record.row_locator}</p>
      <pre>{displayValue(record.native_event)}</pre>
    </section>)}</div>
  </details>;
}

export function ToolEpisodeDetails({episode,context=false}:{episode:ToolEpisode;context?:boolean}) {
  return <div className="aw-tool-episode-detail" data-context={context||undefined}>
    <div className="aw-tool-structured-content">
      <ContentSection label="Arguments" content={episode.arguments_ref}/>
      <ContentSection label="Result" content={episode.result_ref}/>
      <ContentSection label="Error" content={episode.error_ref}/>
    </div>
    <EpisodeFacts episode={episode}/>
    <RawEvidence episode={episode}/>
  </div>;
}

export function ToolEpisodeRow({episode,expanded,selected,onToggle,timestamp}:{episode:ToolEpisode;expanded:boolean;selected:boolean;onToggle:()=>void;timestamp:string}) {
  const summary=[episode.primary_target,episode.parent_call_id?`parent ${episode.parent_call_id}`:null].filter(Boolean).join(" · ")||"Target unavailable in provider record";
  return <div className="aw-native-facts-trail" data-tool-call-id={episode.call_id}>
    <OperationalFactRow
      kind="tool"
      status={episode.outcome}
      title={episode.tool_name??"Unknown tool"}
      summary={summary}
      timestamp={timestamp}
      expanded={expanded}
      selected={selected}
      onToggle={onToggle}
    >
      <ToolEpisodeDetails episode={episode}/>
    </OperationalFactRow>
  </div>;
}
