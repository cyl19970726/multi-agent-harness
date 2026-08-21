use super::*;

/// Execution mode of a declared non-driven member: the user's own
/// already-open interactive provider CLI session (Kimi Code, Codex, or Claude
/// Code), which Harness never spawns or drives. The session polls its Harness
/// inbox and replies through the trusted loopback CLI/MCP; there is no
/// provider-native session record, so evidence claims about this member's
/// work cannot resolve to provider-native execution truth.
pub const EXECUTION_MODE_EXTERNAL_INTERACTIVE: &str = "external_interactive";

/// How one provider member is executed by Harness. Capability claims are
/// mode-specific: `codex_exec` and `kimi_acp` are different products even when
/// their user-facing provider names are simply Codex and Kimi.
///
/// Coding-agent runtime and model routing are deliberately orthogonal. For
/// example, Pi using a DeepSeek model remains the Pi runtime provider.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentRuntimeProvider(pub String);

/// Optional model route selected inside the coding-agent runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRoute {
    pub model_provider: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub route_id: Option<String>,
    #[serde(default)]
    pub route_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapabilityStatus {
    Verified,
    ReviewRequired,
    Degraded,
    #[default]
    Unsupported,
}

/// Dependency admission of an exact resolved capability binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBindingAdmission {
    Active,
    PendingDependency,
    Degraded,
    #[default]
    Failed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapabilityEvidenceKind {
    SourceReview,
    ProtocolProbe,
    DeterministicAcceptance,
    LiveCanary,
    ProviderDocumentation,
    #[default]
    Unknown,
}

/// Evidence for one exact semantic capability. Evidence references provider
/// native or external records; they never copy a provider transcript into a
/// Harness ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapabilityEvidence {
    #[serde(default)]
    pub kind: ProviderCapabilityEvidenceKind,
    pub evidence_ref: String,
    #[serde(default)]
    pub observed_at: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Version-scoped binding from one provider-neutral semantic capability to an
