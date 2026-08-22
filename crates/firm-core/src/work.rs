use super::*;

mod dependency;
mod lifecycle;

pub use dependency::*;
pub use lifecycle::*;

/// Agent Team Work is durable responsibility inside one AgentTeam. A
/// `team_run_id` is the current execution attempt, not the Work's lifetime.
/// WorkEvent is the append-only authority; this row is the latest rebuildable
/// projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPhase {
    Open,
    Active,
    Review,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkCondition {
    Normal,
    Blocked,
    OnHold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkResolution {
    Accepted,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkClaimMode {
    HostAssign,
    TeamClaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPriority {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRef {
    pub team_run_id: String,
    pub work_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCausationRef {
    pub kind: String,
    pub id: String,
}

/// Immutable explanation of why a Work cannot currently progress normally.
/// The Work row only points at the active record; resolving a condition stamps
/// `resolved_at` instead of rewriting the original diagnosis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkConditionRecord {
    pub id: String,
    pub work_id: String,
    pub work_version: u64,
    pub condition: WorkCondition,
    pub owner_actor: TeamActorRef,
    pub impact: String,
    pub resume_condition: String,
    #[serde(default)]
    pub next_check_at: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub supersedes_condition_record_id: Option<String>,
}

/// Immutable submission for one exact Work revision and, when applicable,
/// one exact source/candidate revision pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkReport {
    pub id: String,
    pub work_id: String,
    /// Exact Work projection version produced by this submission report.
    pub work_version: u64,
    pub report_revision: u64,
    pub submitted_by_actor: TeamActorRef,
    #[serde(default)]
    pub base_revision: Option<String>,
    /// Exact immutable candidate identifier. Code submissions should use the
    /// source revision; other submissions use the canonical content digest.
    pub candidate_revision: String,
    pub result_summary: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub check_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub known_risks: Vec<String>,
    pub created_at: String,
}

/// Immutable evidence binding one WorkReport to its exact candidate revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkEvidence {
    pub id: String,
    pub work_id: String,
    pub work_report_id: String,
    pub work_version: u64,
    pub candidate_revision: String,
    pub source_type: String,
    pub source_ref: String,
    pub summary: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDecisionKind {
    Accept,
    Revise,
    Cancel,
    Fail,
    WaiveGate,
}

/// Immutable Host/Operator decision. Store operations validate authority and
/// apply the resulting Work transition atomically with this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOperationalDecision {
    pub id: String,
    pub work_id: String,
    pub expected_work_version: u64,
    pub kind: WorkDecisionKind,
    pub decided_by_actor: TeamActorRef,
    pub rationale: String,
    #[serde(default)]
    pub work_report_id: Option<String>,
    #[serde(default)]
    pub gate_requirement_ref: Option<String>,
    #[serde(default)]
    pub failure_analysis_ref: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub created_at: String,
}

impl Validate for WorkConditionRecord {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkConditionRecord.id")?;
        require_non_empty(&self.work_id, "WorkConditionRecord.work_id")?;
        require_non_empty(&self.owner_actor.id, "WorkConditionRecord.owner_actor.id")?;
        require_non_empty(&self.impact, "WorkConditionRecord.impact")?;
        require_non_empty(
            &self.resume_condition,
            "WorkConditionRecord.resume_condition",
        )?;
        require_non_empty(&self.created_at, "WorkConditionRecord.created_at")?;
        if self.work_version == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkConditionRecord.work_version",
                reason: "must be greater than zero",
            });
        }
        if self.condition == WorkCondition::Normal {
            return Err(ValidationError::Invalid {
                field: "WorkConditionRecord.condition",
                reason: "condition records describe blocked or on-hold Work",
            });
        }
        validate_non_empty_unique_strings(
            &self.evidence_refs,
            "WorkConditionRecord.evidence_refs",
            true,
        )
    }
}

impl Validate for WorkReport {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkReport.id")?;
        require_non_empty(&self.work_id, "WorkReport.work_id")?;
        require_non_empty(
            &self.submitted_by_actor.id,
            "WorkReport.submitted_by_actor.id",
        )?;
        require_non_empty(&self.result_summary, "WorkReport.result_summary")?;
        require_non_empty(&self.created_at, "WorkReport.created_at")?;
        if self.work_version == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkReport.work_version",
                reason: "must be greater than zero",
            });
        }
        if self.report_revision == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkReport.report_revision",
                reason: "must be greater than zero",
            });
        }
        require_non_empty(&self.candidate_revision, "WorkReport.candidate_revision")?;
        if let Some(base) = &self.base_revision {
            require_non_empty(base, "WorkReport.base_revision")?;
        }
        validate_non_empty_unique_strings(&self.artifact_refs, "WorkReport.artifact_refs", true)?;
        validate_non_empty_unique_strings(&self.check_refs, "WorkReport.check_refs", true)?;
        if self.evidence_refs.is_empty() {
            return Err(ValidationError::Required {
                field: "WorkReport.evidence_refs",
            });
        }
        validate_non_empty_unique_strings(&self.evidence_refs, "WorkReport.evidence_refs", true)?;
        validate_non_empty_unique_strings(&self.known_risks, "WorkReport.known_risks", false)
    }
}

