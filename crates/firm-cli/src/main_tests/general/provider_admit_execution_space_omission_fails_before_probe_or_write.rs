use super::*;

    #[test]
    fn provider_admit_execution_space_omission_fails_before_probe_or_write() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let root = std::env::temp_dir().join(format!(
            "harness-provider-admit-explicit-project-test-{}",
            generated_id("cli")
        ));
        let project_id = "ambient-project";
        let store_id = "execution-space:space-a";
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope(project_id, store_id);
        let resolved = ResolvedStore {
            root: root.clone(),
            source: StoreSource::SpaceCurrent,
            project_selection_explicit: false,
            context: Some(ProjectContext {
                id: project_id.into(),
                project_root: root.clone(),
                store_root: root.clone(),
                kind: ProjectKind::Repo,
                is_git_repo: true,
            }),
            execution_space_context: Some(ExecutionSpace {
                id: "space-a".into(),
                name: "Space A".into(),
                store_root: root.clone(),
                default_project_binding_id: Some(project_id.into()),
                company_id: None,
            }),
        };
        let args = vec![
            "--provider".into(),
            "codex".into(),
            "--execution-mode".into(),
            "codex_app_server".into(),
            "--provider-version".into(),
            "9.9.9".into(),
            "--adapter-contract-version".into(),
            "codex-app-server-v1".into(),
            "--evidence".into(),
            "evidence:explicit-project".into(),
        ];
        let probed = AtomicBool::new(false);
        let error = provider_admit_command_with_probe(&store, &resolved, &args, |_| {
            probed.store(true, Ordering::SeqCst);
            Ok("9.9.9".to_string())
        })
        .expect_err("ambient Project Binding must not authorize a space admission");
        assert!(error.to_string().contains("explicit global `--project"));
        assert!(!probed.load(Ordering::SeqCst));
        assert!(store
            .provider_compatibility_admissions()
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

