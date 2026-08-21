use super::*;

    #[cfg(unix)]
    #[test]
    fn claude_runner_guard_kills_stale_active_process_group() {
        let marker = std::env::temp_dir().join(format!(
            "harness-provider-child-group-{}-{}",
            std::process::id(),
            current_unix_ms()
        ));
        let mut command = process_group_command(r#"sleep 30 & echo $! > "$1"; wait"#);
        command.arg("provider-child-test").arg(&marker);
        let child = command.spawn().expect("spawn provider child group");
        let leader_pid = child.id();
        let guard = ProviderChildGuard::new(child);
        let deadline = Instant::now() + Duration::from_secs(2);
        let descendant_pid = loop {
            if let Ok(pid) = fs::read_to_string(&marker) {
                if let Ok(pid) = pid.trim().parse::<u32>() {
                    break pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "descendant pid marker was not populated"
            );
            std::thread::sleep(Duration::from_millis(10));
        };

        drop(guard);

        assert!(
            !process_exists(leader_pid),
            "group leader survived guard drop"
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while process_exists(descendant_pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists(descendant_pid),
            "provider descendant survived stale guard teardown"
        );
        let _ = fs::remove_file(marker);
    }

