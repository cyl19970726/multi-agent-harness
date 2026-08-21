    use super::*;

    #[test]
    fn test_sse_manager_broadcast_to_subscriber() {
        let manager = sse::SseManager::new();
        let rx = manager.subscribe("_test");

        let event = sse::SseEventFrame::Snapshot {
            messages: Vec::new(),
            generated_at: "2025-01-01T00:00:00Z".into(),
        };

        manager.broadcast("_test", event.clone());

        // Verify the event is received
        match rx.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(received) => {
                if let sse::SseEventFrame::Snapshot { generated_at, .. } = received {
                    assert_eq!(generated_at, "2025-01-01T00:00:00Z");
                } else {
                    panic!("Expected snapshot marker");
                }
            }
            Err(_) => panic!("Did not receive event in time"),
        }
    }

    #[test]
    fn member_spawn_cwd_keeps_execution_root_separate_from_store_root() {
        let project_root = std::env::temp_dir().join(generated_id("team-project"));
        let store_root = std::env::temp_dir().join(generated_id("team-store"));
        let execution_root = std::env::temp_dir().join(generated_id("team-execution"));
        let worktree_root = std::env::temp_dir().join(generated_id("team-worktree"));
        let context = ProjectContext {
            id: "team-project".into(),
            project_root: project_root.clone(),
            store_root: store_root.clone(),
            kind: ProjectKind::Repo,
            is_git_repo: true,
        };
        let mut member = ProviderRuntimeProjection {
            id: "member-cwd".into(),
            team_run_id: "team-cwd".into(),
            slot_id: None,
            agent_member_id: "agent-runtime-fixer".into(),
            name: "RuntimeFixer".into(),
            role: "implementer".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: MemberCoordinationStatus::Active,
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            started_at: now_string(),
            last_event_at: None,
            finished_at: None,
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        let run = AgentTeamRun {
            id: "team-cwd".into(),
            agent_team_id: "team-cwd-definition".into(),
            execution_node_id: "node-cwd".into(),
            previous_run_id: None,
            project_binding_id: context.id.clone(),
            host_surface: "test".into(),
            host_thread_id: None,
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "test cwd precedence".into(),
            execution_root: Some(execution_root.display().to_string()),
            status: TeamRunStatus::Planning,
            member_run_ids: vec![member.id.clone()],
            budget_limit_usd: None,
            created_at: now_string(),
            updated_at: now_string(),
            completed_at: None,
        };

        assert_eq!(
            member_spawn_cwd(Some(&context), &run, &member),
            execution_root
        );
        assert_ne!(member_spawn_cwd(Some(&context), &run, &member), store_root);
        member.provider_cwd_hint = Some(worktree_root.display().to_string());
        assert_eq!(
            member_spawn_cwd(Some(&context), &run, &member),
            worktree_root
        );
    }

    #[test]
    fn workspace_override_accepts_external_same_repo_worktree_and_rejects_unrelated_dir() {
        let base = std::env::temp_dir().join(generated_id("workspace-contract"));
        let project_root = base.join("project");
        let external_worktree = base.join("external-codex-worktree");
        let unrelated = base.join("unrelated");
        std::fs::create_dir_all(&project_root).expect("project root");
        std::fs::create_dir_all(&unrelated).expect("unrelated root");
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .output()
                .expect("run git fixture command");
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", project_root.to_str().expect("project path")]);
        git(&[
            "-C",
            project_root.to_str().expect("project path"),
            "config",
            "user.email",
            "workspace-test@example.invalid",
        ]);
        git(&[
            "-C",
            project_root.to_str().expect("project path"),
            "config",
            "user.name",
            "Workspace Test",
        ]);
        std::fs::write(project_root.join("README.md"), "workspace fixture\n").expect("seed");
        git(&[
            "-C",
            project_root.to_str().expect("project path"),
            "add",
            ".",
        ]);
        git(&[
            "-C",
            project_root.to_str().expect("project path"),
            "commit",
            "-m",
            "seed",
        ]);
        git(&[
            "-C",
            project_root.to_str().expect("project path"),
            "worktree",
            "add",
            "-b",
            "workspace-contract-test",
            external_worktree.to_str().expect("worktree path"),
        ]);
        let context = ProjectContext {
            id: "workspace-contract".into(),
            project_root: project_root.clone(),
            store_root: base.join("central-store"),
            kind: ProjectKind::Repo,
            is_git_repo: true,
        };
        assert_eq!(
            validate_workspace_override(
                Some(&context),
                external_worktree.to_str().expect("worktree path"),
                "execution_root",
            )
            .expect("external same-repository worktree"),
            project::canonicalize_best_effort(&external_worktree)
                .display()
                .to_string()
        );
        assert!(validate_workspace_override(
            Some(&context),
            unrelated.to_str().expect("unrelated path"),
            "execution_root",
        )
        .is_err());
        std::fs::remove_dir_all(&base).expect("cleanup workspace fixture");
    }

    #[test]
    fn test_sse_manager_multiple_subscribers() {
        let manager = sse::SseManager::new();
        let rx1 = manager.subscribe("_test");
        let rx2 = manager.subscribe("_test");

        let event = sse::SseEventFrame::Snapshot {
            messages: Vec::new(),
            generated_at: "2025-01-01T00:00:00Z".into(),
        };

        manager.broadcast("_test", event);

        // Both subscribers should receive the event
        let _ = rx1
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("rx1 should receive event");
        let _ = rx2
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("rx2 should receive event");
    }

    /// Regression: a long-lived `/v1/events` SSE connection must not starve
    /// other HTTP requests. Before per-connection threading the single accept
    /// loop blocked inside the SSE handler, so a concurrent `/v1/snapshot` (or a
    /// composer POST) hung until the stream closed. Here we hold an SSE stream
    /// open and assert a concurrent snapshot still returns promptly. The inline
    /// accept loop mirrors serve_command's per-connection threading.
    #[test]
    fn serve_preserves_unregistered_default_worktree_context() {
        let store_root = std::env::temp_dir().join(generated_id("serve-worktree-context-store"));
        let worktree_root =
            std::env::temp_dir().join(generated_id("serve-worktree-context-project"));
        let store = HarnessStore::new(&store_root);
        store.init().expect("init store");
        let expected = ProjectContext {
            id: "synthetic-worktree".to_string(),
            project_root: worktree_root.clone(),
            store_root: store_root.clone(),
            kind: ProjectKind::Repo,
            is_git_repo: true,
        };
        let projects = ServeProjects {
            firm_home: Some(std::env::temp_dir().join(generated_id("unrelated-registry"))),
            default_id: expected.id.clone(),
            default_store: store.clone(),
            default_space: None,
            default_context: Some(expected.clone()),
            dashboard_snapshot_builds: Arc::new(DashboardSnapshotBuildFence::default()),
        };

        let resolved = projects.context_for(Some(&expected.id), None, &store);
        assert_eq!(resolved, expected);
        assert_eq!(resolved.project_root, worktree_root);
        assert_ne!(
            resolved.project_root, resolved.store_root,
            "provider cwd/project selector must never collapse into the JSONL store"
        );

        let _ = std::fs::remove_dir_all(store_root);
        let _ = std::fs::remove_dir_all(worktree_root);
    }

    #[test]
    fn sse_stream_does_not_block_concurrent_requests() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::time::Duration;

        let root = std::env::temp_dir().join(format!(
            "harness-cli-test-{}",
            generated_id("serve-concurrency")
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("init store");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let addr = listener.local_addr().expect("local addr");
        let serve_store = store.clone();
        std::thread::spawn(move || {
            let sse_manager = sse::SseManager::new();
            // Single-project serve mode (no registry): default project routes to the
            // served store, watcher multiplexes over just that one.
            let projects = ServeProjects {
                firm_home: None,
                default_id: "_test".to_string(),
                default_store: serve_store.clone(),
                default_space: None,
                default_context: None,
                dashboard_snapshot_builds: Arc::new(DashboardSnapshotBuildFence::default()),
            };
            let watcher_projects = projects.clone();
            sse::start_sse_watcher(move || watcher_projects.watch_map(), sse_manager.clone())
                .expect("watcher");
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let conn_projects = projects.clone();
                let conn_manager = sse_manager.clone();
                std::thread::spawn(move || {
                    let _ = handle_http_connection(&conn_projects, stream, conn_manager);
                });
            }
        });

        // Open and hold an SSE stream; read its initial `snapshot` frame so we
        // know the server thread is parked inside the SSE handler.
        let mut sse_conn = TcpStream::connect(addr).expect("connect sse");
        sse_conn
            .write_all(b"GET /v1/events HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("send sse request");
        sse_conn
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("set sse read timeout");
        let mut sse_reader = BufReader::new(sse_conn.try_clone().expect("clone sse"));
        let mut saw_snapshot = false;
        for _ in 0..40 {
            let mut line = String::new();
            if sse_reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if line.contains("event: snapshot") {
                saw_snapshot = true;
                break;
            }
        }
        assert!(
            saw_snapshot,
            "SSE stream did not emit an initial snapshot frame"
        );

        // With the stream still held open, a concurrent snapshot request must
        // complete. A short read timeout makes a regression (blocked accept
        // loop) fail fast instead of hanging the whole test.
        let mut snap_conn = TcpStream::connect(addr).expect("connect snapshot");
        snap_conn
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set snapshot read timeout");
        snap_conn
            .write_all(b"GET /v1/snapshot HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("send snapshot request");
        let mut response = String::new();
        snap_conn
            .read_to_string(&mut response)
            .expect("snapshot must respond while an SSE stream is open");
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "expected 200 snapshot while SSE held, got: {}",
            response.lines().next().unwrap_or("<empty>")
        );

        drop(sse_conn);
        let _ = std::fs::remove_dir_all(root);
    }
