use super::*;

    #[cfg(unix)]
    #[test]
    fn claude_runner_guard_disarms_after_normal_completion() {
        let child = process_group_command("exit 0")
            .spawn()
            .expect("spawn normally completing provider child");
        let pid = child.id();
        let mut guard = ProviderChildGuard::new(child);

        let status = guard.wait_and_disarm().expect("normal provider wait");

        assert!(status.success());
        assert!(!guard.armed, "normal wait must disarm teardown");
        drop(guard);
        assert!(
            !process_exists(pid),
            "normally reaped provider child survived"
        );
    }

