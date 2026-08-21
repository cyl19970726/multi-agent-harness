use super::*;

#[test]
fn read_only_leaf_stays_shared_cwd_regardless_of_provider_enforcement() {
    // A read-only leaf (writable=false, no explicit isolation) runs in the
    // shared project cwd. Provider capability does not silently create a git
    // worktree requirement.
    assert!(
        !step_needs_isolation(false, None, None),
        "read-only leaf on an enforcing provider stays in the shared cwd"
    );
    assert!(
        !step_needs_isolation(false, None, None),
        "read-only leaf on a non-enforcing provider also stays in the shared cwd (#190)"
    );
    // Writable / explicit-isolation always isolate.
    assert!(
        step_needs_isolation(true, None, None),
        "writable always isolates"
    );
    assert!(
        step_needs_isolation(false, Some("worktree"), None),
        "explicit isolation always isolates"
    );
    // Sanity: provider enforcement metadata remains honest, but no longer drives
    // cwd isolation (#190). Read-only leaves stay in the shared project root on
    // enforcing (codex) and non-enforcing (kimi) providers alike.
    assert!(provider_enforces_read_only("codex"));
    assert!(!provider_enforces_read_only("kimi"));
    assert!(
        !step_needs_isolation(false, None, None),
        "codex read-only leaf does not need isolation"
    );
    assert!(
        !step_needs_isolation(false, None, None),
        "kimi read-only leaf does not need isolation (#190 — no worktree from a capability gap)"
    );
    assert!(
        !step_needs_isolation(true, None, Some(workflow::WRITE_MODE_DIRECT)),
        "direct write mode writes shared cwd instead of creating a worktree"
    );
}
