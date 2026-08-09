#!/usr/bin/env python3
"""Validate the minimum governed Company OS ModuleDesign contract."""

import json
import pathlib
import sys


REQUIRED = {
    "schema_version", "id", "title", "status", "business_event", "outcome",
    "non_goals",
    "documents", "page_contracts", "record_types", "relations", "views",
    "actors_and_roles",
    "works", "approvals", "financial_relations", "actions", "permissions",
    "automations",
    "custom_page_candidates", "frontend_surfaces", "fallback_views", "migration", "archive_policy",
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
    page_contract_ids = {
        contract.get("id")
        for contract in data["page_contracts"]
        if isinstance(contract, dict)
    }
    for contract in data["page_contracts"]:
        required_contract_keys = {
            "id", "document_id", "primary_question", "required_sections",
            "required_typed_records", "required_relations", "required_views",
            "right_rail_context", "frontend_shape", "fallback_view_id",
        }
        missing_contract_keys = sorted(required_contract_keys - contract.keys())
        if missing_contract_keys:
            fail(
                f"page contract {contract.get('id')} missing keys: "
                f"{', '.join(missing_contract_keys)}"
            )
        if contract.get("fallback_view_id") not in fallback_ids:
            fail(f"page contract {contract.get('id')} lacks a resolvable fallback")
    frontend_surface_ids = {
        surface.get("id")
        for surface in data["frontend_surfaces"]
        if isinstance(surface, dict)
    }
    for surface in data["frontend_surfaces"]:
        if surface.get("page_contract_id") not in page_contract_ids:
            fail(f"frontend surface {surface.get('id')} lacks a resolvable page contract")
    for candidate in data["custom_page_candidates"]:
        refs = set(candidate.get("fallback_view_ids", []))
        if not refs or not refs.issubset(fallback_ids):
            fail(f"custom page {candidate.get('id')} lacks a resolvable fallback")
        page_refs = set(candidate.get("page_contract_ids", []))
        if candidate.get("page_contract_id"):
            page_refs.add(candidate["page_contract_id"])
        if not page_refs or not page_refs.issubset(page_contract_ids):
            fail(f"custom page {candidate.get('id')} lacks a resolvable page contract")
        if (
            candidate.get("frontend_surface_id")
            and candidate["frontend_surface_id"] not in frontend_surface_ids
        ):
            fail(f"custom page {candidate.get('id')} lacks a resolvable frontend surface")
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
