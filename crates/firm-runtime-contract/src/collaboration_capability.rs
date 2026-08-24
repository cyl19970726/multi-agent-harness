use std::fmt;

use sha2::{Digest, Sha256};

/// The only product authority granted by the process-local bearer secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationCapabilityScope {
    ExactSelfRoleActions,
}

/// Reviewed provider seam that carries the capability to an agent tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationCapabilityMechanism {
    DirectAgentToolEnvironment,
    ClaudeSdkToolEnvironment,
    KimiAcpToolEnvironment,
    PiRpcToolEnvironment,
    DeepSeekCordisShellEnv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationCapabilityExpiry {
    LiveSupervisorRegistration,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CollaborationCapabilityBinding {
    pub team_run_id: String,
    pub member_run_id: String,
    pub member_run_generation: u64,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub supervisor_id: String,
    pub supervisor_generation: u64,
}

/// Bearer material is deliberately neither `Clone` nor `Serialize`.
pub struct CollaborationCapabilitySecret(String);

impl CollaborationCapabilitySecret {
    pub fn new(value: String) -> Result<Self, CollaborationCapabilityError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CollaborationCapabilityError::InvalidSecret);
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.0.as_bytes());
        let encoded = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{encoded}")
    }
}

/// Process-local environment that contains the bearer capability.
///
/// The wrapper is deliberately neither `Clone` nor `Serialize`; its `Debug`
/// representation exposes only variable names. Provider launch code may
/// borrow the pairs long enough to spawn the owned runtime, but cannot obtain
/// a second owned environment through this API.
pub struct CollaborationCapabilityEnvironment(Vec<(String, String)>);

impl CollaborationCapabilityEnvironment {
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn extend_non_secret<I>(&mut self, values: I)
    where
        I: IntoIterator<Item = (String, String)>,
    {
        self.0.extend(values);
    }

    pub fn as_pairs(&self) -> &[(String, String)] {
        &self.0
    }
}

impl fmt::Debug for CollaborationCapabilityEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CollaborationCapabilityEnvironment")
            .field(
                "names",
                &self.0.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            )
            .field("values", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for CollaborationCapabilitySecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CollaborationCapabilitySecret([REDACTED])")
    }
}

pub struct CollaborationCapabilityEnvelope {
    secret: CollaborationCapabilitySecret,
    pub binding: CollaborationCapabilityBinding,
    pub scope: CollaborationCapabilityScope,
    pub expiry: CollaborationCapabilityExpiry,
    pub fingerprint: String,
    pub mechanism: CollaborationCapabilityMechanism,
}

impl fmt::Debug for CollaborationCapabilityEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CollaborationCapabilityEnvelope")
            .field("secret", &"[REDACTED]")
            .field("binding", &self.binding)
            .field("scope", &self.scope)
            .field("expiry", &self.expiry)
            .field("fingerprint", &self.fingerprint)
            .field("mechanism", &self.mechanism)
            .finish()
    }
}

impl CollaborationCapabilityEnvelope {
    pub fn new(
        secret: CollaborationCapabilitySecret,
        binding: CollaborationCapabilityBinding,
        mechanism: CollaborationCapabilityMechanism,
    ) -> Result<Self, CollaborationCapabilityError> {
        let fingerprint = secret.fingerprint();
        Ok(Self {
            secret,
            binding,
            scope: CollaborationCapabilityScope::ExactSelfRoleActions,
            expiry: CollaborationCapabilityExpiry::LiveSupervisorRegistration,
            fingerprint,
            mechanism,
        })
    }

    pub fn secret(&self) -> &str {
        self.secret.expose()
    }

    /// Deliberately closed environment: no ambient parent secret is accepted.
    pub fn provider_environment(&self) -> CollaborationCapabilityEnvironment {
        CollaborationCapabilityEnvironment(vec![
            ("FIRM_TEAM_RUN_ID".into(), self.binding.team_run_id.clone()),
            (
                "FIRM_MEMBER_RUN_ID".into(),
                self.binding.member_run_id.clone(),
            ),
            ("FIRM_MEMBER_ROLE_ACTION_TOKEN".into(), self.secret().into()),
        ])
    }

