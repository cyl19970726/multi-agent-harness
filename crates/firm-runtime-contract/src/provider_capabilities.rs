use std::collections::{BTreeMap, BTreeSet};

use harness_core::agentfirm_api::{
    AgentSession, MemberCoordinationStatus, MemberRun, NativeContinuationActivation,
    NativeContinuationProjection, RuntimeCommandBinding, RuntimeCommandPhase, RuntimeCommandRecord,
    RuntimeCommandStatus, RuntimeDriverRef, RuntimeEffectCertainty,
};
use harness_core::{
    NodeDaemonLease, NodeDaemonLeaseStatus, ProviderBindingAdmission, ProviderCapabilityBinding,
    ProviderCapabilityEvidenceKind, ProviderCapabilityStatus, TeamSupervisorLease,
    TeamSupervisorLeaseStatus,
};

use crate::RuntimeContractError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    Degraded,
    Experimental,
}

impl CapabilityStatus {
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CapabilityBinding {
    pub capability: &'static str,
    pub status: CapabilityStatus,
    pub evidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_enforcement_locus: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticCapability {
    OpenOrResume,
    StartCycle,
    InjectCurrentCycle,
    QueueNativeBoundary,
    Interrupt,
    /// Reversible Team-member runtime shutdown. This closes only the owned
    /// adapter/process handle and retains the provider-native session for
    /// Reopen. It is deliberately weaker than Quiesce + Release, which prove
    /// workspace/queue/flush postconditions for driver or composition change.
    CloseRuntime,
    Observe,
    InspectEffect,
    Reconcile,
    InspectContinuation,
    InhibitContinuation,
    ResumeContinuation,
    Quiesce,
    Release,
}

impl SemanticCapability {
    pub const ALL: [Self; 14] = [
        Self::OpenOrResume,
        Self::StartCycle,
        Self::InjectCurrentCycle,
        Self::QueueNativeBoundary,
        Self::Interrupt,
        Self::CloseRuntime,
        Self::Observe,
        Self::InspectEffect,
        Self::Reconcile,
        Self::InspectContinuation,
        Self::InhibitContinuation,
        Self::ResumeContinuation,
        Self::Quiesce,
        Self::Release,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenOrResume => "open_or_resume",
            Self::StartCycle => "start_cycle",
            Self::InjectCurrentCycle => "inject_current_cycle",
            Self::QueueNativeBoundary => "queue_at_native_boundary",
            Self::Interrupt => "interrupt_current_cycle",
            Self::CloseRuntime => "close_runtime",
            Self::Observe => "observe",
            Self::InspectEffect => "inspect_effect",
            Self::Reconcile => "reconcile_effect",
            Self::InspectContinuation => "inspect_continuation",
            Self::InhibitContinuation => "inhibit_continuation",
            Self::ResumeContinuation => "resume_continuation",
            Self::Quiesce => "quiesce",
            Self::Release => "release",
        }
    }
}

impl std::fmt::Display for SemanticCapability {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionDecision {
    pub capability: SemanticCapability,
    pub admission: ProviderBindingAdmission,
    pub required_closure: Vec<String>,
    pub reasons: Vec<String>,
}

impl AdmissionDecision {
    fn allows_effect(&self) -> bool {
        matches!(
            self.admission,
            ProviderBindingAdmission::Active | ProviderBindingAdmission::Degraded
        )
    }
}

/// Process-local resolver over canonical [`ProviderCapabilityBinding`] rows.
/// It never creates a second capability profile.
pub struct CapabilityResolver<'a> {
    bindings: BTreeMap<&'a str, &'a ProviderCapabilityBinding>,
}

impl<'a> CapabilityResolver<'a> {
    pub fn new(bindings: &'a [ProviderCapabilityBinding]) -> Result<Self, RuntimeContractError> {
        let mut indexed = BTreeMap::new();
        for binding in bindings {
            if indexed
                .insert(binding.capability.as_str(), binding)
                .is_some()
            {
                return Err(RuntimeContractError::InvalidCapabilityBindings(format!(
                    "duplicate capability binding: {}",
                    binding.capability
                )));
            }
        }
        Ok(Self { bindings: indexed })
    }

