import type {
  ProviderEventContentReference,
  ProviderEventFragment,
  ProviderNativeEventRecord,
} from "./roleViews";

export interface ProviderEventOccurrence {
  record:ProviderNativeEventRecord;
  fragment:ProviderEventFragment;
}

export interface ToolEpisode {
  kind:"tool_episode";
  episode_id:string;
  call_id:string|null;
  provider:ProviderNativeEventRecord["provider"];
  tool_name:string|null;
  tool_name_unavailable_reason:string|null;
  parent_call_id:string|null;
  operation_category:string|null;
  primary_target:string|null;
  outcome:"requested"|"started"|"completed"|"failed";
  arguments_ref:ToolEpisodeContentReference|null;
  result_ref:ToolEpisodeContentReference|null;
  error_ref:ToolEpisodeContentReference|null;
  occurrences:ProviderEventOccurrence[];
}

export interface ToolEpisodeContentReference {
  record:ProviderNativeEventRecord;
  reference:ProviderEventContentReference;
}

/**
 * A run of adjacent provider envelope/bookkeeping frames the provider adapter
 * could not classify into an operator-level fact (Claude `queue-operation`,
 * `user`, `attachment`, `last-prompt`, …). They are never discarded: the group
 * keeps every occurrence in provider source order so the row can expand back
 * into the exact original records.
 */
export interface EnvelopeGroup {
  kind:"envelope_group";
  group_id:string;
  provider:ProviderNativeEventRecord["provider"];
  occurrences:ProviderEventOccurrence[];
}

export type ProviderTimelineItem =
  | ToolEpisode
  | EnvelopeGroup
  | ({kind:"native"}&ProviderEventOccurrence);

/**
 * Envelope classification is structural, not a per-provider allow list: a
 * fragment is envelope exactly when the adapter projected no operator-level
 * payload for it. Tool, reasoning, assistant, usage, turn, artifact, session
 * metadata and malformed fragments stay first-class rows.
 */
export function isEnvelopeFragment(fragment:ProviderEventFragment):boolean {
  return fragment.payload.type==="native";
}

export function envelopeFrameLabel(occurrence:ProviderEventOccurrence):string {
  const payload=occurrence.fragment.payload;
  if(payload.type!=="native")return occurrence.fragment.semantic_kind;
  return [payload.event_type??"unknown native event",payload.event_subtype].filter(Boolean).join(" · ");
}

function compareRecords(left:ProviderNativeEventRecord,right:ProviderNativeEventRecord) {
  return left.ordering_key.kind===right.ordering_key.kind
    ? left.ordering_key.value-right.ordering_key.value
    : left.ordering_key.kind.localeCompare(right.ordering_key.kind);
}

function toolEpisodeKey(record:ProviderNativeEventRecord,callId:string) {
  return [record.provider,record.source_generation,record.agent_session_id,record.agent_session_generation,callId].join("\u0000");
}

function firstNonempty<T>(values:Array<T|null|undefined>):T|null {
  return values.find((value):value is T=>value!==null&&value!==undefined&&value!=="")??null;
}

function strongestOutcome(occurrences:ProviderEventOccurrence[]):ToolEpisode["outcome"] {
  const outcomes=occurrences.flatMap(({fragment})=>fragment.payload.type==="tool"&&fragment.payload.outcome?[fragment.payload.outcome]:[]);
  if(outcomes.includes("failed"))return "failed";
  if(outcomes.includes("completed"))return "completed";
  if(outcomes.includes("started"))return "started";
  return "requested";
}

function contentReference(occurrences:ProviderEventOccurrence[],field:"arguments"|"result"|"error",reverse=false):ToolEpisodeContentReference|null {
  const source=reverse?[...occurrences].reverse():occurrences;
  for(const occurrence of source){
    const payload=occurrence.fragment.payload;
    if(payload.type==="tool"&&payload[field])return {record:occurrence.record,reference:payload[field]!};
  }
  return null;
}

