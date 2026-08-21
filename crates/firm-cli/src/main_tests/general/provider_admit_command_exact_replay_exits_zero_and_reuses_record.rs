use super::*;

    #[test]
    fn provider_admit_command_exact_replay_exits_zero_and_reuses_record() {
        let root = std::env::temp_dir().join(format!(
            "harness-provider-admit-command-replay-{}",
            generated_id("cli")
        ));
        let project_id = "provider-admit-replay-project";
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
        let args = |evidence: &[&str]| {
            let mut args = vec![
                "--provider".into(),
                "codex".into(),
                "--execution-mode".into(),
                "codex_app_server".into(),
                "--provider-version".into(),
                "9.9.9".into(),
                "--adapter-contract-version".into(),
                "codex-app-server-v1".into(),
                "--actor".into(),
                "operator:test".into(),
                "--json".into(),
            ];
            for evidence_ref in evidence {
                args.push("--evidence".into());
                args.push((*evidence_ref).into());
            }
            args
        };
        let probe = |_: &str| Ok("9.9.9".to_string());
        provider_admit_command_with_probe(
            &store,
            &resolved,
            &args(&["evidence:b", "evidence:a", "evidence:b"]),
            probe,
        )
        .expect("first command exits zero");
        provider_admit_command_with_probe(
            &store,
            &resolved,
            &args(&["evidence:a", "evidence:b"]),
            probe,
        )
        .expect("replayed command exits zero");

        let rows = store.provider_compatibility_admissions().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].evidence_refs, ["evidence:a", "evidence:b"]);
        let _ = std::fs::remove_dir_all(root);
    }

