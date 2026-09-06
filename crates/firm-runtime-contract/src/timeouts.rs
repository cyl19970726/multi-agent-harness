/// The three irreducible cycle time bounds (SPEC-TYPED-CYCLE-OUTCOME-01 §3.1).
///
/// One bare `Duration` previously carried all three meanings and each adapter
/// guessed one (#708). These are transport-layer bounds only: none of them is
/// a plan gate, a work-acceptance gate, or a provider-silence verdict.
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
    /// Liveness-proof boundary: the longest interval allowed WITHOUT a
    /// positive proof that the process and transport are alive. It is NOT a
    /// provider-silence cap (frozen decision D2): an adapter that
    /// continuously proves liveness (e.g. an `ensure_alive` probe or a
    /// reader-thread disconnect branch) uses this only as a probe deadline,
    /// and its expiry alone is not a failure — only a failed or impossible
    /// probe fails, and that failure stays fail-closed as "accepted, outcome
    /// unproven" (invariant I2), never "not applied".
    pub transport_liveness: std::time::Duration,
    /// Control-settle boundary: after Interrupt/Close is sent, the longest
    /// wait for its exact settled confirmation. It bounds control only, never
    /// the cycle itself (A5); an expired settle maps to "unproven" (Unknown),
    /// never to success and never to a cycle failure (decision D3).
    pub control_settle: std::time::Duration,
}

impl CycleTimeouts {
    /// Contract defaults (frozen decision 6): a caller that exposes one
    /// timeout flag sets only `input_acceptance` and takes these for the
    /// other two bounds.
    pub const DEFAULT_INPUT_ACCEPTANCE: std::time::Duration = std::time::Duration::from_secs(300);
    pub const DEFAULT_TRANSPORT_LIVENESS: std::time::Duration = std::time::Duration::from_secs(30);
    pub const DEFAULT_CONTROL_SETTLE: std::time::Duration = std::time::Duration::from_secs(15);

    /// Bounds for a pure control path (Interrupt/Close), where only
    /// `control_settle` is operative and the other two are inert.
    pub fn control_path(control_settle: std::time::Duration) -> Self {
        Self {
            control_settle,
            ..Self::default()
        }
    }

    /// The single-flag shape: an explicit `input_acceptance`, contract
    /// defaults for the rest (frozen decision 6).
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
            transport_liveness: Self::DEFAULT_TRANSPORT_LIVENESS,
            control_settle: Self::DEFAULT_CONTROL_SETTLE,
        }
    }
}
