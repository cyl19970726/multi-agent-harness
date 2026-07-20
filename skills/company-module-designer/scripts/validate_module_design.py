#!/usr/bin/env python3
"""Validate the minimum governed Company OS ModuleDesign contract."""

import json
import pathlib
import sys


REQUIRED = {
    "schema_version", "id", "title", "status", "business_event", "outcome",
    "non_goals",
    "documents", "record_types", "relations", "views", "actors_and_roles",
    "work_items", "approvals", "financial_relations", "actions", "permissions",
    "automations",
    "custom_page_candidates", "fallback_views", "migration", "archive_policy",
    "unknowns", "required_human_approver",
}


def fail(message: str) -> None:
    raise ValueError(message)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: validate_module_design.py <design.json>", file=sys.stderr)
        return 2
    path = pathlib.Path(sys.argv[1])
    data = json.loads(path.read_text(encoding="utf-8"))
    missing = sorted(REQUIRED - data.keys())
    if missing:
        fail(f"missing keys: {', '.join(missing)}")
    if data["schema_version"] != 1 or data["status"] != "proposed":
        fail("schema_version must be 1 and status must be proposed")
    if not str(data["required_human_approver"]).startswith("actor-"):
        fail("required_human_approver must be an explicit actor id")
    fallback_ids = {
        view.get("id") for view in data["fallback_views"] if isinstance(view, dict)
    }
    for candidate in data["custom_page_candidates"]:
        refs = set(candidate.get("fallback_view_ids", []))
        if not refs or not refs.issubset(fallback_ids):
            fail(f"custom page {candidate.get('id')} lacks a resolvable fallback")
    for action in data["actions"]:
        if not action.get("effects") or not action.get("policy_gate"):
            fail(f"action {action.get('id')} lacks effects or policy_gate")
    payment_terms = {"payment", "paid", "settled", "settlement"}
    for relation in data["financial_relations"]:
        kind = str(relation.get("type", "")).lower()
        source = str(relation.get("source_type", "")).lower()
        if kind in payment_terms and source == "commitment":
            fail("a commitment cannot be declared as payment or settlement")
    print(f"valid ModuleDesign: {data['id']}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"invalid ModuleDesign: {exc}", file=sys.stderr)
        raise SystemExit(1)
