import { describe, expect, it } from "vitest";

import { projectProviderTimeline, type ProviderTimelineItem } from "./providerEventTimeline";
import type { ProviderEventFragment, ProviderEventPayload, ProviderNativeEventRecord } from "./roleViews";

let ordinal = 0;

function fragment(payload:ProviderEventPayload, semantic:ProviderEventFragment["semantic_kind"]):ProviderEventFragment {
  return {
    fragment_id: `fragment-${ordinal}-${semantic}`,
    fragment_index: 0,
    semantic_kind: semantic,
    lifecycle_phase: "terminal",
    completeness: "complete",
    content_availability: "available",
    payload,
  };
}

function record(nativeEvent:unknown, fragments:ProviderEventFragment[]):ProviderNativeEventRecord {
  ordinal += 1;
  return {
    schema_version: "agentfirm.provider_native_event_record.v3",
    record_id: `record-${ordinal}`,
    provider: "claude",
    adapter_version: "agentfirm.persisted_provider_event_adapter.v3",
    native_source_ref: "/sessions/claude.jsonl",
    source_generation: "generation-1",
    row_locator: `row-${ordinal}`,
    ordering_key: { kind: "provider_ordinal", value: ordinal },
    agent_member_id: "r5-claude",
    agent_session_id: "agent-session:r5-claude:1",
    agent_session_generation: 1,
    occurred_at: `2026-09-04T10:0${Math.min(ordinal, 9)}:00Z`,
    observed_at: `2026-09-04T10:0${Math.min(ordinal, 9)}:01Z`,
    source_content_fingerprint: `fingerprint-${ordinal}`,
    native_event: nativeEvent,
    fragments: fragments.map((item, index) => ({ ...item, fragment_index: index, fragment_id: `${item.fragment_id}-${ordinal}` })),
  };
}

function envelopeRecord(eventType:string):ProviderNativeEventRecord {
  return record(
    { type: eventType, envelope_detail: `${eventType} bookkeeping` },
    [fragment({ type: "native", event_type: eventType, classification_reason: "unsupported_event_type" }, "unclassified_native")],
  );
}

function assistantRecord(text:string):ProviderNativeEventRecord {
  return record({ type: "assistant", message: { content: [{ type: "text", text }] } }, [fragment({ type: "assistant_response", text }, "assistant_response")]);
}

function toolUseRecord(callId:string, toolName:string):ProviderNativeEventRecord {
  return record(
    { type: "assistant", message: { content: [{ type: "tool_use", id: callId, name: toolName, input: { command: "pnpm test" } }] } },
    [fragment({ type: "tool", tool_name: toolName, call_id: callId, outcome: "requested", arguments: { availability: "available", json_pointer: "/message/content/0/input" } }, "tool_call_requested")],
  );
}

function toolResultRecord(callId:string):ProviderNativeEventRecord {
  return record(
    { type: "user", message: { content: [{ type: "tool_result", tool_use_id: callId, content: "ok" }] } },
    [fragment({ type: "tool", call_id: callId, outcome: "completed", result: { availability: "available", json_pointer: "/message/content/0/content" } }, "tool_call_completed")],
  );
}

function envelopeGroups(timeline:ProviderTimelineItem[]) {
  return timeline.flatMap(item => (item.kind === "envelope_group" ? [item] : []));
}

/** fragment_id -> the timeline row kind that fragment was classified into. */
function fragmentClassification(timeline:ProviderTimelineItem[]) {
  const index = new Map<string,ProviderTimelineItem["kind"]>();
  for (const item of timeline) {
    if (item.kind === "native") index.set(item.fragment.fragment_id, item.kind);
    else for (const occurrence of item.occurrences) index.set(occurrence.fragment.fragment_id, item.kind);
  }
  return index;
}

describe("projectProviderTimeline envelope collapsing", () => {
  it("collapses adjacent envelope frames into one countable group", () => {
    const timeline = projectProviderTimeline([
      envelopeRecord("queue-operation"),
      envelopeRecord("user"),
      envelopeRecord("attachment"),
      envelopeRecord("last-prompt"),
      assistantRecord("Done."),
    ]);

    expect(timeline).toHaveLength(2);
    const [group] = envelopeGroups(timeline);
    expect(group?.occurrences).toHaveLength(4);
    expect(group?.occurrences.map(occurrence => (occurrence.fragment.payload.type === "native" ? occurrence.fragment.payload.event_type : null)))
      .toEqual(["queue-operation", "user", "attachment", "last-prompt"]);
  });

  it("never discards an envelope frame or its original provider record", () => {
    const records = [envelopeRecord("queue-operation"), envelopeRecord("attachment")];
    const [group] = envelopeGroups(projectProviderTimeline(records));

    expect(group?.occurrences.map(occurrence => occurrence.record.native_event)).toEqual(records.map(item => item.native_event));
  });

  it("keeps envelope runs separate when a semantic row falls between them", () => {
    const timeline = projectProviderTimeline([
      envelopeRecord("queue-operation"),
      assistantRecord("Working on it."),
      envelopeRecord("attachment"),
      envelopeRecord("last-prompt"),
    ]);

    expect(timeline.map(item => item.kind)).toEqual(["envelope_group", "native", "envelope_group"]);
    expect(envelopeGroups(timeline).map(group => group.occurrences.length)).toEqual([1, 2]);
  });

  it("leaves tool episodes and assistant rows fully classified", () => {
    const timeline = projectProviderTimeline([
      toolUseRecord("call-1", "Bash"),
      envelopeRecord("queue-operation"),
      toolResultRecord("call-1"),
      assistantRecord("Tests pass."),
    ]);

    expect(timeline.map(item => item.kind)).toEqual(["tool_episode", "envelope_group", "native"]);
    const episode = timeline[0];
    expect(episode.kind === "tool_episode" && episode.tool_name).toBe("Bash");
    expect(episode.kind === "tool_episode" && episode.call_id).toBe("call-1");
    expect(episode.kind === "tool_episode" && episode.outcome).toBe("completed");
    expect(episode.kind === "tool_episode" && Boolean(episode.arguments_ref && episode.result_ref)).toBe(true);
    const assistant = timeline[2];
    expect(assistant.kind === "native" && assistant.fragment.semantic_kind).toBe("assistant_response");
  });

  it("classifies the same records identically in live observation and persisted replay", () => {
    const records = [
      envelopeRecord("queue-operation"),
      toolUseRecord("call-2", "Read"),
      envelopeRecord("attachment"),
      assistantRecord("Read the file."),
    ];
    const replay = fragmentClassification(projectProviderTimeline(records));
    // Live observation appends records one at a time. Every fragment already
    // observed live must carry the same classification as the persisted replay
    // of the whole session, and no fragment may be dropped along the way.
    for (let observed = 1; observed <= records.length; observed += 1) {
      const live = fragmentClassification(projectProviderTimeline(records.slice(0, observed)));
      expect(live.size).toBe(records.slice(0, observed).flatMap(item => item.fragments).length);
      for (const [fragmentId, kind] of live) expect(kind).toBe(replay.get(fragmentId));
    }
  });

  it("does not collapse malformed records, which stay visible integrity evidence", () => {
    const envelope = envelopeRecord("queue-operation");
    const malformed = record({ broken: true }, [fragment({ type: "malformed", reason_code: "unparsable_row" }, "malformed_or_incomplete")]);
    const timeline = projectProviderTimeline([envelope, malformed]);

    expect(timeline.map(item => item.kind)).toEqual(["envelope_group", "native"]);
  });
});
