use super::*;

#[test]
fn managed_host_ignores_external_binding_environment() {
    let ambient_surface = Some("codex-app".to_string());
    let ambient_thread = Some("external-thread".to_string());

    assert_eq!(
        admitted_ambient_host_binding(
            HostControlMode::Managed,
            ambient_surface.clone(),
            ambient_thread.clone(),
        ),
        (None, None),
        "managed Host is a daemon-driven AgentMember, not an external thread binding",
    );
    assert!(!uses_external_host_binding(HostControlMode::Managed));
    assert!(!is_unbound_external_host(HostControlMode::Managed, None));

    assert_eq!(
        admitted_ambient_host_binding(
            HostControlMode::ExternalInteractive,
            ambient_surface.clone(),
            ambient_thread.clone(),
        ),
        (ambient_surface, ambient_thread),
        "external interactive Host retains hook-based binding",
    );
    assert!(uses_external_host_binding(
        HostControlMode::ExternalInteractive
    ));
    assert!(is_unbound_external_host(
        HostControlMode::ExternalInteractive,
        None,
    ));
    assert!(!is_unbound_external_host(
        HostControlMode::ExternalInteractive,
        Some("external-thread"),
    ));
}