    /// Resolve the required transitive closure and optional dependencies.
    /// Required nodes must be verified and executable; missing/review-required
    /// nodes deny the operation before a provider effect. Optional gaps lower
    /// the result to degraded without granting any required capability.
    pub fn admit(
        &self,
        capability: SemanticCapability,
        optional: &[SemanticCapability],
    ) -> AdmissionDecision {
        let mut closure = BTreeSet::new();
        let mut stack = Vec::new();
        let mut reasons = Vec::new();
        let mut hard = HardAdmission::Verified;
        let mut degraded = false;
        self.walk_required(
            capability.as_str(),
            &mut closure,
            &mut stack,
            &mut hard,
            &mut degraded,
            &mut reasons,
        );

        if matches!(hard, HardAdmission::Verified) {
            for optional_capability in optional {
                let mut optional_closure = BTreeSet::new();
                let mut optional_stack = Vec::new();
                let mut optional_hard = HardAdmission::Verified;
                let mut optional_degraded = false;
                let mut optional_reasons = Vec::new();
                self.walk_required(
                    optional_capability.as_str(),
                    &mut optional_closure,
                    &mut optional_stack,
                    &mut optional_hard,
                    &mut optional_degraded,
                    &mut optional_reasons,
                );
                if !matches!(optional_hard, HardAdmission::Verified) || optional_degraded {
                    degraded = true;
                    reasons.push(format!(
                        "optional capability {} is not fully verified",
                        optional_capability
                    ));
                }
            }
        }

        let admission = match hard {
            HardAdmission::Verified if degraded => ProviderBindingAdmission::Degraded,
            HardAdmission::Verified => ProviderBindingAdmission::Active,
            HardAdmission::Pending => ProviderBindingAdmission::PendingDependency,
            HardAdmission::Failed => ProviderBindingAdmission::Failed,
        };
        AdmissionDecision {
            capability,
            admission,
            required_closure: closure.into_iter().map(str::to_string).collect(),
            reasons,
        }
    }

    pub fn require_effect(
        &self,
        capability: SemanticCapability,
        optional: &[SemanticCapability],
    ) -> Result<AdmissionDecision, RuntimeContractError> {
        let decision = self.admit(capability, optional);
        if decision.allows_effect() {
            Ok(decision)
        } else {
            Err(RuntimeContractError::CapabilityAdmissionDenied {
                capability,
                admission: decision.admission,
                reasons: decision.reasons,
            })
        }
    }

