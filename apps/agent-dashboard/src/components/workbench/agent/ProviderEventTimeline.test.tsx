import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { describe, expect, it, vi } from "vitest";

import { projectProviderTimeline } from "../../../model/providerEventTimeline";
import type { ProviderEventFragment, ProviderEventPayload, ProviderNativeEventRecord } from "../../../model/roleViews";
import { EnvelopeGroupRow, ToolEpisodeRow } from "./ProviderEventTimeline";

let ordinal = 0;

function record(nativeEvent:unknown, payload:ProviderEventPayload, semantic:ProviderEventFragment["semantic_kind"]):ProviderNativeEventRecord {
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
    occurred_at: "2026-09-04T10:00:00Z",
    observed_at: "2026-09-04T10:00:01Z",
    source_content_fingerprint: `fingerprint-${ordinal}`,
    native_event: nativeEvent,
    fragments: [{
      fragment_id: `fragment-${ordinal}`,
      fragment_index: 0,
      semantic_kind: semantic,
      lifecycle_phase: "terminal",
      completeness: "complete",
      content_availability: "available",
      payload,
    }],
  };
}

function envelopeRecord(eventType:string, marker:string) {
  return record(
    { type: eventType, marker },
    { type: "native", event_type: eventType, classification_reason: "unsupported_event_type" },
    "unclassified_native",
  );
}

function envelopeGroup(records:ProviderNativeEventRecord[]) {
  const [group] = projectProviderTimeline(records).flatMap(item => (item.kind === "envelope_group" ? [item] : []));
  if (!group) throw new Error("expected an envelope group");
  return group;
}

function toolEpisode() {
  const [episode] = projectProviderTimeline([
    record(
      { type: "assistant", message: { content: [{ type: "tool_use", id: "call-1", name: "Bash", input: { command: "pnpm test:dashboard" } }] } },
      { type: "tool", tool_name: "Bash", call_id: "call-1", primary_target: "pnpm test:dashboard", outcome: "requested", arguments: { availability: "available", json_pointer: "/message/content/0/input" } },
      "tool_call_requested",
    ),
    record(
      { type: "user", message: { content: [{ type: "tool_result", tool_use_id: "call-1", content: "13 tests passed" }] } },
      { type: "tool", call_id: "call-1", outcome: "completed", result: { availability: "available", json_pointer: "/message/content/0/content" } },
      "tool_call_completed",
    ),
  ]).flatMap(item => (item.kind === "tool_episode" ? [item] : []));
  if (!episode) throw new Error("expected a tool episode");
  return episode;
}

function textOf(renderer:ReactTestRenderer) {
  return JSON.stringify(renderer.toJSON());
}

function buttons(renderer:ReactTestRenderer) {
  return renderer.root.findAll(node => node.type === "button");
}

describe("EnvelopeGroupRow", () => {
  it("renders one countable collapsed row instead of one row per envelope frame", () => {
    const group = envelopeGroup([
      envelopeRecord("queue-operation", "marker-queue"),
      envelopeRecord("user", "marker-user"),
      envelopeRecord("attachment", "marker-attachment"),
    ]);
    let renderer!:ReactTestRenderer;
    act(() => { renderer = create(<EnvelopeGroupRow group={group}/>); });

    const rendered = textOf(renderer);
    expect(rendered).toContain("3 envelope frames");
    expect(rendered).toContain("queue-operation");
    expect(renderer.root.findAll(node => node.type === "pre")).toHaveLength(0);
    expect(rendered).not.toContain("marker-queue");
    expect(buttons(renderer)[0]?.props["aria-expanded"]).toBe(false);
  });

  it("reveals every original provider record when expanded", () => {
    const group = envelopeGroup([
      envelopeRecord("queue-operation", "marker-queue"),
      envelopeRecord("last-prompt", "marker-last-prompt"),
    ]);
    let renderer!:ReactTestRenderer;
    act(() => { renderer = create(<EnvelopeGroupRow group={group}/>); });
    act(() => { buttons(renderer)[0]!.props.onClick(); });

    const rendered = textOf(renderer);
    expect(renderer.root.findAll(node => node.type === "pre")).toHaveLength(2);
    expect(rendered).toContain("marker-queue");
    expect(rendered).toContain("marker-last-prompt");
    expect(buttons(renderer)[0]?.props["aria-expanded"]).toBe(true);
  });

  it("hands an expanded frame to the context rail as its exact record and fragment", () => {
    const group = envelopeGroup([envelopeRecord("attachment", "marker-attachment")]);
    const onSelectFrame = vi.fn();
    let renderer!:ReactTestRenderer;
    act(() => { renderer = create(<EnvelopeGroupRow group={group} onSelectFrame={onSelectFrame}/>); });
    act(() => { buttons(renderer)[0]!.props.onClick(); });
    act(() => { buttons(renderer)[1]!.props.onClick(); });

    expect(onSelectFrame).toHaveBeenCalledWith(group.occurrences[0]);
  });

  it("labels a single frame in the singular", () => {
    const group = envelopeGroup([envelopeRecord("user", "marker-user")]);
    let renderer!:ReactTestRenderer;
    act(() => { renderer = create(<EnvelopeGroupRow group={group}/>); });

    expect(textOf(renderer)).toContain("1 envelope frame");
  });
});

describe("ToolEpisodeRow", () => {
  it("keeps a tool_use/tool_result episode fully visible and unaffected by envelope collapsing", () => {
    const episode = toolEpisode();
    let renderer!:ReactTestRenderer;
    act(() => { renderer = create(<ToolEpisodeRow episode={episode} expanded selected={false} timestamp="10:00" onToggle={() => {}}/>); });

    const rendered = textOf(renderer);
    expect(rendered).toContain("Bash");
    expect(rendered).toContain("call-1");
    expect(rendered).toContain("pnpm test:dashboard");
    expect(rendered).toContain("13 tests passed");
    expect(rendered).not.toContain("envelope frame");
  });
});