impl Validate for WorkEvidence {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkEvidence.id")?;
        require_non_empty(&self.work_id, "WorkEvidence.work_id")?;
        require_non_empty(&self.work_report_id, "WorkEvidence.work_report_id")?;
        require_non_empty(&self.candidate_revision, "WorkEvidence.candidate_revision")?;
        require_non_empty(&self.source_type, "WorkEvidence.source_type")?;
        require_non_empty(&self.source_ref, "WorkEvidence.source_ref")?;
        require_non_empty(&self.summary, "WorkEvidence.summary")?;
        require_non_empty(&self.created_at, "WorkEvidence.created_at")?;
        if self.work_version == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkEvidence.work_version",
                reason: "must be greater than zero",
            });
        }
        if self.source_type != "work_candidate_revision" {
            return Err(ValidationError::Invalid {
                field: "WorkEvidence.source_type",
                reason: "must be work_candidate_revision",
            });
        }
        if self.source_ref != self.candidate_revision {
            return Err(ValidationError::Invalid {
                field: "WorkEvidence.source_ref",
                reason: "must equal candidate_revision",
            });
        }
        Ok(())
    }
}

impl Validate for WorkOperationalDecision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkOperationalDecision.id")?;
        require_non_empty(&self.work_id, "WorkOperationalDecision.work_id")?;
        require_non_empty(
            &self.decided_by_actor.id,
            "WorkOperationalDecision.decided_by_actor.id",
        )?;
        require_non_empty(&self.rationale, "WorkOperationalDecision.rationale")?;
        require_non_empty(&self.created_at, "WorkOperationalDecision.created_at")?;
        if self.expected_work_version == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkOperationalDecision.expected_work_version",
                reason: "must be greater than zero",
            });
        }
        match self.kind {
            WorkDecisionKind::Accept | WorkDecisionKind::Revise
                if self.work_report_id.is_none() =>
            {
                return Err(ValidationError::Required {
                    field: "WorkOperationalDecision.work_report_id",
                });
            }
            WorkDecisionKind::WaiveGate if self.gate_requirement_ref.is_none() => {
                return Err(ValidationError::Required {
                    field: "WorkOperationalDecision.gate_requirement_ref",
                });
            }
            WorkDecisionKind::Fail if self.failure_analysis_ref.is_none() => {
                return Err(ValidationError::Required {
                    field: "WorkOperationalDecision.failure_analysis_ref",
                });
            }
            _ => {}
        }
        for (value, field) in [
            (
                self.work_report_id.as_deref(),
                "WorkOperationalDecision.work_report_id",
            ),
            (
                self.gate_requirement_ref.as_deref(),
                "WorkOperationalDecision.gate_requirement_ref",
            ),
            (
                self.failure_analysis_ref.as_deref(),
                "WorkOperationalDecision.failure_analysis_ref",
            ),
        ] {
            if let Some(value) = value {
                require_non_empty(value, field)?;
            }
        }
        validate_non_empty_unique_strings(
            &self.evidence_refs,
            "WorkOperationalDecision.evidence_refs",
            true,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCommandContext {
    pub event_id: String,
    pub performed_by_actor: TeamActorRef,
    #[serde(default)]
    pub authority_actor: Option<TeamActorRef>,
    #[serde(default)]
    pub causation_ref: Option<WorkCausationRef>,
    pub idempotency_key: String,
    pub created_at: String,
    /// When true, skip the duplicate-title guard (recovery flows reuse existing
    /// Work ids; explicit creation of a same-title Work is opt-in).
    #[serde(default)]
    pub duplicate_ok: bool,
}

/// Where a Work executes. The harness creates the workspace before the first
/// member start, injects it as the member's cwd, and cleans it up on Work
/// completion or cancellation (when `auto_cleanup` is true).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkWorkspaceKind {
    /// A git worktree: isolated checkout with its own branch. Required for
    /// code-producing Work where parallel members need disjoint paths.
    Worktree,
    /// A plain directory (no git isolation). For exploration, research, or
    /// single-file documentation work.
    Dir,
    /// The project root. For read-only analysis or ops work that doesn't need
    /// isolation. Member's cwd is the project root.
    Inherit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkWorkspace {
    pub kind: WorkWorkspaceKind,
    /// Absolute or project-relative path. For worktrees, this is OUTSIDE the
    /// main repository (e.g. "../repo-feat-login").
    pub path: String,
    /// For worktrees: the base ref to branch from (e.g. "origin/master").
    #[serde(default)]
    pub base_ref: Option<String>,
    /// Whether the workspace should be removed after Work completes.
    #[serde(default = "default_workspace_auto_cleanup")]
    pub auto_cleanup: bool,
}

fn default_workspace_auto_cleanup() -> bool {
    true
}

/// Kind of GitHub object a [`Work`] is linked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubLinkKind {
    Issue,
    PullRequest,
}

