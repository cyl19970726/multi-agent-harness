use super::*;

#[test]
fn workspace_binding_rejects_relative_and_parent_traversal_without_side_effects() {
    let harness = TestStore::new("workspace-path");
    let host = human("host");
    for (id, root) in [
        ("relative", "project/worktree"),
        ("parent", "/tmp/project/../escape"),
    ] {
        let before = harness.store.canonical_operations().unwrap().len();
        assert_eq!(
            trust_code(
                harness
                    .store
                    .create_trust_workspace_binding(
                        &context(host.clone(), "workspace.bind", id, 0),
                        workspace_binding(id, root, &host),
                    )
                    .expect_err("unsafe path must fail")
            ),
            TrustErrorCode::WorkspacePathUnsafe
        );
        assert_eq!(harness.store.canonical_operations().unwrap().len(), before);
    }
}
