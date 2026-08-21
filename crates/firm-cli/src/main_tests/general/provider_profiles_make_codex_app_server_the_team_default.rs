use super::*;

    #[test]
    fn provider_profiles_make_codex_app_server_the_team_default() {
        let codex_app = team_member_provider_profile_for_mode("codex", Some("codex_app_server"));
        assert_eq!(codex_app.plan_mode, ProviderFeatureMode::Native);
        assert_eq!(codex_app.goal_mode, ProviderFeatureMode::Native);
        assert_eq!(
            codex_app.ordinary_message_boundary,
            OrdinaryMessageBoundary::NextRoundBatched
        );
        assert_eq!(
            team_member_provider_profile("codex").execution_mode,
            "codex_app_server"
        );

        let kimi = team_member_provider_profile_for_mode("kimi", Some("kimi_acp"));
        assert_eq!(kimi.plan_mode, ProviderFeatureMode::Native);
        assert_eq!(kimi.goal_mode, ProviderFeatureMode::Emulated);
        assert_eq!(
            kimi.ordinary_message_boundary,
            OrdinaryMessageBoundary::NextRoundBatched
        );
        assert_eq!(
            team_member_provider_profile_for_mode("claude", Some("claude_agent_sdk"))
                .ordinary_message_boundary,
            OrdinaryMessageBoundary::NextRoundBatched
        );

        // Historical records remain projectable even though new TeamRuns reject
        // codex_exec; Workflow continues to own that one-shot substrate.
        let codex_exec = team_member_provider_profile_for_mode("codex", Some("codex_exec"));
        assert_eq!(codex_exec.plan_mode, ProviderFeatureMode::Unsupported);
        assert_eq!(codex_exec.goal_mode, ProviderFeatureMode::Unsupported);
    }

