use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Blocked,
    Review,
    Done,
    /// Deprecated legacy terminal state (ADR 0019): retained for old rows; new
    /// writers emit `Done`. Read models fold `Complete` into `Done`.
    Complete,
    Archived,
}

/// Goal lifecycle stage (markdown-first model). Distinct from the legacy
/// [`GoalStatus`] kanban state, which is kept for back-compat and derived from
/// the stage via [`GoalStage::to_status`]. Additive: old rows without a `stage`
/// deserialize as `Draft`.
///
/// Phases: exploration (`exploring`→`explored`), work (`working`→`done`),
/// acceptance (`verifying`→`verified`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GoalStage {
    #[default]
    Draft,
    Exploring,
    Explored,
    Working,
    Done,
    Verifying,
    Verified,
}

impl GoalStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStage::Draft => "draft",
            GoalStage::Exploring => "exploring",
            GoalStage::Explored => "explored",
            GoalStage::Working => "working",
            GoalStage::Done => "done",
            GoalStage::Verifying => "verifying",
            GoalStage::Verified => "verified",
        }
    }

    /// Linear position, for forward / back-edge ordering.
    fn order(self) -> u8 {
        match self {
            GoalStage::Draft => 0,
            GoalStage::Exploring => 1,
            GoalStage::Explored => 2,
            GoalStage::Working => 3,
            GoalStage::Done => 4,
            GoalStage::Verifying => 5,
            GoalStage::Verified => 6,
        }
    }

    /// Map the lifecycle stage onto the legacy kanban [`GoalStatus`] so existing
    /// dashboard filters and gates keep working during migration.
    pub fn to_status(self) -> GoalStatus {
        match self {
            GoalStage::Draft | GoalStage::Exploring | GoalStage::Explored | GoalStage::Working => {
                GoalStatus::Active
            }
            GoalStage::Done | GoalStage::Verifying => GoalStatus::Review,
            GoalStage::Verified => GoalStatus::Done,
        }
    }
}

/// One exploration contribution toward a Goal's design. Exploration is
/// multi-agent / multi-round; these raw notes are synthesized into `design_md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exploration {
    pub author: String,
    #[serde(default)]
    pub round: u32,
    pub notes_md: String,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Knowledge-driven phased planning (goal-planning-model, Stage S1a — model only).
// A Goal carries agent-planned, SEQUENTIAL `phases[]` (each owning a task subgraph
// referenced by `Task.phase_id`) and an append-only `knowledge[]` ledger (the truth,
// with provenance) that `design_md` becomes a synthesis view of. All additive +
// `#[serde(default)]` so old goals/tasks still deserialize. Gates/compiler/
// orchestrator are later stages; this is just the data model.
// ---------------------------------------------------------------------------

/// Status of one agent-planned phase. Phases run sequentially; a phase must reach
/// `Passed` (its verdict) before the next begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GoalPhaseStatus {
    #[default]
    NotStarted,
    InProgress,
    Passed,
    Failed,
    Blocked,
}

/// The class of an artifact a phase or task declares it will produce
/// (goal-phase-artifacts). The default `Code` matches today's implicit behavior
/// where a phase's deliverable is a code diff. Serialized snake_case so the wire
/// form reads `design_doc`, `test_report`, `migration_doc`, `registered_doc`, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    DesignDoc,
    Adr,
    #[default]
    Code,
    TestReport,
    MigrationDoc,
    RegisteredDoc,
    Screenshot,
    Other,
}

fn default_artifact_required() -> bool {
    true
}

/// One declared deliverable of a phase or task (goal-phase-artifacts). Makes
/// artifacts first-class and declarative so the verdict gate can VERIFY a phase
/// produced what it promised, instead of only checking the worker didn't crash.
/// All-optional except `id`/`kind`/`purpose`; `required` defaults to TRUE so a
/// declared output is enforced unless explicitly marked optional. An empty
/// `outputs[]` reproduces today's behavior verbatim (the legacy `design_md` is the
/// implicit `design_doc`, the `acceptance` string the implicit gate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSpec {
    /// Stable handle for this artifact within its phase/task.
    pub id: String,
    /// What kind of artifact this is (gate/render hint).
    #[serde(default)]
    pub kind: ArtifactKind,
    /// Where the artifact lands (repo-relative path; glob ok). When present and
    /// `required`, the gate asserts it exists & is non-empty in the worktree diff.
    #[serde(default)]
    pub path: Option<String>,
    /// Why this artifact exists — the human/agent-readable intent.
    pub purpose: String,
    /// Whether the gate must enforce this artifact. Defaults to TRUE.
    #[serde(default = "default_artifact_required")]
    pub required: bool,
    /// Optional per-artifact acceptance criterion (a finer gate than its presence).
    #[serde(default)]
    pub acceptance: Option<String>,
}

impl Default for ArtifactSpec {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: ArtifactKind::default(),
            path: None,
            purpose: String::new(),
            required: default_artifact_required(),
            acceptance: None,
        }
    }
}

/// One agent-planned phase of a goal. It owns a task subgraph (the tasks whose
/// `phase_id == this.id`), and its `acceptance` is the verdict gate before the next
/// phase. Phases replace the fixed `GoalStage` enum as the source of truth for
/// "where is this goal" (the legacy `stage` becomes a derived projection — that
/// rewrite is Stage S1b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalPhase {
    pub id: String,
    pub name: String,
    pub intent: String,
    pub status: GoalPhaseStatus,
    /// Markdown gate condition for this phase (the verdict it must pass).
    #[serde(default)]
    pub acceptance: Option<String>,
    /// The `Decision` recording this phase's verdict, once gated.
    #[serde(default)]
    pub verdict_decision_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    /// Artifacts this phase declares it will produce (goal-phase-artifacts). Empty
    /// reproduces today's behavior (the implicit `design_doc` + `acceptance` gate);
    /// non-empty makes the verdict gate enforce each `required` artifact's presence.
    #[serde(default)]
    pub outputs: Vec<ArtifactSpec>,
}

/// Where a `Knowledge` entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSource {
    #[default]
    Exploration,
    Task,
    Decision,
    Evidence,
}

/// One append-only learning in a goal's knowledge ledger — the durable truth that
/// `design_md` is synthesized from. Carries provenance (`phase_id`/`task_id`/author)
/// so "which task changed the plan, when, by whom" is always answerable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Knowledge {
    pub id: String,
    pub goal_id: String,
    #[serde(default)]
    pub phase_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    pub author: String,
    pub timestamp: String,
    pub notes_md: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: KnowledgeSource,
    #[serde(default)]
    pub superseded_by_knowledge_id: Option<String>,
    pub created_at: String,
}

/// A workflow step's verdict outcome, distinct from its run status: a step that
/// completed but returned `verdict(false)` is a `CleanFail` (drives replan/advance
/// decisions), versus a crash which the step's own `Failed` status carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictOutcome {
    Pass,
    CleanFail,
}

/// Status of a goal-level orchestration run (`harness goal run-phases`): the
/// durable checkpoint that sequences a goal's phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationStatus {
    #[default]
    Running,
    Completed,
    Failed,
}

/// One phase's outcome inside a [`GoalOrchestrationRun`] — which compiled
/// workflow ran it and whether its verdict passed. Append-only audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationPhaseRun {
    pub phase_id: String,
    #[serde(default)]
    pub workflow_run_id: Option<String>,
    #[serde(default)]
    pub compiled_path: Option<String>,
    pub passed: bool,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
}

/// The durable checkpoint for `harness goal run-phases`: it sequences a goal's
/// phases, gating each on its verdict, and records each phase run so `--resume`
/// can re-enter without re-spending completed phases. Latest-row-wins like every
/// other store object (re-appended as the run progresses).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalOrchestrationRun {
    pub id: String,
    pub goal_id: String,
    #[serde(default)]
    pub status: OrchestrationStatus,
    #[serde(default)]
    pub phase_runs: Vec<OrchestrationPhaseRun>,
    pub created_at: String,
    pub updated_at: String,
}

/// Shared git/worktree context for a Goal or Task (ADR 0019). All fields
/// optional; additive — old rows that omit it deserialize as `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GitMetadata {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub pr_ref: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub owned_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub owner_agent_id: String,
    pub status: GoalStatus,
    pub priority: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub vision_id: Option<String>,
    #[serde(default)]
    pub goal_design_id: Option<String>,
    #[serde(default)]
    pub closed_by_decision_id: Option<String>,
    #[serde(default)]
    pub git_metadata: Option<GitMetadata>,
    /// Lifecycle stage (markdown-first model). Additive; old rows default to `Draft`.
    #[serde(default)]
    pub stage: GoalStage,
    /// Draft seed: what this goal is and why.
    #[serde(default)]
    pub description_md: Option<String>,
    /// Written after exploration: key problems FIRST, then Big Picture / Overview
    /// / approach. Absorbs the legacy GoalDesign field soup.
    #[serde(default)]
    pub design_md: Option<String>,
    /// Written BEFORE work starts: the real acceptance — criteria, scenario, and
    /// how to verify for real.
    #[serde(default)]
    pub acceptance_md: Option<String>,
    /// Multi-agent / multi-round exploration notes feeding `design_md`.
    #[serde(default)]
    pub explorations: Vec<Exploration>,
    /// Domain skills needed to DO this goal's work (distinct from `author-goal`).
    #[serde(default)]
    pub skill_refs: Vec<String>,
    /// When `stage` last changed.
    #[serde(default)]
    pub stage_changed_at: Option<String>,
    /// Agent-planned, SEQUENTIAL phases (goal-planning-model). Empty for legacy
    /// goals, which run as a single implicit phase. The source of truth for goal
    /// progress once non-empty; `stage` becomes a derived projection (S1b).
    #[serde(default)]
    pub phases: Vec<GoalPhase>,
    /// Append-only knowledge ledger (the truth `design_md` is synthesized from).
    #[serde(default)]
    pub knowledge: Vec<Knowledge>,
    /// When `design_md` was last (re)synthesized from `knowledge[]`.
    #[serde(default)]
    pub design_synthesis_at: Option<String>,
}

impl Goal {
    /// The goal's effective lifecycle stage.
    ///
    /// For a **phase-driven** goal (`phases` non-empty) the stage is DERIVED
    /// from phase progress — `phases` is the source of truth and the stored
    /// `stage` field is only a coarse legacy projection. For a **legacy** goal
    /// (no `phases`) the stored `stage` field IS the truth (back-compat with
    /// pre-planning rows, which still load and render via the stage bar).
    ///
    /// The derivation is intentionally coarse (the per-phase timeline is the
    /// real view for phase-driven goals); it exists so legacy consumers
    /// (`to_status`, kanban) keep working:
    /// - every phase `Passed` → `Verified` (the whole plan, incl. acceptance, is done)
    /// - any phase started/failed/blocked → `Working` (active)
    /// - all phases `NotStarted` → `Draft`
    pub fn effective_stage(&self) -> GoalStage {
        if self.phases.is_empty() {
            return self.stage;
        }
        if self
            .phases
            .iter()
            .all(|p| p.status == GoalPhaseStatus::Passed)
        {
            GoalStage::Verified
        } else if self
            .phases
            .iter()
            .any(|p| p.status != GoalPhaseStatus::NotStarted)
        {
            GoalStage::Working
        } else {
            GoalStage::Draft
        }
    }

    /// Validate a lifecycle transition, returning `Err(reason)` when disallowed.
    /// Forward moves are strictly one stage at a time and may be gated; the only
    /// back-edges are "re-open exploration" (any → `exploring`) and "real
    /// acceptance failed → rework" (`verifying` → `working`).
    ///
    /// The `from` stage is the [`Goal::effective_stage`], so a phase-driven goal
    /// is checked against its derived stage. Substance gates (`design_md` /
    /// `acceptance_md` non-empty) apply ONLY to legacy goals; a phase-driven
    /// goal gates on per-phase verdicts (driven by the orchestrator), not the
    /// global markdown.
    pub fn check_transition(&self, to: GoalStage) -> Result<(), String> {
        let from = self.effective_stage();
        if to == from {
            return Err(format!("goal is already in stage `{}`", from.as_str()));
        }
        // Back-edges first.
        if to == GoalStage::Exploring {
            return Ok(()); // any stage may re-open exploration
        }
        if from == GoalStage::Verifying && to == GoalStage::Working {
            return Ok(()); // real acceptance failed → back to work
        }
        // Forward: exactly one step.
        if to.order() != from.order() + 1 {
            return Err(format!(
                "illegal transition `{}` → `{}` (forward moves are one stage at a time; \
                 allowed back-edges are any→exploring and verifying→working)",
                from.as_str(),
                to.as_str()
            ));
        }
        // Phase-driven goals gate on per-phase verdicts, not the global md.
        if !self.phases.is_empty() {
            return Ok(());
        }
        // Forward gates — where substance is enforced (legacy goals only).
        let blank = |s: &Option<String>| s.as_deref().map(str::trim).unwrap_or("").is_empty();
        match (from, to) {
            (GoalStage::Exploring, GoalStage::Explored) if blank(&self.design_md) => Err(
                "cannot enter `explored`: design_md is empty — write the design \
                     (key problems FIRST, then Big Picture / Overview) before marking \
                     exploration complete"
                    .to_string(),
            ),
            (GoalStage::Explored, GoalStage::Working) if blank(&self.acceptance_md) => Err(
                "cannot enter `working`: acceptance_md is empty — write the real \
                     acceptance (criteria + scenario + how to verify for real) BEFORE \
                     work starts"
                    .to_string(),
            ),
            _ => Ok(()),
        }
    }

    /// Deterministically assemble a `design_md` view from the knowledge ledger,
    /// grouped by phase. `design_md` is a RE-SYNTHESIZABLE projection of
    /// `knowledge` (the truth): the body is a pure function of `knowledge` +
    /// `phases` (no timestamps), so identical input yields identical output —
    /// the synthesis *time* is recorded separately in `design_synthesis_at`.
    ///
    /// Superseded entries are kept (struck through with their abandoning
    /// knowledge id) so the design carries the full provenance trail. Returns
    /// `Err` when there is no knowledge to synthesize from — this is the
    /// "design_md requires non-empty knowledge" gate.
    pub fn synthesize_design_md(&self) -> Result<String, String> {
        if self.knowledge.is_empty() {
            return Err(
                "cannot synthesize design_md: the goal has no knowledge entries yet — \
                 capture findings with `goal knowledge-add` first"
                    .to_string(),
            );
        }
        let live = self
            .knowledge
            .iter()
            .filter(|k| k.superseded_by_knowledge_id.is_none())
            .count();
        let mut out = String::new();
        out.push_str(&format!("# Design — {}\n\n", self.title));
        out.push_str(&format!(
            "_Synthesized from {} knowledge entr{} ({live} live). \
             This is a derived view of the goal's knowledge ledger._\n",
            self.knowledge.len(),
            if self.knowledge.len() == 1 {
                "y"
            } else {
                "ies"
            },
        ));

        let render = |out: &mut String, k: &Knowledge| {
            out.push_str(&format!("\n### knowledge#{} · {}", k.id, k.source.as_str()));
            if let Some(task) = &k.task_id {
                out.push_str(&format!(" · task#{task}"));
            }
            out.push_str(&format!(" · by {}", k.author));
            if let Some(sup) = &k.superseded_by_knowledge_id {
                out.push_str(&format!(" · ⚠️ superseded by knowledge#{sup}"));
            }
            if !k.tags.is_empty() {
                out.push_str(&format!(" · [{}]", k.tags.join(", ")));
            }
            out.push('\n');
            out.push_str(k.notes_md.trim_end());
            out.push('\n');
        };

        // One section per phase (in plan order), then an "Unscoped" catch-all
        // for knowledge with no/unknown phase. Within a section insertion order
        // is preserved (the ledger is append-only → chronological).
        for phase in &self.phases {
            let entries: Vec<&Knowledge> = self
                .knowledge
                .iter()
                .filter(|k| k.phase_id.as_deref() == Some(phase.id.as_str()))
                .collect();
            if entries.is_empty() {
                continue;
            }
            out.push_str(&format!("\n## Phase: {} ({})\n", phase.name, phase.id));
            for k in entries {
                render(&mut out, k);
            }
        }
        let phase_ids: Vec<&str> = self.phases.iter().map(|p| p.id.as_str()).collect();
        let unscoped: Vec<&Knowledge> = self
            .knowledge
            .iter()
            .filter(|k| match &k.phase_id {
                None => true,
                Some(id) => !phase_ids.contains(&id.as_str()),
            })
            .collect();
        if !unscoped.is_empty() {
            out.push_str("\n## Unscoped\n");
            for k in unscoped {
                render(&mut out, k);
            }
        }
        Ok(out)
    }
}

impl KnowledgeSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            KnowledgeSource::Exploration => "exploration",
            KnowledgeSource::Task => "task",
            KnowledgeSource::Decision => "decision",
            KnowledgeSource::Evidence => "evidence",
        }
    }
}

