use super::*;

#[test]
fn workspace_transitions_reobserve_git_links_dirty_state_and_cleanup_safety() {
    let harness = TestStore::new("workspace-real-safety");
    let host = human("host");
    let team_run = seed_team(&harness.store, "workspace-real-safety", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        false,
    );
    let repo = harness.root.join("workspace");
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "trust@example.invalid"],
        vec!["config", "user.name", "Trust Test"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(repo.join("README.md"), "workspace safety\n").unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["add", "."])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["commit", "-qm", "seed"])
        .status()
        .unwrap()
        .success());
    let canonical_root = std::fs::canonicalize(&repo).unwrap();
    let mut binding = workspace_binding("workspace-real", canonical_root.to_str().unwrap(), &host);
    binding.team_run_id = team_run.id;
    binding.member_run_id = "runtime-member-a".into();
    let created = harness
        .store
        .create_trust_workspace_binding(
            &context(
                host.clone(),
                "workspace.provision",
                "workspace-real-create",
                0,
            ),
            binding,
        )
        .expect("create real workspace binding")
        .projection;
    let clean_proof = |version_root: &str| WorkspaceSafetyProof {
        canonical_root: version_root.into(),
        project_binding_id: "project-test".into(),
        git_common_dir: created.git_common_dir.clone(),
        link_escape_free: true,
        repository_matches: true,
        is_dirty: false,
        is_conflicted: false,
        observed_member_generation: 1,
    };
    harness
        .store
        .transition_trust_workspace_binding(
            &context(
                host.clone(),
                "workspace.transition",
                "workspace-preparing",
                1,
            ),
            &created.id,
            WorkspaceLifecycle::Preparing,
            &clean_proof(&created.canonical_root),
            "t2",
        )
        .expect("requested to preparing");
    harness
        .store
        .transition_trust_workspace_binding(
            &context(host.clone(), "workspace.transition", "workspace-ready", 2),
            &created.id,
            WorkspaceLifecycle::Ready,
            &clean_proof(&created.canonical_root),
            "t3",
        )
        .expect("preparing to ready");

    let outside = harness.root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, repo.join("escape-link")).unwrap();
    let before_link = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .transition_trust_workspace_binding(
                    &context(host.clone(), "workspace.attach", "workspace-link", 3),
                    &created.id,
                    WorkspaceLifecycle::Attached,
                    &clean_proof(&created.canonical_root),
                    "t4",
                )
                .expect_err("link escape must fail")
        ),
        TrustErrorCode::WorkspaceLinkEscape
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_link
    );
    #[cfg(unix)]
    std::fs::remove_file(repo.join("escape-link")).unwrap();

    std::fs::write(repo.join("dirty.txt"), "dirty\n").unwrap();
    let before_dirty = harness.store.canonical_operations().unwrap().len();
    assert_eq!(
        trust_code(
            harness
                .store
                .transition_trust_workspace_binding(
                    &context(host.clone(), "workspace.attach", "workspace-dirty-lie", 3),
                    &created.id,
                    WorkspaceLifecycle::Attached,
                    &clean_proof(&created.canonical_root),
                    "t5",
                )
                .expect_err("caller cannot conceal dirty workspace")
        ),
        TrustErrorCode::WorkspaceDirty
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_dirty
    );
    let mut dirty_proof = clean_proof(&created.canonical_root);
    dirty_proof.is_dirty = true;
    let blocked = harness
        .store
        .transition_trust_workspace_binding(
            &context(host, "workspace.transition", "workspace-cleanup-blocked", 3),
            &created.id,
            WorkspaceLifecycle::CleanupBlocked,
            &dirty_proof,
            "t6",
        )
        .expect("dirty workspace becomes cleanup_blocked")
        .projection;
    assert_eq!(blocked.blocked_reason.as_deref(), Some("WORKSPACE_DIRTY"));
    assert!(blocked.dirty_fingerprint.is_some());
}
