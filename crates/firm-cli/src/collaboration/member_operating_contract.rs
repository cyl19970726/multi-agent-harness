#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberOperatingAction {
    SendInformational,
    SendResponseRequired,
    Reply,
    RequestDecision,
}

impl MemberOperatingAction {
    fn label(self) -> &'static str {
        match self {
            Self::SendInformational => "send message — informational",
            Self::SendResponseRequired => "send message — response required",
            Self::Reply => "reply",
            Self::RequestDecision => "request-decision",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorRole {
    AnyTeamActor,
    HostRecommended,
    MemberRecommended,
}

impl ActorRole {
    fn label(self) -> &'static str {
        match self {
            Self::AnyTeamActor => "Host or active Member",
            Self::HostRecommended => "Host operating action",
            Self::MemberRecommended => "Member operating action",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeAuthorization {
    HostOrActiveMember,
}

impl RuntimeAuthorization {
    fn label(self) -> &'static str {
        match self {
            Self::HostOrActiveMember => {
                "authenticated Member Role Action accepts the exact Host or one active Member"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecipientBinding {
    ExplicitTeamAgent,
    CurrentHost,
}

impl RecipientBinding {
    fn command_argument(self) -> Option<&'static str> {
        match self {
            Self::ExplicitTeamAgent => Some("--recipient-agent-id <stable-agent-identity>"),
            Self::CurrentHost => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ExplicitTeamAgent => "explicit stable Team agent identity",
            Self::CurrentHost => "current Team Host resolved by the route",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkBinding {
    Discussed,
    RecipientFromBoard,
    IncomingMessage,
    Current,
}

impl WorkBinding {
    fn command_value(self, current_work_id: &str) -> String {
        match self {
            Self::Discussed => "<discussed-work-id>".to_string(),
            Self::RecipientFromBoard => "<recipient-work-id-from-board>".to_string(),
            Self::IncomingMessage => "<incoming-work-id>".to_string(),
            Self::Current => current_work_id.to_string(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Discussed => "optional; when present use the Work being discussed",
            Self::RecipientFromBoard => {
                "optional; when present use the recipient's Work from the board"
            }
            Self::IncomingMessage => "optional; preserve the incoming Message Work when present",
            Self::Current => "optional; normally use the actor's current Work",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseIntent {
    Informational,
    ResponseRequired,
    CallerSelectedInformationalDefault,
}

impl ResponseIntent {
    fn command_argument(self) -> Option<&'static str> {
        match self {
            Self::ResponseRequired => Some("--response-required"),
            Self::Informational | Self::CallerSelectedInformationalDefault => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Informational => "informational",
            Self::ResponseRequired => "response required",
            Self::CallerSelectedInformationalDefault => {
                "caller-selected; informational unless --response-required is supplied"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CorrelationBinding {
    RouteGenerated,
    ExactIncomingMessage,
}

impl CorrelationBinding {
    fn command_arguments(self) -> &'static [&'static str] {
        match self {
            Self::RouteGenerated => &[],
            Self::ExactIncomingMessage => &[
                "--correlation-id <correlation-id>",
                "--causation-id <message-id>",
            ],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::RouteGenerated => "route-generated correlation",
            Self::ExactIncomingMessage => "exact incoming correlation and causation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WakeBehavior {
    DoesNotWakeIdleRecipient,
    WakesExactIdleManagedRecipient,
    FollowsCallerSelectedResponseIntent,
    WakesHost,
}

impl WakeBehavior {
    fn label(self) -> &'static str {
        match self {
            Self::DoesNotWakeIdleRecipient => "does not wake an idle recipient",
            Self::WakesExactIdleManagedRecipient => "wakes the exact idle managed recipient",
            Self::FollowsCallerSelectedResponseIntent => {
                "wakes only when the caller selects response-required"
            }
            Self::WakesHost => "wakes the Host",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageRoute {
    Send,
    Reply,
    RequestDecision,
}

impl MessageRoute {
    fn subcommand(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Reply => "reply",
            Self::RequestDecision => "request-decision",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyShape {
    Markdown,
    RequestedAction,
    DecisionRequest,
}

impl BodyShape {
    fn placeholder(self) -> &'static str {
        match self {
            Self::Markdown => "<markdown>",
            Self::RequestedAction => "<action requested, acceptance, next step>",
            Self::DecisionRequest => "<decision needed, options, recommendation>",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MemberOperatingActionSpec {
    pub(crate) action: MemberOperatingAction,
    pub(crate) actor_role: ActorRole,
    pub(crate) runtime_authorization: RuntimeAuthorization,
    pub(crate) recipient: RecipientBinding,
    pub(crate) work_binding: WorkBinding,
    pub(crate) response_intent: ResponseIntent,
    pub(crate) correlation: CorrelationBinding,
    pub(crate) wake_behavior: WakeBehavior,
    route: MessageRoute,
    body_shape: BodyShape,
}

pub(crate) const MEMBER_OPERATING_ACTIONS: [MemberOperatingActionSpec; 4] = [
    MemberOperatingActionSpec {
        action: MemberOperatingAction::SendInformational,
        actor_role: ActorRole::AnyTeamActor,
        runtime_authorization: RuntimeAuthorization::HostOrActiveMember,
        recipient: RecipientBinding::ExplicitTeamAgent,
        work_binding: WorkBinding::Discussed,
        response_intent: ResponseIntent::Informational,
        correlation: CorrelationBinding::RouteGenerated,
        wake_behavior: WakeBehavior::DoesNotWakeIdleRecipient,
        route: MessageRoute::Send,
        body_shape: BodyShape::Markdown,
    },
    MemberOperatingActionSpec {
        action: MemberOperatingAction::SendResponseRequired,
        actor_role: ActorRole::HostRecommended,
        runtime_authorization: RuntimeAuthorization::HostOrActiveMember,
        recipient: RecipientBinding::ExplicitTeamAgent,
        work_binding: WorkBinding::RecipientFromBoard,
        response_intent: ResponseIntent::ResponseRequired,
        correlation: CorrelationBinding::RouteGenerated,
        wake_behavior: WakeBehavior::WakesExactIdleManagedRecipient,
        route: MessageRoute::Send,
        body_shape: BodyShape::RequestedAction,
    },
    MemberOperatingActionSpec {
        action: MemberOperatingAction::Reply,
        actor_role: ActorRole::AnyTeamActor,
        runtime_authorization: RuntimeAuthorization::HostOrActiveMember,
        recipient: RecipientBinding::ExplicitTeamAgent,
        work_binding: WorkBinding::IncomingMessage,
        response_intent: ResponseIntent::CallerSelectedInformationalDefault,
        correlation: CorrelationBinding::ExactIncomingMessage,
        wake_behavior: WakeBehavior::FollowsCallerSelectedResponseIntent,
        route: MessageRoute::Reply,
        body_shape: BodyShape::Markdown,
    },
    MemberOperatingActionSpec {
        action: MemberOperatingAction::RequestDecision,
        actor_role: ActorRole::MemberRecommended,
        runtime_authorization: RuntimeAuthorization::HostOrActiveMember,
        recipient: RecipientBinding::CurrentHost,
        work_binding: WorkBinding::Current,
        response_intent: ResponseIntent::ResponseRequired,
        correlation: CorrelationBinding::RouteGenerated,
        wake_behavior: WakeBehavior::WakesHost,
        route: MessageRoute::RequestDecision,
        body_shape: BodyShape::DecisionRequest,
    },
];

#[derive(Default)]
struct CommandBindings<'a> {
    recipient_agent_id: Option<&'a str>,
    correlation_id: Option<&'a str>,
    causation_id: Option<&'a str>,
    work_id: Option<&'a str>,
    omit_work_if_unbound: bool,
}

impl MemberOperatingActionSpec {
    fn render_command(&self, current_work_id: &str, bindings: CommandBindings<'_>) -> String {
        let mut arguments = vec![
            "\"$FIRM_BIN\"".to_string(),
            "member".to_string(),
            "message".to_string(),
            self.route.subcommand().to_string(),
        ];
        if let Some(argument) = self.response_intent.command_argument() {
            arguments.push(argument.to_string());
        }
        if let Some(argument) = self.recipient.command_argument() {
            arguments.push(
                bindings
                    .recipient_agent_id
                    .map(|value| format!("--recipient-agent-id {value}"))
                    .unwrap_or_else(|| argument.to_string()),
            );
        }
        if self.correlation == CorrelationBinding::ExactIncomingMessage {
            arguments.push(format!(
                "--correlation-id {}",
                bindings.correlation_id.unwrap_or("<correlation-id>")
            ));
            arguments.push(format!(
                "--causation-id {}",
                bindings.causation_id.unwrap_or("<message-id>")
            ));
        } else {
            arguments.extend(
                self.correlation
                    .command_arguments()
                    .iter()
                    .map(|argument| (*argument).to_string()),
            );
        }
        if let Some(work_id) = bindings.work_id {
            arguments.push(format!("--work-id {work_id}"));
        } else if !bindings.omit_work_if_unbound {
            arguments.push(format!(
                "--work-id {}",
                self.work_binding.command_value(current_work_id)
            ));
        }
        arguments.push(format!("--body '{}'", self.body_shape.placeholder()));
        arguments.join(" ")
    }

    fn render_projection(&self, current_work_id: &str) -> String {
        format!(
            "- {}: {}. Contract: operating actor={}; authorization={}; recipient={}; work={}; response={}; correlation={}; wake={}.",
            self.action.label(),
            self.render_command(current_work_id, CommandBindings::default()),
            self.actor_role.label(),
            self.runtime_authorization.label(),
            self.recipient.label(),
            self.work_binding.label(),
            self.response_intent.label(),
            self.correlation.label(),
            self.wake_behavior.label(),
        )
    }
}

fn action_spec(action: MemberOperatingAction) -> &'static MemberOperatingActionSpec {
    MEMBER_OPERATING_ACTIONS
        .iter()
        .find(|spec| spec.action == action)
        .expect("every MemberOperatingAction has one canonical spec")
}

pub(crate) fn member_message_subcommand_usage() -> String {
    let mut subcommands = Vec::new();
    for spec in MEMBER_OPERATING_ACTIONS {
        let subcommand = spec.route.subcommand();
        if !subcommands.contains(&subcommand) {
            subcommands.push(subcommand);
        }
    }
    format!(
        "member message {} --body <markdown> ...",
        subcommands.join("|")
    )
}

pub(crate) struct MemberOperatingContract<'a> {
    current_work_id: &'a str,
}

impl<'a> MemberOperatingContract<'a> {
    pub(crate) fn new(current_work_id: &'a str) -> Self {
        Self { current_work_id }
    }

    pub(crate) fn render_provider_prompt(&self) -> String {
        MEMBER_OPERATING_ACTIONS
            .iter()
            .map(|spec| spec.render_projection(self.current_work_id))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) fn render_incoming_message_reply_command(
    recipient_agent_id: &str,
    correlation_id: &str,
    causation_id: &str,
    work_id: Option<&str>,
) -> String {
    action_spec(MemberOperatingAction::Reply).render_command(
        "<current-work-id>",
        CommandBindings {
            recipient_agent_id: Some(recipient_agent_id),
            correlation_id: Some(correlation_id),
            causation_id: Some(causation_id),
            work_id,
            omit_work_if_unbound: true,
        },
    )
}

pub(crate) fn render_member_message_cli_help() -> String {
    MEMBER_OPERATING_ACTIONS
        .iter()
        .map(|spec| spec.render_projection("<current-work-id>"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_typed_field_drives_the_shared_projection() {
        let base = MEMBER_OPERATING_ACTIONS[0];
        let rendered = base.render_projection("work-7");
        for changed in [
            MemberOperatingActionSpec {
                action: MemberOperatingAction::Reply,
                ..base
            },
            MemberOperatingActionSpec {
                actor_role: ActorRole::HostRecommended,
                ..base
            },
            MemberOperatingActionSpec {
                recipient: RecipientBinding::CurrentHost,
                ..base
            },
            MemberOperatingActionSpec {
                work_binding: WorkBinding::Current,
                ..base
            },
            MemberOperatingActionSpec {
                response_intent: ResponseIntent::ResponseRequired,
                ..base
            },
            MemberOperatingActionSpec {
                correlation: CorrelationBinding::ExactIncomingMessage,
                ..base
            },
            MemberOperatingActionSpec {
                wake_behavior: WakeBehavior::WakesHost,
                ..base
            },
            MemberOperatingActionSpec {
                route: MessageRoute::Reply,
                ..base
            },
            MemberOperatingActionSpec {
                body_shape: BodyShape::DecisionRequest,
                ..base
            },
        ] {
            assert_ne!(rendered, changed.render_projection("work-7"));
        }
    }

    #[test]
    fn action_collection_is_unique_complete_and_honest_about_runtime_choices() {
        assert_eq!(MEMBER_OPERATING_ACTIONS.len(), 4);
        let mut actions = MEMBER_OPERATING_ACTIONS
            .iter()
            .map(|spec| spec.action.label())
            .collect::<Vec<_>>();
        actions.sort_unstable();
        actions.dedup();
        assert_eq!(actions.len(), MEMBER_OPERATING_ACTIONS.len());

        let reply = action_spec(MemberOperatingAction::Reply);
        assert_eq!(
            reply.response_intent,
            ResponseIntent::CallerSelectedInformationalDefault
        );
        assert_eq!(reply.work_binding, WorkBinding::IncomingMessage);
        assert_eq!(
            reply.runtime_authorization,
            RuntimeAuthorization::HostOrActiveMember
        );
        assert!(reply
            .render_projection("work-7")
            .contains("caller-selected; informational unless --response-required"));
    }

    #[test]
    fn provider_cli_usage_and_incoming_reply_share_the_action_collection() {
        let provider = MemberOperatingContract::new("work-7").render_provider_prompt();
        let cli = render_member_message_cli_help();
        for spec in MEMBER_OPERATING_ACTIONS {
            let label = spec.action.label();
            assert!(provider.contains(label));
            assert!(cli.contains(label));
            assert!(provider.contains(&spec.render_command("work-7", CommandBindings::default())));
            assert!(
                cli.contains(&spec.render_command("<current-work-id>", CommandBindings::default()))
            );
        }
        assert_eq!(
            member_message_subcommand_usage(),
            "member message send|reply|request-decision --body <markdown> ..."
        );
        assert_eq!(
            render_incoming_message_reply_command(
                "agent-2",
                "correlation-1",
                "message-1",
                Some("work-1")
            ),
            "\"$FIRM_BIN\" member message reply --recipient-agent-id agent-2 --correlation-id correlation-1 --causation-id message-1 --work-id work-1 --body '<markdown>'"
        );
        assert!(!render_incoming_message_reply_command(
            "agent-2",
            "correlation-1",
            "message-1",
            None
        )
        .contains("--work-id"));
    }
}