/// Render a Rust string as a Starlark string literal. JSON string escaping
/// (quotes, backslashes, control chars) is a valid subset of Starlark's, so a
/// serde_json-encoded string drops straight into generated Starlark.
fn star_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Compile one phase's task DAG into a deterministic Starlark workflow program
/// (the codegen-to-Starlark execution model — the `.star` is a derived, throwaway
/// view; the task graph is the truth).
///
/// Task selection: the phase's LIVE tasks (`task.phase_id == phase.id`, excluding
/// `Superseded`). Dependencies pointing outside this set are ignored — cross-phase
/// ordering is the orchestrator's job, not the compiler's.
///
/// Shape:
/// - tasks are layered by longest dependency path; layer N runs before layer N+1;
/// - within a layer (no intra-layer deps), tasks are greedily partitioned into
///   groups with pairwise-disjoint `owned_paths`. A group with >1 task →
///   `parallel([...])`; a singleton → `agent(...)`. Groups run serially (they were
///   split only because their paths overlap);
/// - a task with non-empty `owned_paths` is WRITABLE → `writable=True,
///   isolation="worktree"` (so concurrent writers never collide on disk);
/// - a non-empty `phase.acceptance` compiles to a judge `agent(schema=…)` plus a
///   `verdict(...)` so the phase has a hard gate.
///
/// The output is a pure function of (goal id/title, phase, tasks) with no
/// timestamps or randomness, so an identical DAG yields a byte-identical script
/// (hence a stable content hash).
pub fn compile_phase_to_starlark(
    goal: &Goal,
    phase: &GoalPhase,
    all_tasks: &[Task],
) -> Result<String, String> {
    use std::collections::{HashMap, HashSet};

    let mut tasks: Vec<&Task> = all_tasks
        .iter()
        .filter(|t| t.phase_id.as_deref() == Some(phase.id.as_str()))
        .filter(|t| t.status != TaskStatus::Superseded)
        .collect();
    tasks.sort_by(|a, b| a.id.cmp(&b.id));
    if tasks.is_empty() {
        return Err(format!(
            "phase `{}` has no live tasks to compile — add tasks with phase_id = \"{}\" \
             (superseded tasks are skipped)",
            phase.id, phase.id
        ));
    }

    // Longest-path layering over in-phase deps (iterative relaxation; a cycle
    // never converges, so cap the passes and report it).
    let mut layer: HashMap<&str, usize> = tasks.iter().map(|t| (t.id.as_str(), 0usize)).collect();
    let cap = tasks.len() + 1;
    let mut changed = true;
    let mut passes = 0;
    while changed {
        changed = false;
        passes += 1;
        if passes > cap {
            return Err(format!(
                "dependency cycle among phase `{}` tasks — cannot compile",
                phase.id
            ));
        }
        for t in &tasks {
            let mut want = 0;
            for dep in &t.depends_on_task_ids {
                if let Some(dl) = layer.get(dep.as_str()) {
                    want = want.max(dl + 1);
                }
            }
            if want != layer[t.id.as_str()] {
                layer.insert(t.id.as_str(), want);
                changed = true;
            }
        }
    }
    let max_layer = tasks
        .iter()
        .map(|t| layer[t.id.as_str()])
        .max()
        .unwrap_or(0);

    let prompt_of = |t: &Task| -> String {
        let body = t
            .design_md
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                if t.objective.trim().is_empty() {
                    t.title.clone()
                } else {
                    t.objective.clone()
                }
            });
        let mut p = format!("Task {}: {}\n\n{}", t.id, t.title, body);
        if !t.acceptance_criteria.is_empty() {
            p.push_str("\n\nAcceptance criteria:");
            for c in &t.acceptance_criteria {
                p.push_str(&format!("\n- {c}"));
            }
        }
        if !t.owned_paths.is_empty() {
            p.push_str(&format!(
                "\n\nYou own (and may write) these paths: {}.",
                t.owned_paths.join(", ")
            ));
        }
        if !t.outputs.is_empty() {
            p.push_str("\n\nRequired artifacts you MUST produce:");
            for o in &t.outputs {
                let path = o
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("(no path declared)");
                let req = if o.required { "required" } else { "optional" };
                p.push_str(&format!("\n- {path} — {} ({req})", o.purpose));
                if let Some(acc) = o
                    .acceptance
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    p.push_str(&format!(" [acceptance: {acc}]"));
                }
            }
        }
        p
    };

    let agent_call = |t: &Task| -> String {
        let mut c = format!("\nagent(\n    {}", star_str(&prompt_of(t)));
        c.push_str(",\n    provider=\"codex\"");
        c.push_str(&format!(",\n    label={}", star_str(&t.id)));
        c.push_str(&format!(",\n    phase={}", star_str(&phase.id)));
        if !t.owned_paths.is_empty() {
            c.push_str(",\n    writable=True");
            c.push_str(",\n    isolation=\"worktree\"");
        }
        c.push_str(",\n)\n");
        c
    };
    let spec_dict = |t: &Task| -> String {
        let mut d = format!("    {{\n        \"prompt\": {}", star_str(&prompt_of(t)));
        d.push_str(",\n        \"provider\": \"codex\"");
        d.push_str(&format!(",\n        \"label\": {}", star_str(&t.id)));
        d.push_str(&format!(",\n        \"phase\": {}", star_str(&phase.id)));
        if !t.owned_paths.is_empty() {
            d.push_str(",\n        \"writable\": True");
            d.push_str(",\n        \"isolation\": \"worktree\"");
        }
        d.push_str("\n    }");
        d
    };

    let intent = if phase.intent.trim().is_empty() {
        format!("Execute phase {} of goal {}.", phase.id, goal.id)
    } else {
        phase.intent.trim().to_string()
    };
    let design_intent = format!(
        "Compiled task DAG for goal {} phase {} ({}): {} \
         Auto-generated by `harness phase compile` — do not edit by hand; \
         recompile from the task graph instead.",
        goal.id, phase.id, phase.name, intent
    );

    let mut s = String::new();
    s.push_str(&format!(
        "workflow(\n    {},\n    {},\n)\n",
        star_str(&format!("phase-{}", phase.id)),
        star_str(&design_intent)
    ));

    for l in 0..=max_layer {
        let layer_tasks: Vec<&Task> = tasks
            .iter()
            .copied()
            .filter(|t| layer[t.id.as_str()] == l)
            .collect();
        // Greedy disjoint-`owned_paths` partition (id order → deterministic).
        let mut groups: Vec<(HashSet<String>, Vec<&Task>)> = Vec::new();
        for t in &layer_tasks {
            let paths: HashSet<String> = t.owned_paths.iter().cloned().collect();
            let mut placed = false;
            for g in groups.iter_mut() {
                if g.0.is_disjoint(&paths) {
                    g.0.extend(paths.iter().cloned());
                    g.1.push(t);
                    placed = true;
                    break;
                }
            }
            if !placed {
                groups.push((paths, vec![t]));
            }
        }
        for (_, members) in &groups {
            if members.len() == 1 {
                s.push_str(&agent_call(members[0]));
            } else {
                s.push_str("\nparallel([\n");
                let specs: Vec<String> = members.iter().map(|t| spec_dict(t)).collect();
                s.push_str(&specs.join(",\n"));
                s.push_str("\n])\n");
            }
        }
    }

    if let Some(acc) = phase
        .acceptance
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
    {
        let judge_prompt = format!(
            "Acceptance check for phase {} ({}) of goal {}.\n\nCriterion:\n{}\n\n\
             Review the work this phase's tasks produced and decide whether the criterion \
             is FULLY met. Set pass=true only if it holds; otherwise pass=false with the gap.",
            phase.id, phase.name, goal.id, acc
        );
        s.push_str(&format!(
            "\n_acc = agent(\n    {},\n    provider=\"codex\",\n    label={},\n    phase={},\n    \
             schema={{\"pass\": \"bool\", \"reason\": \"string\"}},\n)\n",
            star_str(&judge_prompt),
            star_str(&format!("verdict-{}", phase.id)),
            star_str(&phase.id),
        ));
        s.push_str(
            "verdict(bool(_acc) and _acc.get(\"pass\") == True, \
             _acc.get(\"reason\") if _acc else \"acceptance judge returned no structured verdict\")\n",
        );
    }

    Ok(s)
}

/// The structured shape the planner worker (`harness goal plan`) must return.
/// The single top-level `phases` key is what the structured-mode runtime keys
/// on (and the dry-run mock synthesizes); the nested task/phase fields are
/// documented in [`planner_prompt`]. The value is a human shape-hint, mirroring
/// the convention `compile_phase_to_starlark`'s judge schema uses.
pub fn planner_schema() -> serde_json::Value {
    serde_json::json!({
        "phases": "list of {id, name, intent, acceptance, outputs:[{id, kind, path, \
                   purpose, required, acceptance}], tasks:[{id, title, design_md, \
                   acceptance:[string], owned_paths:[string], depends_on:[task-id], \
                   outputs:[{id, kind, path, purpose, required, acceptance}]}]}",
    })
}

/// Compose the planner prompt from a goal's `design_md` + `acceptance_md`: ask the
/// worker to decompose the design into an ORDERED list of sequential phases, each
/// with a task DAG where tasks with disjoint `owned_paths` and no deps run in
/// parallel (exactly the grouping [`compile_phase_to_starlark`] performs). A
/// simple goal may be one phase.
pub fn planner_prompt(goal: &Goal) -> String {
    let design = goal
        .design_md
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no design_md written yet)");
    let acceptance = goal
        .acceptance_md
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no acceptance_md written yet)");
    format!(
        "You are the PLANNER for goal `{id}` (\"{title}\"). Decompose its design \
         into an ORDERED list of phases and, within each phase, a task DAG.\n\n\
         Rules:\n\
         - Phases run SEQUENTIALLY: a phase must pass its acceptance gate before \
         the next begins. A simple goal may be ONE phase.\n\
         - Each task is a mini-goal: give it a concrete `design_md` (a grounded \
         slice of the goal's design), an `acceptance` list, and the `owned_paths` \
         it may write.\n\
         - Declare each phase's and task's deliverables in `outputs` — a list of \
         {{id, kind, path, purpose, required, acceptance}}. `kind` is one of \
         design_doc/adr/code/test_report/migration_doc/registered_doc/screenshot/other \
         (default code); `path` is the repo-relative file (glob ok) the artifact lands \
         at; `required` defaults to true and the verdict gate ENFORCES that each \
         required artifact with a `path` exists and is non-empty. Leave `outputs` empty \
         to keep today's default (the diff is the implicit deliverable, the acceptance \
         string the implicit gate).\n\
         - Within a phase, two tasks run in PARALLEL iff their `owned_paths` are \
         disjoint AND neither depends on the other. Use `depends_on` (task ids in \
         the SAME phase) for ordering; keep owned_paths disjoint where you want \
         parallelism.\n\
         - Use short, stable, kebab-case ids for phases and tasks.\n\n\
         ## design_md\n{design}\n\n## acceptance_md\n{acceptance}",
        id = goal.id,
        title = goal.title,
    )
}

/// Render a flat string→string JSON object as a Starlark dict literal (the
/// `schema=` argument of the planner's `agent()` leaf). Only the flat key→hint
/// shape [`planner_schema`] produces is supported; values are emitted as Starlark
/// string literals.
fn star_schema_dict(schema: &serde_json::Value) -> String {
    let mut entries: Vec<String> = Vec::new();
    if let Some(map) = schema.as_object() {
        for (k, v) in map {
            let hint = v.as_str().unwrap_or("string");
            entries.push(format!("{}: {}", star_str(k), star_str(hint)));
        }
    }
    format!("{{{}}}", entries.join(", "))
}

/// Generate the one-shot PLANNER Starlark program: a mandatory `workflow(...)`
/// header, ONE schema-mode `agent(...)` leaf carrying [`planner_prompt`], and
/// `output(out)` so the run's `final_output.result` carries the planner's
/// structured decomposition verbatim. Run through the SAME real-driver path
/// `goal run-phases` uses (honors `--dry-run`, so tests/CI need no provider).
pub fn compile_planner_script(goal: &Goal) -> String {
    let design_intent = format!(
        "Plan goal {} ({}): decompose design_md into sequential phases + a per-phase \
         task DAG. Auto-generated by `harness goal plan` — do not edit by hand.",
        goal.id, goal.title
    );
    let schema_literal = star_schema_dict(&planner_schema());
    format!(
        "workflow(\n    {},\n    {},\n)\nout = agent(\n    {},\n    provider=\"codex\",\n    \
         label={},\n    schema={},\n)\noutput(out)\n",
        star_str(&format!("plan-{}", goal.id)),
        star_str(&design_intent),
        star_str(&planner_prompt(goal)),
        star_str(&format!("planner-{}", goal.id)),
        schema_literal,
    )
}

/// The structured shape the REVISER worker (`goal run-phases` replan loop) must
/// return when a phase fails its verdict: which live tasks to `supersede` and the
/// `new_tasks` (same task shape as [`planner_schema`]'s tasks) to add in their
/// place. The top-level `revision` key is what the structured-mode runtime keys
/// on (and the dry-run mock synthesizes — a degenerate, empty revision). The value
/// is a human shape-hint, mirroring the convention the other schemas use.
pub fn reviser_schema() -> serde_json::Value {
    serde_json::json!({
        "revision": "{supersede:[task-id], new_tasks:[{id, title, design_md, \
                     acceptance:[string], owned_paths:[string], depends_on:[task-id], \
                     outputs:[{id, kind, path, purpose, required, acceptance}]}]}",
    })
}

/// Compose the reviser prompt: given a phase that FAILED its verdict, its current
/// (live, non-superseded) tasks, and the failure finding captured as knowledge,
/// ask the worker to revise the phase's task graph — supersede the tasks that did
/// not work and add replacements that address the finding. Same task shape the
/// planner emits, so the revision feeds the SAME compiler.
pub fn reviser_prompt(
    goal: &Goal,
    phase: &GoalPhase,
    live_tasks: &[&Task],
    finding: &str,
) -> String {
    let acceptance = phase
        .acceptance
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no explicit acceptance — use the phase intent as the gate)");
    let mut task_lines = String::new();
    for t in live_tasks {
        let paths = if t.owned_paths.is_empty() {
            "(none)".to_string()
        } else {
            t.owned_paths.join(", ")
        };
        task_lines.push_str(&format!(
            "- `{id}` \"{title}\" — owned_paths: {paths}\n",
            id = t.id,
            title = t.title,
        ));
    }
    if task_lines.is_empty() {
        task_lines.push_str("(this phase currently has no live tasks)\n");
    }
    format!(
        "You are the REVISER for goal `{id}` (\"{title}\"), phase `{phase_id}` \
         (\"{phase_name}\"). The phase just FAILED its verdict. Revise its task \
         graph so a re-run can pass.\n\n\
         Phase intent: {intent}\n\
         Phase acceptance gate: {acceptance}\n\n\
         ## Failure finding (captured as knowledge)\n{finding}\n\n\
         ## Current live tasks in this phase\n{task_lines}\n\
         Rules:\n\
         - `supersede`: the ids of the live tasks above that did NOT work and should \
         be retired (they are kept for audit, struck through).\n\
         - `new_tasks`: replacement tasks that address the finding. Each is a mini-goal \
         with a concrete `design_md`, an `acceptance` list, and the `owned_paths` it \
         may write. Declare each task's deliverables in `outputs` ({{id, kind, path, \
         purpose, required, acceptance}}) — the verdict gate enforces that each required \
         artifact with a `path` exists and is non-empty; leave it empty for today's \
         default. Use `depends_on` (ids in THIS phase) for ordering; keep owned_paths \
         disjoint where you want parallelism.\n\
         - Use short, stable, kebab-case ids that do not collide with existing task ids.\n\
         - If nothing actionable can be revised, return an empty `supersede` and empty \
         `new_tasks` — the loop will stop rather than churn.",
        id = goal.id,
        title = goal.title,
        phase_id = phase.id,
        phase_name = phase.name,
        intent = phase.intent,
    )
}

