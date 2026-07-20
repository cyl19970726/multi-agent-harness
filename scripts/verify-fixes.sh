#!/usr/bin/env bash
# Focused multi-project acceptance. The former script mixed project routing with
# the retired Goal/Task planning stack; those checks now live only in the
# verified legacy archive. This script exercises the active project registry,
# resolution, migration, cwd routing, provider delivery, and serve contracts.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ "${1:-}" == "--real" ]]; then
  echo "note: --real no longer adds retired planning-loop checks; running the active multi-project suite"
elif [[ $# -gt 0 ]]; then
  echo "usage: scripts/verify-fixes.sh [--real]" >&2
  exit 2
fi

cd "$repo_root"

cargo test -p harness-core project -- --nocapture
cargo test -p harness-cli \
  --test project_registry \
  --test project_resolution \
  --test init_project \
  --test workflow_cwd \
  --test workflow_project_options \
  --test global_project_workflow \
  --test delivery_project_context \
  --test codex_delivery_cwd \
  --test claude_delivery_cwd \
  --test serve_projects_api \
  --test serve_sse_projects \
  --test project_convergence \
  --test project_cli \
  --test project_migrate \
  --test store_source

echo "multi-project acceptance passed"