/// adapter implementation. The profile-level capability fingerprint commits
/// to the ordered resolved collection of these bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapabilityBinding {
    pub capability: String,
    #[serde(default)]
    pub status: ProviderCapabilityStatus,
    #[serde(default)]
    pub admission: ProviderBindingAdmission,
    #[serde(default)]
    pub provider_version: Option<String>,
    #[serde(default)]
    pub adapter_revision: Option<String>,
    #[serde(default)]
    pub feature_fingerprint: Option<String>,
    #[serde(default)]
    pub required_dependencies: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<ProviderCapabilityEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderIntegrationProfile {
    /// Canonical runtime identity. `provider` remains as the legacy wire field
    /// while existing records and callers migrate.
    #[serde(default)]
    pub agent_runtime_provider: Option<AgentRuntimeProvider>,
    /// Model provider/route used inside the runtime, if known. This never
    /// changes which AgentRuntimeAdapter owns the native session.
    #[serde(default)]
    pub model_route: Option<ModelRoute>,
    pub provider: String,
    pub execution_mode: String,
    /// The exclusive owner allowed to start top-level provider execution
    /// cycles for this ProviderRuntimeProjection. Agent Team modes currently default to
    /// Harness-owned mailbox delivery; provider-owned continuation must be
    /// reviewed explicitly before it can be selected.
    #[serde(default)]
    pub execution_driver: MemberExecutionDriver,
    #[serde(default)]
    pub provider_version: Option<String>,
    #[serde(default)]
    pub adapter_contract_version: Option<String>,
    #[serde(default)]
    pub reviewed_provider_versions: Vec<String>,
    #[serde(default)]
    pub compatibility_status: ProviderCompatibilityStatus,
    #[serde(default)]
    pub adapter_reviewed_at: Option<String>,
    #[serde(default)]
    pub compatibility_note: Option<String>,
    pub interaction_mode: ProviderInteractionMode,
    /// When ordinary queued TeamMessages become visible to this live mode.
    /// Provider-native records remain the execution/transcript authority.
    #[serde(default)]
    pub ordinary_message_boundary: OrdinaryMessageBoundary,
    /// How this exact execution mode implements Member plan negotiation.
    #[serde(default)]
    pub plan_mode: ProviderFeatureMode,
    /// Whether the provider exposes a native session Goal that can mirror the
    /// Harness Assignment objective. Assignment remains canonical either way.
    #[serde(default)]
    pub goal_mode: ProviderFeatureMode,
    pub tool_event_fidelity: ProviderEventFidelity,
    pub artifact_event_fidelity: ProviderEventFidelity,
    pub supports_cancel: bool,
    pub supports_resume: bool,
    pub observes_native_subagents: bool,
    pub observes_background_tasks: bool,
    /// Product policy, not a provider claim. Thinking may only appear through
    /// the sanitized transient live channel and is never durable or replayed.
    pub thinking_transient_only: bool,
    /// How the adapter talks to the runtime: external wire protocol, embedded
    /// SDK, or a native in-process bridge (AgentFirm architecture review
    /// DOC-89 §11.1).
    #[serde(default)]
    pub control_topology: ControlTopology,
    /// Fingerprint of the exact resolved composition this profile claims:
    /// adapter contract + provider version + execution mode + permission
    /// mapping. A RuntimeCommand bound to a different fingerprint must not
    /// take effect against this runtime.
    #[serde(default)]
    pub composition_fingerprint: Option<String>,
    /// Fingerprint of the exact resolved capability bindings and evidence.
    #[serde(default)]
    pub capability_fingerprint: Option<String>,
    #[serde(default)]
    pub capability_bindings: Vec<ProviderCapabilityBinding>,
    /// Aggregate admission of the resolved binding set. The fail-closed
    /// default is `failed`; absent legacy data must never enable a capability.
    #[serde(default)]
    pub binding_admission: ProviderBindingAdmission,
    /// Exact adapter bridge revision this profile was reviewed against
    /// (contract version for external protocols; bridge commit for native
    /// bridges).
    #[serde(default)]
    pub adapter_bridge_revision: Option<String>,
    /// Where the permission ceiling is actually enforced. `none_verified`
    /// declares that no enforcement was proven; requesting a restricted
    /// ceiling against it must fail closed.
    #[serde(default)]
    pub security_enforcement_locus: SecurityEnforcementLocus,
}