/// A GitHub issue/PR link attached to a [`Work`] by
/// `work create --github-issue` / `work submit --github-pr`.
///
/// The link is a durable snapshot: `status`/`ci_status`/`ci_url` are captured
/// from the GitHub API (via the `gh` CLI) at link time and never silently
/// re-synced, so a stored link states the observation made when it was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubLink {
    pub kind: GitHubLinkKind,
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub url: String,
    /// GitHub object state at snapshot time: `OPEN`/`CLOSED` for issues,
    /// `OPEN`/`CLOSED`/`MERGED` for pull requests.
    #[serde(default)]
    pub status: Option<String>,
    /// PR CI outcome at snapshot time: `success`, `failure`, `pending`, or
    /// `unknown` when no checks are reported for the PR.
    #[serde(default)]
    pub ci_status: Option<String>,
    /// Link to the PR checks page / the check that determined `ci_status`.
    #[serde(default)]
    pub ci_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub id: String,
    pub team_run_id: String,
    /// Durable accountable AgentTeam (DOC-106). Required for every current
    /// write; `alias = "team_id"` reads pre-cutover rows under the same value.
    /// Rows with neither field are legacy TeamRun-scoped compatibility rows:
    /// they stay readable for evidence, but no current mutation is accepted
    /// until `migrate_work_responsibility` binds them to one durable Team.
    /// When set, `team_run_id` names only the current execution attempt: the
    /// Work's responsibility survives that TeamRun's completion and a later
    /// execution attempt rebinds `team_run_id` without changing
    /// `accountable_team_id`.
    #[serde(default, alias = "team_id")]
    pub accountable_team_id: Option<String>,
    /// Durable assignee: exactly one TeamMembership of the accountable Team,
    /// never a MemberRun (DOC-106). Assignment does not require a running
    /// provider process; an Inactive membership or Detached runtime retains
    /// responsibility while receiving no new automatic execution authority.
    #[serde(default)]
    pub assignee_membership_id: Option<String>,
    /// Historical Parent/Child Work evidence. Current Work is a flat DAG and
    /// never serializes or mutates this value; new code must use
    /// `prerequisite_work_ids` instead. The renamed field deliberately keeps
    /// old JSONL rows readable without keeping `parent_work_id` in the current
    /// model or schema authority.
    #[serde(default, rename = "parent_work_id", skip_serializing)]
    pub legacy_parent_work_id: Option<String>,
    pub title: String,
    pub context_markdown: String,
    pub completion_criteria_markdown: String,
    pub phase: WorkPhase,
    pub condition: WorkCondition,
    #[serde(default)]
    pub resolution: Option<WorkResolution>,
    /// Stable AgentMember identity of the assignee. This is a derived mirror
    /// of `assignee_membership_id`'s `agent_member_id` kept for provenance and
    /// display; the membership id is the assignee authority (DOC-106).
    /// Runtime generations bind through `active_member_run_id`.
    #[serde(default)]
    pub owner_member_id: Option<String>,
    #[serde(default)]
    pub active_member_run_id: Option<String>,
    pub claim_mode: WorkClaimMode,
    #[serde(default)]
    pub eligible_member_ids: Vec<String>,
    #[serde(default)]
    pub prerequisite_work_ids: Vec<String>,
    pub priority: WorkPriority,
    pub created_by_actor: TeamActorRef,
    /// Durable ProviderLaunchProfile identity of the creator (ADR 0052 provenance).
    /// `None` for Host, Supervising Operator, or external intake; populated
    /// from the bound ProviderRuntimeProjection's stable identity when a Member creates Work.
    #[serde(default)]
    pub created_by_member_id: Option<String>,
    #[serde(default)]
    pub result_summary: Option<String>,
    #[serde(default)]
    pub blocker_reason: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub check_refs: Vec<String>,
    /// GitHub issue/PR linkage snapshot (see [`GitHubLink`]). `#[serde(default)]`
    /// keeps pre-linkage works.jsonl records readable.
    #[serde(default)]
    pub github_links: Vec<GitHubLink>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

impl Work {
    pub fn is_terminal(&self) -> bool {
        self.phase == WorkPhase::Closed
    }

    pub fn is_open(&self) -> bool {
        self.phase == WorkPhase::Open
    }

    pub fn is_active(&self) -> bool {
        self.phase == WorkPhase::Active
    }

    pub fn is_in_review(&self) -> bool {
        self.phase == WorkPhase::Review
    }

    pub fn is_blocked(&self) -> bool {
        self.condition == WorkCondition::Blocked
    }

    pub fn is_accepted(&self) -> bool {
        self.phase == WorkPhase::Closed && self.resolution == Some(WorkResolution::Accepted)
    }

    /// Whether every declared prerequisite has reached Host-accepted `Done`.
    ///
    /// This intentionally says nothing about the Work's own lifecycle state.
    /// A delivery for a revision can be actionable while the Work is already
    /// `in_progress`, `blocked`, or `review`; only a *new claim* is restricted
    /// to an open Work.
    pub fn prerequisites_satisfied<'a>(&self, works: impl IntoIterator<Item = &'a Work>) -> bool {
        let by_id = works
            .into_iter()
            .map(|work| (work.id.as_str(), work.is_accepted()))
            .collect::<std::collections::HashMap<_, _>>();
        self.prerequisite_work_ids
            .iter()
            .all(|id| by_id.get(id.as_str()) == Some(&true))
    }

    /// Whether this Work can be newly claimed from the shared Works board.
    pub fn is_claim_ready<'a>(&self, works: impl IntoIterator<Item = &'a Work>) -> bool {
        self.phase == WorkPhase::Open
            && self.condition == WorkCondition::Normal
            && self.prerequisites_satisfied(works)
    }

    /// Compatibility spelling retained for existing callers. Readiness here
    /// means *claim* readiness, not delivery readiness.
    pub fn is_ready<'a>(&self, works: impl IntoIterator<Item = &'a Work>) -> bool {
        self.is_claim_ready(works)
    }

    /// Structured, deterministic claim-readiness result. Prefer this over the
    /// compatibility boolean helpers for APIs and operator-facing surfaces.
    pub fn readiness(&self, works: &[Work]) -> WorkReadiness {
        work_readiness(self, works)
    }

    /// Whether this Work carries a durable accountable AgentTeam (DOC-106)
    /// rather than only a compatibility TeamRun scope.
    pub fn is_team_scoped(&self) -> bool {
        self.accountable_team_id.is_some()
    }

    /// Assigned/unassigned is a derived view over the assignee responsibility
    /// fields (`assignee_membership_id`, mirrored by `owner_member_id`), never
    /// a stored lifecycle of its own and never a runtime fact (DOC-106).
    pub fn is_assigned(&self) -> bool {
        self.assignee_membership_id.is_some() || self.owner_member_id.is_some()
    }

    pub fn is_unassigned(&self) -> bool {
        !self.is_assigned()
    }
}

