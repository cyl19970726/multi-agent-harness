use super::*;

    #[test]
    fn supervisor_heartbeat_diagnostics_name_run_generation_error_and_action() {
        let retry = supervisor_heartbeat_diagnostic(
            "team-run-x",
            "supervisor-y",
            7,
            "io error: store write lock contention",
            "retry_backoff_1000ms_attempt_2_3",
        );
        for token in [
            "team run team-run-x",
            "supervisor supervisor-y",
            "generation 7",
            "io error: store write lock contention",
            "action=retry_backoff_1000ms_attempt_2_3",
        ] {
            assert!(
                retry.contains(token),
                "retry diagnostic missing {token:?}: {retry}"
            );
        }

        let terminal = supervisor_heartbeat_diagnostic(
            "team-run-x",
            "supervisor-y",
            7,
            "TEAM_SUPERVISOR_PARENT_FENCED: parent NodeDaemon generation is no longer active for TeamRun team-run-x",
            "latched_lease_loss_terminal",
        );
        for token in [
            "team run team-run-x",
            "generation 7",
            "TEAM_SUPERVISOR_PARENT_FENCED",
            "action=latched_lease_loss_terminal",
        ] {
            assert!(
                terminal.contains(token),
                "terminal latch diagnostic missing {token:?}: {terminal}"
            );
        }
    }

