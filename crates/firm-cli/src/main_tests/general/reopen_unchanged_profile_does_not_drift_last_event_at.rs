use super::*;

    #[test]
    fn reopen_unchanged_profile_does_not_drift_last_event_at() {
        assert_unchanged_profile_refresh_has_no_in_memory_revision();
    }