impl Validate for Work {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Work.id")?;
        require_non_empty(&self.team_run_id, "Work.team_run_id")?;
        // DOC-106: the accountable Team is required authority on every current
        // Work write. Legacy TeamRun-scoped rows stay readable through serde
        // but cannot take new mutations until responsibility migration binds
        // them to one durable Team.
        require_non_empty(
            self.accountable_team_id.as_deref().unwrap_or_default(),
            "Work.accountable_team_id",
        )?;
        require_non_empty(&self.title, "Work.title")?;
        require_non_empty(
            &self.completion_criteria_markdown,
            "Work.completion_criteria_markdown",
        )?;
        require_non_empty(&self.created_by_actor.id, "Work.created_by_actor.id")?;
        validate_actor_metadata(&self.created_by_actor, "Work.created_by_actor")?;
        require_non_empty(&self.created_at, "Work.created_at")?;
        require_non_empty(&self.updated_at, "Work.updated_at")?;

        for (value, field) in [
            (
                self.legacy_parent_work_id.as_deref(),
                "Work.legacy_parent_work_id",
            ),
            (
                self.assignee_membership_id.as_deref(),
                "Work.assignee_membership_id",
            ),
            (self.owner_member_id.as_deref(), "Work.owner_member_id"),
            (
                self.active_member_run_id.as_deref(),
                "Work.active_member_run_id",
            ),
            (
                self.created_by_member_id.as_deref(),
                "Work.created_by_member_id",
            ),
            (self.blocker_reason.as_deref(), "Work.blocker_reason"),
        ] {
            if let Some(value) = value {
                require_non_empty(value, field)?;
            }
        }

        validate_non_empty_unique_strings(
            &self.eligible_member_ids,
            "Work.eligible_member_ids",
            true,
        )?;
        validate_non_empty_unique_strings(
            &self.prerequisite_work_ids,
            "Work.prerequisite_work_ids",
            true,
        )?;
        validate_non_empty_unique_strings(&self.artifact_refs, "Work.artifact_refs", false)?;
        validate_non_empty_unique_strings(&self.check_refs, "Work.check_refs", false)?;

        for link in &self.github_links {
            for (value, field) in [
                (link.owner.as_str(), "Work.github_links[].owner"),
                (link.repo.as_str(), "Work.github_links[].repo"),
                (link.url.as_str(), "Work.github_links[].url"),
            ] {
                if value.is_empty() {
                    return Err(ValidationError::Required { field });
                }
            }
            if link.number == 0 {
                return Err(ValidationError::Invalid {
                    field: "Work.github_links[].number",
                    reason: "must be greater than zero",
                });
            }
        }
        if self.version == 0 {
            return Err(ValidationError::Invalid {
                field: "Work.version",
                reason: "must be greater than zero",
            });
        }
        match (self.phase, self.condition, self.resolution) {
            (WorkPhase::Closed, WorkCondition::Normal, Some(_)) => {}
            (WorkPhase::Closed, _, _) => {
                return Err(ValidationError::Invalid {
                    field: "Work.condition",
                    reason: "closed Work must be normal and carry a resolution",
                });
            }
            (_, _, Some(_)) => {
                return Err(ValidationError::Invalid {
                    field: "Work.resolution",
                    reason: "resolution is only valid for closed Work",
                });
            }
            _ => {}
        }
        Ok(())
    }
}

