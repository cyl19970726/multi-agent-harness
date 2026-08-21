# ASSESS -> VERIFY -> SYNTHESIZE — ported from the internal `evaluate-external-workflow`
# program. Evaluation as an ADVERSARIAL DIALOGUE, not a single pass, streamed with
# pipeline() so each dimension verifies the instant its assessment lands:
#
#   pipeline(dims, assess, verify)   each dimension flows assess -> verify with NO
#                                    barrier; the verify stage is fed the assessment
#                                    (forward-injected via {input}) and tries to
#                                    REFUTE it, emitting a CONSOLIDATED verdict
#   synthesize                       one report over the verified verdicts, honoring
#                                    the verifiers' corrections
#
# Why this beats a flat "assess each dimension" fan-out: a single assessor over-
# claims; an independent verifier that must refute each claim catches the
# overstatements, and the synthesis is built from the CORRECTED verdicts. Every
# handoff is a multi-field schema, so the synthesis reads typed fields, not prose.
#
# Read-only. Run:  harness workflow run-script ./assess-verify-synthesize.star \
#   --args '{"subject":"the checkout service","dimensions":["correctness","safety","observability","performance"]}'

workflow(
    "assess-verify-synthesize",
    "Evaluate a subject across dimensions: pipeline each dimension assess -> " +
    "adversarial verify (the verifier is fed the assessment and tries to refute " +
    "each claim, emitting a corrected verdict) with no barrier, then synthesize " +
    "one report over the verified verdicts — evaluation as adversarial dialogue.",
    budget_usd = 6.0,
    success_criterion = "a report whose scorecard reflects the VERIFIED (refuted-where-wrong) scores, not the raw assessments",
)

subject = args["subject"]
dims = args["dimensions"] if "dimensions" in args else ["correctness", "safety", "observability", "performance"]

# ---- typed contracts ----------------------------------------------------------
ASSESS = {
    "dimension": "the dimension assessed",
    "score": "one of: pass | partial | gap",
    "evidence": "concrete evidence you actually found (a list)",
    "gaps": "concrete gaps, empty if none (a list)",
}
# The verifier READS the assessment (forward-injected) and emits a CONSOLIDATED
# verdict, so the pipeline's last-stage result already carries everything synthesis needs.
VERIFY = {
    "dimension": "the dimension",
    "verified_score": "one of: pass | partial | gap (corrected if the assessment overstated)",
    "held_up": "did the assessment hold under scrutiny? true/false",
    "corrections": "corrections to any overstated or wrong claim, empty if it held (a list)",
    "key_point": "the single most important point for the synthesis",
}
REPORT = {
    "overall": "2-3 sentence overall verdict",
    "scorecard": "one line per dimension: dimension -> verified_score (a list)",
    "top_findings": "the most important findings across dimensions (a list)",
    "roadmap": "prioritized next steps, P0..P3 with why (a list)",
}

# ---- pipeline: each dimension streams assess -> verify (no barrier) -----------
phase("evaluate")
verdicts = pipeline(
    dims,
    [
        {
            "prompt": "Subject: " + subject + ".\nAssess it on this ONE dimension: {input}. " +
                      "Ground EVERY claim in real evidence you can point to; do not speculate. " +
                      "Score pass/partial/gap.",
            "schema": ASSESS,
            "label": "assess",
            "phase": "evaluate",
        },
        {
            "prompt": "Subject: " + subject + ".\nAdversarially CHECK this assessment: try to " +
                      "REFUTE each claimed gap — is it REAL, or overstated / already handled? " +
                      "Emit a consolidated verdict with the CORRECTED score.\n\nASSESSMENT:\n{input}",
            "schema": VERIFY,
            "label": "verify",
            "phase": "evaluate",
        },
    ],
)
verified = [v for v in verdicts if type(v) == "dict"]
log("verified " + str(len(verified)) + " of " + str(len(dims)) + " dimensions")

# ---- synthesize: one report over the VERIFIED verdicts ------------------------
phase("synthesize")
report = agent(
    "Synthesize these adversarially-verified dimension verdicts for " + subject +
    " into an overall evaluation: an overall verdict, a scorecard (dimension -> " +
    "verified_score), top findings, and a prioritized P0..P3 roadmap. Honor the " +
    "verifiers' corrections — use the VERIFIED scores, not raw assessments.\n\n" +
    "VERIFIED VERDICTS:\n" + json.encode(verified),
    schema = REPORT,
    label = "synthesize",
)

ok = type(report) == "dict"
verdict(ok, reason = report["overall"] if ok else "no report produced")