impl Validate for ProviderIntegrationProfile {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.provider, "ProviderIntegrationProfile.provider")?;
        require_non_empty(
            &self.execution_mode,
            "ProviderIntegrationProfile.execution_mode",
        )?;
        if let Some(AgentRuntimeProvider(provider)) = &self.agent_runtime_provider {
            require_non_empty(
                provider,
                "ProviderIntegrationProfile.agent_runtime_provider",
            )?;
        }
        if let Some(route) = &self.model_route {
            require_non_empty(
                &route.model_provider,
                "ProviderIntegrationProfile.model_route.model_provider",
            )?;
        }
        if !self.capability_bindings.is_empty() && self.capability_fingerprint.is_none() {
            return Err(ValidationError::Invalid {
                field: "ProviderIntegrationProfile.capability_fingerprint",
                reason: "resolved capability bindings require an exact fingerprint",
            });
        }
        if self.binding_admission == ProviderBindingAdmission::Active
            && (self.capability_bindings.is_empty()
                || self
                    .capability_bindings
                    .iter()
                    .any(|binding| binding.admission != ProviderBindingAdmission::Active))
        {
            return Err(ValidationError::Invalid {
                field: "ProviderIntegrationProfile.binding_admission",
                reason: "active aggregate admission requires a non-empty all-active binding set",
            });
        }
        for binding in &self.capability_bindings {
            require_non_empty(
                &binding.capability,
                "ProviderIntegrationProfile.capability_bindings.capability",
            )?;
            if binding.admission == ProviderBindingAdmission::Active {
                if binding.status != ProviderCapabilityStatus::Verified {
                    return Err(ValidationError::Invalid {
                        field: "ProviderIntegrationProfile.capability_bindings.status",
                        reason: "active capability bindings must be verified",
                    });
                }
                for (value, field) in [
                    (
                        binding.provider_version.as_deref(),
                        "ProviderIntegrationProfile.capability_bindings.provider_version",
                    ),
                    (
                        binding.adapter_revision.as_deref(),
                        "ProviderIntegrationProfile.capability_bindings.adapter_revision",
                    ),
                    (
                        binding.feature_fingerprint.as_deref(),
                        "ProviderIntegrationProfile.capability_bindings.feature_fingerprint",
                    ),
                ] {
                    let value = value.ok_or(ValidationError::Invalid {
                        field,
                        reason: "active capability bindings require an exact versioned value",
                    })?;
                    require_non_empty(value, field)?;
                }
                let has_deterministic = binding.evidence.iter().any(|evidence| {
                    evidence.kind == ProviderCapabilityEvidenceKind::DeterministicAcceptance
                });
                let has_live_canary = binding
                    .evidence
                    .iter()
                    .any(|evidence| evidence.kind == ProviderCapabilityEvidenceKind::LiveCanary);
                if !has_deterministic || !has_live_canary {
                    return Err(ValidationError::Invalid {
                        field: "ProviderIntegrationProfile.capability_bindings.evidence",
                        reason: "active capability bindings require deterministic acceptance and live canary evidence",
                    });
                }
            }
            for evidence in &binding.evidence {
                require_non_empty(
                    &evidence.evidence_ref,
                    "ProviderIntegrationProfile.capability_bindings.evidence.evidence_ref",
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlTopology {
    ExternalProtocol,
    EmbeddedSdk,
    NativeBridge,
    #[default]
    Unknown,
}

/// Where a ProviderRuntimeProjection's effective permission ceiling is enforced.
/// "Generated a mapping string" is not enforcement; the locus names the real
/// mechanism or honestly reports that none was verified.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityEnforcementLocus {
    #[serde(default)]
    pub kind: SecurityEnforcementLocusKind,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEnforcementLocusKind {
    ProviderNativePolicy,
    AdapterToolAllowlist,
    /// The adapter answers the provider's permission requests (e.g. ACP
    /// auto-allow with a one-shot durable receipt).
    AdapterAutoApproval,
    OsSandbox,
    NetworkCredentialBoundary,
    NoneVerified,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrdinaryMessageBoundary {
    InTurn,
    NextRound,
    NextRoundBatched,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFeatureMode {
    Native,
    Emulated,
    Unsupported,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCompatibilityStatus {
    Current,
    ReviewRequired,
    Incompatible,
    Unavailable,
    #[default]
    Unknown,
}

/// Policy attached to one explicit provider compatibility admission.
///
/// An admission is operational authorization, not evidence that an adapter
/// was source-reviewed. In particular, callers must not copy admissions into
/// [`ProviderIntegrationProfile::reviewed_provider_versions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCompatibilityAdmissionPolicy {
    Strict,
    Advisory,
}

/// Append-only lifecycle of a provider compatibility admission key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCompatibilityAdmissionLifecycle {
    Active,
    Revoked,
    Superseded,
}

/// Store-scoped operational admission for one exact provider adapter tuple.
///
/// The compatibility key is exactly `(provider, execution_mode,
/// provider_version, adapter_contract_version)`. `project_id` and `store_id`
/// preserve the authority scope in exported or migrated evidence; the Store
/// root remains the physical isolation boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCompatibilityAdmission {
    pub id: String,
    pub project_id: String,
    pub store_id: String,
    pub provider: String,
    pub execution_mode: String,
    pub provider_version: String,
    pub adapter_contract_version: String,
    pub policy: ProviderCompatibilityAdmissionPolicy,
    pub actor: String,
    pub evidence_refs: Vec<String>,
    pub admitted_at: String,
    pub lifecycle: ProviderCompatibilityAdmissionLifecycle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predecessor_admission_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ProviderCompatibilityAdmission {
    /// Returns the exact adapter tuple authorized by this admission.
    pub fn exact_key(&self) -> (&str, &str, &str, &str) {
        (
            &self.provider,
            &self.execution_mode,
            &self.provider_version,
            &self.adapter_contract_version,
        )
    }

    /// Only an active lifecycle row grants operational compatibility.
    pub fn is_active(&self) -> bool {
        self.lifecycle == ProviderCompatibilityAdmissionLifecycle::Active
    }
}

impl Validate for ProviderCompatibilityAdmission {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderCompatibilityAdmission.id")?;
        require_non_empty(
            &self.project_id,
            "ProviderCompatibilityAdmission.project_id",
        )?;
        require_non_empty(&self.store_id, "ProviderCompatibilityAdmission.store_id")?;
        require_non_empty(&self.provider, "ProviderCompatibilityAdmission.provider")?;
        require_non_empty(
            &self.execution_mode,
            "ProviderCompatibilityAdmission.execution_mode",
        )?;
        require_non_empty(
            &self.provider_version,
            "ProviderCompatibilityAdmission.provider_version",
        )?;
        require_non_empty(
            &self.adapter_contract_version,
            "ProviderCompatibilityAdmission.adapter_contract_version",
        )?;
        require_non_empty(&self.actor, "ProviderCompatibilityAdmission.actor")?;
        require_non_empty(
            &self.admitted_at,
            "ProviderCompatibilityAdmission.admitted_at",
        )?;
        if self.evidence_refs.is_empty() {
            return Err(ValidationError::Invalid {
                field: "ProviderCompatibilityAdmission.evidence_refs",
                reason: "must contain at least one evidence reference",
            });
        }
        for evidence_ref in &self.evidence_refs {
            require_non_empty(evidence_ref, "ProviderCompatibilityAdmission.evidence_refs")?;
        }
        match self.lifecycle {
            ProviderCompatibilityAdmissionLifecycle::Active => {
                if self.predecessor_admission_id.is_some() || self.reason.is_some() {
                    return Err(ValidationError::Invalid {
                        field: "ProviderCompatibilityAdmission.lifecycle",
                        reason: "active admission cannot name a predecessor or transition reason",
                    });
                }
            }
            ProviderCompatibilityAdmissionLifecycle::Revoked
            | ProviderCompatibilityAdmissionLifecycle::Superseded => {
                let predecessor =
                    self.predecessor_admission_id
                        .as_deref()
                        .ok_or(ValidationError::Invalid {
                            field: "ProviderCompatibilityAdmission.predecessor_admission_id",
                            reason: "terminal transition must name its active predecessor",
                        })?;
                require_non_empty(
                    predecessor,
                    "ProviderCompatibilityAdmission.predecessor_admission_id",
                )?;
                let reason = self.reason.as_deref().ok_or(ValidationError::Invalid {
                    field: "ProviderCompatibilityAdmission.reason",
                    reason: "terminal transition must include a reason",
                })?;
                require_non_empty(reason, "ProviderCompatibilityAdmission.reason")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInteractionMode {
    /// The provider can pause the same turn until the client answers.
    PauseAndResume,
    /// The execution mode cannot accept mid-turn input; end the round with a
    /// blocker and start a follow-up after the Host answers.
    EndRoundAndFollowUp,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderEventFidelity {
    None,
    Summary,
    Structured,
}

/// Kind of a routed [`TeamMessageProjection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDispatchIntent {
    Message,
    /// A real runtime control record, not ordinary chat.
    Control,
    /// A provider-native turn emitted a strictly typed, correlated question.
    /// The durable product fact is this authored message; waiting remains a
    /// runtime projection rather than a second interaction lifecycle.
    ProviderInteractionRequest,
    /// The correlated answer to one [`ProviderDispatchIntent::ProviderInteractionRequest`].
    /// Its `causation_id` must point directly at the request message.
    ProviderInteractionResponse,
}

/// Closed provider callback classification. Only `Question` and `PlanReview`
/// are valid in durable correlated Message bodies. Permission callbacks are
/// classified here only so adapters can acknowledge an in-ceiling option or
/// fail closed without creating a second permission workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInteractionType {
    Question,
    ToolApproval,
    PlanReview,
    RejectOnly,
    Unknown,
}

/// One provider-native answer option carried by a correlated Message body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInteractionMessageOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
}

/// Canonical JSON body of a provider-interaction request TeamMessageProjection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInteractionRequestBody {
    #[serde(rename = "type")]
    pub interaction_type: ProviderInteractionType,
    pub prompt: String,
    pub options: Vec<ProviderInteractionMessageOption>,
    pub provider: String,
    pub provider_request_id: String,
    pub method: String,
    pub session: String,
    pub member: String,
    pub generation: u64,
}

/// Canonical JSON body of a provider-interaction response TeamMessageProjection.
/// Exactly one of `choice` and `text` is present. Choice answers are checked
/// against the request's option ids by the Store's atomic response boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInteractionResponseBody {
    #[serde(rename = "type")]
    pub interaction_type: ProviderInteractionType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub session: String,
    pub member: String,
    pub generation: u64,
}

fn require_provider_interaction_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("provider interaction {field} must not be empty"))
    } else {
        Ok(())
    }
}

