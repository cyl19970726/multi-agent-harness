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

/// Closing a Work advances its revision, and the Store checks the expected
/// version before mutability, so the refusal an ordinary Host accept or cancel
/// produces mid-pass is VERSION_CONFLICT -- not the terminal code. Absorbing
/// only the terminal code never absorbed that race at all
/// (`github_poll_skips_a_work_the_host_closed_mid_pass` drives it end to end).
/// The skip is keyed on both the refusal and the Work's settled state, so it
/// stays closed on a concurrent write to a Work that is still open.
#[test]
fn github_poll_absorbs_a_settled_work_whatever_refusal_the_close_produced() {
    let version_conflict =
        "conflict: VERSION_CONFLICT: work work-1 is version 3, expected 2".to_string();
    let terminal = format!(
        "conflict: {}: closed Work work-1 is immutable",
        harness_store::WORK_TERMINAL_IMMUTABLE
    );

    // The Host closed it: both refusals mean the same settled Work.
    assert!(github_poll_refusal_is_settled_work(&version_conflict, true));
    assert!(github_poll_refusal_is_settled_work(&terminal, true));
    // The terminal code alone is enough, even before the re-read sees it.
    assert!(github_poll_refusal_is_settled_work(&terminal, false));

    // Still open: a concurrent writer is a real conflict and stays fatal.
    assert!(!github_poll_refusal_is_settled_work(
        &version_conflict,
        false
    ));

    // A settled Work never launders a pass-level refusal.
    for fatal in [
        "conflict: WORK_GITHUB_EVIDENCE_NODE_FENCED: Work work-1 TeamRun is placed on node-b",
        "conflict: GENERATION_FENCED: daemon generation 2 is not current",
        "conflict: WORK_GITHUB_EVIDENCE_HOST_SOURCE_REQUIRED: exact TeamRun Host source required",
        "conflict: work not found: work-1",
    ] {
        assert!(
            !github_poll_refusal_is_settled_work(fatal, true),
            "the poll must not absorb `{fatal}` just because the Work is closed"
        );
    }
}

/// The settled-Work skip is keyed on the Store's own exported version-fence
/// codes, not on a bare `VERSION_CONFLICT` substring. The bare substring also
/// matched any unrelated code ending in those characters, so a refusal with
/// nothing to do with this Work's revision could have been absorbed the moment
/// the Work happened to be closed.
#[test]
fn github_poll_version_fence_is_named_by_the_stores_own_codes() {
    // Both live spellings are covered: the trust-kernel writers and the legacy
    // ledger writers word the same fence differently.
    for fence in [
        "conflict: WORK_VERSION_CONFLICT: Work work-1 is version 3, expected 2",
        "conflict: VERSION_CONFLICT: work work-1 is version 3, expected 2",
        "conflict: VERSION_CONFLICT: work work-1 is at version 3, expected 2",
    ] {
        assert!(
            github_poll_refusal_is_settled_work(fence, true),
            "`{fence}` is a Work version fence"
        );
        assert!(
            !github_poll_refusal_is_settled_work(fence, false),
            "`{fence}` on an open Work is a real concurrent writer"
        );
    }

    // Codes that merely end in VERSION_CONFLICT are not this Work's fence, and
    // a closed Work never launders them.
    for other in [
        "conflict: DELEGATION_VERSION_CONFLICT: delegation-1 changed concurrently",
        "conflict: TEAM_VERSION_CONFLICT: Team team-1 is revision 4, expected 3",
        "conflict: WORK_GITHUB_EVIDENCE_NODE_FENCED: Work work-1 TeamRun is placed on node-b",
    ] {
        assert!(
            !github_poll_refusal_is_settled_work(other, true),
            "`{other}` must not be absorbed as this Work's version fence"
        );
    }
}

/// A pass that only skipped Work the Host closed mid-pass is not a no-op, so it
/// writes a TeamRun event. That event has to say what happened: reporting only
/// refreshed links made it read "0 link(s) refreshed" and name nothing.
#[test]
fn github_poll_event_detail_names_every_observation_it_reports() {
    let mut summary = GithubPollSummary {
        terminal_skipped: vec!["work-closed-1".into(), "work-closed-2".into()],
        ..Default::default()
    };
    assert!(
        !summary.is_noop(),
        "a skip-only pass still writes its event"
    );
    let detail = github_poll_event_detail(&summary);
    assert!(
        detail.contains("skipped 2 closed mid-pass: work-closed-1, work-closed-2"),
        "skip-only detail must name the Work: {detail}"
    );

    summary.links_refreshed = 3;
    summary.blocked_on_failure = vec!["work-red-1".into()];
    let detail = github_poll_event_detail(&summary);
    assert!(
        detail.starts_with("github linkage poll: 3 link(s) refreshed"),
        "{detail}"
    );
    assert!(detail.contains("held 1 on red CI: work-red-1"), "{detail}");
    assert!(detail.contains("skipped 2 closed mid-pass"), "{detail}");

    // Nothing observed: the line stays exactly what it always was.
    assert_eq!(
        github_poll_event_detail(&GithubPollSummary::default()),
        "github linkage poll: 0 link(s) refreshed"
    );
}
