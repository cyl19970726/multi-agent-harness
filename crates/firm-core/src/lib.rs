use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub mod agentfirm_api;
pub mod collaboration;
pub use agentfirm_api::MemberExecutionDriver;
pub use collaboration::*;

mod legacy_mission;
mod provider_capabilities;
mod provider_integration;
mod provider_launch;
mod registry;
mod team_events;
mod team_runtime;
mod validation_impls;
mod work;
mod workflow;

pub use legacy_mission::*;
pub use provider_capabilities::*;
pub use provider_integration::*;
pub use provider_launch::*;
pub use registry::*;
pub use team_events::*;
pub use team_runtime::*;
pub(crate) use validation_impls::validate_actor_metadata;
pub(crate) use work::validate_non_empty_unique_strings;
pub use work::*;
pub use workflow::*;

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("{field} is required")]
    Required { field: &'static str },
    #[error("{field} is invalid: {reason}")]
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
}

fn require_non_empty(value: &str, field: &'static str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::Required { field })
    } else {
        Ok(())
    }
}

fn require_uuid(value: &str, field: &'static str) -> Result<(), ValidationError> {
    require_non_empty(value, field)?;
    let bytes = value.as_bytes();
    let canonical = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if canonical {
        Ok(())
    } else {
        Err(ValidationError::Invalid {
            field,
            reason: "must be a canonical UUID string",
        })
    }
}

#[cfg(test)]
#[path = "lib_tests/model_contracts.rs"]
mod model_contracts;
#[cfg(test)]
#[path = "lib_tests/provider_launch_contracts.rs"]
mod provider_launch_contracts;
#[cfg(test)]
#[path = "lib_tests/provider_runtime_contracts.rs"]
mod provider_runtime_contracts;
#[cfg(test)]
#[path = "lib_tests/mod.rs"]
mod tests;
#[cfg(test)]
#[path = "lib_tests/work_contracts.rs"]
mod work_contracts;

// ── GateEngine tests ──────────────────────────────────────────

/// Skill reference resolution: maps skill_refs to SKILL.md content.
///
/// A skill is durable at `.agents/skills/<id>/SKILL.md`. This module provides
/// the contract for resolving and validating skill references (Pillar 1 skill
/// contract from docs/agent-integration-model.md).
pub mod skill_resolver;

#[cfg(test)]
mod legacy_wave_serde_tests {
    use super::*;

    #[test]
    fn mission_legacy_wave_ids_keep_the_historical_wire_key() {
        let mission = Mission {
            id: "mission-legacy".into(),
            title: "Imported Mission".into(),
            objective: "Decode historical membership".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Completed,
            legacy_wave_ids: vec!["wave-1".into()],
            outcome_summary: Some("done".into()),
            completed_by: Some("host".into()),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
            completed_at: Some("unix-ms:2".into()),
        };

        let encoded = serde_json::to_value(&mission).expect("serialize Mission");
        assert_eq!(encoded["wave_ids"], serde_json::json!(["wave-1"]));
        assert!(encoded.get("legacy_wave_ids").is_none());

        let decoded: Mission = serde_json::from_value(encoded).expect("deserialize Mission");
        assert_eq!(decoded.legacy_wave_ids, vec!["wave-1"]);
    }

    #[test]
    fn current_mission_wire_omits_empty_legacy_wave_membership() {
        let mission = Mission {
            id: "mission-current".into(),
            title: "Current Mission".into(),
            objective: "Use Mission Log".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Planned,
            legacy_wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };

        let encoded = serde_json::to_value(&mission).expect("serialize Mission");
        assert!(encoded.get("wave_ids").is_none());
    }
}
