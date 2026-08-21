use super::*;

    #[test]
    fn capability_profiles_only_attach_recorded_live_canary_evidence() {
        let mut pi = team_member_provider_profile_for_mode("pi", Some("pi_rpc"));
        apply_provider_version(&mut pi, Some("0.84.2".to_string()));
        let pi_live_capabilities = pi
            .capability_bindings
            .iter()
            .filter(|binding| {
                binding.evidence.iter().any(|evidence| {
                    evidence.kind == harness_core::ProviderCapabilityEvidenceKind::LiveCanary
                })
            })
            .map(|binding| binding.capability.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            pi_live_capabilities,
            std::collections::BTreeSet::from([
                "close_runtime",
                "interrupt_current_cycle",
                "observe",
                "open_or_resume",
                "start_cycle",
            ])
        );
        assert!(pi.supports_cancel);
        assert!(pi.supports_resume);

        let mut claude = team_member_provider_profile_for_mode("claude", Some("claude_agent_sdk"));
        apply_provider_version(&mut claude, Some("2.1.220".to_string()));
        let claude_live_capabilities = claude
            .capability_bindings
            .iter()
            .filter(|binding| {
                binding.evidence.iter().any(|evidence| {
                    evidence.kind == harness_core::ProviderCapabilityEvidenceKind::LiveCanary
                })
            })
            .map(|binding| binding.capability.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            claude_live_capabilities,
            std::collections::BTreeSet::from([
                "close_runtime",
                "interrupt_current_cycle",
                "observe",
                "open_or_resume",
                "start_cycle",
            ])
        );
        let claude_interrupt = claude
            .capability_bindings
            .iter()
            .find(|binding| binding.capability == "interrupt_current_cycle")
            .expect("Claude interrupt binding");
        assert_eq!(
            claude_interrupt.status,
            harness_core::ProviderCapabilityStatus::Verified
        );
        assert_eq!(
            claude_interrupt.admission,
            harness_core::ProviderBindingAdmission::Active
        );
        assert!(claude_interrupt.evidence.iter().any(|evidence| {
            evidence.kind == harness_core::ProviderCapabilityEvidenceKind::DeterministicAcceptance
        }));
        assert!(claude_interrupt.evidence.iter().any(|evidence| {
            evidence.kind == harness_core::ProviderCapabilityEvidenceKind::LiveCanary
                && evidence
                    .evidence_ref
                    .contains("3590068d-b58c-4a90-852c-8c38b7de0250")
        }));
        assert!(claude.supports_cancel);
        assert!(claude.supports_resume);

        let mut codex = team_member_provider_profile_for_mode("codex", Some("codex_app_server"));
        apply_provider_version(&mut codex, Some("0.148.0-alpha.9".to_string()));
        let codex_live_capabilities = codex
            .capability_bindings
            .iter()
            .filter(|binding| {
                binding.evidence.iter().any(|evidence| {
                    evidence.kind == harness_core::ProviderCapabilityEvidenceKind::LiveCanary
                })
            })
            .map(|binding| binding.capability.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            codex_live_capabilities,
            std::collections::BTreeSet::from([
                "close_runtime",
                "interrupt_current_cycle",
                "observe",
                "open_or_resume",
                "start_cycle",
            ])
        );
        assert!(codex.supports_cancel);
        assert!(codex.supports_resume);

        let mut kimi = team_member_provider_profile_for_mode("kimi", Some("kimi_acp"));
        apply_provider_version(&mut kimi, Some("0.36.1".to_string()));
        let live_capabilities = kimi
            .capability_bindings
            .iter()
            .filter(|binding| {
                binding.evidence.iter().any(|evidence| {
                    evidence.kind == harness_core::ProviderCapabilityEvidenceKind::LiveCanary
                })
            })
            .map(|binding| binding.capability.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            live_capabilities,
            std::collections::BTreeSet::from([
                "close_runtime",
                "interrupt_current_cycle",
                "observe",
                "open_or_resume",
                "start_cycle",
            ])
        );
        assert!(kimi.supports_cancel);
        assert!(kimi.supports_resume);
    }

