/// The two irreducible cycle time bounds (SPEC-TYPED-CYCLE-OUTCOME-01 §3.1,
/// trimmed from three by ADR 0076's X1b slice).
///
/// One bare `Duration` previously carried every meaning and each adapter
/// guessed one (#708). These are transport-layer bounds only: neither is a
/// plan gate, a work-acceptance gate, or a provider-silence verdict.
///
/// There is deliberately no liveness bound. It was specified as a probe
/// deadline, shipped as a field, and **never read by any adapter**: all five
/// prove liveness structurally instead — a reader thread's `Disconnected`
/// branch, or an `ensure_alive()` probe on every silent poll — which is
/// strictly better than a wall clock, because it cannot mistake a slow turn
/// for a dead one. Carrying an unread `Duration` invited exactly the
/// silence-verdict reading that frozen decision D2 forbids, so the field is
/// gone and the property it named is proven by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleTimeouts {
    /// Delivery boundary: from writing the input to holding the exact
    /// provider acceptance receipt for THIS input. It is valid only BEFORE
    /// the receipt exists — once the input is accepted, wall-clock or
    /// inter-frame silence alone must never turn the cycle into an `Err` or a
    /// provider terminal failure (invariant I1). A cycle whose receipt never
    /// arrives fails here, and that failure maps to "input never accepted"
    /// (replay-safe), because the provider never took the input.
    pub input_acceptance: std::time::Duration,
    /// Control-settle boundary: after Interrupt/Close is sent, the longest
    /// wait for its exact settled confirmation. It bounds control only, never
    /// the cycle itself (A5); an expired settle maps to "unproven" (Unknown),
    /// never to success and never to a cycle failure (decision D3).
    pub control_settle: std::time::Duration,
}

impl CycleTimeouts {
    /// Contract defaults (frozen decision 6): a caller that exposes one
    /// timeout flag sets only `input_acceptance` and takes this for the
    /// other bound.
    pub const DEFAULT_INPUT_ACCEPTANCE: std::time::Duration = std::time::Duration::from_secs(300);
    pub const DEFAULT_CONTROL_SETTLE: std::time::Duration = std::time::Duration::from_secs(15);

    /// Bounds for a pure control path (Interrupt/Close), where only
    /// `control_settle` is operative and the other two are inert.
    pub fn control_path(control_settle: std::time::Duration) -> Self {
        Self {
            control_settle,
            ..Self::default()
        }
    }

    /// The single-flag shape: an explicit `input_acceptance`, the contract
    /// default for the rest (frozen decision 6).
    pub fn with_input_acceptance(input_acceptance: std::time::Duration) -> Self {
        Self {
            input_acceptance,
            ..Self::default()
        }
    }
}

impl Default for CycleTimeouts {
    fn default() -> Self {
        Self {
            input_acceptance: Self::DEFAULT_INPUT_ACCEPTANCE,
            control_settle: Self::DEFAULT_CONTROL_SETTLE,
        }
    }
}
