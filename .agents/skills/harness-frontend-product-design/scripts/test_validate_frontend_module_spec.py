#!/usr/bin/env python3
"""Regression tests for the Frontend Module Spec validator."""

from __future__ import annotations

import copy
import importlib.util
import json
import re
import unittest
from pathlib import Path

SKILL_ROOT = Path(__file__).resolve().parents[1]
TEMPLATE = SKILL_ROOT / "assets" / "frontend-module-spec.template.json"
VALIDATOR = SKILL_ROOT / "scripts" / "validate_frontend_module_spec.py"

spec = importlib.util.spec_from_file_location("frontend_module_validator", VALIDATOR)
assert spec and spec.loader
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class FrontendModuleSpecValidatorTest(unittest.TestCase):
    def setUp(self) -> None:
        self.data = json.loads(TEMPLATE.read_text(encoding="utf-8"))
        self.data = self._materialize(self.data)

    @classmethod
    def _materialize(cls, value):
        if isinstance(value, dict):
            return {key: cls._materialize(item) for key, item in value.items()}
        if isinstance(value, list):
            return [cls._materialize(item) for item in value]
        if isinstance(value, str):
            return re.sub(r"replace[- ]with", "project-defined", value, flags=re.I)
        return value

    def test_materialized_template_is_structurally_complete(self) -> None:
        self.assertEqual(validator.validate(self.data), [])

    def test_unfilled_template_cannot_authorize_implementation(self) -> None:
        template = json.loads(TEMPLATE.read_text(encoding="utf-8"))
        errors = validator.validate(template)
        self.assertTrue(
            any("unresolved template placeholder" in error for error in errors)
        )

    def test_rejects_unjustified_surface(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["surfaces"][0].pop("distinct_surface_rationale")
        errors = validator.validate(candidate)
        self.assertIn(
            "surfaces[primary-surface].distinct_surface_rationale is required",
            errors,
        )

    def test_rejects_unreachable_discovered_surface(self) -> None:
        candidate = copy.deepcopy(self.data)
        extra = copy.deepcopy(candidate["surfaces"][0])
        extra["id"] = "secondary-surface"
        extra["name"] = "A second project-discovered surface"
        extra["reference_ids"] = ["primary-reference"]
        candidate["surfaces"].append(extra)
        candidate["implementation_slices"][0]["surface_ids"].append(extra["id"])
        candidate["traceability"][0]["surface_ids"].append(extra["id"])
        errors = validator.validate(candidate)
        self.assertTrue(
            any("secondary-surface is unreachable" in error for error in errors)
        )

    def test_rejects_untraced_discovered_surface(self) -> None:
        candidate = copy.deepcopy(self.data)
        extra = copy.deepcopy(candidate["surfaces"][0])
        extra["id"] = "secondary-surface"
        extra["name"] = "A second project-discovered surface"
        extra["parent_surface_id"] = "primary-surface"
        extra["reference_ids"] = ["primary-reference"]
        candidate["surfaces"].append(extra)
        candidate["implementation_slices"][0]["surface_ids"].append(extra["id"])
        errors = validator.validate(candidate)
        self.assertIn(
            "included surface secondary-surface has no traceability row", errors
        )

    def test_rejects_missing_expected_design_coverage(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["surfaces"][0]["required_viewports"].append("mobile")
        errors = validator.validate(candidate)
        self.assertIn(
            "designed surface primary-surface has no approved reference covering state representative at viewport mobile",
            errors,
        )

    def test_rejects_unknown_blocking_screenshot(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["acceptance"]["blocking_screenshots"].append("missing-reference")
        errors = validator.validate(candidate)
        self.assertIn(
            "acceptance.blocking_screenshots references missing design missing-reference",
            errors,
        )

    def test_rejects_unapproved_product_definition(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["readiness"]["pm_approved"] = False
        errors = validator.validate(candidate)
        self.assertIn(
            "readiness.pm_approved must be true before implementation", errors
        )

    def test_rejects_missing_pm_problem(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["product_definition"].pop("problem_statement")
        errors = validator.validate(candidate)
        self.assertIn("product_definition.problem_statement is required", errors)

    def test_template_does_not_choose_the_projects_interface(self) -> None:
        template = json.loads(TEMPLATE.read_text(encoding="utf-8"))
        self.assertTrue(template["module"]["id"].startswith("replace-with"))
        self.assertTrue(template["surfaces"][0]["kind"].startswith("replace-with"))
        self.assertTrue(
            template["surfaces"][0]["distinct_surface_rationale"].startswith(
                "Replace with"
            )
        )
        self.assertEqual(template["transitions"], [])

    def test_rejects_unapproved_spec(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["module"]["status"] = "draft"
        errors = validator.validate(candidate)
        self.assertIn("module.status must be approved before implementation", errors)

    def test_lower_threshold_requires_owner_approved_exception(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["acceptance"]["minimum_score_per_screenshot"] = 90
        errors = validator.validate(candidate)
        self.assertTrue(
            any("acceptance.threshold_exception" in error for error in errors)
        )

    def test_high_risk_design_must_block_acceptance(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["acceptance"]["blocking_screenshots"].clear()
        errors = validator.validate(candidate)
        self.assertIn(
            "high-risk designed surface primary-surface needs a blocking screenshot",
            errors,
        )

    def test_malformed_list_is_reported_without_crashing(self) -> None:
        candidate = copy.deepcopy(self.data)
        candidate["references"][0]["covers"] = 42
        candidate["surfaces"][0]["journey_ids"] = 42
        candidate["acceptance"]["blocking_screenshots"] = 42
        errors = validator.validate(candidate)
        self.assertIn(
            "references[primary-reference].covers must be a non-empty list", errors
        )
        self.assertIn(
            "surfaces[primary-surface].journey_ids must be a non-empty list", errors
        )
        self.assertIn(
            "acceptance.blocking_screenshots must be a non-empty list", errors
        )


if __name__ == "__main__":
    unittest.main()
