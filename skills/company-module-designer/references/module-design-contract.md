# ModuleDesign output contract

Produce one JSON object with these top-level keys:

```json
{
  "schema_version": 1,
  "id": "module-design-...",
  "title": "...",
  "status": "proposed",
  "business_event": "...",
  "outcome": "...",
  "non_goals": [],
  "documents": [],
  "record_types": [],
  "relations": [],
  "views": [],
  "actors_and_roles": [],
  "work_items": [],
  "approvals": [],
  "financial_relations": [],
  "actions": [],
  "permissions": [],
  "automations": [],
  "custom_page_candidates": [],
  "fallback_views": [],
  "migration": {},
  "archive_policy": {},
  "unknowns": [],
  "required_human_approver": "actor-..."
}
```

Required semantics:

- `status` starts as `proposed`; only an external approval record can change it.
- Every relation names `from_type`, `to_type`, cardinality, and ownership.
- Every action names declared effects and the policy/approval gate.
- Every financial relation names a typed state such as commitment or payment.
- Every custom-page candidate names at least one standard fallback view.
- Unknowns remain explicit; do not replace them with plausible defaults.