    pub fn agent_tool_environment(
        &self,
        expected_mechanism: CollaborationCapabilityMechanism,
    ) -> Result<CollaborationCapabilityEnvironment, CollaborationCapabilityError> {
        if self.mechanism != expected_mechanism {
            return Err(CollaborationCapabilityError::MechanismMismatch);
        }
        Ok(self.provider_environment())
    }

    pub fn validate_current(
        &self,
        expected: &CollaborationCapabilityBinding,
        supervisor_registration_live: bool,
    ) -> Result<(), CollaborationCapabilityError> {
        if !supervisor_registration_live {
            return Err(CollaborationCapabilityError::Expired);
        }
        if &self.binding != expected {
            return Err(CollaborationCapabilityError::BindingMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CollaborationCapabilityError {
    #[error("collaboration capability secret must be exactly 32 bytes of hexadecimal entropy")]
    InvalidSecret,
    #[error("collaboration capability expired with its Supervisor lease")]
    Expired,
    #[error("collaboration capability binding does not match the current runtime fence")]
    BindingMismatch,
    #[error("collaboration capability transport mechanism does not match the provider adapter")]
    MechanismMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> CollaborationCapabilityBinding {
        CollaborationCapabilityBinding {
            team_run_id: "team-run-1".into(),
            member_run_id: "member-run-1".into(),
            member_run_generation: 2,
            agent_session_id: "agent-session-1".into(),
            agent_session_generation: 3,
            node_daemon_id: "daemon-1".into(),
            node_daemon_generation: 4,
            supervisor_id: "supervisor-1".into(),
            supervisor_generation: 5,
        }
    }

    fn envelope() -> CollaborationCapabilityEnvelope {
        CollaborationCapabilityEnvelope::new(
            CollaborationCapabilitySecret::new("ab".repeat(32)).unwrap(),
            binding(),
            CollaborationCapabilityMechanism::DirectAgentToolEnvironment,
        )
        .unwrap()
    }

    #[test]
    fn secret_is_redacted_and_provider_environment_is_closed() {
        let envelope = envelope();
        let debug = format!("{envelope:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains(&"ab".repeat(32)));

        let environment = envelope.provider_environment();
        let environment_debug = format!("{environment:?}");
        assert!(environment_debug.contains("[REDACTED]"));
        assert!(!environment_debug.contains(&"ab".repeat(32)));
        let environment = environment.as_pairs();
        assert_eq!(environment.len(), 3);
        assert_eq!(
            environment[0],
            ("FIRM_TEAM_RUN_ID".into(), "team-run-1".into())
        );
        assert_eq!(
            environment[1],
            ("FIRM_MEMBER_RUN_ID".into(), "member-run-1".into())
        );
        assert_eq!(environment[2].0, "FIRM_MEMBER_ROLE_ACTION_TOKEN");
        assert_eq!(environment[2].1, "ab".repeat(32));
        assert!(environment.iter().all(|(name, _)| !matches!(
            name.as_str(),
            "AWS_SECRET_ACCESS_KEY" | "GITHUB_TOKEN" | "DATABASE_PASSWORD"
        )));
    }

    #[test]
    fn expiry_and_exact_runtime_binding_fail_closed() {
        let envelope = envelope();
        assert_eq!(envelope.validate_current(&binding(), true), Ok(()));
        assert_eq!(
            envelope.validate_current(&binding(), false),
            Err(CollaborationCapabilityError::Expired)
        );
        let mut stale = binding();
        stale.supervisor_generation += 1;
        assert_eq!(
            envelope.validate_current(&stale, true),
            Err(CollaborationCapabilityError::BindingMismatch)
        );
    }

    #[test]
    fn invalid_secret_is_rejected() {
        assert_eq!(
            CollaborationCapabilitySecret::new("not-a-secret".into()).unwrap_err(),
            CollaborationCapabilityError::InvalidSecret
        );
    }

    #[test]
    fn provider_mechanism_mismatch_fails_before_environment_export() {
        assert_eq!(
            envelope()
                .agent_tool_environment(CollaborationCapabilityMechanism::PiRpcToolEnvironment)
                .unwrap_err(),
            CollaborationCapabilityError::MechanismMismatch
        );
    }
}