impl ProviderInteractionRequestBody {
    pub fn validate(&self) -> Result<(), String> {
        require_provider_interaction_text(&self.prompt, "prompt")?;
        require_provider_interaction_text(&self.provider, "provider")?;
        require_provider_interaction_text(&self.provider_request_id, "provider_request_id")?;
        require_provider_interaction_text(&self.method, "method")?;
        require_provider_interaction_text(&self.session, "session")?;
        require_provider_interaction_text(&self.member, "member")?;
        if self.generation == 0 {
            return Err("provider interaction generation must be at least 1".to_string());
        }
        let mut option_ids = BTreeSet::new();
        for option in &self.options {
            require_provider_interaction_text(&option.id, "option id")?;
            require_provider_interaction_text(&option.label, "option label")?;
            if option
                .intent
                .as_deref()
                .is_some_and(|intent| intent.trim().is_empty())
            {
                return Err("provider interaction option intent must not be empty".to_string());
            }
            if !option_ids.insert(option.id.as_str()) {
                return Err(format!(
                    "provider interaction option id is duplicated: {}",
                    option.id
                ));
            }
        }
        if !matches!(
            self.interaction_type,
            ProviderInteractionType::Question | ProviderInteractionType::PlanReview
        ) {
            return Err(
                "only provider questions and plan reviews may become durable Messages".to_string(),
            );
        }
        if self.interaction_type == ProviderInteractionType::PlanReview && self.options.is_empty() {
            return Err("provider plan review requires at least one option".to_string());
        }
        Ok(())
    }