    fn walk_required(
        &self,
        capability: &'a str,
        closure: &mut BTreeSet<&'a str>,
        stack: &mut Vec<&'a str>,
        hard: &mut HardAdmission,
        degraded: &mut bool,
        reasons: &mut Vec<String>,
    ) {
        if let Some(cycle_start) = stack.iter().position(|item| *item == capability) {
            let mut cycle = stack[cycle_start..].to_vec();
            cycle.push(capability);
            reasons.push(format!("required dependency cycle: {}", cycle.join(" -> ")));
            *hard = HardAdmission::Failed;
            return;
        }
        if closure.contains(capability) {
            return;
        }
        closure.insert(capability);

        let Some(binding) = self.bindings.get(capability).copied() else {
            reasons.push(format!("required capability {capability} is missing"));
            *hard = HardAdmission::Failed;
            return;
        };
        match binding.status {
            ProviderCapabilityStatus::Verified => {
                if binding.evidence.is_empty()
                    || binding
                        .evidence
                        .iter()
                        .all(|item| item.evidence_ref.trim().is_empty())
                {
                    reasons.push(format!(
                        "verified capability {capability} has no evidence reference"
                    ));
                    *hard = HardAdmission::Failed;
                    return;
                }
                if binding.admission == ProviderBindingAdmission::Active {
                    let has_deterministic = binding.evidence.iter().any(|evidence| {
                        evidence.kind == ProviderCapabilityEvidenceKind::DeterministicAcceptance
                    });
                    let has_live_canary = binding.evidence.iter().any(|evidence| {
                        evidence.kind == ProviderCapabilityEvidenceKind::LiveCanary
                    });
                    if !has_deterministic || !has_live_canary {
                        reasons.push(format!(
                            "active capability {capability} lacks deterministic acceptance or live canary evidence"
                        ));
                        *hard = HardAdmission::Failed;
                        return;
                    }
                }
            }
            ProviderCapabilityStatus::ReviewRequired => {
                reasons.push(format!("required capability {capability} awaits review"));
                if !matches!(*hard, HardAdmission::Failed) {
                    *hard = HardAdmission::Pending;
                }
                return;
            }
            ProviderCapabilityStatus::Degraded | ProviderCapabilityStatus::Unsupported => {
                reasons.push(format!(
                    "required capability {capability} is {:?}",
                    binding.status
                ));
                *hard = HardAdmission::Failed;
                return;
            }
        }
        match binding.admission {
            ProviderBindingAdmission::Active => {}
            ProviderBindingAdmission::Degraded => *degraded = true,
            ProviderBindingAdmission::PendingDependency => {
                reasons.push(format!(
                    "required capability {capability} is pending dependency"
                ));
                if !matches!(*hard, HardAdmission::Failed) {
                    *hard = HardAdmission::Pending;
                }
                return;
            }
            ProviderBindingAdmission::Failed => {
                reasons.push(format!("required capability {capability} failed admission"));
                *hard = HardAdmission::Failed;
                return;
            }
        }

        stack.push(capability);
        for dependency in &binding.required_dependencies {
            let Some(canonical_name) = self
                .bindings
                .get_key_value(dependency.as_str())
                .map(|(k, _)| *k)
            else {
                reasons.push(format!(
                    "required capability {} for {capability} is missing",
                    dependency
                ));
                *hard = HardAdmission::Failed;
                continue;
            };
            self.walk_required(canonical_name, closure, stack, hard, degraded, reasons);
        }
        stack.pop();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HardAdmission {
    Verified,
    Pending,
    Failed,
}

// ---------------------------------------------------------------------------
// Exact command fence over canonical durable types
// ---------------------------------------------------------------------------

macro_rules! generation_type {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            fn require(value: u64, fields: &mut Vec<String>) -> Option<Self> {
                if value == 0 {
                    fields.push($field.to_string());
                    None
                } else {
                    Some(Self(value))
                }
            }

            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

generation_type!(MemberRunGeneration, "member_run.runtime_generation");
generation_type!(AgentSessionGeneration, "agent_session.runtime_generation");
generation_type!(NodeDaemonGeneration, "node_daemon.generation");
generation_type!(TeamSupervisorGeneration, "team_supervisor.generation");
generation_type!(RuntimeDriverGeneration, "agent_session.driver_generation");

/// Process-local proof that one already-admitted durable RuntimeCommand still
/// targets the exact runtime authority observed at admission. Private fields
/// deliberately prevent provider callers from rebuilding authority with a
/// struct literal.
#[derive(Debug, Clone)]
pub struct RuntimeBindingFence {
    command_id: String,
    binding: RuntimeCommandBinding,
    target_node_daemon_id: String,
    target_node_daemon_generation: NodeDaemonGeneration,
    member_run_id: String,
    member_run_generation: MemberRunGeneration,
    agent_session_generation: AgentSessionGeneration,
    driver_generation: RuntimeDriverGeneration,
    team_supervisor_generation: Option<TeamSupervisorGeneration>,
}

impl RuntimeBindingFence {
    fn exact_mismatch_fields(
        binding: &RuntimeCommandBinding,
        target_node_daemon_id: &str,
        target_node_daemon_generation: u64,
        session: &AgentSession,
    ) -> Vec<String> {
        let mut fields = Vec::new();
        if binding.target_session_id.as_deref() != Some(session.id.as_str()) {
            fields.push("target_session_id".to_string());
        }
        if binding.target_runtime_generation != Some(session.runtime_generation) {
            fields.push("target_runtime_generation".to_string());
        }
        if binding.target_driver_generation != Some(session.control_state.driver_generation) {
            fields.push("target_driver_generation".to_string());
        }
        if binding.target_driver != session.control_state.driver_ref {
            fields.push("target_driver".to_string());
        }
        if target_node_daemon_id != session.node_daemon_id {
            fields.push("target_node_daemon_id".to_string());
        }
        if target_node_daemon_generation != session.node_daemon_generation {
            fields.push("target_node_daemon_generation".to_string());
        }
        if binding.composition_fingerprint.as_deref()
            != session.control_state.composition_fingerprint.as_deref()
        {
            fields.push("composition_fingerprint".to_string());
        }
        if binding.capability_fingerprint.as_deref()
            != session.control_state.capability_fingerprint.as_deref()
        {
            fields.push("capability_fingerprint".to_string());
        }
        if binding.permission_envelope_ref.as_deref()
            != Some(session.permission_envelope_ref.as_str())
        {
            fields.push("permission_envelope_ref".to_string());
        }
        if binding.native_session_ref.as_ref() != session.native_session_ref.as_ref() {
            fields.push("native_session_ref".to_string());
        }
        fields
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_admitted_command(
        command: &RuntimeCommandRecord,
        session: &AgentSession,
        member_run: &MemberRun,
        node_daemon: &NodeDaemonLease,
        team_supervisor: Option<&TeamSupervisorLease>,
        now_unix_ms: u64,
    ) -> Result<Self, RuntimeContractError> {
        let mut fields = Vec::new();
        if command.status != RuntimeCommandStatus::Accepted
            || command.phase != RuntimeCommandPhase::Prepared
            || command.effect_certainty != RuntimeEffectCertainty::Unknown
        {
            fields.push("runtime_command.admission_state".to_string());
        }
        if command.target_session_id.as_deref() != Some(session.id.as_str())
            || command.target_session_generation != Some(session.runtime_generation)
        {
            fields.push("runtime_command.target_session".to_string());
        }
        if command.binding.target_member_run_id.as_deref() != Some(member_run.id.as_str())
            || command.binding.target_member_run_generation != Some(member_run.runtime_generation)
        {
            fields.push("runtime_command.target_member_run".to_string());
        }
        if command.execution_space_id != session.execution_space_id
            || command.target_node_id != session.node_id
        {
            fields.push("runtime_command.session_scope".to_string());
        }
        if node_daemon.status != NodeDaemonLeaseStatus::Active
            || node_daemon.expires_unix_ms <= now_unix_ms
            || node_daemon.node_id != session.node_id
            || node_daemon.daemon_id != session.node_daemon_id
            || node_daemon.generation != session.node_daemon_generation
            || command.target_node_daemon_id != node_daemon.daemon_id
            || command.target_node_daemon_generation != node_daemon.generation
        {
            fields.push("node_daemon.current_lease".to_string());
        }
        if member_run.coordination_status != MemberCoordinationStatus::Active {
            fields.push("member_run.coordination_status".to_string());
        }
        if member_run.agent_member_id != session.agent_member_id {
            fields.push("member_run.agent_member_id".to_string());
        }

        let team_supervisor_generation = match &session.control_state.driver_ref {
            RuntimeDriverRef::TeamSupervisor {
                team_run_id,
                team_supervisor_id,
                team_supervisor_generation,
            } => match team_supervisor {
                Some(lease)
                    if lease.status == TeamSupervisorLeaseStatus::Active
                        && lease.expires_unix_ms > now_unix_ms
                        && lease.team_run_id == *team_run_id
                        && lease.team_run_id == member_run.team_run_id
                        && lease.supervisor_id == *team_supervisor_id
                        && lease.generation == *team_supervisor_generation
                        && lease.execution_space_id == session.execution_space_id
                        && lease.node_id == session.node_id
                        && lease.node_daemon_id == node_daemon.daemon_id
                        && lease.node_daemon_generation == node_daemon.generation =>
                {
                    TeamSupervisorGeneration::require(lease.generation, &mut fields)
                }
                _ => {
                    fields.push("team_supervisor.current_lease".to_string());
                    None
                }
            },
            _ => None,
        };

        let target_node_daemon_generation =
            NodeDaemonGeneration::require(node_daemon.generation, &mut fields);
        let member_run_generation =
            MemberRunGeneration::require(member_run.runtime_generation, &mut fields);
        let agent_session_generation =
            AgentSessionGeneration::require(session.runtime_generation, &mut fields);
        let driver_generation =
            RuntimeDriverGeneration::require(session.control_state.driver_generation, &mut fields);

        fields.extend(Self::exact_mismatch_fields(
            &command.binding,
            &command.target_node_daemon_id,
            command.target_node_daemon_generation,
            session,
        ));
        fields.sort();
        fields.dedup();
        if !fields.is_empty() {
            return Err(RuntimeContractError::FenceMismatch { fields });
        }
        Ok(Self {
            command_id: command.id.clone(),
            binding: command.binding.clone(),
            target_node_daemon_id: command.target_node_daemon_id.clone(),
            target_node_daemon_generation: target_node_daemon_generation
                .expect("nonzero NodeDaemon generation was validated"),
            member_run_id: member_run.id.clone(),
            member_run_generation: member_run_generation
                .expect("nonzero MemberRun generation was validated"),
            agent_session_generation: agent_session_generation
                .expect("nonzero AgentSession generation was validated"),
            driver_generation: driver_generation
                .expect("nonzero runtime driver generation was validated"),
            team_supervisor_generation,
        })
    }

    pub fn command_id(&self) -> &str {
        &self.command_id
    }

    pub fn member_run_id(&self) -> &str {
        &self.member_run_id
    }

    pub fn member_run_generation(&self) -> MemberRunGeneration {
        self.member_run_generation
    }

    pub fn agent_session_generation(&self) -> AgentSessionGeneration {
        self.agent_session_generation
    }

    pub fn node_daemon_generation(&self) -> NodeDaemonGeneration {
        self.target_node_daemon_generation
    }

    pub fn driver_generation(&self) -> RuntimeDriverGeneration {
        self.driver_generation
    }

    pub fn team_supervisor_generation(&self) -> Option<TeamSupervisorGeneration> {
        self.team_supervisor_generation
    }

    pub fn validate_exact(&self, session: &AgentSession) -> Result<(), RuntimeContractError> {
        let fields = Self::exact_mismatch_fields(
            &self.binding,
            &self.target_node_daemon_id,
            self.target_node_daemon_generation.get(),
            session,
        );
        if fields.is_empty() {
            Ok(())
        } else {
            Err(RuntimeContractError::FenceMismatch { fields })
        }
    }
}

/// Reject a stale continuation definition or process-local activation before
/// compiling continuation control into a native operation.
pub(crate) fn validate_continuation_exact(
    expected: &NativeContinuationProjection,
    session: &AgentSession,
) -> Result<(), RuntimeContractError> {
    let current = &session.control_state.continuation;
    let mut fields = Vec::new();
    if expected.definition.continuation_ref != current.definition.continuation_ref {
        fields.push("continuation.definition.continuation_ref".to_string());
    }
    if expected.definition.revision != current.definition.revision {
        fields.push("continuation.definition.revision".to_string());
    }
    if expected.definition.phase != current.definition.phase {
        fields.push("continuation.definition.phase".to_string());
    }
    if expected.definition.budget != current.definition.budget {
        fields.push("continuation.definition.budget".to_string());
    }
    if expected.activation != current.activation {
        fields.push("continuation.activation".to_string());
    }
    if let NativeContinuationActivation::Armed {
        runtime_generation,
        driver_generation,
    } = &expected.activation
    {
        if *runtime_generation != session.runtime_generation {
            fields.push("continuation.activation.runtime_generation".to_string());
        }
        if *driver_generation != session.control_state.driver_generation {
            fields.push("continuation.activation.driver_generation".to_string());
        }
    }
    if fields.is_empty() {
        Ok(())
    } else {
        Err(RuntimeContractError::StaleContinuation { fields })
    }
}

// ---------------------------------------------------------------------------
// Provider-neutral operations and receipts
// ---------------------------------------------------------------------------
