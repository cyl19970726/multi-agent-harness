# Member Workspace Binding

```text
status: implemented kernel; supervisor integration in progress
owner_role: execution-foundation
canonical_for: MemberWorkspaceBinding identity, safety and lifecycle
truthRefs:
  - kind: schema
    ref: schemas/member-workspace-binding.schema.json
  - kind: store
    ref: crates/firm-store/src/trust_kernel.rs
```

## Boundary

Workspace placement belongs to `MemberWorkspaceBinding`, not Work. One binding
names the exact Execution Space, project binding, AgentMember, MemberRun,
TeamRun, Work, absolute path, repository identity, base revision, generation
and safety policy.

```text
requested -> provisioning -> ready -> attached -> archived -> removed
```

Every transition is CAS-protected and recorded in the canonical operation
ledger. Provider execution may start only from Ready or Attached with the same
MemberRun generation.

## Safety proof

Git bindings prove repository identity, expected base revision, worktree
registration and the declared clean/dirty policy. Directory bindings reject
relative paths, parent traversal, repository-root targeting, symlink escape and
cross-project placement.

Cleanup is explicit. It refuses an unverified, dirty or symlinked target and
must never recursively traverse a link. A failed cleanup remains inspectable
and recoverable; no stronger or broader deletion retry is automatic.

## Runtime use

The supervisor resolves cwd only from the current canonical binding. Process
cwd, Work prose and provider session metadata are observations, not placement
authority. A replacement MemberRun receives a new binding or an explicit
generation-fenced transition; it never inherits a path by guessing.
