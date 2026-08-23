use super::*;
use harness_core::SecurityEnforcementLocusKind;

#[test]
fn binding_registry_is_the_closed_runner_dispatch_registry() {
    for (provider, mode, kind) in [
        ("pi", "pi_rpc", SharedTeamRuntimeKind::Pi),
        ("kimi", "kimi_acp", SharedTeamRuntimeKind::Kimi),
        ("codex", "codex_app_server", SharedTeamRuntimeKind::Codex),
        ("claude", "claude_agent_sdk", SharedTeamRuntimeKind::Claude),
        (
            "deepseek_harness",
            "deepseek_sdk",
            SharedTeamRuntimeKind::DeepSeek,
        ),
    ] {
        assert_eq!(shared_team_runtime_kind(provider, Some(mode)), Some(kind));
        assert_eq!(shared_team_runtime_kind(provider, None), Some(kind));
        assert!(capability_bindings_for(provider).is_some());
    }
    for (provider, mode) in [
        ("codex", "codex_exec"),
        ("claude", "claude_cli"),
        ("kimi", "pi_rpc"),
        ("unknown", "unknown"),
    ] {
        assert_eq!(shared_team_runtime_kind(provider, Some(mode)), None);
    }
    assert!(capability_bindings_for("unknown").is_none());
}

#[test]
fn compatibility_registry_cannot_drift_from_the_canonical_provider_catalog() {
    let catalog = harness_application::PROVIDERS
        .iter()
        .filter_map(|descriptor| {
            descriptor
                .direct_delivery_compatibility
                .map(|_| descriptor.provider)
        })
        .collect::<Vec<_>>();
    assert_eq!(crate::supported_provider_names(), catalog);
    assert!(catalog
        .iter()
        .all(|provider| crate::compatibility_delivery_binding(provider).is_some()));
    assert!(crate::compatibility_delivery_binding("unknown").is_none());
}

#[test]
fn pi_launch_policy_only_compiles_admissible_ceilings() {
    assert_eq!(
        pi_tools_allowlist_for_ceiling(PermissionCeiling::ReadOnly).unwrap(),
        Some("read,grep,find,ls")
    );
    assert!(pi_tools_allowlist_for_ceiling(PermissionCeiling::WorkspaceWrite).is_err());
    assert_eq!(
        pi_tools_allowlist_for_ceiling(PermissionCeiling::FullAccess).unwrap(),
        None
    );
}

#[test]
fn readonly_allowlist_never_includes_mutating_tools() {
    let allowlist = pi_tools_allowlist_for_ceiling(PermissionCeiling::ReadOnly)
        .unwrap()
        .unwrap();
    for forbidden in ["bash", "write", "edit"] {
        assert!(!allowlist.split(',').any(|tool| tool == forbidden));
    }
}

#[test]
fn enforcement_locus_matches_compilation() {
    let restricted = pi_security_enforcement_locus(PermissionCeiling::ReadOnly);
    assert_eq!(
        restricted.kind,
        SecurityEnforcementLocusKind::AdapterToolAllowlist
    );
    let workspace_write = pi_security_enforcement_locus(PermissionCeiling::WorkspaceWrite);
    assert_eq!(
        workspace_write.kind,
        SecurityEnforcementLocusKind::NoneVerified
    );
    let full = pi_security_enforcement_locus(PermissionCeiling::FullAccess);
    assert_eq!(full.kind, SecurityEnforcementLocusKind::NoneVerified);
}

#[test]
fn pi_permission_admission_fails_closed_without_filesystem_containment() {
    assert!(
        admit_pi_permission_ceiling(PermissionCeiling::ReadOnly, Some("read,grep,find,ls")).is_ok()
    );
    assert!(admit_pi_permission_ceiling(PermissionCeiling::FullAccess, None).is_ok());
    let error = admit_pi_permission_ceiling(
        PermissionCeiling::WorkspaceWrite,
        Some("read,grep,find,ls,write,edit"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("filesystem containment"));
    let error = admit_pi_permission_ceiling(PermissionCeiling::ReadOnly, None).unwrap_err();
    assert!(error.to_string().contains("expected tools"));
}

#[test]
fn pi_capability_bindings_are_honest() {
    let bindings = capability_bindings_for("pi").expect("pi binding report");
    // Every Supported claim must name its evidence.
    for binding in &bindings {
        if binding.status.is_supported() {
            assert!(
                !binding.evidence.trim().is_empty(),
                "{} is Supported without evidence",
                binding.capability
            );
        }
    }
    // Continuation intents are honestly Unsupported: Pi has no native Goal.
    for capability in [
        "inspect_continuation",
        "inhibit_continuation",
        "resume_continuation",
    ] {
        let binding = bindings
            .iter()
            .find(|binding| binding.capability == capability)
            .unwrap();
        assert_eq!(
            binding.status,
            CapabilityStatus::Unsupported,
            "{capability}"
        );
    }
    // reconcile_effect was the static-matrix overclaim; the executable
    // report must not claim it.
    let reconcile = bindings
        .iter()
        .find(|binding| binding.capability == "reconcile_effect")
        .unwrap();
    assert!(!reconcile.status.is_supported());
    // Permission enforcement is conditional per admitted session. The
    // static binding must not describe trusted full_access as verified.
    let permission = bindings
        .iter()
        .find(|binding| binding.capability == "permission_enforcement")
        .unwrap();
    assert_eq!(permission.status, CapabilityStatus::Degraded);
    assert!(permission.security_enforcement_locus.is_none());
}

#[test]
fn kimi_capability_bindings_match_the_reviewed_acp_surface() {
    let bindings = capability_bindings_for("kimi").expect("Kimi ACP binding report");
    for capability in [
        "open_or_resume",
        "start_cycle",
        "interrupt_current_cycle",
        "observe",
    ] {
        let binding = bindings
            .iter()
            .find(|binding| binding.capability == capability)
            .unwrap();
        assert_eq!(binding.status, CapabilityStatus::Supported, "{capability}");
        assert!(!binding.evidence.trim().is_empty(), "{capability}");
    }
    for capability in [
        "inject_current_cycle",
        "queue_at_native_boundary",
        "inspect_continuation",
        "reconcile_effect",
    ] {
        let binding = bindings
            .iter()
            .find(|binding| binding.capability == capability)
            .unwrap();
        assert_eq!(
            binding.status,
            CapabilityStatus::Unsupported,
            "{capability}"
        );
    }
    for capability in ["quiesce", "release", "permission_enforcement"] {
        let binding = bindings
            .iter()
            .find(|binding| binding.capability == capability)
            .unwrap();
        assert_eq!(binding.status, CapabilityStatus::Degraded, "{capability}");
    }
}

#[test]
fn unknown_providers_without_a_binding_report_none() {
    // The generic model label remains unregistered; the coding-agent provider
    // has the explicit deepseek_harness identity above.
    assert!(capability_bindings_for("deepseek").is_none());
    for provider in ["codex", "claude", "deepseek_harness"] {
        let bindings = capability_bindings_for(provider).expect("executable binding report");
        assert!(bindings.iter().any(|binding| {
            binding.capability == "close_runtime" && binding.status == CapabilityStatus::Supported
        }));
    }
}
