# Notion projection

Notion should show the simple workflow directly, not the machinery that could theoretically implement it.

## The two default tables

### Development Tasks

Use one current Task database with:

- Task ID
- Goal / Acceptance
- Owner
- Current Session / executor
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

Use only these statuses: `Planned`, `Doing`, `In Review`, `Changes Required`,
`Blocked`, `Done`, `Cancelled`. `Cancelled` is terminal for an obsolete,
explicitly superseded, or no-longer-authorized outcome; it never means Pass.
Ordinary current views exclude both `Done` and `Cancelled`.

### Development Documents

Use the existing Development Documents table for both human-readable Dev/Spec
submissions and immutable Review Documents. Keep a simple type field such as
`Dev Document`, `Spec`, or `Review` and common relations to the owning Task.

A reviewed Dev Document has a named immutable version and directly readable
body. A Review Document has:

- Task relation
- Submission Number
- exact Git SHA or directly readable Dev Document version
- Verdict
- Findings
- Reviewer
- Reviewed At

One submission produces one Review Document. Prior Dev and Review Documents
stay visible. Do not add a third submissions, snapshots, payload, carrier,
protocol-event, or Session-state table.

Large machine-readable inventories and structured manifests live in the
repository; Notion links their exact Git SHA, path, and file hash and keeps the
human decision and review readable.

## Migration from the advanced model

- Choose either the existing Work or Run record as the single current Task authority; never maintain both.
- Prefer the object that already contains the task's goal, acceptance criteria, owner, and user-facing identity.
- Demote the other object and advanced Protocol Event, Snapshot/Result split, CAS, fingerprint, and Merge Authorization fields to hidden `Legacy / Advanced` history.
- Preserve historical records and relations. Do not delete them merely to make the default view look clean.
- Remove advanced fields from default views, templates, Playbook steps, and agent instructions.
- A combined legacy Review may remain historical, but new submissions use one
  simple Review Document.
- Mark carrier/payload fragments as Legacy / Advanced transport evidence and
  hide them from ordinary views; preserve rather than deleting historical
  evidence.

## Verification

After migration, verify:

1. A user can understand current work from one Task page.
2. Dev can submit without Candidate or a readiness gate.
3. Reviewer can Pass or return the same Task.
4. Development Documents retains every submission and exact revision/version.
5. No default view or Playbook asks users to maintain Work and Run together.
6. Advanced legacy records are clearly non-authoritative and hidden from ordinary operation.
7. An actionable repository defect can be traced to one GitHub Issue; when
   Brain promotes it, one or more Issues may map to one current Notion Task
   without duplicate status writing.
8. A Session is visible only as the Task's current executor binding; Task state
   is never inferred from a sidebar badge or mirrored Session ledger.