/// Generate the one-shot REVISER Starlark program: a mandatory `workflow(...)`
/// header, ONE schema-mode `agent(...)` leaf carrying [`reviser_prompt`], and
/// `output(out)` so the run's `final_output.result` carries the structured
/// revision verbatim. Runs through the SAME real-driver path `goal run-phases`
/// uses (honors `--dry-run`, so tests/CI need no provider — the dry-run mock
/// yields a degenerate, empty revision the loop treats as "no actionable replan").
pub fn compile_reviser_script(
    goal: &Goal,
    phase: &GoalPhase,
    live_tasks: &[&Task],
    finding: &str,
) -> String {
    let design_intent = format!(
        "Revise goal {} ({}) phase {}: supersede the failing tasks and add replacements \
         after a failed verdict. Auto-generated by `harness goal run-phases` — do not \
         edit by hand.",
        goal.id, goal.title, phase.id
    );
    let schema_literal = star_schema_dict(&reviser_schema());
    format!(
        "workflow(\n    {},\n    {},\n)\nout = agent(\n    {},\n    provider=\"codex\",\n    \
         label={},\n    schema={},\n)\noutput(out)\n",
        star_str(&format!("revise-{}-{}", goal.id, phase.id)),
        star_str(&design_intent),
        star_str(&reviser_prompt(goal, phase, live_tasks, finding)),
        star_str(&format!("reviser-{}-{}", goal.id, phase.id)),
        schema_literal,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMemberStatus {
    Creating,
    Idle,
    Assigned,
    Running,
    WaitingForInput,
    WaitingForApproval,
    Reviewing,
    Blocked,
    Closing,
    Closed,
    Error,
    Paused,
    Stale,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentProviderConfig {
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub collaboration_mode: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub approvals_reviewer: Option<String>,
    #[serde(default)]
    pub sandbox_policy: Option<String>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<String>,
    #[serde(default)]
    pub environment_id: Option<String>,
    /// Optional MCP servers attached to this member (Pillar 2).
    /// When present, `build_launch_spec` carries this to the neutral launch spec.
    #[serde(default)]
    pub mcp: Option<LaunchMcp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMember {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub provider: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub provider_config: AgentProviderConfig,
    pub capabilities: Vec<String>,
    pub team_ids: Vec<String>,
    pub prompt_ref: Option<String>,
    pub skill_refs: Vec<String>,
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub worktree_ref: Option<String>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<String>,
    pub status: AgentMemberStatus,
    pub current_task_id: Option<String>,
    pub current_proposal_id: Option<String>,
    pub provider_runtime_id: Option<String>,
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_agent_path: Option<String>,
    #[serde(default)]
    pub provider_agent_nickname: Option<String>,
    #[serde(default)]
    pub provider_agent_role: Option<String>,
    pub control_endpoint: Option<String>,
    pub created_at: String,
    pub last_seen_at: Option<String>,
}

/// Neutral permission posture for a single delivery turn.
///
/// This is the launch-spec `permission` enum from the launch-spec table in
/// [docs/agent-integration-model.md](../../../docs/agent-integration-model.md).
/// It deliberately does **not** reuse Codex wire vocabulary
/// (`readOnly` / `workspaceWrite` / `dangerFullAccess`): each provider adapter
/// (Pillar 3) translates this neutral enum onto its own controls — Codex
/// sandbox/approval flags, Claude `--permission-mode`, a future platform's
/// controls — per ADR 0011. The snake_case wire values (`read_only`,
/// `workspace_write`, `full_access`) are the neutral spelling, distinct from any
/// platform's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchPermission {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

impl LaunchPermission {
    pub fn as_str(&self) -> &'static str {
        match self {
            LaunchPermission::ReadOnly => "read_only",
            LaunchPermission::WorkspaceWrite => "workspace_write",
            LaunchPermission::FullAccess => "full_access",
        }
    }
}

impl Default for LaunchPermission {
    /// The safe default posture: a turn that has not declared a writable
    /// permission is read-only, never silently writable.
    fn default() -> Self {
        LaunchPermission::WorkspaceWrite
    }
}

/// One neutral MCP server entry for the launch spec.
///
/// This is the minimal neutral shape from the PROPOSED `mcp` block in
/// [docs/agent-integration-model.md](../../../docs/agent-integration-model.md)
/// (Pillar 2). It carries no platform wire vocabulary: each adapter maps it onto
/// `--config mcp_servers.*` (Codex) or `--mcp-config` (Claude). Provider-neutral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchMcpServer {
    /// Stable id for the server.
    pub id: String,
    /// Transport hint (`stdio` / `http` / `sse`); free string, neutral.
    #[serde(default)]
    pub transport: Option<String>,
    /// argv for a local stdio server.
    #[serde(default)]
    pub command: Vec<String>,
    /// endpoint for a remote http/sse server.
    #[serde(default)]
    pub url: Option<String>,
    /// Tool allowlist for this server; empty = all tools on the server.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

/// Minimal neutral MCP block for the launch spec (PROPOSED shape, Pillar 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LaunchMcp {
    #[serde(default)]
    pub servers: Vec<LaunchMcpServer>,
}

/// The provider-neutral launch spec: one normalized per-turn request.
///
/// This is the launch-spec table in
/// [docs/agent-integration-model.md](../../../docs/agent-integration-model.md).
/// The harness builds it from the member (Pillars 1–2) and the claimed
/// [`Message`] via [`build_launch_spec`]; each provider adapter (Pillar 3) then
/// maps it onto its own CLI/SDK call. It is the seam that keeps the operator
/// composer and Dashboard uniform across Codex, Claude, and future platforms.
///
/// Per ADR 0011 this neutral object carries **no** Codex wire vocabulary:
/// `permission` is the neutral [`LaunchPermission`] enum and `writable_roots`
/// replaces Codex's `workspaceWrite.writableRoots`. The Codex-leaking
/// `AgentProviderConfig` fields (`sandbox_policy`, `approval_policy`,
/// `service_tier`, `collaboration_mode`, …) are abstracted here, not reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSpec {
    /// Composed system/developer instructions (Pillar 1 prompt stack), read as a
    /// durable artifact reference — not inline chat text. `None` when the member
    /// has no role prompt.
    #[serde(default)]
    pub prompt_ref: Option<String>,
    /// The turn input: the claimed [`Message`] envelope + content.
    pub message_content: String,
    /// Model selection (Pillar 1). `None` = provider default.
    #[serde(default)]
    pub model: Option<String>,
    /// Reasoning effort (Pillar 1). `None` = provider default.
    #[serde(default)]
    pub effort: Option<String>,
    /// Optional structured-output schema to enforce natively when the provider
    /// supports it. `None` = no schema flag.
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
    /// Neutral permission posture for this turn.
    pub permission: LaunchPermission,
    /// Paths the turn may write (basis for `workspaceWrite` / `--add-dir`).
    #[serde(default)]
    pub writable_roots: Vec<String>,
    /// Abstract allowed-tool set; empty = adapter default.
    #[serde(default)]
    pub tools: Vec<String>,
    /// cwd / worktree root the turn runs in.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Neutral MCP block (PROPOSED, Pillar 2). `None` = no MCP attachment.
    #[serde(default)]
    pub mcp: Option<LaunchMcp>,
    /// Skills to inject (Pillar 1 skill contract); skill `<id>` refs.
    #[serde(default)]
    pub skill_refs: Vec<String>,
    /// Resume an existing provider session (Codex `--session`, Claude
    /// `--resume`); `None` = a fresh session.
    #[serde(default)]
    pub resume: Option<String>,
    /// The event-stream output contract the adapter should request so its native
    /// output normalizes into [`AgentEvent`] (Codex `--json`, Claude
    /// `--output-format stream-json`). Free string, neutral.
    #[serde(default)]
    pub output: Option<String>,
}

/// Provider-neutral delivery handle: how the harness reaches a member's runtime
/// for a delivery.
///
/// This generalizes `control_endpoint` (a raw `unix://socket`) into a
/// process/session descriptor, per ADR 0018 ("Generalize the `control_endpoint`
/// … neither provider needs a long-lived socket in the target design"). It is
/// **additive and pass-through only** in this work package: it does not remove
/// `control_endpoint` and does not change delivery behavior. The existing
/// `socket_path_from_endpoint` resolution stays where it is; this handle simply
/// preserves the raw endpoint string verbatim so callers that still inspect it
/// keep working while the exec-stream path is built in later work packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryHandle {
    /// The raw control endpoint as stored on the member / runtime (e.g.
    /// `unix://…/codex.sock`, or a future exec/session descriptor). Preserved
    /// verbatim; no scheme is assumed.
    pub endpoint: String,
}

impl DeliveryHandle {
    /// Construct a handle that passes the endpoint through unchanged.
    pub fn from_endpoint(endpoint: impl Into<String>) -> Self {
        DeliveryHandle {
            endpoint: endpoint.into(),
        }
    }

    /// The raw endpoint string, verbatim.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// Map a member's existing (Codex-flavored) `sandbox_policy` onto the neutral
/// [`LaunchPermission`] enum.
///
/// Accepts both the dashed and camelCase spellings that the CLI provider layer
/// already tolerates (`read-only`/`readOnly`, `workspace-write`/`workspaceWrite`,
/// `danger-full-access`/`dangerFullAccess`). An absent or unrecognized policy
/// falls back to the safe [`LaunchPermission::default`] posture, so a member that
/// never declared one is not silently elevated.
fn permission_from_sandbox_policy(policy: Option<&str>) -> LaunchPermission {
    match policy {
        Some("read-only") | Some("readOnly") => LaunchPermission::ReadOnly,
        Some("workspace-write") | Some("workspaceWrite") => LaunchPermission::WorkspaceWrite,
        Some("danger-full-access") | Some("dangerFullAccess") => LaunchPermission::FullAccess,
        _ => LaunchPermission::default(),
    }
}

/// Compose the neutral turn-input envelope for a claimed [`Message`].
///
/// This mirrors the harness message-envelope shape the CLI provider layer
/// already hands to a turn (message id / kind / task / routing + content) but
/// keeps it provider-neutral text: the adapter decides how to deliver it (Codex
/// `input` item, Claude `-p`, …).
fn compose_message_content(message: &Message) -> String {
    format!(
        "Harness message envelope:\nmessage_id: {}\nkind: {}\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: {}\ncontent:\n{}",
        message.id,
        message_kind_wire(&message.kind),
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.channel.as_deref().unwrap_or("-"),
        message.content
    )
}

fn message_kind_wire(kind: &MessageKind) -> &'static str {
    match kind {
        MessageKind::Message => "message",
        MessageKind::Task => "task",
        MessageKind::Report => "report",
    }
}

/// Build the provider-neutral [`LaunchSpec`] for one turn from a member and the
/// claimed [`Message`].
///
/// This is the additive composition seam (ADR 0018 WP-1). It reads the existing
/// `AgentMember` / `AgentProviderConfig` fields — including the Codex-flavored
/// `sandbox_policy` — and produces a neutral spec: the permission posture and
/// `writable_roots` are abstracted out of the Codex `workspaceWrite` vocabulary,
/// and no Codex wire names appear on the result (ADR 0011). It does not perform
/// any delivery side effect and does not require a live provider binary.
pub fn build_launch_spec(member: &AgentMember, message: &Message) -> LaunchSpec {
    let permission =
        permission_from_sandbox_policy(member.provider_config.sandbox_policy.as_deref());

    // Writable roots are member-level then provider_config-level roots, in that
    // order, de-duplicated. They are only meaningful when the turn may write, so
    // a read-only posture carries no writable roots.
    let writable_roots = if matches!(permission, LaunchPermission::ReadOnly) {
        Vec::new()
    } else {
        let mut roots: Vec<String> = Vec::new();
        for root in member
            .runtime_workspace_roots
            .iter()
            .chain(member.provider_config.runtime_workspace_roots.iter())
        {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        roots
    };

    LaunchSpec {
        prompt_ref: member.prompt_ref.clone(),
        message_content: compose_message_content(message),
        model: member.model.clone(),
        effort: member.provider_config.effort.clone(),
        output_schema: member.provider_config.output_schema.clone(),
        permission,
        writable_roots,
        // The abstract allowed-tool set is not yet sourced from a neutral member
        // field; left empty until the tool contract lands (Pillar 1/3). Adapters
        // apply their own default meanwhile.
        tools: Vec::new(),
        workspace: member.worktree_ref.clone(),
        // MCP from provider_config (Pillar 2); now available.
        mcp: member.provider_config.mcp.clone(),
        skill_refs: member.skill_refs.clone(),
        // Resume an existing provider session when the member already carries a
        // provider thread/session id from a prior delivery. This is what lets
        // memory carry across deliveries: the next turn is dispatched as a
        // resume of the same session (Codex `exec resume <id>`, Claude
        // `--resume <id>`) instead of a fresh session. `None` (no prior id) = a
        // fresh session.
        resume: member.provider_thread_id.clone(),
        output: None,
    }
}

/// Dispatch discriminant for the provider seam.
///
/// This is **not** a schema field: `AgentMember.provider` (and the other
/// `provider` fields across the model) remain free `String`s, serialized
/// verbatim and validated only as non-empty. `ProviderKind` exists purely so
/// the CLI provider layer can `match` on a member's provider when routing to
/// runtime spawn / delivery / probe / ingest, while keeping the core
/// provider-neutral per ADR 0011.
///
/// Any provider string the harness does not recognise round-trips through
/// [`ProviderKind::Unknown`] so fidelity is never lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Codex,
    Claude,
    Unknown(String),
}

impl ProviderKind {
    pub fn as_str(&self) -> &str {
        match self {
            ProviderKind::Codex => "codex",
            ProviderKind::Claude => "claude",
            ProviderKind::Unknown(value) => value,
        }
    }
}

impl From<&str> for ProviderKind {
    fn from(value: &str) -> Self {
        match value {
            "codex" => ProviderKind::Codex,
            "claude" => ProviderKind::Claude,
            other => ProviderKind::Unknown(other.to_string()),
        }
    }
}

impl From<String> for ProviderKind {
    fn from(value: String) -> Self {
        ProviderKind::from(value.as_str())
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Rough public list price ($ per 1M tokens) `(input, output)` per provider — an
/// ESTIMATE used only to bound workflow spend when the provider reports no dollar
/// cost. The single source of truth for provider pricing across the harness.
/// Unknown providers fall back to the codex/gpt-5-class rate to preserve behavior.
pub fn provider_price_per_mtok(provider: &str) -> (f64, f64) {
    match provider {
        "claude" => (3.0, 15.0),
        _ => (1.25, 10.0), // codex / gpt-5-class default
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamStatus {
    Active,
    Closed,
    Archived,
}

fn default_agent_team_status() -> AgentTeamStatus {
    AgentTeamStatus::Active
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTeam {
    pub id: String,
    pub name: String,
    pub description: String,
    pub owner_agent_id: String,
    #[serde(default = "default_agent_team_status")]
    pub status: AgentTeamStatus,
    pub member_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Planned,
    Assigned,
    Running,
    Blocked,
    Review,
    Done,
    /// Abandoned because a knowledge finding changed the plan (goal-planning-model).
    /// The task is kept (never deleted) with `superseded_by_knowledge_id` set.
    Superseded,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub goal_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub objective: String,
    pub owner_agent_id: String,
    pub assignee_agent_id: Option<String>,
    pub reviewer_agent_id: Option<String>,
    pub status: TaskStatus,
    pub depends_on_task_ids: Vec<String>,
    pub workspace_ref: Option<String>,
    pub branch_ref: Option<String>,
    pub pr_ref: Option<String>,
    pub owned_paths: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub scope_refs: Vec<String>,
    #[serde(default)]
    pub requires_human_approval: bool,
    #[serde(default)]
    pub verdict_decision_id: Option<String>,
    /// Full task write-up (markdown). `objective` stays the one-line summary.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub git_metadata: Option<GitMetadata>,
    /// Full per-task design (goal-planning-model): a grounded SLICE of the goal's
    /// design for this task. `description` stays the simple/short version.
    #[serde(default)]
    pub design_md: Option<String>,
    /// The `GoalPhase.id` this task belongs to (goal-planning-model); `None` for
    /// legacy tasks not yet placed in a phase.
    #[serde(default)]
    pub phase_id: Option<String>,
    /// When `status == Superseded`, the `Knowledge.id` whose finding killed it.
    #[serde(default)]
    pub superseded_by_knowledge_id: Option<String>,
    /// The `WorkflowStep`s that executed this task (reverse link; goal-planning-model).
    #[serde(default)]
    pub workflow_step_ids: Vec<String>,
    /// Artifacts this task declares it will produce (goal-phase-artifacts). Empty
    /// reproduces today's behavior (the implicit `design_md` doc + acceptance gate).
    #[serde(default)]
    pub outputs: Vec<ArtifactSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    Task,
    Report,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDeliveryStatus {
    Queued,
    Delivered,
    Acknowledged,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSessionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Stale,
}

/// Identity class of a [`Message`] sender. Distinguishes harness-managed agents
/// from external operators (humans / external agents acting on their own behalf)
/// and system-emitted messages, so an operator-authored message is never
/// rendered as if it came from the Lead agent. Provider-neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SenderKind {
    #[default]
    Agent,
    Operator,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageTerminalSource {
    TurnCompleted,
    ThreadIdle,
    ThreadRead,
    HookStop,
    DryRun,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDelivery {
    #[serde(default)]
    pub provider_session_id: Option<String>,
    #[serde(default)]
    pub provider_request_id: Option<String>,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub terminal_source: Option<MessageTerminalSource>,
    #[serde(default)]
    pub delivered_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSession {
    pub id: String,
    pub provider: String,
    pub agent_member_id: String,
    pub task_id: Option<String>,
    pub workspace_ref: Option<String>,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub terminal_source: Option<MessageTerminalSource>,
    pub status: ProviderSessionStatus,
    pub command: String,
    pub args: Vec<String>,
    pub prompt_ref: Option<String>,
    pub prompt_summary: Option<String>,
    pub provider_session_ref: Option<String>,
    pub stdout_ref: Option<String>,
    pub jsonl_ref: Option<String>,
    pub transcript_ref: Option<String>,
    pub last_message_ref: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub evidence_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Multi-project identity (goal-multi-project, Stage 0 — pure layer, no I/O).
//
// A project's STORE (the JSONL ledgers + provider-sessions) is centralized under
// `~/.harness/projects/<id>/`, but its PROJECT ROOT (the git repo / dir where a
// worker runs and reads CLAUDE.md / AGENTS.md / memory) stays where it is. These
// two roots are deliberately distinct — see `ProjectContext`.
// ---------------------------------------------------------------------------

/// Reserved project id for the GLOBAL project, rooted at `$HOME` itself. Its
/// relative path is empty, so it cannot share the slug space — hence a reserved id.
pub const GLOBAL_PROJECT_ID: &str = "_global";

/// Whether a project is a specific repo/dir or the reserved global (`$HOME`) one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Repo,
    Global,
}

/// A resolved project. `project_root` is where workers run (and CLAUDE.md /
/// AGENTS.md / memory resolve); `store_root` is the centralized
/// `~/.harness/projects/<id>/` where the JSONL ledgers live. Keeping them separate
/// is the core of multi-project: the store centralizes while worktrees + agent cwd
/// stay tied to the project's own directory/git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContext {
    pub id: String,
    pub project_root: std::path::PathBuf,
    pub store_root: std::path::PathBuf,
    pub kind: ProjectKind,
    pub is_git_repo: bool,
}

/// FNV-1a 64-bit — a small, stable, dependency-free hash used to content-address
/// projects OUTSIDE `$HOME` (where there is no clean relative slug). Stable across
/// runs/platforms (unlike `std::hash::DefaultHasher`), which is what a durable
/// project id needs; it is not used for any security purpose.
fn fnv1a_hex16(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A stable 16-hex content hash of a string (FNV-1a, dependency-free). Used to
/// name compiled phase workflows so an identical DAG → identical filename.
pub fn content_hash_hex16(s: &str) -> String {
    fnv1a_hex16(s.as_bytes())
}

/// Make one path segment filesystem-safe for use inside a project-id slug:
/// keep `[A-Za-z0-9._-]`, replace every other char (incl. path separators) with `-`.
fn sanitize_id_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Derive a STABLE project id from a project's canonical absolute path, relative to
/// the canonical `$HOME`:
/// - `path == home` → [`GLOBAL_PROJECT_ID`] (`_global`).
/// - under `home` → the relative path with separators flattened to `-`
///   (e.g. `~/ai-luodi/jyx3d` → `ai-luodi-jyx3d`).
/// - outside `home` → `proj-<fnv1a-hex16>` of the canonical path string.
///
/// Callers should pass realpath-canonicalized paths so symlinks / `..` don't mint
/// two ids for one project. NOTE (known edge): the `/`→`-` flattening can collide
/// `a/b-c` with `a-b/c`; acceptable for v1, revisit if it bites.
pub fn project_id_for_path(path: &std::path::Path, home: &std::path::Path) -> String {
    if path == home {
        return GLOBAL_PROJECT_ID.to_string();
    }
    match path.strip_prefix(home) {
        Ok(rel) => {
            let slug = sanitize_id_segment(
                &rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "-"),
            );
            // A leading/trailing or doubled '-' from odd paths is harmless; an empty
            // slug (shouldn't happen, since path != home) falls back to the hash.
            if slug.is_empty() {
                format!("proj-{}", fnv1a_hex16(path.to_string_lossy().as_bytes()))
            } else {
                slug
            }
        }
        Err(_) => format!("proj-{}", fnv1a_hex16(path.to_string_lossy().as_bytes())),
    }
}

/// The centralized store root for a project id, under a harness home (`~/.harness`):
/// `<harness_home>/projects/<id>`.
pub fn project_store_root(harness_home: &std::path::Path, id: &str) -> std::path::PathBuf {
    harness_home.join("projects").join(id)
}

/// Normalized, provider-neutral turn-event kind. Maps both codex
/// (`type`/`item`) and claude (stream-json `type`/subtype) vocabularies onto
/// one taxonomy so the dashboard and a 3rd provider need no per-provider branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessTurnEventKind {
    TurnStarted,
    TurnCompleted,
    MessageDelta,
    Message,
    ToolCall,
    ToolResult,
    Reasoning,
    Usage,
    Error,
    ProviderMeta,
    Unknown,
}

/// A normalized tool invocation (`ToolCall` kind).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub args: serde_json::Value,
}

/// A normalized tool result (`ToolResult` kind).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessToolResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub content: String,
    pub is_error: bool,
}

/// Normalized token usage (`Usage`/`TurnCompleted` kinds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_output_tokens: Option<u64>,
}

/// One normalized turn event. `raw_provider_event` always retains the original
/// provider JSON for audit / debugging / a "show raw" toggle. `seq` is a
/// harness-assigned monotonic per-session counter; `ts` is harness-assigned at
/// ingest. `session_id` is the harness provider-session row id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessTurnEvent {
    pub session_id: String,
    pub provider: String,
    pub seq: u64,
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_item_id: Option<String>,
    pub kind: HarnessTurnEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<HarnessToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<HarnessToolResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<HarnessTokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub raw_provider_event: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentRuntimeHealth {
    #[serde(default)]
    pub process_alive: bool,
    #[serde(default)]
    pub socket_exists: bool,
    #[serde(default)]
    pub protocol_probe: Option<String>,
    #[serde(default)]
    pub delivery_probe: Option<String>,
    #[serde(default)]
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntime {
    pub id: String,
    pub agent_member_id: String,
    pub provider: String,
    pub status: AgentRuntimeStatus,
    pub pid: Option<u32>,
    pub control_endpoint: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub health: AgentRuntimeHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub id: String,
    pub agent_member_id: String,
    pub provider_runtime_id: Option<String>,
    pub task_id: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub provider_child_thread_id: Option<String>,
    pub event_type: String,
    pub summary: String,
    pub payload_ref: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderChildThreadStatus {
    Open,
    Running,
    Completed,
    Interrupted,
    Errored,
    Closed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderChildThread {
    pub id: String,
    pub provider: String,
    pub agent_member_id: String,
    pub provider_runtime_id: Option<String>,
    pub task_id: Option<String>,
    pub parent_provider_thread_id: Option<String>,
    pub provider_thread_id: String,
    pub provider_agent_path: Option<String>,
    pub provider_agent_nickname: Option<String>,
    pub provider_agent_role: Option<String>,
    pub status: ProviderChildThreadStatus,
    pub last_message_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Submitted,
    Accepted,
    Rejected,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub task_id: String,
    pub agent_member_id: String,
    pub title: String,
    pub summary: String,
    pub status: ProposalStatus,
    pub changed_paths: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub task_id: Option<String>,
    pub from_agent_id: String,
    pub to_agent_id: Option<String>,
    pub channel: Option<String>,
    pub kind: MessageKind,
    pub delivery_status: MessageDeliveryStatus,
    pub content: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub delivery: Option<MessageDelivery>,
    /// Identity class of the sender. Defaults to [`SenderKind::Agent`] so existing
    /// records (which omit the field) deserialize unchanged. When
    /// [`SenderKind::Operator`], `from_agent_id` uses the reserved `"operator"` id
    /// convention rather than a roster member id.
    #[serde(default)]
    pub sender_kind: SenderKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub task_id: Option<String>,
    pub source_type: String,
    pub source_ref: String,
    pub summary: String,
    pub created_at: String,
    #[serde(default)]
    pub evidence_kind: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub task_id: String,
    pub decision: String,
    pub rationale: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub decision_kind: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
    #[serde(default)]
    pub is_waiver: bool,
    #[serde(default)]
    pub follow_up_task_id: Option<String>,
}

/// Verdict carried by a [`Review`]. Open enum: the canonical, harness-owned set
/// is modelled as named variants for type safety; any other value supplied by an
/// adapter or skill round-trips through [`ReviewVerdict::Other`].
///
/// `#[serde(other)]` only supports unit variants and would discard the original
/// string, so this uses `from`/`into` String conversions to preserve fidelity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum ReviewVerdict {
    Pass,
    Fail,
    Blocked,
    NeedsChanges,
    Other(String),
}

impl ReviewVerdict {
    pub fn as_str(&self) -> &str {
        match self {
            ReviewVerdict::Pass => "pass",
            ReviewVerdict::Fail => "fail",
            ReviewVerdict::Blocked => "blocked",
            ReviewVerdict::NeedsChanges => "needs_changes",
            ReviewVerdict::Other(value) => value,
        }
    }
}

impl From<String> for ReviewVerdict {
    fn from(value: String) -> Self {
        match value.as_str() {
            "pass" => ReviewVerdict::Pass,
            "fail" => ReviewVerdict::Fail,
            "blocked" => ReviewVerdict::Blocked,
            "needs_changes" => ReviewVerdict::NeedsChanges,
            _ => ReviewVerdict::Other(value),
        }
    }
}

impl From<ReviewVerdict> for String {
    fn from(value: ReviewVerdict) -> Self {
        value.as_str().to_string()
    }
}

/// First-class evaluator/critic output. Today an unstructured report Message; the
/// Review object captures verdict + findings + residual risk as structured data.
///
/// Concept-model invariant: a Review is *evidence for* a Decision, not the global
/// decision itself — a Lead/gate still issues the Decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    pub task_id: Option<String>,
    pub goal_id: Option<String>,
    pub reviewer_agent_id: String,
    pub review_kind: String,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub blockers: Vec<String>,
    pub residual_risk: Option<String>,
    pub missing_validation: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
}

/// Severity of a [`Gap`]. Truly-closed, harness-owned set (matches the GAP
/// ledger P0/P1/P2 convention), so it is a hard enum on both wire and schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapSeverity {
    P0,
    P1,
    P2,
}

/// Lifecycle status of a [`Gap`]. Unifies the GAP checkbox state and the bug
/// ledger state machine into one closed, harness-owned set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapStatus {
    Open,
    InProgress,
    Fixed,
    Blocked,
    Deferred,
    Wontfix,
}

/// A first-class Gap ledger entry, absorbing the bug ledger: a Bug is simply a
/// Gap with `category = "bug"` (plus the optional `repro_ref`/`closing_test_ref`).
///
/// `category` is an open enum (free string): the canonical generic dimensions are
/// ux/data/observability/parity/tooling/workflow/docs/bug/other, but an adapter may
/// keep a domain-flavored category here without a schema bump. `severity` and
/// `status` are closed harness-owned enums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub id: String,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    pub category: String,
    pub severity: GapSeverity,
    pub status: GapStatus,
    pub summary: String,
    pub evidence_ids: Vec<String>,
    pub next_step: Option<String>,
    pub owner_agent_id: Option<String>,
    pub repro_ref: Option<String>,
    pub closing_test_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Executable thesis for a Goal: the generic subset of let-me-try's strategy
/// creation checklist. Graduates from `Evidence(source_type=goal_design)`; both
/// representations coexist (dual-read by `goal_id`, no backfill).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalDesign {
    pub id: String,
    pub goal_id: String,
    pub scenario_summary: String,
    pub non_goals: Vec<String>,
    pub risk_and_permission_boundaries: String,
    pub required_infra: Vec<String>,
    /// Team id or inline team description; nullable when not yet assigned.
    pub agent_team: Option<String>,
    /// Task ids forming the design's task graph.
    pub task_graph: Vec<String>,
    pub evidence_plan: Vec<String>,
    pub acceptance_gates: Vec<String>,
    pub created_at: String,
}

/// Outcome of a [`GoalEvaluation`]. Open enum: the canonical generic set is
/// success/partial/failed/blocked, but an adapter may supply another value that
/// round-trips through [`EvaluationOutcome::Other`] without a schema bump.
///
/// `#[serde(other)]` only supports unit variants and would discard the original
/// string, so this uses `from`/`into` String conversions to preserve fidelity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum EvaluationOutcome {
    Success,
    Partial,
    Failed,
    Blocked,
    Other(String),
}

