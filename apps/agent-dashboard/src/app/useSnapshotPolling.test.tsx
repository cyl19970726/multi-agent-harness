import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  degradedSnapshotPollIntervalMs,
  healthySnapshotPollIntervalMs,
  useSnapshotPolling,
  type SnapshotPollingOptions,
} from "./useSnapshotPolling";

function PollingHarness(options: SnapshotPollingOptions) {
  useSnapshotPolling(options);
  return null;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("useSnapshotPolling", () => {
  it("does not use the sub-second or degraded cadence while SSE is healthy", () => {
    vi.useFakeTimers();
    const onPoll = vi.fn();
    let renderer!: ReactTestRenderer;
    act(() => {
      renderer = create(
        <PollingHarness isLive pollEnabled streamMode="sse" onPoll={onPoll} />,
      );
    });

    act(() => {
      vi.advanceTimersByTime(degradedSnapshotPollIntervalMs);
    });
    expect(onPoll).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(
        healthySnapshotPollIntervalMs - degradedSnapshotPollIntervalMs,
      );
    });
    expect(onPoll).toHaveBeenCalledTimes(1);
    act(() => renderer.unmount());
  });

  it("resumes the five-second fallback when the stream disconnects", () => {
    vi.useFakeTimers();
    const onPoll = vi.fn();
    let renderer!: ReactTestRenderer;
    act(() => {
      renderer = create(
        <PollingHarness isLive pollEnabled={false} streamMode="sse" onPoll={onPoll} />,
      );
    });

    act(() => {
      vi.advanceTimersByTime(degradedSnapshotPollIntervalMs);
    });
    expect(onPoll).not.toHaveBeenCalled();
    act(() => {
      renderer.update(
        <PollingHarness
          isLive
          pollEnabled={false}
          streamMode="reconnecting"
          onPoll={onPoll}
        />,
      );
    });
    act(() => {
      vi.advanceTimersByTime(degradedSnapshotPollIntervalMs);
    });
    expect(onPoll).toHaveBeenCalledTimes(1);
    act(() => renderer.unmount());
  });
});
