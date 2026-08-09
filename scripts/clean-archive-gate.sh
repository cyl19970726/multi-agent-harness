#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
candidate_ref="${1:-HEAD}"
candidate_sha="$(git -C "$repo_root" rev-parse --verify "${candidate_ref}^{commit}")"

if [ -n "$(git -C "$repo_root" status --porcelain)" ]; then
  echo "clean-archive gate requires a clean source worktree" >&2
  exit 1
fi

pnpm_version="$(pnpm --version)"
if [ "$pnpm_version" != "9.15.4" ]; then
  echo "clean-archive gate requires pnpm 9.15.4, found $pnpm_version" >&2
  exit 1
fi

archive_root="$(mktemp -d /tmp/star-harness-clean-archive.XXXXXX)"
archive_root="$(cd "$archive_root" && pwd -P)"
case "$archive_root" in
  /private/tmp/star-harness-clean-archive.*|/tmp/star-harness-clean-archive.*) ;;
  *)
    echo "refusing unexpected clean-archive path: $archive_root" >&2
    exit 1
    ;;
esac
cleanup() {
  case "$archive_root" in
    /private/tmp/star-harness-clean-archive.*|/tmp/star-harness-clean-archive.*)
      rm -rf -- "$archive_root"
      ;;
  esac
}
trap cleanup EXIT

git -C "$repo_root" archive "$candidate_sha" | tar -x -C "$archive_root"
cd "$archive_root"

export FIRM_BUILD_GIT_REV="$candidate_sha"
export CARGO_TARGET_DIR="$archive_root/.target"

echo "clean-archive candidate: $candidate_sha"
pnpm install --frozen-lockfile
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
cargo run -q -p firm-cli -- governance check
pnpm check

echo "clean-archive gate passed: $candidate_sha"