    pub fn to_canonical_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| error.to_string())
    }

    pub fn parse_canonical_json(body: &str) -> Result<Self, String> {
        let parsed: Self = serde_json::from_str(body).map_err(|error| error.to_string())?;
        parsed.validate()?;
        if parsed.to_canonical_json()? != body {
            return Err("provider interaction request body is not canonical JSON".to_string());
        }
        Ok(parsed)
    }

    /// Stable, unambiguous correlation derived from provider, native session,
    /// and native request id. Length prefixes avoid delimiter collisions.
    pub fn correlation_id(&self) -> String {
        format!(
            "provider-interaction:{}:{}:{}:{}:{}",
            self.provider.len(),
            self.provider,
            self.session.len(),
            self.session,
            self.provider_request_id
        )
    }
}

impl ProviderInteractionResponseBody {
    pub fn validate(&self) -> Result<(), String> {
        require_provider_interaction_text(&self.session, "session")?;
        require_provider_interaction_text(&self.member, "member")?;
        if self.generation == 0 {
            return Err("provider interaction generation must be at least 1".to_string());
        }
        match (self.choice.as_deref(), self.text.as_deref()) {
            (Some(choice), None) => require_provider_interaction_text(choice, "choice")?,
            (None, Some(text)) => require_provider_interaction_text(text, "text")?,
            (Some(_), Some(_)) => {
                return Err(
                    "provider interaction response choice and text are mutually exclusive"
                        .to_string(),
                )
            }
            (None, None) => {
                return Err(
                    "provider interaction response requires exactly one of choice or text"
                        .to_string(),
                )
            }
        }
        if self.text.is_some() && self.interaction_type != ProviderInteractionType::Question {
            return Err("only provider questions accept free text".to_string());
        }
        Ok(())
    }

