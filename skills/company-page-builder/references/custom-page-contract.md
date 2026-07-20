# Governed custom-page contract

A package declares presentation code around existing Company OS records. It is
not a new database or privileged write path.

Required manifest fields:

- `schema_version`, `package_id`, `definition_id`, `version`, `entrypoint`;
- `declared_queries` and `declared_actions`;
- `fallback_document_id` and non-empty `fallback_view_ids`;
- `fixture_id`, `expected_artifact`, `expected_hash`, `approval_ref`;
- `permissions`, `audit_events`, and `rollback_to_version`.

Runtime rules:

1. Grant only the intersection of package and registered definition scopes.
2. Return immutable query results from declared standard Views.
3. Dispatch declared Actions through policy, permission, approval, idempotency,
   and audit checks.
4. Reject effects the Action did not declare.
5. Render the fallback document and Views when code, data, or package loading
   fails.
6. Preserve canonical record links in visual cards and activity output.

Visual evidence uses separate paths for current baseline, candidate, approved
expected, implemented capture, and comparison. Never overwrite evidence from a
different fixture or viewport.
