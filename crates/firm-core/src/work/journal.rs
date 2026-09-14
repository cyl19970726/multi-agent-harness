//! What names a Work revision, and what carries it.
//!
//! One journal needs one transition name per event kind and one scope type for
//! the readers that narrow: both live here, next to nothing else, so a writer
//! and a reader cannot end up holding different tables.
use super::{TeamActorKind, WorkEvent, WorkEventKind};
use serde::{Deserialize, Serialize};

impl WorkEventKind {
    /// The canonical `work` aggregate transition name this event kind commits
    /// as, and reads back as.
    ///
    /// One journal needs one name per transition, and the writer and the
    /// reader must never disagree about it: a name only the writer knows is a
    /// Work revision the reader cannot label, and the Work journal fails
    /// closed on exactly that. Both directions therefore live here, next to
    /// the kinds, instead of in two private tables on opposite sides of the
    /// store.
    pub const fn canonical_transition(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Assigned => "assigned",
            Self::Claimed => "claimed",
            Self::Started => "started",
            Self::Released => "released",
            Self::Blocked => "blocked",
            Self::Resumed => "resumed",
            Self::Submitted => "submitted",
            Self::ChangesRequested => "changes_requested",
            Self::Accepted => "accepted",
            Self::Cancelled => "cancelled",
            Self::Updated => "updated",
            Self::DependenciesChanged => "dependencies_changed",
            Self::Rebound => "rebound",
            Self::ExecutionRetargeted => "execution_retargeted",
            Self::ExecutionRecovered => "execution_recovered",
        }
    }

    /// The event kind a canonical `work` transition names, or `None` when this
    /// binary cannot read that transition as a WorkEvent honestly.
    ///
    /// The one retired transition needs no arm: a pre-W1 `failed` envelope
    /// carries `resolution: "failed"`, which `WorkResolution` no longer
    /// decodes, so the Work projection refuses it before this map is asked.
    pub fn from_canonical_transition(transition: &str) -> Option<Self> {
        Some(match transition {
            "created" => Self::Created,
            "assigned" => Self::Assigned,
            "claimed" => Self::Claimed,
            "started" => Self::Started,
            "released" => Self::Released,
            "blocked" => Self::Blocked,
            "resumed" => Self::Resumed,
            "submitted" => Self::Submitted,
            "changes_requested" => Self::ChangesRequested,
            "accepted" => Self::Accepted,
            "cancelled" => Self::Cancelled,
            "updated" => Self::Updated,
            "dependencies_changed" => Self::DependenciesChanged,
            "rebound" => Self::Rebound,
            "execution_retargeted" => Self::ExecutionRetargeted,
            "execution_recovered" => Self::ExecutionRecovered,
            _ => return None,
        })
    }
}

impl WorkEvent {
    /// The MemberRun generation that executed this transition, in either
    /// persisted shape: the explicit evidence field a current writer records,
    /// or the legacy performer whose kind *was* the runtime projection.
    ///
    /// One accessor, because the two shapes must never diverge across the
    /// predicates that read member provenance.
    pub fn executing_member_run_id(&self) -> Option<&str> {
        if let Some(member_run_id) = self.executed_by_member_run_id.as_deref() {
            return Some(member_run_id);
        }
        (self.performed_by_actor.kind == TeamActorKind::ProviderRuntimeProjection)
            .then_some(self.performed_by_actor.id.as_str())
    }
}

/// The identity of one Execution Space.
///
/// A Work reader that narrows to a scope and a Work reader that narrows to a
/// TeamRun are different questions with the same-shaped answer, and both ids
/// are bare strings. That has already cost one reviewer a misread of two
/// same-named `current_work` functions, so the scope is a type: a TeamRun id
/// no longer compiles where an Execution Space is required.
///
/// Construction is deliberately explicit. There is no `From<&str>`, because an
/// implicit conversion would put the same silent mistake back — the whole
/// point is that turning a string into a scope is a claim the caller makes on
/// purpose.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExecutionSpaceId(String);

impl ExecutionSpaceId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for ExecutionSpaceId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ExecutionSpaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