function buildEpisode(episodeId:string,callId:string|null,occurrences:ProviderEventOccurrence[]):ToolEpisode {
  const payloads=occurrences.flatMap(({fragment})=>fragment.payload.type==="tool"?[fragment.payload]:[]);
  return {
    kind:"tool_episode",
    episode_id:episodeId,
    call_id:callId,
    provider:occurrences[0]!.record.provider,
    tool_name:firstNonempty(payloads.map(payload=>payload.tool_name)),
    tool_name_unavailable_reason:firstNonempty(payloads.map(payload=>payload.tool_name_unavailable_reason)),
    parent_call_id:firstNonempty(payloads.map(payload=>payload.parent_call_id)),
    operation_category:firstNonempty(payloads.map(payload=>payload.operation_category)),
    primary_target:firstNonempty(payloads.map(payload=>payload.primary_target)),
    outcome:strongestOutcome(occurrences),
    arguments_ref:contentReference(occurrences,"arguments"),
    result_ref:contentReference(occurrences,"result",true),
    error_ref:contentReference(occurrences,"error",true),
    occurrences,
  };
}

/**
 * Builds one readable timeline without inventing provider relationships.
 * Tool fragments are paired only by their exact response-local call id and
 * provider/session/source generation. Missing call ids become structured,
 * standalone episodes and are never paired by adjacency.
 *
 * Adjacent envelope frames additionally collapse into one low-priority
 * `envelope_group` row. Grouping is adjacency-only in the already-ordered
 * timeline, so it never reorders records and never merges frames across an
 * intervening semantic row.
 */
export function projectProviderTimeline(records:ProviderNativeEventRecord[]):ProviderTimelineItem[] {
  const occurrences=records
    .flatMap(record=>record.fragments.map(fragment=>({record,fragment})))
    .sort((left,right)=>compareRecords(left.record,right.record)||left.fragment.fragment_index-right.fragment.fragment_index);
  const groups=new Map<string,ProviderEventOccurrence[]>();
  for(const occurrence of occurrences){
    const payload=occurrence.fragment.payload;
    if(payload.type!=="tool"||!payload.call_id)continue;
    const key=toolEpisodeKey(occurrence.record,payload.call_id);
    const group=groups.get(key);
    if(group)group.push(occurrence);else groups.set(key,[occurrence]);
  }
  const emitted=new Set<string>();
  const timeline:ProviderTimelineItem[]=[];
  for(const occurrence of occurrences){
    const payload=occurrence.fragment.payload;
    if(isEnvelopeFragment(occurrence.fragment)){
      const previous=timeline[timeline.length-1];
      if(previous&&previous.kind==="envelope_group"){previous.occurrences.push(occurrence);continue;}
      timeline.push({
        kind:"envelope_group",
        group_id:`envelope:${occurrence.record.record_id}:${occurrence.fragment.fragment_id}`,
        provider:occurrence.record.provider,
        occurrences:[occurrence],
      });
      continue;
    }
    if(payload.type!=="tool"){timeline.push({kind:"native",...occurrence});continue;}
    if(!payload.call_id){
      timeline.push(buildEpisode(`tool:${occurrence.record.record_id}:${occurrence.fragment.fragment_id}`,null,[occurrence]));
      continue;
    }
    const key=toolEpisodeKey(occurrence.record,payload.call_id);
    if(emitted.has(key))continue;
    emitted.add(key);
    timeline.push(buildEpisode(`tool:${key}`,payload.call_id,groups.get(key)??[occurrence]));
  }
  return timeline;
}

export type ResolvedContentReference =
  | {available:true;value:unknown}
  | {available:false;reason:string};

/** Resolves only an RFC 6901 pointer inside the response-local native event. */
export function resolveContentReference(record:ProviderNativeEventRecord,reference:ProviderEventContentReference|null):ResolvedContentReference {
  if(!reference)return {available:false,reason:"not projected"};
  if(reference.availability!=="available")return {available:false,reason:reference.unavailable_reason??"provider content unavailable"};
  const pointer=reference.json_pointer;
  if(pointer===null||pointer===undefined)return {available:false,reason:"content pointer missing"};
  if(pointer==="")return {available:true,value:record.native_event};
  if(!pointer.startsWith("/"))return {available:false,reason:"invalid content pointer"};
  let value:unknown=record.native_event;
  for(const encoded of pointer.slice(1).split("/")){
    const token=encoded.replace(/~1/g,"/").replace(/~0/g,"~");
    if(Array.isArray(value)){
      if(!/^(0|[1-9]\d*)$/.test(token))return {available:false,reason:"content pointer not found"};
      value=value[Number(token)];
    }else if(value!==null&&typeof value==="object"&&Object.prototype.hasOwnProperty.call(value,token)){
      value=(value as Record<string,unknown>)[token];
    }else return {available:false,reason:"content pointer not found"};
    if(value===undefined)return {available:false,reason:"content pointer not found"};
  }
  return {available:true,value};
}