pub(crate) fn validate_non_empty_unique_strings(
    values: &[String],
    field: &'static str,
    unique: bool,
) -> Result<(), ValidationError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if value.is_empty() {
            return Err(ValidationError::Required { field });
        }
        if unique && !seen.insert(value) {
            return Err(ValidationError::Invalid {
                field,
                reason: "must not contain duplicate values",
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkEventKind {
    Created,
    Assigned,
    Claimed,
    Started,
    Released,
    Blocked,
    Resumed,
    Submitted,
    ChangesRequested,
    Accepted,
    Cancelled,
    Failed,
    Updated,
    /// Canonical replacement of the Work's hard `depends_on` edge set. The
    /// event payload is [`WorkDependenciesChangedPayload`].
    DependenciesChanged,
    Rebound,
    /// The execution attempt (`team_run_id`) of a Team-scoped Work moved to a
    /// successor TeamRun of the same AgentTeam. Durable scope (`team_id`),
    /// owner, and provenance are unchanged (ADR 0052).
    ExecutionRetargeted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkEvent {
    pub id: String,
    pub team_run_id: String,
    pub work_id: String,
    pub sequence: u64,
    pub kind: WorkEventKind,
    pub expected_version: u64,
    pub resulting_version: u64,
    pub performed_by_actor: TeamActorRef,
    #[serde(default)]
    pub authority_actor: Option<TeamActorRef>,
    #[serde(default)]
    pub causation_ref: Option<WorkCausationRef>,
    pub idempotency_key: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderWorkDispatchStatus {
    Queued,
    Claimed,
    ProviderReceived,
    Failed,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderWorkDispatch {
    pub id: String,
    pub work_event_id: String,
    pub team_run_id: String,
    pub work_id: String,
    pub work_version: u64,
    pub recipient_member_run_id: String,
    pub status: ProviderWorkDispatchStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_by_supervisor_id: Option<String>,
    #[serde(default)]
    pub claimed_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub updated_at: String,
}

/// One crash-atomic store row: event, resulting projection, and initial outbox
/// deliveries are serialized as one JSONL record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOperation {
    pub event: WorkEvent,
    pub work: Work,
    /// Immutable records committed in the same crash-atomic row as the Work
    /// transition they explain.
    #[serde(default)]
    pub condition_records: Vec<WorkConditionRecord>,
    #[serde(default)]
    pub reports: Vec<WorkReport>,
    #[serde(default)]
    pub evidence_records: Vec<WorkEvidence>,
    #[serde(default)]
    pub decisions: Vec<WorkOperationalDecision>,
    #[serde(default)]
    pub deliveries: Vec<ProviderWorkDispatch>,
    #[serde(default)]
    pub delivery_updates: Vec<ProviderWorkDispatchUpdate>,
    /// Delegation projection transitions caused by this exact Work mutation.
    /// Keeping them in the same row closes the crash gap between target Work
    /// state and its cross-Team responsibility projection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub delegation_revisions: Vec<WorkDelegationRevision>,
}

/// Resolution outcome for one migrated responsibility field (DOC-106). The
/// migration never guesses: an ambiguous or missing target is reported and
/// the Work keeps its prior field state for manual reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkResponsibilityResolution {
    /// The field already carried the canonical value; no write was needed.
    AlreadyCanonical,
    /// The field resolved to exactly one durable target and was written.
    Resolved { value: String },
    /// The Work has no assignee; nothing to resolve.
    Unassigned,
    /// Resolution was ambiguous or impossible; nothing was written for this
    /// field and the reason is recorded for operator reconciliation.
    Unresolved { reason: String },
}

/// Per-Work outcome of one responsibility migration pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkResponsibilityMigrationEntry {
    pub work_id: String,
    pub from_version: u64,
    /// Present only when a migration WorkOperation was appended.
    #[serde(default)]
    pub to_version: Option<u64>,
    pub accountable_team: WorkResponsibilityResolution,
    pub assignee: WorkResponsibilityResolution,
}

/// Append-only migration report. Work IDs, versions, Operation/Event history,
/// provenance, reports, evidence, gates and decisions are preserved; the only
/// writes are new `Updated` WorkOperations carrying the resolved
/// `accountable_team_id`/`assignee_membership_id` fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkResponsibilityMigrationReport {
    pub execution_space_id: String,
    pub migrated_work_ids: Vec<String>,
    pub entries: Vec<WorkResponsibilityMigrationEntry>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderWorkDispatchUpdate {
    pub delivery_id: String,
    /// Store-global ordering for delivery projection updates. Legacy rows
    /// deserialize as zero and are folded before sequenced writes.
    #[serde(default)]
    pub update_sequence: u64,
    pub status: ProviderWorkDispatchStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_by_supervisor_id: Option<String>,
    #[serde(default)]
    pub claimed_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub updated_at: String,
}

/// Why the exact bound Host must inspect durable Agent Team state.
///
/// This is deliberately separate from [`ProviderDispatchIntent`] and
/// [`WorkEventKind`]. Work remains the responsibility/status plane, while a
/// Host attention row is only a durable notification that a particular Work
/// state now needs Host action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAttentionKind {
    HostBindingStale,
    WorkChanged,
    WorkReviewRequested,
    WorkBlocked,
    WorkAccepted,
    WorkChangesRequested,
    WorkCancelled,
    WorkPrerequisiteCompleted,
    WorkPrerequisiteNeedsReconciliation,
    WorkDeliveryFailed,
    MemberStoppedWithOwnedReadyWork,
    MemberFailedWithOwnedReadyWork,
}

/// Transport/intake state for one Host attention row.
///
/// `Delivered` proves only that the exact provider-native Host task accepted
/// the notification. `Acknowledged` proves Host intake. `EscalationRequired`
/// is set by a headless host dispatcher when the attention needs explicit human
/// decision (accept/merge/cancel) that the triage-only host cannot make.
/// Neither `Acknowledged` nor `EscalationRequired` mutates the referenced Work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAttentionStatus {
    Actionable,
    Claimed,
    Delivered,
    Acknowledged,
    EscalationRequired,
}

/// Durable notification derived from a Work-state or member-runtime fact.
///
/// Host binding is intentionally not copied into this row. Read projections
/// resolve the latest [`AgentTeamRun`] binding, so an item created while
/// unbound cannot leak to another task and becomes deliverable only after an
/// explicit binding exists. Claim fields snapshot the exact binding that owns
/// an in-flight delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAttention {
    pub id: String,
    pub team_run_id: String,
    pub kind: HostAttentionKind,
    pub work_id: String,
    pub work_version: u64,
    /// Exact WorkEvent, TeamRunEvent, or provider control event that caused
    /// this notification. Runtime integration should derive `id`
    /// deterministically from this reference so retries remain idempotent.
    pub source_event_ref: String,
    #[serde(default)]
    pub member_run_id: Option<String>,
    pub status: HostAttentionStatus,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_host_surface: Option<String>,
    #[serde(default)]
    pub claimed_host_thread_id: Option<String>,
    /// Present only for claims made under a durable Host binding lease. These
    /// fields fence completion after lease expiry, release, or takeover.
    #[serde(default)]
    pub claimed_host_lease_id: Option<String>,
    #[serde(default)]
    pub claimed_host_lease_generation: Option<u64>,
    #[serde(default)]
    pub claimed_host_lease_owner_id: Option<String>,
    /// Exact managed Host delivery fence. These fields are absent for the
    /// external interactive Host transport and prevent a successor session or
    /// daemon generation from settling an older provider effect.
    #[serde(default)]
    pub claimed_recipient_member_run_id: Option<String>,
    #[serde(default)]
    pub claimed_recipient_session_id: Option<String>,
    #[serde(default)]
    pub claimed_recipient_session_generation: Option<u64>,
    #[serde(default)]
    pub claimed_node_daemon_id: Option<String>,
    #[serde(default)]
    pub claimed_node_daemon_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub last_failure_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl HostAttention {
    /// Delivered rows remain actionable until the exact Host explicitly ACKs
    /// intake or escalates. A claim is also visible so another transport cannot
    /// double-wake the same Host while the first attempt is in flight.
    pub fn needs_host_action(&self) -> bool {
        self.status != HostAttentionStatus::Acknowledged
            && self.status != HostAttentionStatus::EscalationRequired
    }
}

impl Validate for HostAttention {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "HostAttention.id")?;
        require_non_empty(&self.team_run_id, "HostAttention.team_run_id")?;
        if self.kind == HostAttentionKind::HostBindingStale {
            if !self.work_id.is_empty() || self.work_version != 0 || self.member_run_id.is_some() {
                return Err(ValidationError::Invalid {
                    field: "HostAttention.host_binding_stale",
                    reason: "must not name Work or ProviderRuntimeProjection state",
                });
            }
        } else {
            require_non_empty(&self.work_id, "HostAttention.work_id")?;
        }
        require_non_empty(&self.source_event_ref, "HostAttention.source_event_ref")?;
        require_non_empty(&self.created_at, "HostAttention.created_at")?;
        require_non_empty(&self.updated_at, "HostAttention.updated_at")?;
        if let Some(member_run_id) = &self.member_run_id {
            require_non_empty(member_run_id, "HostAttention.member_run_id")?;
        }
        if let Some(claim_id) = &self.claim_id {
            require_non_empty(claim_id, "HostAttention.claim_id")?;
        }
        if let Some(surface) = &self.claimed_host_surface {
            require_non_empty(surface, "HostAttention.claimed_host_surface")?;
        }
        if let Some(thread_id) = &self.claimed_host_thread_id {
            require_non_empty(thread_id, "HostAttention.claimed_host_thread_id")?;
        }
        if let Some(lease_id) = &self.claimed_host_lease_id {
            require_non_empty(lease_id, "HostAttention.claimed_host_lease_id")?;
        }
        if let Some(owner_id) = &self.claimed_host_lease_owner_id {
            require_non_empty(owner_id, "HostAttention.claimed_host_lease_owner_id")?;
        }
        if let Some(member_run_id) = &self.claimed_recipient_member_run_id {
            require_non_empty(
                member_run_id,
                "HostAttention.claimed_recipient_member_run_id",
            )?;
        }
        if let Some(session_id) = &self.claimed_recipient_session_id {
            require_non_empty(session_id, "HostAttention.claimed_recipient_session_id")?;
        }
        if let Some(daemon_id) = &self.claimed_node_daemon_id {
            require_non_empty(daemon_id, "HostAttention.claimed_node_daemon_id")?;
        }
        if let Some(receipt_id) = &self.provider_receipt_id {
            require_non_empty(receipt_id, "HostAttention.provider_receipt_id")?;
        }
        if let Some(reason) = &self.last_failure_reason {
            require_non_empty(reason, "HostAttention.last_failure_reason")?;
        }
        let claim_binding = (
            self.claim_id.is_some(),
            self.claimed_host_surface.is_some(),
            self.claimed_host_thread_id.is_some(),
        );
        let lease_fence = (
            self.claimed_host_lease_id.is_some(),
            self.claimed_host_lease_generation.is_some(),
            self.claimed_host_lease_owner_id.is_some(),
        );
        let managed_fence = (
            self.claimed_recipient_member_run_id.is_some(),
            self.claimed_recipient_session_id.is_some(),
            self.claimed_recipient_session_generation.is_some(),
            self.claimed_node_daemon_id.is_some(),
            self.claimed_node_daemon_generation.is_some(),
        );
        if !matches!(lease_fence, (false, false, false) | (true, true, true)) {
            return Err(ValidationError::Invalid {
                field: "HostAttention.claimed_host_lease",
                reason: "lease_id, generation, and owner_id must be all present or all absent",
            });
        }
        if !matches!(
            managed_fence,
            (false, false, false, false, false) | (true, true, true, true, true)
        ) {
            return Err(ValidationError::Invalid {
                field: "HostAttention.claimed_managed_host",
                reason: "MemberRun, AgentSession, session generation, NodeDaemon, and daemon generation must be all present or all absent",
            });
        }
        let external_claim = claim_binding == (true, true, true)
            && managed_fence == (false, false, false, false, false);
        let managed_claim = claim_binding == (true, true, false)
            && self.claimed_host_surface.as_deref() == Some("managed")
            && lease_fence == (false, false, false)
            && managed_fence == (true, true, true, true, true);
        match self.status {
            HostAttentionStatus::Actionable | HostAttentionStatus::EscalationRequired => {
                if claim_binding != (false, false, false)
                    || lease_fence != (false, false, false)
                    || managed_fence != (false, false, false, false, false)
                    || self.provider_receipt_id.is_some()
                {
                    return Err(ValidationError::Invalid {
                        field: "HostAttention.status",
                        reason: "actionable and escalated rows must be unclaimed and have no binding, lease, or provider receipt",
                    });
                }
            }
            HostAttentionStatus::Claimed => {
                if (!external_claim && !managed_claim) || self.provider_receipt_id.is_some() {
                    return Err(ValidationError::Invalid {
                        field: "HostAttention.status",
                        reason: "claimed rows require exactly one external thread or managed session/daemon fence and cannot have a provider receipt",
                    });
                }
            }
            HostAttentionStatus::Delivered | HostAttentionStatus::Acknowledged => {
                if (!external_claim && !managed_claim) || self.provider_receipt_id.is_none() {
                    return Err(ValidationError::Invalid {
                        field: "HostAttention.status",
                        reason: "delivered and acknowledged rows require exactly one external thread or managed session/daemon fence and a provider receipt",
                    });
                }
            }
        }
        Ok(())
    }
}