    pub fn to_canonical_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| error.to_string())
    }

    pub fn parse_canonical_json(body: &str) -> Result<Self, String> {
        let parsed: Self = serde_json::from_str(body).map_err(|error| error.to_string())?;
        parsed.validate()?;
        if parsed.to_canonical_json()? != body {
            return Err("provider interaction response body is not canonical JSON".to_string());
        }
        Ok(parsed)
    }
}

/// Deterministic id of the only response allowed for one provider-interaction
/// request. The request id is length-prefixed to keep the mapping unambiguous.
pub fn provider_interaction_response_id(request_message_id: &str) -> Result<String, String> {
    require_provider_interaction_text(request_message_id, "request message id")?;
    Ok(format!(
        "provider-interaction-response:{}:{}",
        request_message_id.len(),
        request_message_id
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamActorKind {
    Host,
    ProviderRuntimeProjection,
    AgentMember,
    Operator,
    Service,
}

/// Authorship provenance for a coordination message. `authn_source` names the
/// trusted local connection or gateway that selected the actor; it never
/// contains a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamActorRef {
    pub kind: TeamActorKind,
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub authn_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRecipientKind {
    Host,
    ProviderRuntimeProjection,
    AgentMember,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRecipientRef {
    pub kind: TeamRecipientKind,
    pub id: String,
}

/// How a [`TeamMessageProjection`] should be delivered to one recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamDeliveryPolicy {
    Queue,
    Inject,
    Interrupt,
    ManualAck,
}

/// Per-recipient delivery state of a [`TeamMessageProjection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamDeliveryStatus {
    Queued,
    Claimed,
    Delivered,
    Acknowledged,
    Failed,
    Expired,
}

/// Explicit response intent carried by a [`TeamMessageProjection`] (ADR 0046 §4). A
/// transport delivery and a semantic reply are distinct facts: mail that only
/// informs or acknowledges must stay durable and correlated without starting
/// another provider round, so two Agents can converge instead of bouncing
/// acknowledgement-only mail back and forth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderResponseIntent {
    /// Durable, correlated mail that does not by itself start a provider
    /// round. It is batched into the next round some response-required
    /// message triggers, and it never fences a same-correlation Handoff.
    Informational,
    /// The sender asks for a semantic reply: an idle recipient starts a new
    /// provider round for this message, and a pending delivery fences a
    /// same-correlation Handoff as stale.
    ResponseRequired,
}

/// One recipient's delivery record inside a [`TeamMessageProjection`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDispatchAttempt {
    pub member_id: String,
    pub policy: TeamDeliveryPolicy,
    pub status: TeamDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_by_supervisor_id: Option<String>,
    #[serde(default)]
    pub claimed_generation: Option<u64>,
    #[serde(default)]
    pub claimed_unix_ms: Option<u64>,
    #[serde(default)]
    pub claim_expires_unix_ms: Option<u64>,
    /// Provider-native turn/request id returned after the selected protocol
    /// accepted this content. Absence on a claimed delivery is intentionally
    /// treated as uncertain after a Supervisor crash.
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    /// Why this delivery failed. Only set when status is
    /// [`TeamDeliveryStatus::Failed`].
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub updated_at: String,
}

