use super::*;

#[test]
fn start_warns_and_auto_binds_when_unbound() {
    assert!(is_unbound_external_host(
        HostControlMode::ExternalInteractive,
        None,
    ));
    assert!(!is_unbound_external_host(HostControlMode::Managed, None));

    let ambient = admitted_ambient_host_binding(
        HostControlMode::ExternalInteractive,
        Some("codex-app".into()),
        Some("start-thread".into()),
    );
    assert_eq!(ambient.0.as_deref(), Some("codex-app"));
    assert_eq!(ambient.1.as_deref(), Some("start-thread"));
}