/// TeamRun-scoped read projection. `warning` is populated for an unbound run;
/// exact native-thread queries never return such a projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAttentionInbox {
    pub team_run_id: String,
    pub host_surface: String,
    #[serde(default)]
    pub host_thread_id: Option<String>,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub attentions: Vec<HostAttention>,
}

/// Configuration for the daemon-driven headless host dispatcher.
///
/// The dispatcher watches for actionable [`HostAttention`] rows older than
/// `attention_age_threshold_secs` and spawns a headless host round when the
/// host binding lease is not held by a live human session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDispatchConfig {
    /// Minimum age in seconds before a pending attention is eligible for
    /// headless dispatch. Default 300 (5 minutes).
    #[serde(default = "HostDispatchConfig::default_age_threshold")]
    pub attention_age_threshold_secs: u64,
    /// How often the supervisor daemon polls for actionable attentions, in
    /// seconds. Default 60.
    #[serde(default = "HostDispatchConfig::default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    /// When false (default), the headless host is triage-only: it may inspect
    /// attentions, reply to members, and escalate, but MUST NOT accept, merge,
    /// or cancel Work. Set true to allow those mutations without human review.
    #[serde(default)]
    pub accept_merge_enabled: bool,
}

impl Default for HostDispatchConfig {
    fn default() -> Self {
        Self {
            attention_age_threshold_secs: Self::default_age_threshold(),
            poll_interval_secs: Self::default_poll_interval_secs(),
            accept_merge_enabled: false,
        }
    }
}

