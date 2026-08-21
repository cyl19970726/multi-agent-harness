use super::*;

    #[test]
    fn provider_admit_command_refuses_source_reviewed_current_tuple_without_writing() {
        let root = std::env::temp_dir().join(format!(
            "harness-provider-admit-current-test-{}",
            generated_id("cli")
        ));
        let project_id = "provider-admit-current-project";
        let store_id = format!("project-store:{project_id}");
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope(project_id, &store_id);
        let resolved = ResolvedStore {
            root: root.clone(),
            source: StoreSource::StoreFlag,
            project_selection_explicit: false,
            context: Some(ProjectContext {
                id: project_id.into(),
                project_root: root.clone(),
                store_root: root.clone(),
                kind: ProjectKind::Repo,
                is_git_repo: true,
            }),
            execution_space_context: None,
        };
        let version = "0.148.0-alpha.9";
        let args = vec![
            "--provider".into(),
            "codex".into(),
            "--execution-mode".into(),
            "codex_app_server".into(),
            "--provider-version".into(),
            version.into(),
            "--adapter-contract-version".into(),
            "codex-app-server-v1".into(),
            "--evidence".into(),
            "evidence:unneeded".into(),
        ];
        let error =
            provider_admit_command_with_probe(
                &store,
                &resolved,
                &args,
                |_| Ok(version.to_string()),
            )
            .expect_err("source-reviewed current tuple needs no operational admission");
        assert!(error
            .to_string()
            .contains("only an actually observed review-required"));
        assert!(store
            .provider_compatibility_admissions()
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