/// A routed message inside an [`AgentTeamRun`]. `sender_runtime_id` is either the
/// reserved `"host"` id or a `ProviderRuntimeProjection` id. `correlation_id` groups a message
/// with its replies; `causation_id` points at the message this one answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMessageProjection {
    pub id: String,
    pub team_run_id: String,
    /// Optional Work discussed by this message. The relation is navigational
    /// and conversational only: ownership and lifecycle remain authoritative
    /// on `Work`/`WorkEvent`, never on the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_id: Option<String>,
    /// Optional pre-ADR 0051 Legacy Wave reference retained for historical
    /// message decoding only. Current messages leave this empty and relate
    /// directly to Mission and Work.
    /// It is navigation metadata only and never controls message or member
    /// lifecycle.
    #[serde(default)]
    pub source_plan_ref: Option<String>,
    /// Typed provenance for new writes. Historical rows infer it from
    /// `sender_runtime_id`.
    #[serde(default)]
    pub sender: Option<TeamActorRef>,
    pub sender_runtime_id: String,
    /// Typed recipients for new writes. `recipient_runtime_ids` remains the historical
    /// TeamRun projection.
    #[serde(default)]
    pub recipients: Vec<TeamRecipientRef>,
    #[serde(default)]
    pub recipient_runtime_ids: Vec<String>,
    pub kind: ProviderDispatchIntent,
    pub body: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    /// Explicit response intent. Absent on historical rows; the effective
    /// intent then derives from `kind` (see
    /// [`TeamMessageProjection::effective_response_intent`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_intent: Option<ProviderResponseIntent>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub deliveries: Vec<ProviderDispatchAttempt>,
    pub created_at: String,
}

impl TeamMessageProjection {
    /// Effective response intent: the explicit field always wins; otherwise
    /// kind **and sender** decide (ADR 0046 §4).
    ///
    /// Handoffs and Control records carry real review or runtime semantics and
    /// always require a response round regardless of sender. Durable work
    /// ownership lives in `Work`; messages never impersonate assignments.
    ///
    /// Ordinary `message` mail is sender-aware, because `message` is the only
    /// legal carrier for every remaining semantic category after ADR 0039
    /// retired the typed question/blocker/review kinds:
    /// - a coordination-plane sender (Host, Operator, Service) is directing the
    ///   member — questions, revisions, acceptance decisions — so it defaults to
    ///   `response_required` and wakes an idle member;
    /// - a peer member sender is confirming or informing another member, so it
    ///   defaults to `informational` and the team converges without
    ///   confirmation ping-pong.
    pub fn effective_response_intent(&self) -> ProviderResponseIntent {
        if let Some(intent) = self.response_intent {
            return intent;
        }
        match self.kind {
            ProviderDispatchIntent::Message if self.sent_by_peer_member() => {
                ProviderResponseIntent::Informational
            }
            ProviderDispatchIntent::Message
            | ProviderDispatchIntent::Control
            | ProviderDispatchIntent::ProviderInteractionRequest => {
                ProviderResponseIntent::ResponseRequired
            }
            ProviderDispatchIntent::ProviderInteractionResponse => {
                ProviderResponseIntent::Informational
            }
        }
    }

    /// True when this message was authored by another team member rather than
    /// by the coordination plane (Host, Operator, Service). Historical rows
    /// carry no typed `sender`, so they fall back to the reserved `"host"`
    /// `sender_runtime_id` convention.
    fn sent_by_peer_member(&self) -> bool {
        match self.sender.as_ref().map(|sender| sender.kind) {
            Some(TeamActorKind::ProviderRuntimeProjection) | Some(TeamActorKind::AgentMember) => {
                true
            }
            Some(TeamActorKind::Host)
            | Some(TeamActorKind::Operator)
            | Some(TeamActorKind::Service) => false,
            None => self.sender_runtime_id != "host",
        }
    }