impl HostDispatchConfig {
    pub const fn default_age_threshold() -> u64 {
        300
    }
    pub const fn default_poll_interval_secs() -> u64 {
        60
    }
}

/// Result from one invocation of the headless host dispatcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDispatchOutcome {
    /// Number of attentions the headless host inspected.
    pub inspected: usize,
    /// Attentions escalated to human (terminal `EscalationRequired`).
    pub escalated: Vec<String>,
    /// Attentions the headless host was able to handle (replied / noted).
    pub handled: Vec<String>,
    /// Attentions the dispatcher could not process (error / unavailable).
    pub failed: Vec<String>,
    /// Human-readable summary of what the headless host did.
    pub summary: Option<String>,
}

impl HostDispatchOutcome {
    pub fn empty() -> Self {
        Self {
            inspected: 0,
            escalated: Vec::new(),
            handled: Vec::new(),
            failed: Vec::new(),
            summary: None,
        }
    }

    pub fn is_noop(&self) -> bool {
        self.inspected == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDelegationState {
    Active,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDelegationTransition {
    Created,
    Blocked,
    Resumed,
    Completed,
    Failed,
    Cancelled,
}

/// Durable relationship between an exact source Work revision and a target
/// Work owned by another flat AgentTeam. The source owner retains integration
/// responsibility; target completion never mutates or accepts the source Work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDelegation {
    pub id: String,
    pub source_work_ref: WorkRef,
    pub source_work_version: u64,
    pub source_owner_member_id: String,
    #[serde(default)]
    pub created_by_member_run_id: Option<String>,
    pub target_agent_team_id: String,
    pub target_work_ref: WorkRef,
    pub delegated_by_actor: TeamActorRef,
    pub state: WorkDelegationState,
    #[serde(default)]
    pub resolution_summary: Option<String>,
    #[serde(default)]
    pub blocker_reason: Option<String>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

/// One append-only transition of a [`WorkDelegation`]. Optimistic concurrency
/// is explicit: every event consumes `expected_version` and produces exactly
/// the next `resulting_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDelegationEvent {
    pub id: String,
    pub delegation_id: String,
    pub sequence: u64,
    pub transition: WorkDelegationTransition,
    pub expected_version: u64,
    pub resulting_version: u64,
    pub performed_by_actor: TeamActorRef,
    #[serde(default)]
    pub causation_ref: Option<WorkCausationRef>,
    pub idempotency_key: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

/// One WorkDelegation event and its resulting projection. Revisions caused by
/// target Work mutations are embedded in the same crash-atomic WorkOperation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDelegationRevision {
    pub delegation: WorkDelegation,
    pub event: WorkDelegationEvent,
}

fn validate_work_ref(reference: &WorkRef, field: &'static str) -> Result<(), ValidationError> {
    if reference.team_run_id.trim().is_empty() || reference.work_id.trim().is_empty() {
        return Err(ValidationError::Required { field });
    }
    Ok(())
}

impl Validate for WorkDelegation {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkDelegation.id")?;
        validate_work_ref(&self.source_work_ref, "WorkDelegation.source_work_ref")?;
        validate_work_ref(&self.target_work_ref, "WorkDelegation.target_work_ref")?;
        require_non_empty(
            &self.source_owner_member_id,
            "WorkDelegation.source_owner_member_id",
        )?;
        require_non_empty(
            &self.target_agent_team_id,
            "WorkDelegation.target_agent_team_id",
        )?;
        require_non_empty(
            &self.delegated_by_actor.id,
            "WorkDelegation.delegated_by_actor.id",
        )?;
        validate_actor_metadata(
            &self.delegated_by_actor,
            "WorkDelegation.delegated_by_actor",
        )?;
        require_non_empty(&self.created_at, "WorkDelegation.created_at")?;
        require_non_empty(&self.updated_at, "WorkDelegation.updated_at")?;
        if self.source_work_version == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkDelegation.source_work_version",
                reason: "must be greater than zero",
            });
        }
        if self.version == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkDelegation.version",
                reason: "must be greater than zero",
            });
        }
        if self.source_work_ref == self.target_work_ref {
            return Err(ValidationError::Invalid {
                field: "WorkDelegation.target_work_ref",
                reason: "must differ from source_work_ref",
            });
        }
        if let Some(member_run_id) = &self.created_by_member_run_id {
            require_non_empty(member_run_id, "WorkDelegation.created_by_member_run_id")?;
        }
        match self.state {
            WorkDelegationState::Active => {
                if self.resolution_summary.is_some() || self.blocker_reason.is_some() {
                    return Err(ValidationError::Invalid {
                        field: "WorkDelegation.state",
                        reason: "active delegations cannot carry blocker or resolution fields",
                    });
                }
            }
            WorkDelegationState::Blocked => {
                let blocker = self.blocker_reason.as_deref().unwrap_or_default();
                require_non_empty(blocker, "WorkDelegation.blocker_reason")?;
                if self.resolution_summary.is_some() {
                    return Err(ValidationError::Invalid {
                        field: "WorkDelegation.resolution_summary",
                        reason: "blocked delegations are not resolved",
                    });
                }
            }
            WorkDelegationState::Completed
            | WorkDelegationState::Failed
            | WorkDelegationState::Cancelled => {
                let summary = self.resolution_summary.as_deref().unwrap_or_default();
                require_non_empty(summary, "WorkDelegation.resolution_summary")?;
            }
        }
        Ok(())
    }
}

