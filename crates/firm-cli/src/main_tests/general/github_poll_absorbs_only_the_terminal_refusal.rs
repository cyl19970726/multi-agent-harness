use super::*;

/// A GitHub evidence poll reads the Work list once and writes per Work, so the
/// Host can close a Work in between. Terminal Work is immutable, so that one
/// refusal is a per-Work skip; every other refusal means the pass itself is
/// wrong and must stay fatal.
///
/// The terminal code is asserted through `harness_store`'s exported constant,
/// which is the same value the Store's writers format into their refusals
/// (pinned on the Store side by
/// `closed_work_refuses_every_host_mutation_with_one_terminal_code`), so the
/// classifier cannot drift from the text it classifies.
#[test]
fn github_poll_absorbs_only_the_terminal_refusal() {
    let terminal = format!(
        "conflict: {}: closed Work work-1 is immutable; external GitHub evidence \
         cannot advance a closed Work revision",
        harness_store::WORK_TERMINAL_IMMUTABLE
    );
    assert!(
        github_poll_refusal_is_terminal_skip(&terminal),
        "a Work closed between the read and the write is skipped, not fatal"
    );

    for fatal in [
        "conflict: VERSION_CONFLICT: work work-1 is version 3, expected 2",
        "conflict: WORK_GITHUB_EVIDENCE_NODE_FENCED: Work work-1 TeamRun is placed on node-b, not node-a",
        "conflict: GENERATION_FENCED: daemon generation 2 is not current",
        "conflict: WORK_GITHUB_EVIDENCE_HOST_SOURCE_REQUIRED: exact TeamRun Host source required",
        "conflict: work not found: work-1",
    ] {
        assert!(
            !github_poll_refusal_is_terminal_skip(fatal),
            "the poll must not absorb `{fatal}`"
        );
    }
}