impl EvaluationOutcome {
    pub fn as_str(&self) -> &str {
        match self {
            EvaluationOutcome::Success => "success",
            EvaluationOutcome::Partial => "partial",
            EvaluationOutcome::Failed => "failed",
            EvaluationOutcome::Blocked => "blocked",
            EvaluationOutcome::Other(value) => value,
        }
    }
}

impl From<String> for EvaluationOutcome {
    fn from(value: String) -> Self {
        match value.as_str() {
            "success" => EvaluationOutcome::Success,
            "partial" => EvaluationOutcome::Partial,
            "failed" => EvaluationOutcome::Failed,
            "blocked" => EvaluationOutcome::Blocked,
            _ => EvaluationOutcome::Other(value),
        }
    }
}

impl From<EvaluationOutcome> for String {
    fn from(value: EvaluationOutcome) -> Self {
        value.as_str().to_string()
    }
}

/// Retrospective for a Goal: what worked / failed, reusable patterns and
/// anti-patterns, and the follow-up / proposed-goal pointers that feed the next
/// round. Graduates from `Evidence(source_type=goal_evaluation)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalEvaluation {
    pub id: String,
    pub goal_id: String,
    pub evaluator_agent_id: String,
    pub outcome: EvaluationOutcome,
    pub what_worked: String,
    pub what_failed: String,
    pub missing_infra: Vec<String>,
    pub missing_evidence: Vec<String>,
    pub team_design_feedback: String,
    pub task_graph_feedback: String,
    pub dashboard_feedback: String,
    pub reusable_patterns: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub follow_up_task_ids: Vec<String>,
    pub proposed_goal_ids: Vec<String>,
    pub created_at: String,
}

/// Reusable teaching artifact distilled from a completed Goal. The human-facing
/// files under `examples/goal-cases/<case-id>/` remain the artifact; this is the
/// optional structured manifest over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalCase {
    pub case_id: String,
    pub source_goal_id: String,
    pub scenario_type: String,
    pub project_adapter: Option<String>,
    pub goal_design_ref: Option<String>,
    pub evaluation_ref: Option<String>,
    pub reusable_patterns: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub follow_up_refs: Vec<String>,
    pub tags: Vec<String>,
    pub created_at: String,
}

/// A durable product vision a Goal can be scheduled against. The autonomous
/// next-goal proposal compares a [`GoalEvaluation`] against the linked Vision;
/// there is no NextRoundPlan object (a proposed goal stays Goal+Task+Message+Decision).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vision {
    pub id: String,
    pub summary: String,
    /// PRD / design-basis doc paths backing the vision.
    pub source_refs: Vec<String>,
    pub created_at: String,
}

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("{field} is required")]
    Required { field: &'static str },
}

fn require_non_empty(value: &str, field: &'static str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::Required { field })
    } else {
        Ok(())
    }
}

impl Validate for AgentMember {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentMember.id")?;
        require_non_empty(&self.name, "AgentMember.name")?;
        require_non_empty(&self.description, "AgentMember.description")?;
        require_non_empty(&self.role, "AgentMember.role")?;
        require_non_empty(&self.provider, "AgentMember.provider")?;
        require_non_empty(&self.created_at, "AgentMember.created_at")
    }
}

impl Validate for AgentTeam {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentTeam.id")?;
        require_non_empty(&self.name, "AgentTeam.name")?;
        require_non_empty(&self.description, "AgentTeam.description")?;
        require_non_empty(&self.owner_agent_id, "AgentTeam.owner_agent_id")?;
        require_non_empty(&self.created_at, "AgentTeam.created_at")?;
        require_non_empty(&self.updated_at, "AgentTeam.updated_at")
    }
}

impl Validate for Goal {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Goal.id")?;
        require_non_empty(&self.title, "Goal.title")?;
        require_non_empty(&self.owner_agent_id, "Goal.owner_agent_id")?;
        require_non_empty(&self.priority, "Goal.priority")?;
        require_non_empty(&self.created_at, "Goal.created_at")?;
        require_non_empty(&self.updated_at, "Goal.updated_at")
    }
}

impl Validate for Task {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Task.id")?;
        require_non_empty(&self.title, "Task.title")?;
        require_non_empty(&self.objective, "Task.objective")?;
        require_non_empty(&self.owner_agent_id, "Task.owner_agent_id")?;
        require_non_empty(&self.created_at, "Task.created_at")?;
        require_non_empty(&self.updated_at, "Task.updated_at")
    }
}

impl Validate for Message {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Message.id")?;
        require_non_empty(&self.from_agent_id, "Message.from_agent_id")?;
        require_non_empty(&self.content, "Message.content")?;
        require_non_empty(&self.created_at, "Message.created_at")
    }
}

impl Validate for ProviderSession {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderSession.id")?;
        require_non_empty(&self.provider, "ProviderSession.provider")?;
        require_non_empty(&self.agent_member_id, "ProviderSession.agent_member_id")?;
        require_non_empty(&self.command, "ProviderSession.command")?;
        require_non_empty(&self.started_at, "ProviderSession.started_at")
    }
}

impl Validate for AgentRuntime {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentRuntime.id")?;
        require_non_empty(&self.agent_member_id, "AgentRuntime.agent_member_id")?;
        require_non_empty(&self.provider, "AgentRuntime.provider")?;
        require_non_empty(&self.command, "AgentRuntime.command")?;
        require_non_empty(&self.started_at, "AgentRuntime.started_at")
    }
}

impl Validate for AgentEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentEvent.id")?;
        require_non_empty(&self.agent_member_id, "AgentEvent.agent_member_id")?;
        require_non_empty(&self.provider, "AgentEvent.provider")?;
        require_non_empty(&self.event_type, "AgentEvent.event_type")?;
        require_non_empty(&self.summary, "AgentEvent.summary")?;
        require_non_empty(&self.created_at, "AgentEvent.created_at")
    }
}

impl Validate for Proposal {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Proposal.id")?;
        require_non_empty(&self.task_id, "Proposal.task_id")?;
        require_non_empty(&self.agent_member_id, "Proposal.agent_member_id")?;
        require_non_empty(&self.title, "Proposal.title")?;
        require_non_empty(&self.summary, "Proposal.summary")?;
        require_non_empty(&self.created_at, "Proposal.created_at")?;
        require_non_empty(&self.updated_at, "Proposal.updated_at")
    }
}

impl Validate for Evidence {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Evidence.id")?;
        require_non_empty(&self.source_type, "Evidence.source_type")?;
        require_non_empty(&self.source_ref, "Evidence.source_ref")?;
        require_non_empty(&self.summary, "Evidence.summary")?;
        require_non_empty(&self.created_at, "Evidence.created_at")
    }
}

impl Validate for Decision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Decision.id")?;
        require_non_empty(&self.task_id, "Decision.task_id")?;
        require_non_empty(&self.decision, "Decision.decision")?;
        require_non_empty(&self.rationale, "Decision.rationale")?;
        require_non_empty(&self.created_at, "Decision.created_at")
    }
}

impl Validate for Review {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Review.id")?;
        require_non_empty(&self.reviewer_agent_id, "Review.reviewer_agent_id")?;
        require_non_empty(&self.review_kind, "Review.review_kind")?;
        require_non_empty(self.verdict.as_str(), "Review.verdict")?;
        require_non_empty(&self.summary, "Review.summary")?;
        require_non_empty(&self.created_at, "Review.created_at")
    }
}

impl Validate for Gap {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Gap.id")?;
        require_non_empty(&self.category, "Gap.category")?;
        require_non_empty(&self.summary, "Gap.summary")?;
        require_non_empty(&self.created_at, "Gap.created_at")?;
        require_non_empty(&self.updated_at, "Gap.updated_at")
    }
}

impl Validate for GoalDesign {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "GoalDesign.id")?;
        require_non_empty(&self.goal_id, "GoalDesign.goal_id")?;
        require_non_empty(&self.scenario_summary, "GoalDesign.scenario_summary")?;
        require_non_empty(
            &self.risk_and_permission_boundaries,
            "GoalDesign.risk_and_permission_boundaries",
        )?;
        require_non_empty(&self.created_at, "GoalDesign.created_at")
    }
}

impl Validate for GoalEvaluation {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "GoalEvaluation.id")?;
        require_non_empty(&self.goal_id, "GoalEvaluation.goal_id")?;
        require_non_empty(
            &self.evaluator_agent_id,
            "GoalEvaluation.evaluator_agent_id",
        )?;
        require_non_empty(self.outcome.as_str(), "GoalEvaluation.outcome")?;
        require_non_empty(&self.what_worked, "GoalEvaluation.what_worked")?;
        require_non_empty(&self.what_failed, "GoalEvaluation.what_failed")?;
        require_non_empty(&self.created_at, "GoalEvaluation.created_at")
    }
}

impl Validate for GoalCase {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.case_id, "GoalCase.case_id")?;
        require_non_empty(&self.source_goal_id, "GoalCase.source_goal_id")?;
        require_non_empty(&self.scenario_type, "GoalCase.scenario_type")?;
        require_non_empty(&self.created_at, "GoalCase.created_at")
    }
}

impl Validate for Vision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Vision.id")?;
        require_non_empty(&self.summary, "Vision.summary")?;
        require_non_empty(&self.created_at, "Vision.created_at")
    }
}

// ---------------------------------------------------------------------------
// Dynamic workflow runtime objects (WP1)
//
// Additive, ADR-0017-style: a `WorkflowRun` is a standalone object with its own
// id and lifecycle. It is NOT bound to a `Goal`/`Task` yet (design decision 2 in
// docs/research/dynamic-workflow-runtime-design.md). Each `WorkflowStep` is the
// workflow-layer wrapper around one `agent()` call; it references the
// `ProviderSession` that the delivery produced rather than re-recording the
// execution. Both journal to their own append-only JSONL with latest-wins
// projection, exactly like every other harness object.
// ---------------------------------------------------------------------------

/// Lifecycle of a [`WorkflowRun`]. WP1 only exercises Running -> Completed and
/// Running -> Failed; Pending/Paused are reserved for the scheduler/resume work
/// packages (WP2/WP4) so existing rows remain forward-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

/// Status of a single [`WorkflowStep`] (one `agent()` call). WP1 uses
/// Running -> Completed / Failed. Queued/Cached are reserved for the
/// scheduler/resume work packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cached,
}