impl Validate for WorkDelegationEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkDelegationEvent.id")?;
        require_non_empty(&self.delegation_id, "WorkDelegationEvent.delegation_id")?;
        require_non_empty(
            &self.performed_by_actor.id,
            "WorkDelegationEvent.performed_by_actor.id",
        )?;
        validate_actor_metadata(
            &self.performed_by_actor,
            "WorkDelegationEvent.performed_by_actor",
        )?;
        require_non_empty(&self.idempotency_key, "WorkDelegationEvent.idempotency_key")?;
        require_non_empty(&self.created_at, "WorkDelegationEvent.created_at")?;
        if self.sequence == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkDelegationEvent.sequence",
                reason: "must be greater than zero",
            });
        }
        if self.resulting_version != self.expected_version.saturating_add(1) {
            return Err(ValidationError::Invalid {
                field: "WorkDelegationEvent.resulting_version",
                reason: "must equal expected_version + 1",
            });
        }
        if self.transition == WorkDelegationTransition::Created && self.expected_version != 0 {
            return Err(ValidationError::Invalid {
                field: "WorkDelegationEvent.expected_version",
                reason: "created transition must start at version zero",
            });
        }
        if self.transition != WorkDelegationTransition::Created && self.expected_version == 0 {
            return Err(ValidationError::Invalid {
                field: "WorkDelegationEvent.expected_version",
                reason: "non-created transitions require an existing version",
            });
        }
        if let Some(causation) = &self.causation_ref {
            if causation.kind.trim().is_empty() || causation.id.trim().is_empty() {
                return Err(ValidationError::Required {
                    field: "WorkDelegationEvent.causation_ref",
                });
            }
        }
        Ok(())
    }
}
