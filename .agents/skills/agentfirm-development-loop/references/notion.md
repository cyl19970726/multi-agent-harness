# Notion projection

Notion should show the simple workflow directly, not the machinery that could theoretically implement it.

## Default databases

### Tasks

Use one current Task database with:

- Task ID
- Goal / Acceptance
- Owner
- Status
- Next Action
- Blocker
- Working Revision
- Review Revision
- Current Reviewer

An optional `GitHub Issue` URL may record the repository problem that caused a
Task. It is provenance only: GitHub does not own the Notion Task status, and
Notion must not mirror a second Issue lifecycle. Do not add `Issue` as a Task
status. Do not require a `Task Kind` field unless users demonstrate a real
filtering need.

Use only these statuses: `Planned`, `Doing`, `In Review`, `Changes Required`, `Blocked`, `Done`.

### Reviews

Use one Review history database or typed document view with:

- Task relation
- Submission Number
- Review Revision / Version
- Verdict
- Findings
- Reviewer
- Reviewed At

One submission produces one Review record. Prior Reviews stay visible.

## Migration from the advanced model

- Choose either the existing Work or Run record as the single current Task authority; never maintain both.
- Prefer the object that already contains the task's goal, acceptance criteria, owner, and user-facing identity.
- Demote the other object and advanced Protocol Event, Snapshot/Result split, CAS, fingerprint, and Merge Authorization fields to hidden `Legacy / Advanced` history.
- Preserve historical records and relations. Do not delete them merely to make the default view look clean.
- Remove advanced fields from default views, templates, Playbook steps, and agent instructions.
- A combined legacy Review may remain historical, but new submissions use one simple Review record.

## Verification

After migration, verify:

1. A user can understand current work from one Task page.
2. Dev can submit without Candidate or a readiness gate.
3. Reviewer can Pass or return the same Task.
4. Review history retains every submission and exact revision/version.
5. No default view or Playbook asks users to maintain Work and Run together.
6. Advanced legacy records are clearly non-authoritative and hidden from ordinary operation.
7. An independently actionable repository defect can be traced from one
   GitHub Issue to one current Notion Task without duplicate status writing.
