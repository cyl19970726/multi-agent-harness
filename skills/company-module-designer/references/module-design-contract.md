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
  "page_contracts": [],
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
  "frontend_surfaces": [],
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
- Every core page contract names a primary question, required sections,
  required typed records, relations, Views, right-rail context, front-end shape,
  and fallback route.
- Every action names declared effects and the policy/approval gate.
- Every financial relation names a typed state such as commitment or payment.
- Every custom-page candidate names at least one standard fallback view and
  explains why a standard Document or Module page is insufficient.
- Unknowns remain explicit; do not replace them with plausible defaults.
