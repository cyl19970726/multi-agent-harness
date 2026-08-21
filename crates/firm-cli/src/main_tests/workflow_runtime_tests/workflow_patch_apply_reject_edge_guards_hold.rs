use super::*;

#[test]
fn workflow_patch_apply_reject_edge_guards_hold() {
    let store = temp_store("patch-edges");
    let project_root = init_gc_git_project("patch-edges", &store);
    std::fs::create_dir_all(project_root.join("src")).expect("mk src");
    let patch_dir = store.root().join("workflow-patches").join("wfrun-edges");
    std::fs::create_dir_all(&patch_dir).expect("mk patch dir");
    let new_file_diff = |path: &str, content: &str| {
        format!(
                "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{content}\n"
            )
    };
    let make_patch = |label: &str,
                      diff: String,
                      changed_paths: Vec<&str>,
                      owned_paths: Vec<&str>|
     -> WorkflowPatch {
        let patch_ref = patch_dir.join(format!("{label}.patch"));
        std::fs::write(&patch_ref, diff).expect("write patch");
        let patch = WorkflowPatch {
            id: format!("wfpatch-{label}"),
            run_id: "wfrun-edges".into(),
            step_id: format!("wfstep-{label}"),
            label: label.into(),
            phase: "develop".into(),
            provider: "codex".into(),
            status: WorkflowPatchStatus::PendingApply,
            changed_paths: changed_paths.into_iter().map(str::to_string).collect(),
            patch_ref: patch_ref.display().to_string(),
            base_sha: None,
            owned_paths: owned_paths.into_iter().map(str::to_string).collect(),
            persist_changes: Some("patch".into()),
            created_at: now_string(),
            updated_at: None,
            actor: None,
            reason: None,
            conflict_detail: None,
            applied_at: None,
            rejected_at: None,
        };
        store.append_workflow_patch(&patch).expect("append patch");
        patch
    };
    let latest_status = |id: &str| {
        latest_workflow_patches_in_append_order(&store)
            .expect("read patches")
            .into_iter()
            .find(|patch| patch.id == id)
            .expect("patch exists")
            .status
    };

    let outside = make_patch(
        "outside",
        new_file_diff("docs/outside.txt", "outside"),
        vec!["docs/outside.txt"],
        vec!["src"],
    );
    let err = apply_workflow_patch_record(&store, None, &outside, Some("test".into()), None, false)
        .expect_err("owned path violation must fail");
    assert!(err.to_string().contains("outside owned_paths"));
    assert_eq!(
        latest_status(&outside.id),
        WorkflowPatchStatus::Conflict,
        "owned-path violations become conflict rows"
    );

    let rejected_first = make_patch(
        "reject-first",
        new_file_diff("src/reject-first.txt", "reject first"),
        vec!["src/reject-first.txt"],
        vec!["src"],
    );
    let rejected = reject_workflow_patch_record(
        &store,
        &rejected_first,
        Some("test".into()),
        Some("no".into()),
    )
    .expect("reject pending patch");
    assert_eq!(rejected.status, WorkflowPatchStatus::Rejected);
    assert!(
        apply_workflow_patch_record(&store, None, &rejected, Some("test".into()), None, false,)
            .is_err(),
        "rejected patches cannot be applied later"
    );

    // D6: an UNRELATED untracked file no longer blocks a patch that touches
    // disjoint paths — the dirty guard is scoped to the patch's own paths.
    let dirty = make_patch(
        "dirty",
        new_file_diff("src/dirty.txt", "dirty"),
        vec!["src/dirty.txt"],
        vec!["src"],
    );
    std::fs::write(project_root.join("untracked.tmp"), "unrelated").expect("dirty file");
    let applied_dirty =
        apply_workflow_patch_record(&store, None, &dirty, Some("test".into()), None, false)
            .expect("unrelated dirt must not block a disjoint patch (D6)");
    assert_eq!(applied_dirty.status, WorkflowPatchStatus::Applied);
    assert_eq!(
        std::fs::read_to_string(project_root.join("src/dirty.txt")).expect("applied file"),
        "dirty\n"
    );
    std::fs::remove_file(project_root.join("untracked.tmp")).expect("clean dirty file");
    // Remove just the untracked file this patch created; keep the src/ dir.
    std::fs::remove_file(project_root.join("src/dirty.txt")).expect("clean applied file");

    // D6: but a patch whose OWN target path is locally modified DOES block.
    std::fs::create_dir_all(project_root.join("src")).expect("mk src");
    std::fs::write(project_root.join("src/collide.txt"), "committed\n")
        .expect("write collide seed");
    git_in(&project_root, &["add", "-A"]).expect("add collide");
    git_in(&project_root, &["commit", "-m", "collide seed"]).expect("commit collide");
    std::fs::write(project_root.join("src/collide.txt"), "locally modified\n")
        .expect("modify collide target");
    let target_dirty = make_patch(
        "target-dirty",
        "diff --git a/src/collide.txt b/src/collide.txt\n\
             index 1111111..2222222 100644\n\
             --- a/src/collide.txt\n\
             +++ b/src/collide.txt\n\
             @@ -1 +1 @@\n\
             -committed\n\
             +from patch\n"
            .to_string(),
        vec!["src/collide.txt"],
        vec!["src"],
    );
    let err = apply_workflow_patch_record(
        &store,
        None,
        &target_dirty,
        Some("test".into()),
        None,
        false,
    )
    .expect_err("a modified target path must block without --allow-dirty (D6)");
    assert!(
        err.to_string().contains("uncommitted changes"),
        "scoped dirty guard names the colliding path: {err}"
    );
    assert_eq!(
        latest_status(&target_dirty.id),
        WorkflowPatchStatus::PendingApply,
        "dirty-target refusal leaves the patch pending for a later apply"
    );
    git_in(&project_root, &["checkout", "--", "src/collide.txt"]).expect("restore collide");

    std::fs::write(project_root.join("src/existing.txt"), "existing\n").expect("write existing");
    git_in(&project_root, &["add", "-A"]).expect("add existing");
    git_in(&project_root, &["commit", "-m", "existing"]).expect("commit existing");
    let conflict = make_patch(
        "conflict",
        new_file_diff("src/existing.txt", "conflict"),
        vec!["src/existing.txt"],
        vec!["src"],
    );
    assert!(
        apply_workflow_patch_record(&store, None, &conflict, Some("test".into()), None, false,)
            .is_err(),
        "git apply --check conflicts must fail"
    );
    assert_eq!(latest_status(&conflict.id), WorkflowPatchStatus::Conflict);

    let good = make_patch(
        "good",
        new_file_diff("src/good.txt", "good"),
        vec!["src/good.txt"],
        vec!["src"],
    );
    let applied =
        apply_workflow_patch_record(&store, None, &good, Some("test".into()), None, false)
            .expect("apply clean patch");
    assert_eq!(applied.status, WorkflowPatchStatus::Applied);
    assert!(
        reject_workflow_patch_record(&store, &applied, Some("test".into()), None).is_err(),
        "applied patches cannot be rewritten to rejected"
    );

    let _ = std::fs::remove_dir_all(&project_root);
    let _ = std::fs::remove_dir_all(store.root());
}
