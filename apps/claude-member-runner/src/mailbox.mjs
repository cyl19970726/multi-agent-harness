/**
 * Persistent member mailbox.
 *
 * This is the whole point of the runner, so it is worth stating plainly.
 *
 * The current Claude and Codex team-member loops in `harness-cli` are shaped
 * like this:
 *
 *     let queued = ledger.queued_messages_for(member)?;
 *     if queued.is_empty() { break; }          // <- member terminates here
 *
 * A member therefore stops existing at the instant its queue is momentarily
 * empty. A TeamMessage that arrives one millisecond later has no recipient:
 * it stays `queued` forever and nobody ever consumes it. That is the runtime
 * half of ADR 0037's unmet clause — a Member owns its active Work, Workspace,
 * and native session until the Team Lead explicitly closes it — and it is why
 * acceptance items 5 and 6 originally had no test.
 *
 * A Mailbox is an async message stream that does NOT end when it is empty.
 * `next()` parks until either `push()` delivers a message or `close()` is
 * called explicitly. Only the Host closes it, and only on a terminal
 * acceptance decision. Emptiness is not a terminal condition.
 */
export class Mailbox {
  #queue = [];
  #waiters = [];
  #closed = false;
  #closeReason = null;
  // Bumped when the current consumer is retired without ending the member.
  // `interrupt()` kills the SDK query, not the member, so the runner opens a
  // fresh query on the same native session; the old `stream()` generator has
  // to stop reading first or two consumers race for the same messages.
  #generation = 0;

  /** Number of messages waiting for the member's next round. */
  get pending() {
    return this.#queue.length;
  }

  get closed() {
    return this.#closed;
  }

  get closeReason() {
    return this.#closeReason;
  }

  /**
   * Hand a message to the member. If the member is parked waiting, it wakes
   * immediately; otherwise the message queues for the next round.
   *
   * @param {object} message a Work or TeamMessage delivery envelope
   */
  push(message) {
    if (this.#closed) {
      throw new Error(
        `mailbox is closed (${this.#closeReason}); cannot deliver ${message?.id ?? "message"}`,
      );
    }
    const waiter = this.#waiters.shift();
    if (waiter) {
      waiter({ value: message, done: false });
      return;
    }
    this.#queue.push(message);
  }

  /**
   * End the member's lifetime. This is a Host decision (an ordinary acceptance
   * message followed by close, an explicit stop, or run teardown) — never an
   * inference from an empty queue.
   *
   * Messages already queued are preserved and reported by `drain()` so the
   * caller can requeue them rather than lose them, which is the same contract
   * the SDK's `interrupt()` provides via `still_queued`.
   */
  close(reason = "closed_by_host") {
    if (this.#closed) return;
    this.#closed = true;
    this.#closeReason = reason;
    while (this.#waiters.length > 0) {
      this.#waiters.shift()({ value: undefined, done: true });
    }
  }

  /**
   * Retire the current `stream()` consumer **without** ending the member.
   * Queued messages are preserved for the next consumer.
   */
  supersede() {
    this.#generation += 1;
    while (this.#waiters.length > 0) {
      this.#waiters.shift()({ value: undefined, done: true });
    }
  }

  /** Remove and return everything still queued. Used on interrupt/close. */
  drain() {
    const rest = this.#queue;
    this.#queue = [];
    return rest;
  }

  /**
   * The stream handed to `query({ prompt })`. Yields `SDKUserMessage`-shaped
   * values produced by `render`.
   *
   * @param {(message: object) => object} render TeamMessage -> SDKUserMessage
   */
  async *stream(render) {
    const generation = this.#generation;
    while (true) {
      if (this.#generation !== generation) return; // superseded; do not consume
      if (this.#queue.length > 0) {
        yield render(this.#queue.shift());
        continue;
      }
      if (this.#closed) return;
      const next = await new Promise((resolve) => this.#waiters.push(resolve));
      if (next.done) return;
      yield render(next.value);
    }
  }
}

/**
 * Render a Harness Work or TeamMessage input as an SDK user message.
 *
 * Kept deliberately dumb: the envelope stays readable in the provider-native
 * transcript so a human opening the session in Claude sees who sent what and,
 * for conversation, under which correlation. Harness still owns Work state;
 * this runner does not copy or advance that state machine.
 */
export function renderTeamMessage(message) {
  const header = `--- ${message.from_member_id} (${message.kind}) ---`;
  const lineage = message.correlation_id
    ? `correlation: ${message.correlation_id}`
    : "correlation: (none)";
  // Shape is `SDKUserMessage` from the SDK's own type declarations:
  //   { type: 'user', message: MessageParam, parent_tool_use_id: string | null }
  // Note the nested `message`. The published TypeScript reference's
  // "Streaming Input Mode" example omits it and yields `{type, content}`
  // directly; the runtime rejects that with
  //   Error: Expected message role 'user', got 'undefined'
  // and the SDK process exits code 1. Follow the .d.ts, not that example.
  return {
    type: "user",
    message: {
      role: "user",
      content: [
        { type: "text", text: `${header}\n${lineage}\n\n${message.body}` },
      ],
    },
    parent_tool_use_id: null,
  };
}
