workflow(
    "verify-fixes",
    """Real regression probe for the Issue #107 Gap 5 + Issue #139 fixes, run via
REAL codex (no --dry-run): a typed-schema leaf (#5 schema coercion + #2 final-message
structured), a shell-command leaf (Gap 5 A2 codex command fidelity, asserted against
the normalized endpoint by verify-fixes.sh), a bare-POSITIONAL pipeline stage (#4),
and a POSITIONAL verdict reason (#6). Driven by scripts/verify-fixes.sh --real.""",
)

# #5 schema coercion + #2 final-message structured + #1 stdin (real codex --output-schema).
# If the coercion works, `ok` is a real bool and `n` a real int — NOT the strings
# "true"/"7" — so the comparisons in the verdict below are meaningful.
typed = agent(
    "Return ok=true and n=7 and label=\"done\". Emit ONLY the final structured JSON object, nothing before it.",
    schema = {"ok": "bool", "n": "int", "label": "string"},
    label = "typed",
)
log("typed=" + str(typed) + " ok_type=" + (type(typed["ok"]) if type(typed) == "dict" else "?"))

# Gap 5 A2: a real codex command_execution. The normalized endpoint must expand this
# session into tool_call + tool_result (verify-fixes.sh curls it for this run's store).
agent("Run the shell command: ls crates ; then in one sentence say which crates exist.", label = "cmd")

# #4: a pipeline with a BARE-POSITIONAL stage (NOT wrapped in a list) + a real codex stage.
pipe = pipeline(["beta"], {"prompt": "Reply with exactly this word: {input}", "label": "echo"})
log("pipe_len=" + str(len(pipe)))

# #6: verdict with a POSITIONAL reason. `ok` is True ONLY if #5 produced a real bool+int
# (a string "true" would fail `== True`), so a passing verdict is itself the #5 proof.
ok = type(typed) == "dict" and typed["ok"] == True and typed["n"] == 7
verdict(ok, "real codex returned a real bool+int via coerced schema")
output({"typed": typed, "pipe_len": len(pipe)})
