use super::*;

    #[test]
    fn apply_enforces_owned_paths_via_numstat_not_headers() {
        let store = temp_store("numstat-guard");
        let project_root = init_gc_git_project("numstat-guard", &store);
        std::fs::create_dir_all(project_root.join("src")).unwrap();
        let patch_dir = store.root().join("workflow-patches").join("wfrun-ns");
        std::fs::create_dir_all(&patch_dir).unwrap();
        // The `diff --git` header lies (`src/ok.txt`), but the `+++` / hunk target
        // is `docs/evil.txt` — git apply --numstat resolves to docs/evil.txt.
        let crafted = "diff --git a/src/ok.txt b/src/ok.txt\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/docs/evil.txt\n@@ -0,0 +1 @@\n+evil\n";
        let patch_ref = patch_dir.join("crafted.patch");
        std::fs::write(&patch_ref, crafted).unwrap();
        let patch = WorkflowPatch {
            id: "wfpatch-crafted".into(),
            run_id: "wfrun-ns".into(),
            step_id: "wfstep-crafted".into(),
            label: "crafted".into(),
            phase: "p".into(),
            provider: "codex".into(),
            status: WorkflowPatchStatus::PendingApply,
            // The recorded (header-derived) changed_paths claim only src/ok.txt.
            changed_paths: vec!["src/ok.txt".into()],
            patch_ref: patch_ref.display().to_string(),
            base_sha: None,
            owned_paths: vec!["src".into()],
            persist_changes: Some("patch".into()),
            created_at: now_string(),
            updated_at: None,
            actor: None,
            reason: None,
            conflict_detail: None,
            applied_at: None,
            rejected_at: None,
        };
        store.append_workflow_patch(&patch).unwrap();
        let err =
            apply_workflow_patch_record(&store, None, &patch, Some("test".into()), None, false)
                .expect_err("crafted header must not slip past owned_paths");
        // numstat sees docs/evil.txt which is neither in changed_paths nor owned —
        // caught as an undisclosed-path mismatch (fail closed) OR an owned violation.
        assert!(
            err.to_string().contains("numstat") || err.to_string().contains("outside owned_paths"),
            "crafted-header write is caught: {err}"
        );
        assert!(
            !project_root.join("docs/evil.txt").exists(),
            "the out-of-bounds write never touches the tree"
        );

        let _ = std::fs::remove_dir_all(&project_root);
        let _ = std::fs::remove_dir_all(store.root());
    }