    /// True when this message may trigger a new provider round for an idle
    /// recipient and fences a same-correlation Handoff while still pending.
    pub fn requires_response(&self) -> bool {
        self.effective_response_intent() == ProviderResponseIntent::ResponseRequired
    }

    /// Validate only the additive provider-interaction envelope. Ordinary and
    /// historical TeamMessages remain byte-for-byte compatible.
    pub fn validate_provider_interaction_contract(&self) -> Result<(), String> {
        match self.kind {
            ProviderDispatchIntent::ProviderInteractionRequest => {
                let body = ProviderInteractionRequestBody::parse_canonical_json(&self.body)?;
                if self.response_intent == Some(ProviderResponseIntent::Informational) {
                    return Err("provider interaction request must require a response".to_string());
                }
                if body.member != self.sender_runtime_id {
                    return Err(
                        "provider interaction request member must match sender_runtime_id"
                            .to_string(),
                    );
                }
                if !matches!(
                    self.sender.as_ref(),
                    Some(TeamActorRef {
                        kind: TeamActorKind::ProviderRuntimeProjection,
                        id,
                        ..
                    }) if id == &body.member
                ) {
                    return Err(
                        "provider interaction request sender must be its ProviderRuntimeProjection"
                            .to_string(),
                    );
                }
                if self.recipients.as_slice()
                    != [TeamRecipientRef {
                        kind: TeamRecipientKind::Host,
                        id: "host".to_string(),
                    }]
                    || self.recipient_runtime_ids.as_slice() != ["host"]
                    || self.deliveries.len() != 1
                    || self.deliveries[0].member_id != "host"
                {
                    return Err("provider interaction request must route only to Host".to_string());
                }
                if self.correlation_id != body.correlation_id() {
                    return Err(
                        "provider interaction request correlation_id is not provider/session/request-derived"
                            .to_string(),
                    );
                }
                if self.causation_id.is_some() {
                    return Err(
                        "provider interaction request must start its correlation without causation_id"
                            .to_string(),
                    );
                }
            }
            ProviderDispatchIntent::ProviderInteractionResponse => {
                let body = ProviderInteractionResponseBody::parse_canonical_json(&self.body)?;
                if self.response_intent == Some(ProviderResponseIntent::ResponseRequired) {
                    return Err("provider interaction response must be informational".to_string());
                }
                if self.causation_id.as_deref().is_none_or(str::is_empty) {
                    return Err(
                        "provider interaction response requires request causation_id".to_string(),
                    );
                }
                let canonical_sender = match self.sender.as_ref() {
                    Some(sender) if sender.id.trim().is_empty() => false,
                    Some(sender) if sender.kind == TeamActorKind::Host => {
                        self.sender_runtime_id == "host"
                    }
                    Some(sender) if sender.kind == TeamActorKind::Operator => {
                        self.sender_runtime_id == format!("operator:{}", sender.id)
                    }
                    Some(sender) if sender.kind == TeamActorKind::Service => {
                        self.sender_runtime_id == format!("service:{}", sender.id)
                    }
                    _ => false,
                };
                if !canonical_sender {
                    return Err(
                        "provider interaction response sender/from provenance is invalid"
                            .to_string(),
                    );
                }
                if self.recipients.as_slice()
                    != [TeamRecipientRef {
                        kind: TeamRecipientKind::ProviderRuntimeProjection,
                        id: body.member.clone(),
                    }]
                    || self.recipient_runtime_ids.as_slice() != [body.member.as_str()]
                    || self.deliveries.len() != 1
                    || self.deliveries[0].member_id != body.member
                {
                    return Err(
                        "provider interaction response must route only to its ProviderRuntimeProjection"
                            .to_string(),
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }
}
