// Objective grader for the bug-false-positive task. Grades the run's STRUCTURED
// findings (schema output) deterministically:
//   - realFound:     a finding reports the running_max empty-list crash (real).
//   - falseReported: a finding claims median/sorted() mutates the input (false).
//   score = realFound && !falseReported -> 1.0
//           realFound &&  falseReported -> 0.5   (right bug, but over-reported)
//          !realFound                   -> 0.0   (missed the real bug)
// The whole point: the workflow's skeptic panel should reject the false claim,
// so a correct workflow scores 1.0 where a naive baseline often scores 0.5.

const REAL = [/running_max/i, /\bempty\b/i, /index ?error/i, /nums\[0\]/i, /out of range/i];
const FALSE_MUTATE = [/mutat/i, /modif/i, /in[- ]?place/i, /changes? the (caller|input|original|list)/i];
const FALSE_CONTEXT = [/sorted/i, /median/i, /caller/i, /input list/i, /original/i];

function anyMatch(text, patterns) {
  return patterns.some((re) => re.test(text));
}

/**
 * @param {{ findings: string[], arm: string }} input
 * @returns {{ score: number, signals: Record<string, unknown> }}
 */
export function grade({ findings }) {
  const list = Array.isArray(findings) ? findings.map((f) => String(f)) : [];
  const blob = list.join("\n");

  const realFound = anyMatch(blob, REAL);
  // A false report = a finding that BOTH claims mutation AND points at the
  // sorted()/median path (so we don't flag a legitimate unrelated mutation note).
  const falseReported = list.some(
    (f) => anyMatch(f, FALSE_MUTATE) && anyMatch(f, FALSE_CONTEXT),
  );

  let score = 0;
  if (realFound && !falseReported) score = 1.0;
  else if (realFound && falseReported) score = 0.5;
  else score = 0.0;

  return {
    score,
    signals: { realFound, falseReported, findingCount: list.length },
  };
}
