use super::*;

impl HarnessStore {
    pub fn create_trust_workspace_binding(
        &self,
        context: &MutationContext,
        mut binding: MemberWorkspaceBinding,
    ) -> StoreResult<CanonicalMutationResult<MemberWorkspaceBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(
            &binding.canonical_root,
            "MemberWorkspaceBinding.canonical_root",
        )?;
        if binding.version != 1 || binding.lifecycle != WorkspaceLifecycle::Requested {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "workspace binding create requires requested lifecycle and version 1",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let path = std::path::Path::new(&binding.canonical_root);
        if !path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "canonical_root must be an absolute normalized path",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let run = self
            .trust_member_runs(&context.execution_space_id)?
            .into_iter()
            .find(|run| run.id == binding.member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace binding references a missing MemberRun",
                    "workspace_binding",
                    &binding.id,
                    None,
                )
            })?;
        if run.team_run_id != binding.team_run_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace binding TeamRun does not match MemberRun",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|team_run| team_run.id == binding.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace TeamRun is missing",
                    "workspace_binding",
                    &binding.id,
                    None,
                )
            })?;
        if team_run.project_binding_id != binding.project_binding_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace ProjectBinding does not match TeamRun placement",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let mut cursor = std::path::PathBuf::new();
        for component in path.components() {
            cursor.push(component.as_os_str());
            match std::fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceLinkEscape,
                        "workspace canonical path contains a symbolic-link component",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
        if path.exists() {
            let observed = observe_workspace_safety(path)?;
            if observed.canonical_root != path {
                return Err(trust_error(
                    TrustErrorCode::WorkspacePathUnsafe,
                    "canonical_root must equal the filesystem canonical path",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if !observed.link_escape_free {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceLinkEscape,
                    "workspace tree contains a symbolic-link escape",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if matches!(
                binding.mode,
                WorkspaceMode::Worktree | WorkspaceMode::SharedLive
            ) && observed.git_common_dir.is_none()
            {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "worktree/shared_live workspace must resolve a Git common directory",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if let (Some(expected), Some(actual)) = (
                binding.git_common_dir.as_deref(),
                observed.git_common_dir.as_ref(),
            ) {
                let expected = canonical_git_path(path, expected)?;
                if &expected != actual {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceRepositoryMismatch,
                        "workspace Git common directory does not match the binding",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
            }
            binding.git_common_dir = observed
                .git_common_dir
                .map(|value| value.display().to_string());
            binding.dirty_fingerprint = observed.dirty_fingerprint;
        }
        if binding.mode == WorkspaceMode::SharedLive {
            if binding.ownership != WorkspaceOwnership::SharedProject {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "shared_live requires shared_project ownership",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            let member = self
                .trust_agent_members(&context.execution_space_id)?
                .into_iter()
                .find(|member| member.id == run.agent_member_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "workspace AgentMember is missing",
                        "workspace_binding",
                        &binding.id,
                        None,
                    )
                })?;
            if member.permission_ceiling != firm_core::agentfirm_api::PermissionCeiling::ReadOnly {
                if context.authority_actor.is_none() {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "writable shared_live requires explicit Host authority",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
                if self
                    .trust_workspace_bindings(&context.execution_space_id)?
                    .iter()
                    .any(|existing| {
                        existing.canonical_root == binding.canonical_root
                            && existing.lifecycle == WorkspaceLifecycle::Attached
                    })
                {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceGenerationFenced,
                        "shared_live writable workspace already has an attached writer",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "workspace_binding",
            &binding.id,
            "requested",
            serde_json::to_value(&binding)?,
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_workspace_bindings(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MemberWorkspaceBinding>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "workspace_binding")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn transition_trust_workspace_binding(
        &self,
        context: &MutationContext,
        binding_id: &str,
        next: WorkspaceLifecycle,
        proof: &WorkspaceSafetyProof,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberWorkspaceBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut binding = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "workspace_binding")?
            .remove(binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace binding not found",
                    "workspace_binding",
                    binding_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberWorkspaceBinding>(&envelope))?;
        if proof.canonical_root != binding.canonical_root {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "safety proof canonical path differs from binding",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.project_binding_id != binding.project_binding_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace ProjectBinding does not match",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let root = Path::new(&binding.canonical_root);
        let observed = if root.exists() {
            Some(observe_workspace_safety(root)?)
        } else {
            None
        };
        if next == WorkspaceLifecycle::Removed {
            if observed.is_some() {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceCleanupBlocked,
                    "workspace cleanup cannot complete while canonical_root still exists",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
        } else if next != WorkspaceLifecycle::Preparing && observed.is_none() {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "workspace path is missing for the requested lifecycle transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if let Some(observed) = observed.as_ref() {
            if observed.canonical_root != root {
                return Err(trust_error(
                    TrustErrorCode::WorkspacePathUnsafe,
                    "workspace path no longer equals its canonical filesystem path",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if !observed.link_escape_free || !proof.link_escape_free {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceLinkEscape,
                    "workspace contains a symlink/reparse escape",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if matches!(
                binding.mode,
                WorkspaceMode::Worktree | WorkspaceMode::SharedLive
            ) && observed.git_common_dir.is_none()
            {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "workspace no longer resolves the required Git repository",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if let Some(expected) = binding.git_common_dir.as_deref() {
                let expected = canonical_git_path(root, expected)?;
                if observed.git_common_dir.as_ref() != Some(&expected)
                    || proof
                        .git_common_dir
                        .as_deref()
                        .map(|value| canonical_git_path(root, value))
                        .transpose()?
                        .as_ref()
                        != Some(&expected)
                {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceRepositoryMismatch,
                        "workspace Git identity differs from binding or safety proof",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
            }
            if !proof.repository_matches {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "workspace safety proof did not affirm the bound repository",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if observed.conflicted != proof.is_conflicted {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceConflicted,
                    "workspace conflict proof differs from the filesystem observation",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if observed.dirty != proof.is_dirty {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceDirty,
                    "workspace dirty proof differs from the filesystem observation",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            binding.dirty_fingerprint = observed.dirty_fingerprint.clone();
        } else if !proof.link_escape_free {
            return Err(trust_error(
                TrustErrorCode::WorkspaceLinkEscape,
                "workspace safety proof did not establish a link-safe path",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if binding
            .attached_member_generation
            .is_some_and(|generation| generation != proof.observed_member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceGenerationFenced,
                "workspace safety proof used a stale MemberRun generation",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.is_conflicted
            && next != WorkspaceLifecycle::Conflicted
            && next != WorkspaceLifecycle::CleanupBlocked
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceConflicted,
                "conflicted workspace cannot make the requested transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.is_dirty
            && next != WorkspaceLifecycle::Dirty
            && next != WorkspaceLifecycle::CleanupBlocked
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceDirty,
                "dirty workspace cannot make the requested transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let allowed = matches!(
            (binding.lifecycle, next),
            (WorkspaceLifecycle::Requested, WorkspaceLifecycle::Preparing)
                | (WorkspaceLifecycle::Preparing, WorkspaceLifecycle::Ready)
                | (WorkspaceLifecycle::Ready, WorkspaceLifecycle::Attached)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Dirty)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Conflicted)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Ready, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Dirty, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Conflicted, WorkspaceLifecycle::Archived)
                | (
                    WorkspaceLifecycle::Ready,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Attached,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Dirty,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Conflicted,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (WorkspaceLifecycle::Dirty, WorkspaceLifecycle::Attached)
                | (WorkspaceLifecycle::Conflicted, WorkspaceLifecycle::Attached)
                | (
                    WorkspaceLifecycle::CleanupBlocked,
                    WorkspaceLifecycle::Archived
                )
                | (WorkspaceLifecycle::Archived, WorkspaceLifecycle::Removed)
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "workspace lifecycle transition is not allowed",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if next == WorkspaceLifecycle::Attached {
            let run = self.claimable_member_run(
                &context.execution_space_id,
                &binding.member_run_id,
                proof.observed_member_generation,
            )?;
            binding.attached_member_generation = Some(run.runtime_generation);
        }
        if next == WorkspaceLifecycle::CleanupBlocked {
            binding.blocked_reason = Some(
                if proof.is_conflicted {
                    "WORKSPACE_CONFLICTED"
                } else if proof.is_dirty {
                    "WORKSPACE_DIRTY"
                } else {
                    "WORKSPACE_CLEANUP_BLOCKED"
                }
                .to_string(),
            );
        } else {
            binding.blocked_reason = None;
        }
        binding.lifecycle = next;
        binding.version += 1;
        binding.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "workspace_binding",
            binding_id,
            "lifecycle_transitioned",
            serde_json::json!({"next": next, "proof": proof}),
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }
}
