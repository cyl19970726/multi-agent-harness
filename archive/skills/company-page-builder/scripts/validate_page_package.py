#!/usr/bin/env python3
"""Validate a declarative governed Company OS custom-page package."""

import json
import pathlib
import re
import sys


REQUIRED = {
    "schema_version", "package_id", "definition_id", "version", "entrypoint",
    "declared_queries", "declared_actions", "fallback_document_id",
    "fallback_view_ids", "fixture_id", "expected_artifact", "expected_hash",
    "approval_ref", "permissions", "audit_events", "rollback_to_version",
}


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: validate_page_package.py <package.json>", file=sys.stderr)
        return 2
    data = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
    missing = sorted(REQUIRED - data.keys())
    if missing:
        raise ValueError(f"missing keys: {', '.join(missing)}")
    if data["schema_version"] != 1:
        raise ValueError("schema_version must be 1")
    if not re.fullmatch(r"\d+\.\d+\.\d+", str(data["version"])):
        raise ValueError("version must be semantic x.y.z")
    if not data["fallback_document_id"] or not data["fallback_view_ids"]:
        raise ValueError("fallback document and at least one view are required")
    if not data["approval_ref"]:
        raise ValueError("an explicit visual/design approval reference is required")
    if any("write" in str(permission).lower() for permission in data["permissions"]):
        raise ValueError("direct write permissions are forbidden; use governed Actions")
    if data["rollback_to_version"] in {None, ""}:
        raise ValueError("rollback target is required")
    print(f"valid custom page package: {data['package_id']}@{data['version']}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"invalid custom page package: {exc}", file=sys.stderr)
        raise SystemExit(1)
