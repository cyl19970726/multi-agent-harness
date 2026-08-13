#!/usr/bin/env python3
"""Validate the structural closure of a Frontend Module Spec JSON file."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")
PLACEHOLDER = re.compile(r"\b(replace with|replace-with)\b", re.IGNORECASE)
COVERAGE = {"designed", "pattern", "existing-accepted", "excluded"}
REFERENCE_KINDS = {"expected-design", "system-pattern", "existing-surface"}
REQUIRED_TOP = {
    "schema_version",
    "module",
    "sources",
    "product_definition",
    "outcomes",
    "actors",
    "non_goals",
    "real_data_scenarios",
    "journeys",
    "references",
    "surfaces",
    "transitions",
    "product_invariants",
    "ux_contract",
    "visual_system",
    "architecture",
    "implementation_slices",
    "traceability",
    "readiness",
    "approved_deviations",
    "acceptance",
}


def nonempty(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def unresolved(value: Any) -> bool:
    return isinstance(value, str) and bool(PLACEHOLDER.search(value))


def ids(
    items: Any,
    label: str,
    errors: list[str],
    *,
    allow_empty: bool = False,
) -> dict[str, dict[str, Any]]:
    found: dict[str, dict[str, Any]] = {}
    if not isinstance(items, list) or (not items and not allow_empty):
        qualifier = "a list" if allow_empty else "a non-empty list"
        errors.append(f"{label} must be {qualifier}")
        return found
    for index, item in enumerate(items):
        if not isinstance(item, dict) or not nonempty(item.get("id")):
            errors.append(f"{label}[{index}] must be an object with a non-empty id")
            continue
        item_id = item["id"]
        if item_id in found:
            errors.append(f"duplicate {label} id: {item_id}")
        found[item_id] = item
    return found


def require_fields(obj: Any, fields: set[str], label: str, errors: list[str]) -> None:
    if not isinstance(obj, dict):
        errors.append(f"{label} must be an object")
        return
    for field in sorted(fields):
        if field not in obj or obj[field] in (None, "", [], {}):
            errors.append(f"{label}.{field} is required")


def string_list(
    value: Any,
    label: str,
    errors: list[str],
    *,
    allow_empty: bool = False,
) -> list[str]:
    if not isinstance(value, list) or (not value and not allow_empty):
        qualifier = "a list" if allow_empty else "a non-empty list"
        errors.append(f"{label} must be {qualifier}")
        return []
    result: list[str] = []
    for index, item in enumerate(value):
        if not nonempty(item):
            errors.append(f"{label}[{index}] must be a non-empty string")
        else:
            result.append(item)
    return result


def validate(data: Any) -> list[str]:
    errors: list[str] = []
    if not isinstance(data, dict):
        return ["spec root must be an object"]
    for key in sorted(REQUIRED_TOP - data.keys()):
        errors.append(f"missing top-level field: {key}")
    if data.get("schema_version") != 1:
        errors.append("schema_version must be 1")

    def find_placeholders(value: Any, path: str = "spec") -> None:
        if unresolved(value):
            errors.append(f"{path} contains unresolved template placeholder")
        elif isinstance(value, dict):
            for key, item in value.items():
                find_placeholders(item, f"{path}.{key}")
        elif isinstance(value, list):
            for index, item in enumerate(value):
                find_placeholders(item, f"{path}[{index}]")

    find_placeholders(data)

    require_fields(
        data.get("module"),
        {
            "id",
            "title",
            "canonical_spec_ref",
            "spec_revision",
            "owner",
            "approved_by",
            "approved_at",
            "status",
        },
        "module",
        errors,
    )
    if isinstance(data.get("module"), dict) and data["module"].get("status") != "approved":
        errors.append("module.status must be approved before implementation")
    for field in ("outcomes", "actors", "non_goals", "real_data_scenarios"):
        string_list(data.get(field), field, errors)
    product_definition = data.get("product_definition")
    require_fields(
        product_definition,
        {
            "problem_statement",
            "success_measures",
            "capabilities",
            "business_rules",
            "scope",
            "risks",
        },
        "product_definition",
        errors,
    )
    if isinstance(product_definition, dict):
        for field in ("dependencies", "assumptions", "unknowns"):
            if field not in product_definition:
                errors.append(f"product_definition.{field} is required")
        for field in ("success_measures", "capabilities", "business_rules", "scope", "risks"):
            string_list(
                product_definition.get(field),
                f"product_definition.{field}",
                errors,
            )
        for field in ("dependencies", "assumptions", "unknowns"):
            string_list(
                product_definition.get(field),
                f"product_definition.{field}",
                errors,
                allow_empty=True,
            )
    source_by_id = ids(data.get("sources"), "sources", errors)
    journey_by_id = ids(data.get("journeys"), "journeys", errors)
    reference_by_id = ids(data.get("references"), "references", errors)
    surface_by_id = ids(data.get("surfaces"), "surfaces", errors)
    transition_by_id = ids(data.get("transitions"), "transitions", errors, allow_empty=True)
    invariant_by_id = ids(data.get("product_invariants"), "product_invariants", errors)
    slice_by_id = ids(data.get("implementation_slices"), "implementation_slices", errors)
    trace_rows = data.get("traceability")
    if not isinstance(trace_rows, list) or not trace_rows:
        errors.append("traceability must be a non-empty list")
        trace_rows = []

    for source_id, source in source_by_id.items():
        require_fields(source, {"ref", "revision"}, f"sources[{source_id}]", errors)

    for journey_id, journey in journey_by_id.items():
        require_fields(
            journey,
            {"name", "outcome", "entry_surface_id"},
            f"journeys[{journey_id}]",
            errors,
        )
        entry = journey.get("entry_surface_id")
        if not nonempty(entry):
            errors.append(f"journeys[{journey_id}].entry_surface_id must be a string")
        elif entry not in surface_by_id:
            errors.append(f"journey {journey_id} references missing entry surface {entry}")

    reference_covers: dict[str, list[str]] = {}
    for reference_id, reference in reference_by_id.items():
        require_fields(
            reference,
            {"kind", "ref", "sha256", "approved_by", "approved_at", "covers"},
            f"references[{reference_id}]",
            errors,
        )
        if not isinstance(reference.get("kind"), str) or reference.get("kind") not in REFERENCE_KINDS:
            errors.append(
                f"reference {reference_id} has invalid kind {reference.get('kind')!r}"
            )
        if nonempty(reference.get("sha256")) and not SHA256.fullmatch(reference["sha256"]):
            errors.append(f"reference {reference_id} sha256 must be 64 hexadecimal characters")
        reference_covers[reference_id] = string_list(
            reference.get("covers"),
            f"references[{reference_id}].covers",
            errors,
        )

    surface_states: dict[str, list[str]] = {}
    surface_viewports: dict[str, list[str]] = {}
    surface_reference_ids: dict[str, list[str]] = {}
    surface_journey_ids: dict[str, list[str]] = {}
    for surface_id, surface in surface_by_id.items():
        require_fields(
            surface,
            {
                "name",
                "kind",
                "journey_ids",
                "primary_user_question",
                "distinct_surface_rationale",
                "risk",
                "coverage",
                "contract_ref",
            },
            f"surfaces[{surface_id}]",
            errors,
        )
        surface_journey_ids[surface_id] = string_list(
            surface.get("journey_ids"),
            f"surfaces[{surface_id}].journey_ids",
            errors,
        )
        for journey_id in surface_journey_ids[surface_id]:
            if journey_id not in journey_by_id:
                errors.append(
                    f"surface {surface_id} references missing journey {journey_id}"
                )
        parent_id = surface.get("parent_surface_id")
        if parent_id is not None and not nonempty(parent_id):
            errors.append(f"surface {surface_id} parent_surface_id must be a string")
            parent_id = None
        elif parent_id is not None and parent_id not in surface_by_id:
            errors.append(f"surface {surface_id} references missing parent surface {parent_id}")
        if parent_id == surface_id:
            errors.append(f"surface {surface_id} cannot be its own parent")
        coverage = surface.get("coverage")
        if not isinstance(coverage, str) or coverage not in COVERAGE:
            errors.append(f"surface {surface_id} has invalid coverage {coverage!r}")
            continue
        if not isinstance(surface.get("risk"), str) or surface.get("risk") not in {
            "low",
            "medium",
            "high",
        }:
            errors.append(
                f"surface {surface_id} risk must be low, medium, or high"
            )
        if coverage != "excluded":
            surface_states[surface_id] = string_list(
                surface.get("required_states"),
                f"surfaces[{surface_id}].required_states",
                errors,
            )
            surface_viewports[surface_id] = string_list(
                surface.get("required_viewports"),
                f"surfaces[{surface_id}].required_viewports",
                errors,
            )
        if coverage == "designed":
            refs = string_list(
                surface.get("reference_ids"),
                f"surfaces[{surface_id}].reference_ids",
                errors,
            )
            surface_reference_ids[surface_id] = refs
            for ref in refs:
                if ref not in reference_by_id:
                    errors.append(f"surface {surface_id} references missing design {ref}")
                elif reference_by_id[ref].get("kind") != "expected-design":
                    errors.append(
                        f"designed surface {surface_id} reference {ref} must be expected-design"
                    )
        elif coverage in {"pattern", "existing-accepted"}:
            if not nonempty(surface.get("pattern_ref")):
                errors.append(f"{coverage} surface {surface_id} needs pattern_ref")
            elif surface["pattern_ref"] not in reference_by_id:
                errors.append(
                    f"{coverage} surface {surface_id} references missing pattern {surface['pattern_ref']}"
                )
            else:
                expected_kind = (
                    "system-pattern" if coverage == "pattern" else "existing-surface"
                )
                if reference_by_id[surface["pattern_ref"]].get("kind") != expected_kind:
                    errors.append(
                        f"{coverage} surface {surface_id} reference {surface['pattern_ref']} "
                        f"must be {expected_kind}"
                    )
        elif coverage == "excluded":
            exclusion = surface.get("exclusion")
            require_fields(exclusion, {"rationale", "impact", "approved_by"}, f"surface {surface_id}.exclusion", errors)

    covered_tokens: dict[str, set[tuple[str, str]]] = {}
    for reference_id, covers in reference_covers.items():
        for token in covers:
            if len(token.split(":")) != 3:
                errors.append(
                    f"reference {reference_id} cover {token!r} must be surface:state:viewport"
                )
                continue
            surface_id, state, viewport = token.split(":")
            if surface_id not in surface_by_id:
                errors.append(f"reference {reference_id} covers missing surface {surface_id}")
                continue
            covered_tokens.setdefault(surface_id, set()).add((state, viewport))

    for surface_id, surface in surface_by_id.items():
        coverage = surface.get("coverage")
        if coverage == "designed":
            allowed_reference_ids = surface_reference_ids.get(surface_id, [])
        elif coverage in {"pattern", "existing-accepted"}:
            allowed_reference_ids = [surface.get("pattern_ref")]
        else:
            continue
        tokens: set[tuple[str, str]] = set()
        for reference_id in allowed_reference_ids:
            for token in reference_covers.get(reference_id, []):
                parts = token.split(":")
                if len(parts) == 3 and parts[0] == surface_id:
                    tokens.add((parts[1], parts[2]))
        for state in surface_states.get(surface_id, []):
            for viewport in surface_viewports.get(surface_id, []):
                if (state, viewport) not in tokens:
                    errors.append(
                        f"{coverage} surface {surface_id} has no approved reference "
                        f"covering state {state} at viewport {viewport}"
                    )

    graph: dict[str, set[str]] = {surface_id: set() for surface_id in surface_by_id}
    for surface_id, surface in surface_by_id.items():
        parent_id = surface.get("parent_surface_id")
        if nonempty(parent_id) and parent_id in graph and parent_id != surface_id:
            graph[parent_id].add(surface_id)
    for transition_id, transition in transition_by_id.items():
        require_fields(transition, {"journey_id", "from", "trigger", "to", "return_behavior"}, f"transitions[{transition_id}]", errors)
        values: dict[str, str] = {}
        for field in ("journey_id", "from", "trigger", "to", "return_behavior"):
            value = transition.get(field)
            if not nonempty(value):
                errors.append(f"transitions[{transition_id}].{field} must be a string")
            else:
                values[field] = value
        journey_id = values.get("journey_id")
        if journey_id is not None and journey_id not in journey_by_id:
            errors.append(f"transition {transition_id} references missing journey {journey_id}")
        for field in ("from", "to"):
            surface_id = values.get(field)
            if surface_id is not None and surface_id not in surface_by_id:
                errors.append(f"transition {transition_id} references missing {field} surface {surface_id}")
        from_id = values.get("from")
        to_id = values.get("to")
        if from_id in graph and to_id in surface_by_id:
            graph[from_id].add(to_id)

    entry_ids = {
        journey.get("entry_surface_id")
        for journey in journey_by_id.values()
        if nonempty(journey.get("entry_surface_id"))
        and journey.get("entry_surface_id") in surface_by_id
    }
    reachable = set(entry_ids)
    frontier = list(entry_ids)
    while frontier:
        source_id = frontier.pop()
        for target_id in graph.get(source_id, set()):
            if target_id not in reachable:
                reachable.add(target_id)
                frontier.append(target_id)
    for surface_id, surface in surface_by_id.items():
        if surface.get("coverage") != "excluded" and surface_id not in reachable:
            errors.append(
                f"included surface {surface_id} is unreachable from a journey entry "
                "through containment or transitions"
            )

    require_fields(
        data.get("ux_contract"),
        {
            "information_architecture",
            "navigation_and_return",
            "interaction_and_disclosure",
            "responsive_behavior",
            "accessibility_and_focus",
            "content_and_state_behavior",
        },
        "ux_contract",
        errors,
    )
    require_fields(
        data.get("visual_system"),
        {
            "composition_and_density",
            "typography",
            "spacing",
            "color_and_surfaces",
            "controls_and_assets",
            "focus_and_motion",
        },
        "visual_system",
        errors,
    )
    require_fields(
        data.get("architecture"),
        {
            "routes_and_navigation",
            "components_and_reuse",
            "state_read_model_and_api",
            "permissions_and_actions",
            "accessibility",
            "migration_and_old_code",
        },
        "architecture",
        errors,
    )

    covered_by_slice: set[str] = set()
    seen_slice_ids: set[str] = set()
    for slice_id, item in slice_by_id.items():
        require_fields(
            item,
            {
                "surface_ids",
                "journey_ids",
                "requirement_ids",
                "data_api_refs",
                "owned_paths",
                "checks",
                "screenshot_checkpoint",
                "stop_threshold",
            },
            f"implementation_slices[{slice_id}]",
            errors,
        )
        dependencies = string_list(
            item.get("dependencies"),
            f"implementation_slices[{slice_id}].dependencies",
            errors,
            allow_empty=True,
        )
        for dependency_id in dependencies:
            if dependency_id not in slice_by_id:
                errors.append(
                    f"implementation slice {slice_id} references missing dependency {dependency_id}"
                )
            elif dependency_id == slice_id:
                errors.append(f"implementation slice {slice_id} cannot depend on itself")
            elif dependency_id not in seen_slice_ids:
                errors.append(
                    f"implementation slice {slice_id} dependency {dependency_id} "
                    "must appear earlier in implementation_slices"
                )
        for field in ("owned_paths", "checks"):
            string_list(
                item.get(field),
                f"implementation_slices[{slice_id}].{field}",
                errors,
            )
        for field in ("journey_ids", "data_api_refs"):
            values = string_list(
                item.get(field),
                f"implementation_slices[{slice_id}].{field}",
                errors,
            )
            if field == "journey_ids":
                for journey_id in values:
                    if journey_id not in journey_by_id:
                        errors.append(
                            f"implementation slice {slice_id} references missing journey {journey_id}"
                        )
        for surface_id in string_list(
            item.get("surface_ids"),
            f"implementation_slices[{slice_id}].surface_ids",
            errors,
        ):
            if surface_id not in surface_by_id:
                errors.append(f"implementation slice {slice_id} references missing surface {surface_id}")
            covered_by_slice.add(surface_id)
        for requirement_id in string_list(
            item.get("requirement_ids"),
            f"implementation_slices[{slice_id}].requirement_ids",
            errors,
        ):
            if requirement_id not in invariant_by_id:
                errors.append(f"implementation slice {slice_id} references missing requirement {requirement_id}")
        seen_slice_ids.add(slice_id)
    for surface_id, surface in surface_by_id.items():
        if surface.get("coverage") != "excluded" and surface_id not in covered_by_slice:
            errors.append(f"included surface {surface_id} is not covered by an implementation slice")

    traced_requirements: set[str] = set()
    traced_surfaces: set[str] = set()
    traced_journeys: set[str] = set()
    for index, row in enumerate(trace_rows):
        label = f"traceability[{index}]"
        require_fields(
            row,
            {
                "requirement_id",
                "journey_ids",
                "surface_ids",
                "reference_ids",
                "implementation_refs",
                "data_refs",
                "test_refs",
                "evidence_slots",
            },
            label,
            errors,
        )
        if not isinstance(row, dict):
            continue
        requirement_id = row.get("requirement_id")
        if not nonempty(requirement_id):
            errors.append(f"{label}.requirement_id must be a string")
        elif requirement_id not in invariant_by_id:
            errors.append(f"{label} references missing requirement {requirement_id}")
        else:
            traced_requirements.add(requirement_id)
        row_lists = {
            field: string_list(
                row.get(field),
                f"{label}.{field}",
                errors,
            )
            for field in (
                "journey_ids",
                "surface_ids",
                "reference_ids",
                "implementation_refs",
                "data_refs",
                "test_refs",
                "evidence_slots",
            )
        }
        for journey_id in row_lists["journey_ids"]:
            if journey_id not in journey_by_id:
                errors.append(f"{label} references missing journey {journey_id}")
            traced_journeys.add(journey_id)
        for surface_id in row_lists["surface_ids"]:
            if surface_id not in surface_by_id:
                errors.append(f"{label} references missing surface {surface_id}")
            traced_surfaces.add(surface_id)
        for reference_id in row_lists["reference_ids"]:
            if reference_id not in reference_by_id:
                errors.append(f"{label} references missing design {reference_id}")
    for requirement_id in invariant_by_id:
        if requirement_id not in traced_requirements:
            errors.append(f"product invariant {requirement_id} has no traceability row")
    for journey_id in journey_by_id:
        if journey_id not in traced_journeys:
            errors.append(f"journey {journey_id} has no traceability row")
    for surface_id, surface in surface_by_id.items():
        if surface.get("coverage") != "excluded" and surface_id not in traced_surfaces:
            errors.append(f"included surface {surface_id} has no traceability row")

    readiness = data.get("readiness")
    readiness_fields = {
        "pm_approved",
        "ux_approved",
        "ui_approved",
        "critic_continue",
        "architect_feasible",
        "owner_approved",
    }
    require_fields(readiness, readiness_fields, "readiness", errors)
    if isinstance(readiness, dict):
        for field in sorted(readiness_fields):
            if readiness.get(field) is not True:
                errors.append(f"readiness.{field} must be true before implementation")

    deviations = data.get("approved_deviations")
    if not isinstance(deviations, list):
        errors.append("approved_deviations must be a list")
    else:
        for index, deviation in enumerate(deviations):
            label = f"approved_deviations[{index}]"
            require_fields(
                deviation,
                {"surface_id", "reference_id", "rationale", "approved_by", "approved_at"},
                label,
                errors,
            )
            if isinstance(deviation, dict):
                surface_id = deviation.get("surface_id")
                reference_id = deviation.get("reference_id")
                if not nonempty(surface_id):
                    errors.append(f"{label}.surface_id must be a string")
                elif surface_id not in surface_by_id:
                    errors.append(
                        f"{label} references missing surface {surface_id}"
                    )
                if not nonempty(reference_id):
                    errors.append(f"{label}.reference_id must be a string")
                elif reference_id not in reference_by_id:
                    errors.append(
                        f"{label} references missing design {reference_id}"
                    )

    acceptance = data.get("acceptance")
    require_fields(
        acceptance,
        {
            "blocking_screenshots",
            "minimum_score_per_screenshot",
            "minimum_dimension_score",
            "require_zero_p0_p1",
            "self_review_required",
            "independent_reviewer_required",
            "owner_acceptance_required",
            "invalidation_triggers",
        },
        "acceptance",
        errors,
    )
    if isinstance(acceptance, dict):
        blocking_references = set(
            string_list(
                acceptance.get("blocking_screenshots"),
                "acceptance.blocking_screenshots",
                errors,
            )
        )
        string_list(
            acceptance.get("invalidation_triggers"),
            "acceptance.invalidation_triggers",
            errors,
        )
        for reference_id in blocking_references:
            if reference_id not in reference_by_id:
                errors.append(
                    f"acceptance.blocking_screenshots references missing design {reference_id}"
                )
            elif reference_by_id[reference_id].get("kind") != "expected-design":
                errors.append(
                    f"acceptance blocking screenshot {reference_id} must be expected-design"
                )
        for surface_id, surface in surface_by_id.items():
            if surface.get("coverage") != "designed" or surface.get("risk") != "high":
                continue
            if not blocking_references.intersection(
                surface_reference_ids.get(surface_id, [])
            ):
                errors.append(
                    f"high-risk designed surface {surface_id} needs a blocking screenshot"
                )
        for field in ("minimum_score_per_screenshot", "minimum_dimension_score"):
            value = acceptance.get(field)
            if not isinstance(value, (int, float)) or isinstance(value, bool) or not 0 <= value <= 100:
                errors.append(f"acceptance.{field} must be a number from 0 to 100")
        for field in (
            "require_zero_p0_p1",
            "self_review_required",
            "independent_reviewer_required",
            "owner_acceptance_required",
        ):
            if acceptance.get(field) is not True:
                errors.append(f"acceptance.{field} must be true for an accepted module spec")
        score = acceptance.get("minimum_score_per_screenshot")
        if isinstance(score, (int, float)) and not isinstance(score, bool) and score < 95:
            require_fields(
                acceptance.get("threshold_exception"),
                {"rationale", "approved_by", "approved_at"},
                "acceptance.threshold_exception",
                errors,
            )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("spec", type=Path)
    args = parser.parse_args()
    try:
        data = json.loads(args.spec.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2
    errors = validate(data)
    if errors:
        print(f"FAIL: {len(errors)} issue(s)", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print(f"PASS: complete frontend module spec ({args.spec})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