/// One run of a built-in (registered) workflow. The `workflow_name` selects the
/// registered Rust fn (option C in the design). `step_ids` orders the steps in
/// the sequence they were started, so the journal alone reconstructs the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: String,
    pub workflow_name: String,
    pub status: WorkflowRunStatus,
    #[serde(default)]
    pub step_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    /// Optional human-facing summary set when the run reaches a terminal state.
    #[serde(default)]
    pub summary: Option<String>,
    /// Optional JSON parameterization the run was authored with (the dynamic
    /// `run-script` path carries the Starlark `args` global). `None` for registry
    /// runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// How many agent steps this run spawned (the per-run agent count). Defaults
    /// to 0 for legacy rows that predate the field.
    #[serde(default)]
    pub agents_spawned: u64,
    /// The collected structured output of the run (e.g. each step's result),
    /// set when the run reaches a terminal state. `None` while running / legacy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_output: Option<serde_json::Value>,
    /// Who initiated this run — an agent member id (e.g. a Codex / Claude member)
    /// or "operator" for a human-triggered CLI run. `None` for legacy rows that
    /// predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<String>,
    /// The mandatory `design_intent` a Starlark program declares via its
    /// `workflow(name, design_intent)` header — the WHY behind the run's shape.
    /// Every dynamic (`run-script`) run carries it; `None` for registry runs and
    /// legacy rows that predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_intent: Option<String>,
    /// The authored source the dynamic path was run with — for `run-script` the
    /// raw Starlark program text, snapshotted as the small durable audit record
    /// of the run shape. `None` for registry runs / legacy rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<serde_json::Value>,
    /// Retention policy for the heavy per-node provider turn-event trace:
    /// "durable" (default) persists the trace so any completed run can be drilled
    /// into; "live" streams the trace over SSE during execution but does not
    /// retain it. Live streaming is independent of this and always happens.
    #[serde(default = "default_trace_retention")]
    pub trace_retention: String,
    /// OS process id of the `harness workflow run-script`/`run` invocation that
    /// drives this run, stamped on the initial `running` row. The serve-side
    /// reaper uses it to detect an ABANDONED run: if the run is still `running`
    /// but this pid is no longer alive on the host, the driver died (killed /
    /// crashed / Ctrl-C) before journaling a terminal outcome, so the reaper
    /// flips it (and its non-terminal steps) to `failed`. `None` for legacy rows
    /// that predate the field — those fall back to a stale-activity timeout.
    /// Same-host only (the store, serve, and driver all run locally).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_pid: Option<u32>,
    /// True when this run was a `--dry-run` validation (mock driver, no provider
    /// spawned, no tokens spent), false for a real (live) run. A dry-run journals
    /// the SAME `workflow_name` into the SAME store, so without this marker a dry
    /// validation run is easily mistaken for a real one when reading the jsonl or
    /// the dashboard (issue #89 item 2). `#[serde(default)]` → legacy rows read as
    /// `false` (they predate the flag; dry-run journaling is newer).
    #[serde(default)]
    pub dry_run: bool,
}

/// Default retention policy for a [`WorkflowRun`]'s turn-event trace. Legacy rows
/// and registry runs that predate the field deserialize as "durable".
fn default_trace_retention() -> String {
    "durable".to_string()
}

/// One agent step inside a [`WorkflowRun`]. `phase` is the declarative grouping
/// marker (e.g. "audit", "synthesize"); `label` names the step within the phase.
/// `provider_session_id` links to the [`ProviderSession`] the delivery produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub run_id: String,
    pub phase: String,
    pub label: String,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    pub status: WorkflowStepStatus,
    #[serde(default)]
    pub output_summary: Option<String>,
    /// Optional structured result for this step (beyond the human-facing
    /// `output_summary`). The dynamic IR path carries each `StepResult`'s
    /// structured payload here. `None` for legacy / summary-only steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    /// The `Task` this step executed (goal-planning-model: the phase compiler stamps
    /// each step with its task id so outcomes flow back to `Task.status`).
    #[serde(default)]
    pub task_id: Option<String>,
    /// Whether the step's `verdict()` passed or cleanly failed — distinct from a
    /// crash (carried by `status`). Drives the orchestrator's advance/replan choice.
    #[serde(default)]
    pub verdict_outcome: Option<VerdictOutcome>,
}

impl Validate for WorkflowRun {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowRun.id")?;
        require_non_empty(&self.workflow_name, "WorkflowRun.workflow_name")?;
        require_non_empty(&self.created_at, "WorkflowRun.created_at")
    }
}

impl Validate for WorkflowStep {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowStep.id")?;
        require_non_empty(&self.run_id, "WorkflowStep.run_id")?;
        require_non_empty(&self.label, "WorkflowStep.label")?;
        require_non_empty(&self.started_at, "WorkflowStep.started_at")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_round_trips_via_str() {
        for (input, expected) in [
            ("codex", ProviderKind::Codex),
            ("claude", ProviderKind::Claude),
        ] {
            let kind = ProviderKind::from(input);
            assert_eq!(kind, expected);
            // Display must reproduce the original provider string verbatim.
            assert_eq!(kind.to_string(), input);
            assert_eq!(kind.as_str(), input);
        }
    }

    #[test]
    fn provider_kind_unknown_preserves_value() {
        let kind = ProviderKind::from("gemini");
        assert_eq!(kind, ProviderKind::Unknown("gemini".to_string()));
        // Unknown providers round-trip without losing fidelity.
        assert_eq!(kind.to_string(), "gemini");
        assert_eq!(ProviderKind::from("gemini".to_string()), kind);
    }

    #[test]
    fn provider_price_per_mtok_preserves_provider_rates() {
        assert_eq!(provider_price_per_mtok("claude"), (3.0, 15.0));
        assert_eq!(provider_price_per_mtok("codex"), (1.25, 10.0));
        assert_eq!(provider_price_per_mtok("gemini"), (1.25, 10.0));
    }

    #[test]
    fn task_round_trips_json() {
        let task = Task {
            design_md: None,
            phase_id: None,
            superseded_by_knowledge_id: None,
            workflow_step_ids: Vec::new(),
            id: "task-1".to_string(),
            goal_id: Some("goal-1".to_string()),
            parent_task_id: None,
            title: "Inspect issue".to_string(),
            objective: "Find the root cause".to_string(),
            owner_agent_id: "leader-1".to_string(),
            assignee_agent_id: Some("agent-1".to_string()),
            reviewer_agent_id: Some("reviewer-1".to_string()),
            status: TaskStatus::Assigned,
            depends_on_task_ids: vec!["task-0".to_string()],
            workspace_ref: Some("../worktrees/task-1".to_string()),
            branch_ref: Some("agent/task-1".to_string()),
            pr_ref: Some("https://github.com/cyl19970726/multi-agent-harness/pull/1".to_string()),
            owned_paths: vec!["crates/harness-core".to_string()],
            acceptance_criteria: vec!["Evidence is attached".to_string()],
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
            phase: Some("design".to_string()),
            scope_refs: vec!["scope-1".to_string()],
            requires_human_approval: true,
            verdict_decision_id: Some("decision-1".to_string()),
            description: Some("Trace the failing assertion to its root cause.".to_string()),
            git_metadata: Some(GitMetadata {
                branch: Some("agent/task-1".to_string()),
                base_branch: Some("master".to_string()),
                owned_paths: vec!["crates/harness-core".to_string()],
                ..Default::default()
            }),
            outputs: Vec::new(),
        };

        let json = serde_json::to_string(&task).expect("serialize task");
        let parsed: Task = serde_json::from_str(&json).expect("deserialize task");

        assert_eq!(parsed, task);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn goal_round_trips_json() {
        let goal = Goal {
            phases: Vec::new(),
            knowledge: Vec::new(),
            design_synthesis_at: None,
            id: "goal-1".to_string(),
            title: "Self-host MVP".to_string(),
            owner_agent_id: "leader-1".to_string(),
            status: GoalStatus::Active,
            priority: "p0".to_string(),
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
            vision_id: Some("vision-1".to_string()),
            goal_design_id: Some("goal-design-1".to_string()),
            closed_by_decision_id: Some("decision-1".to_string()),
            git_metadata: Some(GitMetadata {
                repo: Some("multi-agent-harness".to_string()),
                branch: Some("feature/self-host".to_string()),
                base_branch: Some("master".to_string()),
                ..Default::default()
            }),
            stage: GoalStage::Explored,
            description_md: Some("what and why".to_string()),
            design_md: Some("key problems first, then overview".to_string()),
            acceptance_md: Some("real acceptance: use it for real".to_string()),
            explorations: vec![Exploration {
                author: "observer".to_string(),
                round: 1,
                notes_md: "first grounded pass".to_string(),
                created_at: "2026-05-26T00:00:00Z".to_string(),
            }],
            skill_refs: vec!["author-goal".to_string()],
            stage_changed_at: Some("2026-05-26T00:00:00Z".to_string()),
        };

        let json = serde_json::to_string(&goal).expect("serialize goal");
        let parsed: Goal = serde_json::from_str(&json).expect("deserialize goal");

        assert_eq!(parsed, goal);
        assert!(parsed.validate().is_ok());
    }

    fn goal_in_stage(stage: GoalStage) -> Goal {
        Goal {
            phases: Vec::new(),
            knowledge: Vec::new(),
            design_synthesis_at: None,
            id: "g".into(),
            title: "t".into(),
            owner_agent_id: "lead".into(),
            status: GoalStatus::Active,
            priority: "p0".into(),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            vision_id: None,
            goal_design_id: None,
            closed_by_decision_id: None,
            git_metadata: None,
            stage,
            description_md: None,
            design_md: None,
            acceptance_md: None,
            explorations: vec![],
            skill_refs: vec![],
            stage_changed_at: None,
        }
    }

    #[test]
    fn goal_stage_defaults_to_draft_for_legacy_rows() {
        // A pre-lifecycle goal row (no stage / md fields) must still deserialize.
        let legacy = r#"{"id":"g","title":"t","owner_agent_id":"lead",
            "status":"active","priority":"p0",
            "created_at":"unix-ms:1","updated_at":"unix-ms:1"}"#;
        let g: Goal = serde_json::from_str(legacy).expect("deserialize legacy goal");
        assert_eq!(g.stage, GoalStage::Draft);
        assert!(g.design_md.is_none());
        assert!(g.explorations.is_empty());
        assert!(g.skill_refs.is_empty());
        // goal-planning-model (S1a): legacy rows default the new planning fields.
        assert!(g.phases.is_empty());
        assert!(g.knowledge.is_empty());
        assert!(g.design_synthesis_at.is_none());
    }

    #[test]
    fn goal_phase_and_knowledge_round_trip_snake_case() {
        let phase = GoalPhase {
            id: "phase-explore".into(),
            name: "Explore".into(),
            intent: "ground the problem space".into(),
            status: GoalPhaseStatus::InProgress,
            acceptance: Some("design written".into()),
            verdict_decision_id: None,
            created_at: "unix-ms:1".into(),
            started_at: Some("unix-ms:2".into()),
            ended_at: None,
            outputs: Vec::new(),
        };
        let pj = serde_json::to_string(&phase).expect("ser phase");
        assert!(
            pj.contains("\"status\":\"in_progress\""),
            "snake_case status: {pj}"
        );
        assert_eq!(
            serde_json::from_str::<GoalPhase>(&pj).expect("de phase"),
            phase
        );

        let k = Knowledge {
            id: "k1".into(),
            goal_id: "g".into(),
            phase_id: Some("phase-explore".into()),
            task_id: Some("t1".into()),
            author: "lead".into(),
            timestamp: "unix-ms:3".into(),
            notes_md: "approach X is unviable because Y".into(),
            tags: vec!["risk".into()],
            source: KnowledgeSource::Task,
            superseded_by_knowledge_id: None,
            created_at: "unix-ms:3".into(),
        };
        let kj = serde_json::to_string(&k).expect("ser knowledge");
        assert!(
            kj.contains("\"source\":\"task\""),
            "snake_case source: {kj}"
        );
        assert_eq!(
            serde_json::from_str::<Knowledge>(&kj).expect("de knowledge"),
            k
        );
    }

    #[test]
    fn goal_with_phases_and_knowledge_round_trips() {
        let mut g = goal_in_stage(GoalStage::Working);
        g.phases = vec![GoalPhase {
            id: "p1".into(),
            name: "Design".into(),
            intent: "i".into(),
            status: GoalPhaseStatus::Passed,
            acceptance: None,
            verdict_decision_id: None,
            created_at: "unix-ms:1".into(),
            started_at: None,
            ended_at: None,
            outputs: Vec::new(),
        }];
        g.knowledge = vec![Knowledge {
            id: "k1".into(),
            goal_id: "g".into(),
            phase_id: Some("p1".into()),
            task_id: None,
            author: "lead".into(),
            timestamp: "unix-ms:1".into(),
            notes_md: "n".into(),
            tags: Vec::new(),
            source: KnowledgeSource::Exploration,
            superseded_by_knowledge_id: None,
            created_at: "unix-ms:1".into(),
        }];
        g.design_synthesis_at = Some("unix-ms:2".into());
        let j = serde_json::to_string(&g).expect("ser goal");
        assert_eq!(serde_json::from_str::<Goal>(&j).expect("de goal"), g);
    }

