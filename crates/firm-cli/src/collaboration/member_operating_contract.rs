#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberOperatingAction {
    SendInformational,
    SendResponseRequired,
    Reply,
    RequestDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorRole {
    AnyMember,
    Host,
    Member,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecipientBinding {
    ExplicitStableAgent,
    CurrentHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkBinding {
    DiscussedWork,
    RecipientWorkFromBoard,
    IncomingMessageWork,
    CurrentWork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseIntent {
    Informational,
    ResponseRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CorrelationBinding {
    None,
    ExactIncomingMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WakeBehavior {
    DoesNotWakeIdleRecipient,
    WakesExactIdleManagedRecipient,
    WakesHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemberOperatingActionSpec {
    pub(crate) action: MemberOperatingAction,
    pub(crate) actor_role: ActorRole,
    pub(crate) recipient: RecipientBinding,
    pub(crate) work_binding: WorkBinding,
    pub(crate) response_intent: ResponseIntent,
    pub(crate) correlation: CorrelationBinding,
    pub(crate) wake_behavior: WakeBehavior,
}

pub(crate) const MEMBER_OPERATING_ACTIONS: [MemberOperatingActionSpec; 4] = [
    MemberOperatingActionSpec {
        action: MemberOperatingAction::SendInformational,
        actor_role: ActorRole::AnyMember,
        recipient: RecipientBinding::ExplicitStableAgent,
        work_binding: WorkBinding::DiscussedWork,
        response_intent: ResponseIntent::Informational,
        correlation: CorrelationBinding::None,
        wake_behavior: WakeBehavior::DoesNotWakeIdleRecipient,
    },
    MemberOperatingActionSpec {
        action: MemberOperatingAction::SendResponseRequired,
        actor_role: ActorRole::Host,
        recipient: RecipientBinding::ExplicitStableAgent,
        work_binding: WorkBinding::RecipientWorkFromBoard,
        response_intent: ResponseIntent::ResponseRequired,
        correlation: CorrelationBinding::None,
        wake_behavior: WakeBehavior::WakesExactIdleManagedRecipient,
    },
    MemberOperatingActionSpec {
        action: MemberOperatingAction::Reply,
        actor_role: ActorRole::AnyMember,
        recipient: RecipientBinding::ExplicitStableAgent,
        work_binding: WorkBinding::IncomingMessageWork,
        response_intent: ResponseIntent::Informational,
        correlation: CorrelationBinding::ExactIncomingMessage,
        wake_behavior: WakeBehavior::DoesNotWakeIdleRecipient,
    },
    MemberOperatingActionSpec {
        action: MemberOperatingAction::RequestDecision,
        actor_role: ActorRole::Member,
        recipient: RecipientBinding::CurrentHost,
        work_binding: WorkBinding::CurrentWork,
        response_intent: ResponseIntent::ResponseRequired,
        correlation: CorrelationBinding::None,
        wake_behavior: WakeBehavior::WakesHost,
    },
];

pub(crate) fn member_message_subcommand_usage() -> &'static str {
    "member message send|reply|request-decision --body <markdown> ..."
}

fn action_spec(action: MemberOperatingAction) -> &'static MemberOperatingActionSpec {
    MEMBER_OPERATING_ACTIONS
        .iter()
        .find(|spec| spec.action == action)
        .expect("every MemberOperatingAction has one canonical spec")
}

fn generic_command(action: MemberOperatingAction, current_work_id: &str) -> String {
    match action {
        MemberOperatingAction::SendInformational => "\"$HARNESS_BIN\" member message send --recipient-agent-id <stable-agent-identity> --work-id <discussed-work-id> --body '<markdown>'".to_string(),
        MemberOperatingAction::SendResponseRequired => "\"$HARNESS_BIN\" member message send --response-required --recipient-agent-id <stable-agent-identity> --work-id <recipient-work-id-from-board> --body '<action requested, acceptance, next step>'".to_string(),
        MemberOperatingAction::Reply => "\"$HARNESS_BIN\" member message reply --recipient-agent-id <stable-agent-identity> --correlation-id <correlation-id> --causation-id <message-id> --work-id <incoming-work-id> --body '<markdown>'".to_string(),
        MemberOperatingAction::RequestDecision => format!(
            "\"$HARNESS_BIN\" member message request-decision --work-id {current_work_id} --body '<decision needed, options, recommendation>'"
        ),
    }
}

pub(crate) struct MemberOperatingContract<'a> {
    current_work_id: &'a str,
}

impl<'a> MemberOperatingContract<'a> {
    pub(crate) fn new(current_work_id: &'a str) -> Self {
        Self { current_work_id }
    }

    pub(crate) fn render_provider_prompt(&self) -> String {
        let informational = generic_command(
            action_spec(MemberOperatingAction::SendInformational).action,
            self.current_work_id,
        );
        let response_required = generic_command(
            action_spec(MemberOperatingAction::SendResponseRequired).action,
            self.current_work_id,
        );
        let reply = generic_command(
            action_spec(MemberOperatingAction::Reply).action,
            self.current_work_id,
        );
        let request_decision = generic_command(
            action_spec(MemberOperatingAction::RequestDecision).action,
            self.current_work_id,
        );
        format!(
            "- Send an informational canonical Work-linked Message through the authenticated Member Role Action, which does not wake an idle recipient, with: {informational}. For ordinary member progress, `<discussed-work-id>` is {current_work_id}. The bound command derives your sender identity and live runtime scope; never select a sender identity.\n\
             - When a Host needs a Member to act, send response-required mail so that exact idle managed recipient gets a provider round: {response_required}. Use this for Host assignment/progress/retry, and never use the Host Work id for another member's action.\n\
             - Reply with the exact correlation, causation, and Work id printed beside an incoming Message: {reply}. Never replace the incoming Work id with your own Work id.\n\
             - A Member asks the Host to decide, review, or accept with: {request_decision}. This command routes to the Host; a response-required Message wakes the Host but never transfers or accepts Work.",
            current_work_id = self.current_work_id,
        )
    }
}

pub(crate) fn render_incoming_message_reply_command(
    recipient_agent_id: &str,
    correlation_id: &str,
    causation_id: &str,
    work_id: Option<&str>,
) -> String {
    let spec = action_spec(MemberOperatingAction::Reply);
    debug_assert_eq!(spec.correlation, CorrelationBinding::ExactIncomingMessage);
    let work_argument = work_id
        .map(|work_id| format!(" --work-id {work_id}"))
        .unwrap_or_default();
    format!(
        "\"$HARNESS_BIN\" member message reply --recipient-agent-id {recipient_agent_id} --correlation-id {correlation_id} --causation-id {causation_id}{work_argument} --body '<markdown>'"
    )
}

pub(crate) fn render_member_message_cli_help() -> String {
    MEMBER_OPERATING_ACTIONS
        .iter()
        .map(|spec| {
            let command = generic_command(spec.action, "<current-work-id>");
            format!(
                "  {command}\n      actor={:?}; recipient={:?}; work={:?}; response={:?}; correlation={:?}; wake={:?}",
                spec.actor_role,
                spec.recipient,
                spec.work_binding,
                spec.response_intent,
                spec.correlation,
                spec.wake_behavior,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operating_actions_have_one_typed_semantic_source() {
        assert_eq!(MEMBER_OPERATING_ACTIONS.len(), 4);
        assert_eq!(
            action_spec(MemberOperatingAction::SendInformational).wake_behavior,
            WakeBehavior::DoesNotWakeIdleRecipient
        );
        assert_eq!(
            action_spec(MemberOperatingAction::SendResponseRequired).work_binding,
            WorkBinding::RecipientWorkFromBoard
        );
        assert_eq!(
            action_spec(MemberOperatingAction::Reply).correlation,
            CorrelationBinding::ExactIncomingMessage
        );
        assert_eq!(
            action_spec(MemberOperatingAction::RequestDecision).recipient,
            RecipientBinding::CurrentHost
        );
    }

    #[test]
    fn provider_and_cli_renderers_consume_the_same_action_specs() {
        let provider = MemberOperatingContract::new("work-7").render_provider_prompt();
        let cli = render_member_message_cli_help();
        for command in [
            "member message send --recipient-agent-id",
            "member message send --response-required",
            "member message reply --recipient-agent-id",
            "member message request-decision --work-id",
        ] {
            assert!(provider.contains(command));
            assert!(cli.contains(command));
        }
        assert!(provider.contains("request-decision --work-id work-7"));
        assert!(cli.contains("request-decision --work-id <current-work-id>"));
    }
}
