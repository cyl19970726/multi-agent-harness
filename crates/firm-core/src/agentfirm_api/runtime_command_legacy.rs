//! Historical wire compatibility only. Current command writers and decisions
//! have one phase; the original journal envelope is never rewritten on read.
use super::{RuntimeCommandPhase, RuntimeCommandRecord};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyStatus {
    Requested,
    Accepted,
    Quiesced,
    Applied,
    Failed,
    RecoveryRequired,
}

impl LegacyStatus {
    fn fold(self, phase: Option<RuntimeCommandPhase>) -> RuntimeCommandPhase {
        use RuntimeCommandPhase as Phase;
        let expected = match self {
            Self::Requested => Phase::Unknown,
            Self::Accepted => Phase::Prepared,
            Self::Quiesced => Phase::Observed,
            Self::Applied => Phase::Settled,
            Self::Failed => Phase::Rejected,
            Self::RecoveryRequired => Phase::RecoveryRequired,
        };
        match phase {
            Some(phase) if phase == expected => phase,
            Some(_) => Phase::Unknown,
            // A transport acceptance without its phase is not permission to drive.
            None if expected == Phase::Prepared => Phase::Unknown,
            None => expected,
        }
    }
}

impl Serialize for RuntimeCommandRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serde's remote derive keeps the current field definition in one place.
        RuntimeCommandRecord::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for RuntimeCommandRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = serde_json::Value::deserialize(deserializer)?;
        if let Some(object) = value.as_object_mut() {
            if let Some(status) = object.remove("status") {
                let status: LegacyStatus =
                    serde_json::from_value(status).map_err(serde::de::Error::custom)?;
                let phase = object
                    .get("phase")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                object.insert(
                    "phase".into(),
                    serde_json::to_value(status.fold(phase)).map_err(serde::de::Error::custom)?,
                );
            } else if !object.contains_key("phase") {
                return Err(serde::de::Error::missing_field("phase"));
            }
        }
        RuntimeCommandRecord::deserialize(value).map_err(serde::de::Error::custom)
    }
}