    #[test]
    fn task_superseded_and_planning_fields_round_trip_and_legacy_defaults() {
        // A legacy task (no planning fields) defaults them.
        let legacy = r#"{"id":"t","goal_id":null,"parent_task_id":null,"title":"x","objective":"o",
            "owner_agent_id":"lead","assignee_agent_id":null,"reviewer_agent_id":null,
            "status":"planned","depends_on_task_ids":[],"workspace_ref":null,"branch_ref":null,
            "pr_ref":null,"owned_paths":[],"acceptance_criteria":[],
            "created_at":"unix-ms:1","updated_at":"unix-ms:1"}"#;
        let mut t: Task = serde_json::from_str(legacy).expect("de legacy task");
        assert!(
            t.design_md.is_none()
                && t.phase_id.is_none()
                && t.superseded_by_knowledge_id.is_none()
                && t.workflow_step_ids.is_empty()
                // goal-phase-artifacts: a legacy task with no `outputs` key defaults empty.
                && t.outputs.is_empty()
        );
        // Superseded + the new fields round-trip.
        t.status = TaskStatus::Superseded;
        t.phase_id = Some("p1".into());
        t.design_md = Some("task design slice".into());
        t.superseded_by_knowledge_id = Some("k1".into());
        t.workflow_step_ids = vec!["step-1".into()];
        let j = serde_json::to_string(&t).expect("ser task");
        let back: Task = serde_json::from_str(&j).expect("de task");
        assert_eq!(back, t);
        assert_eq!(back.status, TaskStatus::Superseded);
    }

    #[test]
    fn artifact_spec_and_kind_round_trip_snake_case_and_required_defaults_true() {
        // ArtifactKind serializes snake_case (the multi-word kinds are the bar).
        assert_eq!(
            serde_json::to_string(&ArtifactKind::DesignDoc).unwrap(),
            "\"design_doc\""
        );
        assert_eq!(
            serde_json::to_string(&ArtifactKind::TestReport).unwrap(),
            "\"test_report\""
        );
        assert_eq!(
            serde_json::to_string(&ArtifactKind::MigrationDoc).unwrap(),
            "\"migration_doc\""
        );
        assert_eq!(
            serde_json::to_string(&ArtifactKind::RegisteredDoc).unwrap(),
            "\"registered_doc\""
        );
        // Default kind is Code.
        assert_eq!(ArtifactKind::default(), ArtifactKind::Code);
        assert_eq!(
            serde_json::from_str::<ArtifactKind>("\"adr\"").unwrap(),
            ArtifactKind::Adr
        );

        // A fully-specified spec round-trips.
        let spec = ArtifactSpec {
            id: "report".into(),
            kind: ArtifactKind::TestReport,
            path: Some("reports/r.md".into()),
            purpose: "the live-acceptance evidence".into(),
            required: false,
            acceptance: Some("contains a PASS line".into()),
        };
        let j = serde_json::to_string(&spec).expect("ser spec");
        assert_eq!(
            serde_json::from_str::<ArtifactSpec>(&j).expect("de spec"),
            spec
        );

        // A minimal spec (no kind/path/required/acceptance keys) defaults: kind=Code,
        // path=None, required=TRUE, acceptance=None.
        let minimal: ArtifactSpec =
            serde_json::from_str(r#"{"id":"a","purpose":"p"}"#).expect("de minimal spec");
        assert_eq!(minimal.kind, ArtifactKind::Code);
        assert!(minimal.path.is_none());
        assert!(minimal.required, "required must default to true");
        assert!(minimal.acceptance.is_none());
        // And `Default` agrees on the required=true invariant.
        assert!(ArtifactSpec::default().required);
    }

    #[test]
    fn phase_and_task_outputs_round_trip_and_legacy_defaults_empty() {
        let outputs = vec![
            ArtifactSpec {
                id: "design".into(),
                kind: ArtifactKind::DesignDoc,
                path: Some("docs/design.md".into()),
                purpose: "the phase design".into(),
                required: true,
                acceptance: None,
            },
            ArtifactSpec {
                id: "shot".into(),
                kind: ArtifactKind::Screenshot,
                path: None,
                purpose: "dashboard picker proof".into(),
                required: false,
                acceptance: None,
            },
        ];

        // GoalPhase with non-empty outputs round-trips.
        let mut phase = test_phase("p-out", GoalPhaseStatus::InProgress);
        phase.outputs = outputs.clone();
        let pj = serde_json::to_string(&phase).expect("ser phase");
        assert_eq!(
            serde_json::from_str::<GoalPhase>(&pj).expect("de phase"),
            phase
        );

        // Task with non-empty outputs round-trips.
        let mut task = compile_task("t-out", "p-out", &[], &["crates/x"]);
        task.outputs = outputs;
        let tj = serde_json::to_string(&task).expect("ser task");
        assert_eq!(serde_json::from_str::<Task>(&tj).expect("de task"), task);

        // A legacy GoalPhase JSON WITHOUT the `outputs` key defaults to empty.
        let legacy_phase = r#"{"id":"p","name":"P","intent":"i","status":"not_started",
            "created_at":"unix-ms:1"}"#;
        let p: GoalPhase = serde_json::from_str(legacy_phase).expect("de legacy phase");
        assert!(p.outputs.is_empty());

        // A legacy Task JSON WITHOUT the `outputs` key defaults to empty.
        let legacy_task = r#"{"id":"t","goal_id":null,"parent_task_id":null,"title":"x",
            "objective":"o","owner_agent_id":"lead","assignee_agent_id":null,
            "reviewer_agent_id":null,"status":"planned","depends_on_task_ids":[],
            "workspace_ref":null,"branch_ref":null,"pr_ref":null,"owned_paths":[],
            "acceptance_criteria":[],"created_at":"unix-ms:1","updated_at":"unix-ms:1"}"#;
        let lt: Task = serde_json::from_str(legacy_task).expect("de legacy task");
        assert!(lt.outputs.is_empty());
    }

    #[test]
    fn goal_orchestration_run_round_trips_and_status_is_snake_case() {
        let run = GoalOrchestrationRun {
            id: "goalrun-1".into(),
            goal_id: "g".into(),
            status: OrchestrationStatus::Failed,
            phase_runs: vec![OrchestrationPhaseRun {
                phase_id: "p1".into(),
                workflow_run_id: Some("wfrun-1".into()),
                compiled_path: Some(".harness/compiled/g__p1__abc.star".into()),
                passed: false,
                started_at: "unix-ms:1".into(),
                ended_at: Some("unix-ms:2".into()),
            }],
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
        };
        let j = serde_json::to_string(&run).expect("ser");
        assert!(
            j.contains("\"status\":\"failed\""),
            "snake_case status: {j}"
        );
        assert_eq!(
            serde_json::from_str::<GoalOrchestrationRun>(&j).expect("de"),
            run
        );
        // Legacy/default: status defaults to running, phase_runs to empty.
        let legacy =
            r#"{"id":"r","goal_id":"g","created_at":"unix-ms:1","updated_at":"unix-ms:1"}"#;
        let d: GoalOrchestrationRun = serde_json::from_str(legacy).expect("de legacy");
        assert_eq!(d.status, OrchestrationStatus::Running);
        assert!(d.phase_runs.is_empty());
    }

    #[test]
    fn workflow_step_task_id_and_verdict_outcome_round_trip_and_legacy_defaults() {
        let legacy = r#"{"id":"s","run_id":"r","phase":"p","label":"l",
            "status":"running","started_at":"unix-ms:1"}"#;
        let mut s: WorkflowStep = serde_json::from_str(legacy).expect("de legacy step");
        assert!(s.task_id.is_none() && s.verdict_outcome.is_none());
        s.task_id = Some("t1".into());
        s.verdict_outcome = Some(VerdictOutcome::CleanFail);
        let j = serde_json::to_string(&s).expect("ser step");
        assert!(
            j.contains("\"verdict_outcome\":\"clean_fail\""),
            "snake_case verdict_outcome: {j}"
        );
        assert_eq!(
            serde_json::from_str::<WorkflowStep>(&j).expect("de step"),
            s
        );
    }

    #[test]
    fn explored_gate_requires_design_md() {
        let mut g = goal_in_stage(GoalStage::Exploring);
        assert!(g.check_transition(GoalStage::Explored).is_err());
        g.design_md = Some("grounded design with key problems".into());
        assert!(g.check_transition(GoalStage::Explored).is_ok());
    }

    #[test]
    fn working_gate_requires_acceptance_md() {
        let mut g = goal_in_stage(GoalStage::Explored);
        assert!(g.check_transition(GoalStage::Working).is_err());
        g.acceptance_md = Some("real acceptance: use it for real".into());
        assert!(g.check_transition(GoalStage::Working).is_ok());
    }

    #[test]
    fn back_edges_are_allowed() {
        for s in [GoalStage::Draft, GoalStage::Working, GoalStage::Verified] {
            assert!(
                goal_in_stage(s)
                    .check_transition(GoalStage::Exploring)
                    .is_ok(),
                "{:?} should be able to re-open exploration",
                s
            );
        }
        assert!(goal_in_stage(GoalStage::Verifying)
            .check_transition(GoalStage::Working)
            .is_ok());
    }

    #[test]
    fn forward_skip_and_same_stage_rejected() {
        assert!(goal_in_stage(GoalStage::Draft)
            .check_transition(GoalStage::Working)
            .is_err());
        assert!(goal_in_stage(GoalStage::Working)
            .check_transition(GoalStage::Working)
            .is_err());
        // verifying -> verified is a clean one-step forward (no gate).
        assert!(goal_in_stage(GoalStage::Verifying)
            .check_transition(GoalStage::Verified)
            .is_ok());
    }

    #[test]
    fn stage_maps_to_legacy_status() {
        assert_eq!(GoalStage::Working.to_status(), GoalStatus::Active);
        assert_eq!(GoalStage::Done.to_status(), GoalStatus::Review);
        assert_eq!(GoalStage::Verified.to_status(), GoalStatus::Done);
    }

    fn test_phase(id: &str, status: GoalPhaseStatus) -> GoalPhase {
        GoalPhase {
            id: id.into(),
            name: id.into(),
            intent: "i".into(),
            status,
            acceptance: None,
            verdict_decision_id: None,
            created_at: "unix-ms:1".into(),
            started_at: None,
            ended_at: None,
            outputs: Vec::new(),
        }
    }

    #[test]
    fn effective_stage_falls_back_to_stage_field_when_no_phases() {
        // Legacy goal (no phases) → the stored stage IS the truth.
        for s in [GoalStage::Draft, GoalStage::Exploring, GoalStage::Working] {
            assert_eq!(goal_in_stage(s).effective_stage(), s);
        }
    }

    #[test]
    fn effective_stage_derives_from_phases_when_present() {
        use GoalPhaseStatus::*;
        // The stored `stage` is deliberately stale to prove phases win.
        let with = |statuses: &[GoalPhaseStatus]| {
            let mut g = goal_in_stage(GoalStage::Draft);
            g.phases = statuses
                .iter()
                .enumerate()
                .map(|(i, s)| test_phase(&format!("p{i}"), *s))
                .collect();
            g.effective_stage()
        };
        assert_eq!(with(&[NotStarted, NotStarted]), GoalStage::Draft);
        assert_eq!(with(&[InProgress, NotStarted]), GoalStage::Working);
        assert_eq!(with(&[Passed, InProgress]), GoalStage::Working);
        assert_eq!(with(&[Passed, Failed]), GoalStage::Working);
        assert_eq!(with(&[Blocked]), GoalStage::Working);
        assert_eq!(with(&[Passed, Passed]), GoalStage::Verified);
    }

    #[test]
    fn phase_driven_goal_skips_legacy_md_gates_but_keeps_structural_rules() {
        // One in-progress phase → effective_stage == Working, with blank md.
        let mut g = goal_in_stage(GoalStage::Draft);
        g.design_md = None;
        g.acceptance_md = None;
        g.phases = vec![test_phase("p0", GoalPhaseStatus::InProgress)];
        assert_eq!(g.effective_stage(), GoalStage::Working);
        // Working → Done is a clean one-step forward; the md gates do NOT fire.
        assert!(g.check_transition(GoalStage::Done).is_ok());
        // Structural rules still hold: can't skip two stages at once.
        assert!(g.check_transition(GoalStage::Verified).is_err());
        // Back-edge to exploring is still always allowed.
        assert!(g.check_transition(GoalStage::Exploring).is_ok());
    }

    fn test_knowledge(id: &str, phase_id: Option<&str>, superseded: Option<&str>) -> Knowledge {
        Knowledge {
            id: id.into(),
            goal_id: "g".into(),
            phase_id: phase_id.map(Into::into),
            task_id: Some(format!("task-{id}")),
            author: "explorer".into(),
            timestamp: "unix-ms:1".into(),
            notes_md: format!("finding {id}"),
            tags: vec!["risk".into()],
            source: KnowledgeSource::Exploration,
            superseded_by_knowledge_id: superseded.map(Into::into),
            created_at: "unix-ms:1".into(),
        }
    }

    #[test]
    fn synthesize_design_md_rejects_empty_knowledge() {
        let g = goal_in_stage(GoalStage::Working);
        assert!(g.knowledge.is_empty());
        assert!(g.synthesize_design_md().is_err());
    }

    #[test]
    fn synthesize_design_md_is_deterministic_and_groups_by_phase() {
        let mut g = goal_in_stage(GoalStage::Working);
        g.phases = vec![
            test_phase("phase-a", GoalPhaseStatus::Passed),
            test_phase("phase-b", GoalPhaseStatus::InProgress),
        ];
        g.knowledge = vec![
            test_knowledge("k1", Some("phase-a"), None),
            test_knowledge("k2", Some("phase-b"), None),
        ];
        let first = g.synthesize_design_md().expect("synthesize");
        let second = g.synthesize_design_md().expect("synthesize again");
        // Pure function of knowledge + phases → byte-identical (no timestamps).
        assert_eq!(first, second);
        assert!(first.contains("## Phase: phase-a (phase-a)"));
        assert!(first.contains("## Phase: phase-b (phase-b)"));
        assert!(first.contains("knowledge#k1"));
        assert!(first.contains("task#task-k1"));
        assert!(first.contains("finding k2"));
        // phase-a precedes phase-b (plan order).
        assert!(first.find("phase-a").unwrap() < first.find("phase-b").unwrap());
    }

    fn compile_task(id: &str, phase_id: &str, deps: &[&str], owned: &[&str]) -> Task {
        Task {
            id: id.into(),
            goal_id: Some("g".into()),
            parent_task_id: None,
            title: format!("title {id}"),
            objective: format!("objective {id}"),
            owner_agent_id: "lead".into(),
            assignee_agent_id: None,
            reviewer_agent_id: None,
            status: TaskStatus::Planned,
            depends_on_task_ids: deps.iter().map(|s| s.to_string()).collect(),
            workspace_ref: None,
            branch_ref: None,
            pr_ref: None,
            owned_paths: owned.iter().map(|s| s.to_string()).collect(),
            acceptance_criteria: Vec::new(),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            phase: None,
            scope_refs: Vec::new(),
            requires_human_approval: false,
            verdict_decision_id: None,
            description: None,
            git_metadata: None,
            design_md: Some(format!("design for {id}")),
            phase_id: Some(phase_id.into()),
            superseded_by_knowledge_id: None,
            workflow_step_ids: Vec::new(),
            outputs: Vec::new(),
        }
    }

    #[test]
    fn compile_phase_parallelizes_disjoint_and_serializes_chain_and_emits_verdict() {
        let goal = goal_in_stage(GoalStage::Working);
        let mut phase = test_phase("phase-1", GoalPhaseStatus::InProgress);
        phase.acceptance = Some("All wiring compiles and the smoke test passes.".into());
        let tasks = vec![
            // Layer 0: two independent writers with disjoint paths → parallel().
            compile_task("t-a", "phase-1", &[], &["crates/a"]),
            compile_task("t-b", "phase-1", &[], &["crates/b"]),
            // Layer 1: depends on both → serial agent() after the parallel block.
            compile_task("t-c", "phase-1", &["t-a", "t-b"], &["crates/c"]),
            // Different phase / superseded → excluded.
            compile_task("t-other", "phase-2", &[], &["x"]),
        ];
        let mut superseded = compile_task("t-dead", "phase-1", &[], &["crates/a"]);
        superseded.status = TaskStatus::Superseded;
        let mut all = tasks;
        all.push(superseded);

        let script = compile_phase_to_starlark(&goal, &phase, &all).expect("compile");
        // Header + auto-generated marker.
        assert!(script.starts_with("workflow(\n    \"phase-phase-1\""));
        assert!(script.contains("Auto-generated by `harness phase compile`"));
        // Layer 0 → one parallel() block carrying both disjoint writers.
        assert!(script.contains("parallel(["));
        assert!(script.contains("\"label\": \"t-a\""));
        assert!(script.contains("\"label\": \"t-b\""));
        // Writers are isolated.
        assert!(script.contains("\"isolation\": \"worktree\""));
        assert!(script.contains("\"writable\": True"));
        // Layer 1 → a serial agent() AFTER the parallel block.
        let par = script.find("parallel([").unwrap();
        let tc = script.find("label=\"t-c\"").expect("t-c agent call");
        assert!(
            par < tc,
            "the dependent task must come after the parallel layer"
        );
        // Excluded tasks are absent.
        assert!(!script.contains("t-other"));
        assert!(!script.contains("t-dead"));
        // Acceptance → judge agent + verdict gate.
        assert!(script.contains("label=\"verdict-phase-1\""));
        assert!(script.contains("\"pass\": \"bool\""));
        assert!(script.contains("verdict(bool(_acc)"));
    }

    #[test]
    fn compile_phase_is_deterministic_for_identical_dag() {
        let goal = goal_in_stage(GoalStage::Working);
        let phase = test_phase("p", GoalPhaseStatus::InProgress);
        let all = vec![
            compile_task("t2", "p", &["t1"], &["b"]),
            compile_task("t1", "p", &[], &["a"]),
        ];
        let first = compile_phase_to_starlark(&goal, &phase, &all).expect("compile");
        // Input order shuffled — output must be byte-identical (sorted by id).
        let shuffled = vec![all[1].clone(), all[0].clone()];
        let second = compile_phase_to_starlark(&goal, &phase, &shuffled).expect("compile");
        assert_eq!(first, second);
        assert_eq!(content_hash_hex16(&first), content_hash_hex16(&second));
    }

    #[test]
    fn compile_phase_errors_on_empty_and_on_cycle() {
        let goal = goal_in_stage(GoalStage::Working);
        let phase = test_phase("p", GoalPhaseStatus::InProgress);
        assert!(compile_phase_to_starlark(&goal, &phase, &[]).is_err());
        let cyclic = vec![
            compile_task("t1", "p", &["t2"], &["a"]),
            compile_task("t2", "p", &["t1"], &["b"]),
        ];
        let err = compile_phase_to_starlark(&goal, &phase, &cyclic).unwrap_err();
        assert!(err.contains("cycle"), "got: {err}");
    }

    #[test]
    fn compile_phase_injects_required_artifacts_block_for_tasks_with_outputs() {
        let goal = goal_in_stage(GoalStage::Working);
        let phase = test_phase("p", GoalPhaseStatus::InProgress);
        // A task that declares outputs (one required, one optional) — its compiled
        // worker prompt must list each artifact's path + purpose + required/optional.
        let mut with_outputs = compile_task("t-doc", "p", &[], &["docs"]);
        with_outputs.outputs = vec![
            ArtifactSpec {
                id: "report".into(),
                kind: ArtifactKind::TestReport,
                path: Some("docs/report.md".into()),
                purpose: "the e2e test report".into(),
                required: true,
                acceptance: Some("covers the gate path".into()),
            },
            ArtifactSpec {
                id: "shot".into(),
                kind: ArtifactKind::Screenshot,
                path: Some("docs/shot.png".into()),
                purpose: "dashboard screenshot".into(),
                required: false,
                acceptance: None,
            },
        ];
        // A second task with NO outputs must get no artifact block (today's default).
        let no_outputs = compile_task("t-code", "p", &[], &["crates/x"]);
        let all = vec![with_outputs, no_outputs];

        let script = compile_phase_to_starlark(&goal, &phase, &all).expect("compile");
        assert!(
            script.contains("Required artifacts you MUST produce:"),
            "the artifact block header must appear"
        );
        assert!(script.contains("docs/report.md — the e2e test report (required)"));
        assert!(script.contains("[acceptance: covers the gate path]"));
        assert!(script.contains("docs/shot.png — dashboard screenshot (optional)"));
        // The task without outputs must not carry an artifact block of its own;
        // the header appears exactly once (only for t-doc).
        assert_eq!(
            script
                .matches("Required artifacts you MUST produce:")
                .count(),
            1,
            "only the task that declared outputs gets the block"
        );
    }

    #[test]
    fn planner_and_reviser_schemas_and_prompts_advertise_outputs() {
        // The autonomous planner/reviser can only author a manifest if `outputs`
        // is in the schema shape hint and mentioned in the prompt.
        let planner = serde_json::to_string(&planner_schema()).unwrap();
        assert!(
            planner.contains("outputs"),
            "planner schema must list outputs"
        );
        let reviser = serde_json::to_string(&reviser_schema()).unwrap();
        assert!(
            reviser.contains("outputs"),
            "reviser schema must list outputs"
        );

        let goal = goal_in_stage(GoalStage::Working);
        assert!(planner_prompt(&goal).contains("outputs"));

        let phase = test_phase("p", GoalPhaseStatus::InProgress);
        let prompt = reviser_prompt(&goal, &phase, &[], "the gate found no report");
        assert!(prompt.contains("outputs"));
    }

    #[test]
    fn synthesize_design_md_marks_superseded_and_buckets_unscoped() {
        let mut g = goal_in_stage(GoalStage::Working);
        g.phases = vec![test_phase("phase-a", GoalPhaseStatus::InProgress)];
        g.knowledge = vec![
            test_knowledge("k1", Some("phase-a"), Some("k3")),
            test_knowledge("k2", None, None),
            test_knowledge("k3", Some("unknown-phase"), None),
        ];
        let md = g.synthesize_design_md().expect("synthesize");
        assert!(md.contains("superseded by knowledge#k3"));
        // k2 (no phase) and k3 (unknown phase) both fall under Unscoped.
        assert!(md.contains("## Unscoped"));
        let unscoped = &md[md.find("## Unscoped").unwrap()..];
        assert!(unscoped.contains("knowledge#k2"));
        assert!(unscoped.contains("knowledge#k3"));
    }

    #[test]
    fn review_round_trips_json() {
        let review = Review {
            id: "review-1".to_string(),
            task_id: Some("task-1".to_string()),
            goal_id: Some("goal-1".to_string()),
            reviewer_agent_id: "evaluator-1".to_string(),
            review_kind: "acceptance".to_string(),
            verdict: ReviewVerdict::Pass,
            summary: "Acceptance gates met; evidence backs the verdict.".to_string(),
            blockers: vec![],
            residual_risk: Some("Snapshot regeneration not yet automated.".to_string()),
            missing_validation: vec!["load test deferred".to_string()],
            evidence_ids: vec!["evidence-1".to_string()],
            created_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&review).expect("serialize review");
        let parsed: Review = serde_json::from_str(&json).expect("deserialize review");

        assert_eq!(parsed, review);
        assert!(parsed.validate().is_ok());
        // Canonical verdict serializes to its snake_case wire value.
        assert!(json.contains("\"verdict\":\"pass\""));
    }

    #[test]
    fn review_verdict_open_enum_round_trips_unknown_value() {
        // An adapter-supplied verdict that is not in the canonical set must
        // round-trip through ReviewVerdict::Other without losing the string.
        let review = Review {
            id: "review-2".to_string(),
            task_id: None,
            goal_id: Some("goal-1".to_string()),
            reviewer_agent_id: "critic-1".to_string(),
            review_kind: "safety".to_string(),
            verdict: ReviewVerdict::Other("conditional_pass".to_string()),
            summary: "Goal-level review with adapter verdict.".to_string(),
            blockers: vec!["needs second safety sign-off".to_string()],
            residual_risk: None,
            missing_validation: vec![],
            evidence_ids: vec![],
            created_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&review).expect("serialize review");
        assert!(json.contains("\"verdict\":\"conditional_pass\""));

        let parsed: Review = serde_json::from_str(&json).expect("deserialize review");
        assert_eq!(
            parsed.verdict,
            ReviewVerdict::Other("conditional_pass".to_string())
        );
        assert_eq!(parsed, review);
        assert!(parsed.validate().is_ok());

        // A canonical value deserialized from the wire collapses to its named variant.
        let canonical: Review =
            serde_json::from_str(&json.replace("conditional_pass", "needs_changes"))
                .expect("deserialize canonical verdict");
        assert_eq!(canonical.verdict, ReviewVerdict::NeedsChanges);
    }

    #[test]
    fn gap_round_trips_json() {
        let gap = Gap {
            id: "gap-1".to_string(),
            goal_id: Some("goal-1".to_string()),
            task_id: None,
            category: "observability".to_string(),
            severity: GapSeverity::P1,
            status: GapStatus::Open,
            summary: "Dashboard does not surface open reviews per task.".to_string(),
            evidence_ids: vec!["evidence-1".to_string()],
            next_step: Some("Wire reviewsByTask into the task surface.".to_string()),
            owner_agent_id: Some("worker-1".to_string()),
            repro_ref: None,
            closing_test_ref: None,
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&gap).expect("serialize gap");
        let parsed: Gap = serde_json::from_str(&json).expect("deserialize gap");

        assert_eq!(parsed, gap);
        assert!(parsed.validate().is_ok());
        // Closed severity/status enums serialize to their snake_case wire values.
        assert!(json.contains("\"severity\":\"p1\""));
        assert!(json.contains("\"status\":\"open\""));
    }

    #[test]
    fn gap_bug_round_trips_with_bug_fields() {
        // A Bug is a Gap with category="bug" carrying the optional repro/closing-test
        // refs; no separate Bug object exists.
        let bug = Gap {
            id: "gap-bug-1".to_string(),
            goal_id: None,
            task_id: Some("task-1".to_string()),
            category: "bug".to_string(),
            severity: GapSeverity::P0,
            status: GapStatus::InProgress,
            summary: "Snapshot serialization drops the new gaps key.".to_string(),
            evidence_ids: vec![],
            next_step: None,
            owner_agent_id: Some("worker-2".to_string()),
            repro_ref: Some("artifacts/repro-1.log".to_string()),
            closing_test_ref: Some("crates/harness-cli/src/main.rs::snapshot_test".to_string()),
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T01:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&bug).expect("serialize bug gap");
        let parsed: Gap = serde_json::from_str(&json).expect("deserialize bug gap");

        assert_eq!(parsed, bug);
        assert!(parsed.validate().is_ok());
        assert!(json.contains("\"status\":\"in_progress\""));
        assert_eq!(parsed.severity, GapSeverity::P0);
    }

    #[test]
    fn goal_design_round_trips_json() {
        let design = GoalDesign {
            id: "goal-design-1".to_string(),
            goal_id: "goal-1".to_string(),
            scenario_summary: "Render the learning layer on the dashboard.".to_string(),
            non_goals: vec!["No backfill of legacy Evidence rows.".to_string()],
            risk_and_permission_boundaries: "Read-only snapshot; no auto-merge.".to_string(),
            required_infra: vec!["harness-store goal_designs.jsonl".to_string()],
            agent_team: Some("team-1".to_string()),
            task_graph: vec!["task-1".to_string(), "task-2".to_string()],
            evidence_plan: vec!["screenshot of GoalDocument".to_string()],
            acceptance_gates: vec![
                "cargo test green".to_string(),
                "pnpm check green".to_string(),
            ],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&design).expect("serialize goal design");
        let parsed: GoalDesign = serde_json::from_str(&json).expect("deserialize goal design");

        assert_eq!(parsed, design);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn goal_evaluation_round_trips_json() {
        let evaluation = GoalEvaluation {
            id: "goal-eval-1".to_string(),
            goal_id: "goal-1".to_string(),
            evaluator_agent_id: "evaluator-1".to_string(),
            outcome: EvaluationOutcome::Success,
            what_worked: "Dual-read union surfaced both objects and legacy evidence.".to_string(),
            what_failed: "Demo snapshot lagged the new keys until late.".to_string(),
            missing_infra: vec![],
            missing_evidence: vec!["load test".to_string()],
            team_design_feedback: "Solo WP was sufficient.".to_string(),
            task_graph_feedback: "Linear graph held.".to_string(),
            dashboard_feedback: "GoalDocument now shows real sections.".to_string(),
            reusable_patterns: vec!["additive-optional fields".to_string()],
            anti_patterns: vec!["required new fields on existing objects".to_string()],
            follow_up_task_ids: vec!["task-3".to_string()],
            proposed_goal_ids: vec!["goal-2".to_string()],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&evaluation).expect("serialize goal evaluation");
        let parsed: GoalEvaluation =
            serde_json::from_str(&json).expect("deserialize goal evaluation");

        assert_eq!(parsed, evaluation);
        assert!(parsed.validate().is_ok());
        assert!(json.contains("\"outcome\":\"success\""));
    }

    #[test]
    fn evaluation_outcome_open_enum_round_trips_unknown_value() {
        // An adapter-supplied outcome that is not in the canonical set must
        // round-trip through EvaluationOutcome::Other without losing the string.
        let outcome = EvaluationOutcome::Other("partially_blocked".to_string());
        let json = serde_json::to_string(&outcome).expect("serialize outcome");
        assert_eq!(json, "\"partially_blocked\"");

        let parsed: EvaluationOutcome = serde_json::from_str(&json).expect("deserialize outcome");
        assert_eq!(
            parsed,
            EvaluationOutcome::Other("partially_blocked".to_string())
        );

        // A canonical value deserialized from the wire collapses to its named variant.
        let canonical: EvaluationOutcome =
            serde_json::from_str("\"partial\"").expect("deserialize canonical outcome");
        assert_eq!(canonical, EvaluationOutcome::Partial);
    }

    #[test]
    fn goal_case_round_trips_json() {
        let case = GoalCase {
            case_id: "goal-case-1".to_string(),
            source_goal_id: "goal-1".to_string(),
            scenario_type: "dashboard-rendering".to_string(),
            project_adapter: None,
            goal_design_ref: Some("goal-design-1".to_string()),
            evaluation_ref: Some("goal-eval-1".to_string()),
            reusable_patterns: vec!["additive-optional fields".to_string()],
            anti_patterns: vec![],
            follow_up_refs: vec!["task-3".to_string()],
            tags: vec!["learning-layer".to_string()],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&case).expect("serialize goal case");
        let parsed: GoalCase = serde_json::from_str(&json).expect("deserialize goal case");

        assert_eq!(parsed, case);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn vision_round_trips_json() {
        let vision = Vision {
            id: "vision-1".to_string(),
            summary: "Generic harness object-model with a closed learning loop.".to_string(),
            source_refs: vec!["docs/goal-learning-loop.md".to_string()],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&vision).expect("serialize vision");
        let parsed: Vision = serde_json::from_str(&json).expect("deserialize vision");

        assert_eq!(parsed, vision);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn provider_session_round_trips_json() {
        let session = ProviderSession {
            id: "session-1".to_string(),
            provider: "codex".to_string(),
            agent_member_id: "agent-1".to_string(),
            task_id: Some("task-1".to_string()),
            workspace_ref: Some("../worktrees/task-1".to_string()),
            provider_thread_id: Some("thread-1".to_string()),
            provider_turn_id: Some("turn-1".to_string()),
            terminal_source: Some(MessageTerminalSource::TurnCompleted),
            status: ProviderSessionStatus::Succeeded,
            command: "codex".to_string(),
            args: vec!["exec".to_string()],
            prompt_ref: Some(".harness/prompts/task-1.md".to_string()),
            prompt_summary: Some("Implement task-1".to_string()),
            provider_session_ref: None,
            stdout_ref: Some(".harness/provider-sessions/session-1/stdout.log".to_string()),
            jsonl_ref: Some(".harness/provider-sessions/session-1/events.jsonl".to_string()),
            transcript_ref: None,
            last_message_ref: Some(
                ".harness/provider-sessions/session-1/last-message.md".to_string(),
            ),
            exit_code: Some(0),
            started_at: "2026-05-26T00:00:00Z".to_string(),
            ended_at: Some("2026-05-26T00:05:00Z".to_string()),
            evidence_ids: vec!["evidence-1".to_string()],
        };

        let json = serde_json::to_string(&session).expect("serialize provider session");
        let parsed: ProviderSession =
            serde_json::from_str(&json).expect("deserialize provider session");

        assert_eq!(parsed, session);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn project_id_for_path_home_is_global() {
        let home = std::path::Path::new("/Users/me");
        assert_eq!(project_id_for_path(home, home), GLOBAL_PROJECT_ID);
    }

    #[test]
    fn project_id_for_path_under_home_flattens_to_slug() {
        let home = std::path::Path::new("/Users/me");
        assert_eq!(
            project_id_for_path(std::path::Path::new("/Users/me/multi-agent-harness"), home),
            "multi-agent-harness"
        );
        assert_eq!(
            project_id_for_path(std::path::Path::new("/Users/me/ai-luodi/jyx3d"), home),
            "ai-luodi-jyx3d"
        );
    }

    #[test]
    fn project_id_for_path_outside_home_is_stable_hash() {
        let home = std::path::Path::new("/Users/me");
        let id = project_id_for_path(std::path::Path::new("/opt/work/thing"), home);
        assert!(id.starts_with("proj-"), "external path → hashed id: {id}");
        // Stable across calls (a durable id must not change run-to-run).
        assert_eq!(
            id,
            project_id_for_path(std::path::Path::new("/opt/work/thing"), home)
        );
        // Distinct paths → distinct ids.
        assert_ne!(
            id,
            project_id_for_path(std::path::Path::new("/opt/work/other"), home)
        );
    }

    #[test]
    fn project_store_root_is_under_projects() {
        let home = std::path::Path::new("/Users/me/.harness");
        assert_eq!(
            project_store_root(home, "ai-luodi-jyx3d"),
            std::path::Path::new("/Users/me/.harness/projects/ai-luodi-jyx3d")
        );
        assert_eq!(
            project_store_root(home, GLOBAL_PROJECT_ID),
            std::path::Path::new("/Users/me/.harness/projects/_global")
        );
    }

    #[test]
    fn project_context_round_trips_json() {
        let ctx = ProjectContext {
            id: "ai-luodi-jyx3d".into(),
            project_root: std::path::PathBuf::from("/Users/me/ai-luodi/jyx3d"),
            store_root: std::path::PathBuf::from("/Users/me/.harness/projects/ai-luodi-jyx3d"),
            kind: ProjectKind::Repo,
            is_git_repo: true,
        };
        let json = serde_json::to_string(&ctx).expect("serialize");
        assert_eq!(
            serde_json::from_str::<ProjectContext>(&json).expect("deserialize"),
            ctx
        );
        // kind is snake_case on the wire.
        assert!(json.contains("\"kind\":\"repo\""));
    }

    #[test]
    fn harness_turn_event_round_trips_json_and_omits_absent_optionals() {
        let tool_event = HarnessTurnEvent {
            session_id: "session-1".to_string(),
            provider: "codex".to_string(),
            seq: 1,
            ts: "2026-06-13T00:00:00Z".to_string(),
            provider_thread_id: None,
            provider_turn_id: Some("turn-1".to_string()),
            provider_item_id: None,
            kind: HarnessTurnEventKind::ToolCall,
            role: None,
            text: None,
            delta: None,
            tool_call: Some(HarnessToolCall {
                id: Some("call-1".to_string()),
                name: "shell".to_string(),
                args: serde_json::json!({ "cmd": "cargo test -p harness-core" }),
            }),
            tool_result: None,
            usage: None,
            model: Some("gpt-5".to_string()),
            duration_ms: None,
            cost_usd: None,
            status: None,
            error: None,
            raw_provider_event: serde_json::json!({
                "type": "item",
                "item": { "type": "tool_call", "id": "call-1" }
            }),
        };

        let usage_event = HarnessTurnEvent {
            session_id: "session-1".to_string(),
            provider: "codex".to_string(),
            seq: 2,
            ts: "2026-06-13T00:00:01Z".to_string(),
            provider_thread_id: None,
            provider_turn_id: None,
            provider_item_id: None,
            kind: HarnessTurnEventKind::Usage,
            role: None,
            text: None,
            delta: None,
            tool_call: None,
            tool_result: None,
            usage: Some(HarnessTokenUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                cached_input_tokens: None,
                reasoning_output_tokens: Some(5),
            }),
            model: None,
            duration_ms: None,
            cost_usd: None,
            status: None,
            error: None,
            raw_provider_event: serde_json::json!({
                "type": "usage",
                "input_tokens": 10,
                "output_tokens": 20
            }),
        };

        for event in [tool_event, usage_event] {
            let json = serde_json::to_value(&event).expect("serialize turn event");
            let parsed: HarnessTurnEvent =
                serde_json::from_value(json.clone()).expect("deserialize turn event");

            assert_eq!(parsed, event);
            assert!(json.get("raw_provider_event").is_some());
            assert!(json.get("provider_thread_id").is_none());
            assert!(json.get("provider_item_id").is_none());
            assert!(json.get("role").is_none());
            assert!(json.get("text").is_none());
            assert!(json.get("delta").is_none());
            assert!(json.get("duration_ms").is_none());
            assert!(json.get("cost_usd").is_none());
            assert!(json.get("status").is_none());
            assert!(json.get("error").is_none());
        }
    }

    #[test]
    fn harness_turn_event_kind_wire_spellings_are_snake_case() {
        // These wire strings are the API contract the SSE stream, the
        // /v1/.../events endpoints, and the dashboard key on — pin every variant.
        let cases = [
            (HarnessTurnEventKind::TurnStarted, "turn_started"),
            (HarnessTurnEventKind::TurnCompleted, "turn_completed"),
            (HarnessTurnEventKind::MessageDelta, "message_delta"),
            (HarnessTurnEventKind::Message, "message"),
            (HarnessTurnEventKind::ToolCall, "tool_call"),
            (HarnessTurnEventKind::ToolResult, "tool_result"),
            (HarnessTurnEventKind::Reasoning, "reasoning"),
            (HarnessTurnEventKind::Usage, "usage"),
            (HarnessTurnEventKind::Error, "error"),
            (HarnessTurnEventKind::ProviderMeta, "provider_meta"),
            (HarnessTurnEventKind::Unknown, "unknown"),
        ];
        for (kind, wire) in cases {
            assert_eq!(
                serde_json::to_value(&kind).expect("serialize kind"),
                serde_json::Value::String(wire.to_string()),
                "kind {kind:?} should serialize to {wire:?}"
            );
            let back: HarnessTurnEventKind =
                serde_json::from_value(serde_json::json!(wire)).expect("deserialize kind");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn validation_rejects_missing_required_id() {
        let member = AgentMember {
            id: "".to_string(),
            name: "Leader".to_string(),
            description: "Lead agent".to_string(),
            role: "leader".to_string(),
            provider: "codex".to_string(),
            model: None,
            profile: None,
            provider_config: AgentProviderConfig::default(),
            capabilities: vec![],
            team_ids: vec![],
            prompt_ref: None,
            skill_refs: vec![],
            workspace_policy: None,
            worktree_ref: None,
            permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            status: AgentMemberStatus::Idle,
            current_task_id: None,
            current_proposal_id: None,
            provider_runtime_id: None,
            provider_thread_id: None,
            provider_agent_path: None,
            provider_agent_nickname: None,
            provider_agent_role: None,
            control_endpoint: None,
            created_at: "2026-05-26T00:00:00Z".to_string(),
            last_seen_at: None,
        };

        assert_eq!(
            member.validate(),
            Err(ValidationError::Required {
                field: "AgentMember.id"
            })
        );
    }

    #[test]
    fn message_sender_kind_defaults_to_agent_and_persists_operator() {
        // A record persisted before sender_kind existed omits the field entirely.
        // It must deserialize as SenderKind::Agent (additive-optional backfill).
        let legacy_json = r#"{
            "id": "msg-legacy",
            "task_id": null,
            "from_agent_id": "leader-1",
            "to_agent_id": "agent-1",
            "channel": null,
            "kind": "message",
            "delivery_status": "queued",
            "content": "hello",
            "evidence_ids": [],
            "created_at": "2026-05-26T00:00:00Z",
            "delivery": null
        }"#;
        let legacy: Message =
            serde_json::from_str(legacy_json).expect("deserialize legacy message");
        assert_eq!(legacy.sender_kind, SenderKind::Agent);
        assert!(legacy.validate().is_ok());

        // An operator-authored message uses the reserved "operator" from id and
        // round-trips its sender_kind without loss.
        let operator = Message {
            id: "msg-op".to_string(),
            task_id: None,
            from_agent_id: "operator".to_string(),
            to_agent_id: Some("agent-1".to_string()),
            channel: None,
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "do the thing".to_string(),
            evidence_ids: vec![],
            created_at: "2026-05-26T00:00:00Z".to_string(),
            delivery: None,
            sender_kind: SenderKind::Operator,
        };
        let json = serde_json::to_string(&operator).expect("serialize operator message");
        assert!(
            json.contains("\"sender_kind\":\"operator\""),
            "operator message must serialize sender_kind as snake_case: {json}"
        );
        let parsed: Message = serde_json::from_str(&json).expect("deserialize operator message");
        assert_eq!(parsed, operator);
        assert_eq!(parsed.sender_kind, SenderKind::Operator);
        assert!(parsed.validate().is_ok());
    }

    fn sample_member() -> AgentMember {
        AgentMember {
            id: "agent-1".to_string(),
            name: "Worker".to_string(),
            description: "A worker member".to_string(),
            role: "worker".to_string(),
            provider: "codex".to_string(),
            model: Some("o3".to_string()),
            profile: None,
            provider_config: AgentProviderConfig::default(),
            capabilities: vec!["code".to_string()],
            team_ids: vec![],
            prompt_ref: Some(".harness/prompts/worker.md".to_string()),
            skill_refs: vec!["harness-workflow".to_string()],
            workspace_policy: None,
            worktree_ref: Some("../worktrees/task-1".to_string()),
            permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            status: AgentMemberStatus::Idle,
            current_task_id: None,
            current_proposal_id: None,
            provider_runtime_id: None,
            provider_thread_id: None,
            provider_agent_path: None,
            provider_agent_nickname: None,
            provider_agent_role: None,
            control_endpoint: None,
            created_at: "2026-05-26T00:00:00Z".to_string(),
            last_seen_at: None,
        }
    }

    fn sample_message() -> Message {
        Message {
            id: "msg-1".to_string(),
            task_id: Some("task-1".to_string()),
            from_agent_id: "leader-1".to_string(),
            to_agent_id: Some("agent-1".to_string()),
            channel: Some("team".to_string()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Implement the launch spec.".to_string(),
            evidence_ids: vec![],
            created_at: "2026-05-26T00:00:00Z".to_string(),
            delivery: None,
            sender_kind: SenderKind::Agent,
        }
    }

    #[test]
    fn launch_spec_composes_from_member_and_message() {
        let mut member = sample_member();
        member.provider_config.sandbox_policy = Some("workspace-write".to_string());
        member.provider_config.effort = Some("high".to_string());
        member.runtime_workspace_roots = vec!["crates/harness-core".to_string()];
        member.provider_config.runtime_workspace_roots = vec!["crates/harness-cli".to_string()];
        let message = sample_message();

        let spec = build_launch_spec(&member, &message);

        // Pillar 1 base configuration flows through unchanged.
        assert_eq!(
            spec.prompt_ref.as_deref(),
            Some(".harness/prompts/worker.md")
        );
        assert_eq!(spec.model.as_deref(), Some("o3"));
        assert_eq!(spec.effort.as_deref(), Some("high"));
        assert_eq!(spec.skill_refs, vec!["harness-workflow".to_string()]);
        // Pillar 2 workspace flows through as the cwd / worktree root.
        assert_eq!(spec.workspace.as_deref(), Some("../worktrees/task-1"));
        // The turn input carries the message envelope + content.
        assert!(spec.message_content.contains("message_id: msg-1"));
        assert!(spec.message_content.contains("kind: task"));
        assert!(spec.message_content.contains("task_id: task-1"));
        assert!(spec.message_content.contains("Implement the launch spec."));
        // Fields with no neutral source yet are empty/none, not invented.
        assert!(spec.tools.is_empty());
        assert!(spec.mcp.is_none());
        // A fresh member (no prior provider thread/session) carries no resume token.
        assert!(spec.resume.is_none());
        assert!(spec.output.is_none());
    }

    #[test]
    fn launch_spec_carries_resume_from_member_provider_thread_id() {
        // A member that already has a provider thread/session id (from a prior
        // delivery) must produce a spec that resumes that session, so memory
        // carries across deliveries instead of starting fresh each turn.
        let mut member = sample_member();
        member.provider_thread_id = Some("thread-abc-123".to_string());
        let message = sample_message();

        let spec = build_launch_spec(&member, &message);

        assert_eq!(spec.resume.as_deref(), Some("thread-abc-123"));
    }

    #[test]
    fn launch_spec_maps_codex_sandbox_vocabulary_onto_neutral_permission() {
        // Each Codex sandbox spelling (dashed and camelCase) maps onto the neutral
        // permission enum; no Codex wire vocabulary survives onto the spec.
        let cases = [
            ("read-only", LaunchPermission::ReadOnly),
            ("readOnly", LaunchPermission::ReadOnly),
            ("workspace-write", LaunchPermission::WorkspaceWrite),
            ("workspaceWrite", LaunchPermission::WorkspaceWrite),
            ("danger-full-access", LaunchPermission::FullAccess),
            ("dangerFullAccess", LaunchPermission::FullAccess),
        ];
        for (policy, expected) in cases {
            let mut member = sample_member();
            member.provider_config.sandbox_policy = Some(policy.to_string());
            let spec = build_launch_spec(&member, &sample_message());
            assert_eq!(
                spec.permission, expected,
                "policy {policy} should map to {expected:?}"
            );
        }
    }

    #[test]
    fn launch_spec_writable_roots_dedupe_and_drop_on_read_only() {
        // workspace_write carries de-duplicated member + provider_config roots.
        let mut member = sample_member();
        member.provider_config.sandbox_policy = Some("workspaceWrite".to_string());
        member.runtime_workspace_roots = vec!["shared".to_string(), "a".to_string()];
        member.provider_config.runtime_workspace_roots =
            vec!["shared".to_string(), "b".to_string()];
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(
            spec.writable_roots,
            vec!["shared".to_string(), "a".to_string(), "b".to_string()],
            "writable roots must be member-then-config order, de-duplicated"
        );

        // read_only never carries writable roots even if the member declares them.
        member.provider_config.sandbox_policy = Some("read-only".to_string());
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(spec.permission, LaunchPermission::ReadOnly);
        assert!(
            spec.writable_roots.is_empty(),
            "a read-only turn must not carry writable roots"
        );
    }

    #[test]
    fn launch_spec_absent_sandbox_policy_falls_back_to_safe_default() {
        // A member that never declared a sandbox policy must not be silently
        // elevated; it falls back to the default posture.
        let member = sample_member();
        assert!(member.provider_config.sandbox_policy.is_none());
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(spec.permission, LaunchPermission::default());
    }

    #[test]
    fn launch_spec_round_trips_json() {
        let mut member = sample_member();
        member.provider_config.sandbox_policy = Some("workspaceWrite".to_string());
        member.provider_config.effort = Some("medium".to_string());
        member.provider_config.output_schema = Some(serde_json::json!({
            "type": "object",
            "properties": { "verdict": { "type": "string" } },
            "required": ["verdict"]
        }));
        member.runtime_workspace_roots = vec!["crates".to_string()];
        let spec = build_launch_spec(&member, &sample_message());

        let json = serde_json::to_string(&spec).expect("serialize launch spec");
        let parsed: LaunchSpec = serde_json::from_str(&json).expect("deserialize launch spec");
        assert_eq!(parsed, spec);
        // The neutral permission serializes to its snake_case wire spelling, not
        // the Codex `workspaceWrite` vocabulary it was mapped from.
        assert!(json.contains("\"permission\":\"workspace_write\""));
        assert!(json.contains("\"effort\":\"medium\""));
        assert!(json.contains("\"output_schema\""));
        assert_eq!(
            parsed.output_schema, member.provider_config.output_schema,
            "launch spec should round-trip the optional output schema"
        );
        assert!(!json.contains("workspaceWrite"));
    }

    #[test]
    fn effort_defaults_to_none_for_legacy_json() {
        let provider_config: AgentProviderConfig = serde_json::from_value(serde_json::json!({
            "service_tier": "default"
        }))
        .expect("legacy provider config without effort should deserialize");
        assert!(provider_config.effort.is_none());
        assert!(provider_config.output_schema.is_none());

        let spec: LaunchSpec = serde_json::from_value(serde_json::json!({
            "message_content": "legacy turn",
            "model": "o3",
            "permission": "workspace_write"
        }))
        .expect("legacy launch spec without effort should deserialize");
        assert!(spec.effort.is_none());
        assert!(spec.output_schema.is_none());
    }

    #[test]
    fn build_launch_spec_carries_output_schema_from_provider_config() {
        let mut member = sample_member();
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"]
        });
        member.provider_config.output_schema = Some(schema.clone());
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(spec.output_schema, Some(schema));
    }

    #[test]
    fn launch_permission_wire_values_are_neutral() {
        assert_eq!(LaunchPermission::ReadOnly.as_str(), "read_only");
        assert_eq!(LaunchPermission::WorkspaceWrite.as_str(), "workspace_write");
        assert_eq!(LaunchPermission::FullAccess.as_str(), "full_access");
        // Round-trip each variant through serde to confirm the wire spelling.
        for variant in [
            LaunchPermission::ReadOnly,
            LaunchPermission::WorkspaceWrite,
            LaunchPermission::FullAccess,
        ] {
            let json = serde_json::to_string(&variant).expect("serialize permission");
            assert_eq!(json, format!("\"{}\"", variant.as_str()));
            let parsed: LaunchPermission =
                serde_json::from_str(&json).expect("deserialize permission");
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn delivery_handle_passes_endpoint_through_verbatim() {
        // The neutral delivery handle preserves any endpoint scheme verbatim; it
        // does not interpret or strip `unix://` (that stays in the CLI layer).
        for endpoint in [
            "unix:///tmp/agent/codex.sock",
            "exec://session/abc",
            "/tmp/plain/path",
        ] {
            let handle = DeliveryHandle::from_endpoint(endpoint);
            assert_eq!(handle.endpoint(), endpoint);
            let json = serde_json::to_string(&handle).expect("serialize handle");
            let parsed: DeliveryHandle = serde_json::from_str(&json).expect("deserialize handle");
            assert_eq!(parsed, handle);
            assert_eq!(parsed.endpoint(), endpoint);
        }
    }

    #[test]
    fn launch_mcp_block_round_trips_when_present() {
        // The MCP block is omitted by build_launch_spec today, but the neutral
        // shape must round-trip so later WPs can populate it.
        let mcp = LaunchMcp {
            servers: vec![LaunchMcpServer {
                id: "fs".to_string(),
                transport: Some("stdio".to_string()),
                command: vec!["mcp-fs".to_string(), "--root".to_string()],
                url: None,
                allowed_tools: vec!["read".to_string()],
            }],
        };
        let json = serde_json::to_string(&mcp).expect("serialize mcp");
        let parsed: LaunchMcp = serde_json::from_str(&json).expect("deserialize mcp");
        assert_eq!(parsed, mcp);
    }

    #[test]
    fn build_launch_spec_carries_mcp_from_provider_config() {
        let mut member = sample_member();
        member.provider_config.mcp = Some(LaunchMcp {
            servers: vec![LaunchMcpServer {
                id: "fs".to_string(),
                transport: Some("stdio".to_string()),
                command: vec!["mcp-fs".to_string()],
                url: None,
                allowed_tools: vec![],
            }],
        });
        let spec = build_launch_spec(&member, &sample_message());
        assert!(
            spec.mcp.is_some(),
            "launch spec should carry mcp from provider_config"
        );
        let mcp = spec.mcp.as_ref().unwrap();
        assert_eq!(mcp.servers.len(), 1);
        assert_eq!(mcp.servers[0].id, "fs");
    }

    #[test]
    fn build_launch_spec_mcp_none_when_absent() {
        let member = sample_member();
        assert!(member.provider_config.mcp.is_none());
        let spec = build_launch_spec(&member, &sample_message());
        assert!(
            spec.mcp.is_none(),
            "launch spec mcp should be none when member has no mcp"
        );
    }

    #[test]
    fn build_launch_spec_mcp_round_trips_json() {
        let mut member = sample_member();
        member.provider_config.mcp = Some(LaunchMcp {
            servers: vec![LaunchMcpServer {
                id: "api".to_string(),
                transport: Some("http".to_string()),
                command: vec![],
                url: Some("http://localhost:3000".to_string()),
                allowed_tools: vec!["query".to_string()],
            }],
        });
        let spec = build_launch_spec(&member, &sample_message());
        let json = serde_json::to_string(&spec).expect("serialize spec");
        let parsed: LaunchSpec = serde_json::from_str(&json).expect("deserialize spec");
        assert_eq!(parsed.mcp, spec.mcp);
    }

    #[test]
    fn provider_capabilities_codex_matches_doc_table() {
        let cap = ProviderCapabilities::codex_exec();
        assert!(cap.streaming, "Codex exec has --json streaming");
        assert!(cap.resume, "Codex exec has --session resume");
        assert!(
            !cap.mid_turn_approval,
            "Codex exec has policy pre-approve, no mid-turn"
        );
        assert!(cap.subagents, "Codex supports subagents");
        assert!(cap.mcp, "Codex exec has --config mcp_servers");
        assert!(!cap.hooks, "Codex exec has limited hooks");
    }

    #[test]
    fn provider_capabilities_claude_matches_doc_table() {
        let cap = ProviderCapabilities::claude_exec();
        assert!(cap.streaming, "Claude -p has --output-format stream-json");
        assert!(cap.resume, "Claude has --resume");
        assert!(!cap.mid_turn_approval, "Claude -p has no mid-turn approval");
        assert!(cap.subagents, "Claude supports subagents");
        assert!(cap.mcp, "Claude has --mcp-config");
        assert!(!cap.hooks, "Claude has no documented hooks");
    }

    #[test]
    fn provider_capabilities_round_trips_json() {
        let cap = ProviderCapabilities::codex_exec();
        let json = serde_json::to_string(&cap).expect("serialize capabilities");
        let parsed: ProviderCapabilities =
            serde_json::from_str(&json).expect("deserialize capabilities");
        assert_eq!(parsed, cap);
    }

    #[test]
    fn provider_capabilities_display_shows_enabled_features() {
        let cap = ProviderCapabilities::codex_exec();
        let display = cap.to_string();
        assert!(display.contains("streaming"));
        assert!(display.contains("resume"));
        assert!(display.contains("mcp"));
        assert!(display.contains("subagents"));
        assert!(
            !display.contains("mid_turn_approval"),
            "disabled features should not show"
        );
    }

    #[test]
    fn supports_streaming_exec_check() {
        let mut cap = ProviderCapabilities::codex_exec();
        assert!(
            cap.supports_streaming_exec(),
            "streaming + no mid-turn should be ok"
        );
        cap.mid_turn_approval = true;
        assert!(
            !cap.supports_streaming_exec(),
            "mid-turn approval blocks streaming exec"
        );
    }
}

/// Skill reference resolution: maps skill_refs to SKILL.md content.
///
/// A skill is durable at `.agents/skills/<id>/SKILL.md`. This module provides
/// the contract for resolving and validating skill references (Pillar 1 skill
/// contract from docs/agent-integration-model.md).
pub mod skill_resolver {
    use std::path::PathBuf;

    /// Result of resolving a skill reference.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ResolvedSkill {
        /// The skill id (matches `.agents/skills/<id>/`)
        pub id: String,
        /// The absolute or relative path to SKILL.md
        pub path: PathBuf,
        /// The full content of SKILL.md (header + body)
        pub content: String,
    }

    /// Error type for skill resolution.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SkillResolutionError {
        /// The skill reference does not resolve to an existing SKILL.md.
        SkillNotFound { skill_id: String, path: PathBuf },
        /// An IO error occurred while reading the skill file.
        IoError { skill_id: String, reason: String },
    }

    impl std::fmt::Display for SkillResolutionError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                SkillResolutionError::SkillNotFound { skill_id, path } => {
                    write!(f, "skill '{}' not found at {}", skill_id, path.display())
                }
                SkillResolutionError::IoError { skill_id, reason } => {
                    write!(f, "failed to read skill '{}': {}", skill_id, reason)
                }
            }
        }
    }

    impl std::error::Error for SkillResolutionError {}

    /// Resolve a single skill reference using the given skills root directory.
    ///
    /// The contract: a skill_ref `<id>` resolves to `.agents/skills/<id>/SKILL.md`.
    /// If the file exists and is readable, returns the content and path.
    /// If not found or unreadable, returns SkillResolutionError.
    ///
    /// This function is synchronous and does not require a live provider binary.
    pub fn resolve_skill(
        skill_id: &str,
        skills_root: &std::path::Path,
    ) -> Result<ResolvedSkill, SkillResolutionError> {
        let skill_path = skills_root.join(skill_id).join("SKILL.md");
        let content =
            std::fs::read_to_string(&skill_path).map_err(|e| SkillResolutionError::IoError {
                skill_id: skill_id.to_string(),
                reason: e.to_string(),
            })?;
        Ok(ResolvedSkill {
            id: skill_id.to_string(),
            path: skill_path,
            content,
        })
    }

    /// Resolve all skill references at once using the given skills root directory.
    ///
    /// Returns a Vec of resolved skills in the order they appear in the input.
    /// If any skill fails to resolve, returns an error (fail-fast); the caller
    /// must decide whether to report it or continue.
    pub fn resolve_skills(
        skill_ids: &[String],
        skills_root: &std::path::Path,
    ) -> Result<Vec<ResolvedSkill>, SkillResolutionError> {
        let mut resolved = Vec::new();
        for id in skill_ids {
            resolved.push(resolve_skill(id, skills_root)?);
        }
        Ok(resolved)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn skill_resolution_error_displays_clearly() {
            let err = SkillResolutionError::SkillNotFound {
                skill_id: "my-skill".to_string(),
                path: PathBuf::from(".agents/skills/my-skill/SKILL.md"),
            };
            let msg = err.to_string();
            assert!(msg.contains("my-skill"));
            assert!(msg.contains(".agents/skills"));
        }

        #[test]
        fn skill_not_found_error() {
            let result = resolve_skill("nonexistent", PathBuf::from(".").as_path());
            assert!(result.is_err());
            match result {
                Err(SkillResolutionError::IoError { skill_id, .. }) => {
                    assert_eq!(skill_id, "nonexistent");
                }
                _ => panic!("expected IoError"),
            }
        }
    }
}

