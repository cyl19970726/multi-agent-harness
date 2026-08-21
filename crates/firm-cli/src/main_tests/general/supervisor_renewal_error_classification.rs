use super::*;

#[test]
fn supervisor_renewal_error_classification() {
    assert!(
        !is_terminal_supervisor_renewal_error(&StoreError::Io(std::io::Error::other(
            "store write lock contention",
        ))),
        "IO/lock-contention errors must be transient"
    );
    assert!(
        !is_terminal_supervisor_renewal_error(&StoreError::LockTimeout("store lock".to_string())),
        "lock timeouts must be transient"
    );
    assert!(
        is_terminal_supervisor_renewal_error(&StoreError::Conflict(
            "TEAM_SUPERVISOR_PARENT_FENCED: Node n has no active parent".to_string()
        )),
        "parent fence must be terminal"
    );
    assert!(
            is_terminal_supervisor_renewal_error(&StoreError::Conflict(
                "TEAM_SUPERVISOR_PARENT_FENCED: parent NodeDaemon generation is no longer active for TeamRun r".to_string()
            )),
            "parent fence must be terminal"
        );
    assert!(
        is_terminal_supervisor_renewal_error(&StoreError::Conflict(
            "Supervisor lease for team run r is no longer owned by s generation 1".to_string()
        )),
        "superseded lease must be terminal"
    );
    assert!(
        is_terminal_supervisor_renewal_error(&StoreError::Conflict(
            "team run r has no Supervisor lease to renew".to_string()
        )),
        "missing lease row must be terminal"
    );
    assert!(
        !is_terminal_supervisor_renewal_error(&StoreError::Conflict(
            "some unrelated store conflict".to_string()
        )),
        "unexpected conflicts must fall back to transient retries"
    );
}
