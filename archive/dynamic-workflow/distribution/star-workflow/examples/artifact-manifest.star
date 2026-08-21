# ARTIFACT MANIFEST - preserve generated files from a writable worktree.
#
# expected_artifacts copies declared repo-relative files out of the throwaway
# worktree. artifact_manifest records exists/size/hash/status for dashboard and
# later review. Use this for reports, generated assets, screenshots, and other
# files that should survive worktree cleanup.
#
# Run:
#   harness workflow run-script ./artifact-manifest.star \
#     --args '{"topic":"workflow patch landing","artifact":"out/workflow-report.md"}'

workflow(
    "artifact-manifest",
    "Generate a declared repo-relative artifact in a writable worktree, copy it " +
    "back with expected_artifacts, and record a WorkflowArtifactManifest so the " +
    "file's existence/hash/status are visible after cleanup.",
    budget_usd = 4.0,
    success_criterion = "the artifact path is declared, copied back, and manifest-tracked",
)

topic = args["topic"]
artifact = args["artifact"] if "artifact" in args else "out/workflow-report.md"
artifact_root = args["artifact_root"] if "artifact_root" in args else "out"

RESULT = {
    "created": "bool",
    "artifact_path": "the repo-relative artifact path",
    "summary": "what the artifact contains",
}

phase("build-artifact")
result = agent(
    """Create a non-empty Markdown report artifact.

TOPIC: {topic}
ARTIFACT PATH: {artifact}

Requirements:
- Write the file exactly at ARTIFACT PATH, relative to the repo root.
- Include a title, a concise summary, and 3-5 concrete bullets.
- Do not write outside the artifact root.

Return created=true only after the file exists and is non-empty.""".format(
        topic=topic,
        artifact=artifact,
    ),
    provider = "codex",
    label = "artifact-builder",
    writable = True,
    expected_artifacts = [artifact],
    artifact_root = artifact_root,
    write_roots = [artifact_root],
    schema = RESULT,
)

artifact_manifest(
    [artifact],
    label = "artifact-builder",
    artifact_root = artifact_root,
    write_roots = [artifact_root],
)

output(result)
ok = type(result) == "dict" and result["created"] == True
verdict(ok, reason = result["summary"] if type(result) == "dict" else "artifact worker produced no JSON")