/// Provider capabilities declaration: what a platform can technically support.
///
/// This is distinct from member-level `AgentMember.capabilities` (intent: what
/// the member is *meant* to do). This declares what the *platform* can do
/// (streaming, resume, mid-turn approval, subagents, MCP, hooks).
///
/// See Pillar 3 and the capability declaration table in
/// docs/agent-integration-model.md for the current capability set per provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Platform supports incremental event stream during a turn.
    pub streaming: bool,
    /// Platform supports session resume (`--session`, `--resume`, etc).
    pub resume: bool,
    /// Platform supports mid-turn tool approval/denial (approve/reject before execution).
    pub mid_turn_approval: bool,
    /// Platform supports native child threads / subagents.
    pub subagents: bool,
    /// Platform supports MCP server attachment.
    pub mcp: bool,
    /// Platform supports lifecycle hooks.
    pub hooks: bool,
}

impl ProviderCapabilities {
    /// Codex exec capabilities per the capability declaration table in
    /// docs/agent-integration-model.md.
    pub fn codex_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // --json NDJSON
            resume: true,             // --session
            mid_turn_approval: false, // policy pre-approve only
            subagents: true,          // observed in Codex
            mcp: true,                // --config mcp_servers.*
            hooks: false,             // limited in exec mode
        }
    }

    /// Claude exec capabilities per the capability declaration table.
    pub fn claude_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // --output-format stream-json
            resume: true,             // --resume
            mid_turn_approval: false, // not documented for -p; Tier-3 only
            subagents: true,          // observed in Claude
            mcp: true,                // --mcp-config JSON
            hooks: false,             // not documented
        }
    }

    /// Check if all critical capabilities for basic streaming exec are present.
    pub fn supports_streaming_exec(&self) -> bool {
        self.streaming && !self.mid_turn_approval
    }
}

impl std::fmt::Display for ProviderCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let features = [
            ("streaming", self.streaming),
            ("resume", self.resume),
            ("mid_turn_approval", self.mid_turn_approval),
            ("subagents", self.subagents),
            ("mcp", self.mcp),
            ("hooks", self.hooks),
        ];
        let enabled: Vec<&str> = features
            .iter()
            .filter_map(|(name, enabled)| if *enabled { Some(*name) } else { None })
            .collect();
        write!(f, "{{{}}}", enabled.join(", "))
    }
}
