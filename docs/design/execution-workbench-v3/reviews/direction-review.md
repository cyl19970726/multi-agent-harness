# V3 direction review

Status: **approved by user on 2026-07-21**

## What materially changed

- Mission progress is one continuous execution rail instead of several large bordered cards.
- The active Wave expands in place, preserving the Mission-to-Wave mental model.
- Agent Team presence is one connected live rail instead of a row of isolated member cards.
- Collaboration history becomes a semantic event spine; multi-tool work is summarized and expandable.
- Pressure is anchored to the exact Gate, member, and activity event that needs intervention.
- Context remains available in the right rail, but is secondary to the execution surface.

## Why it should feel better

The page now establishes focus through continuity, type scale, whitespace, and one active color trace. Borders no longer carry the entire hierarchy. Agent portraits and restrained live motion make execution feel inhabited without turning the product into an entertainment surface.

## Risks to resolve in implementation

- The generated right rail still uses more framing than the final CSS should; use dividers and background planes first, borders second.
- Long real objectives, member names, and artifacts must be tested; generated text density is only representative.
- The active trace must not imply exact percentage completion unless the store has that fact.
- Team presence animation indicates liveness, not provider thinking or unrecorded progress.
- The decision connector must degrade to an inline relation on tablet/mobile rather than becoming a fragile absolute-position line.

## Approved decisions

1. The continuous Mission execution rail is the V3 primary hierarchy.
2. Agent Team uses a connected presence rail and event spine.
3. Coral is reserved for operator actions/pressure, blue for active execution, green for accepted, and amber for waiting.
4. Implementation uses existing dependencies and CSS motion first; no new motion library was needed for the P0 surfaces.
