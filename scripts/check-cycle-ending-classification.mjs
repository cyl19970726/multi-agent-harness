#!/usr/bin/env node
/**
 * ADR 0076 gate: every way a cycle can end is classified.
 *
 * The closed `CycleEnding` table only stays closed if every `Err` leaving an
 * adapter's cycle body records one. The per-adapter tests prove the local enum
 * maps totally onto the table; they cannot prove that every `Err` SITE records
 * a variant. Review round 1 of X1a found four that did not, and the inventory
 * had missed them because it was built by grepping for the classification
 * helper — four of the hits were in sibling functions that make the same calls
 * as `run_cycle`.
 *
 * So this walks instead of grepping. For each cycle entry point the function is
 * located by signature, its body is brace-matched to its exact closing brace,
 * comments and string literals are stripped, and every remaining `?` operator
 * and `return Err(` is reported with the statement it terminates. A site counts
 * as classified only with a stated reason:
 *
 *   self      the statement routes through classify( / self.fail( /
 *             inspect_err, or assigns the adapter's typed failure field;
 *   preceding the immediately preceding statement assigns it (guard-then-call);
 *   callee    it propagates out of a function that records the typed failure at
 *             every one of its own exits — and that function is itself walked
 *             here, so the claim is checked rather than asserted;
 *   caller    it is inside a closure the adapter hands to a client, and the
 *             client wraps that call and records (cited per site);
 *   enclosing a typed-failure assignment earlier in the body sits at a brace
 *             depth no deeper than the site, so it is on the site's own path.
 *             Scoped by depth on purpose: an assignment in a SIBLING branch is
 *             not on this path, and the self-test pins that it is rejected.
 *
 * This is a `scripts/` gate rather than a Rust test because it is a
 * cross-crate SOURCE-STRUCTURE assertion spanning five provider crates plus the
 * contract, which is exactly the shape of every other boundary gate here
 * (provider-runtime packages, work-kernel, runtime-composition, node-daemon,
 * native-session). A Rust test would have to live in one crate and read four
 * siblings' sources at test time, which nothing in this tree does.
 *
 * Run `--self-test` for the negative control: the analyser is run against six
 * synthetic bodies, including two it MUST reject (a bare `?` and a bare
 * multi-line `return Err`) and one whose only typed-failure assignment sits in
 * a sibling branch, which is not on the failing path and must not count. A gate
 * that cannot fail proves nothing, so those are the cases that matter.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("..", import.meta.url)));

/** Every cycle entry point, by exact signature prefix. */
const ENTRY_POINTS = [
  ["claude", "crates/firm-provider-claude/src/transport.rs", "    pub(crate) fn run_cycle("],
  ["codex", "crates/firm-provider-codex/src/team_runtime.rs", "    fn run_cycle("],
  ["kimi", "crates/firm-provider-kimi/src/team_runtime.rs", "    fn run_cycle("],
  ["kimi", "crates/firm-provider-kimi/src/lib.rs", "    pub fn prompt("],
  ["kimi", "crates/firm-provider-kimi/src/lib.rs", "    fn drive_prompt("],
  ["deepseek", "crates/firm-provider-deepseek/src/lib.rs", "    fn run_cycle("],
  ["pi", "crates/firm-provider-pi/src/team_runtime.rs", "    fn run_cycle("],
  ["pi", "crates/firm-provider-pi/src/lib.rs", "    pub fn prompt_dyn("],
  ["pi", "crates/firm-provider-pi/src/lib.rs", "    fn apply_cycle_control("],
  ["pi", "crates/firm-provider-pi/src/lib.rs", "    pub fn request_blocking("],
];

/**
 * Callees that record the typed failure at EVERY one of their own exits.
 *
 * The invariant is that each is itself an ENTRY POINT above, so the claim is
 * verified by this same walk rather than taken on trust. That used to be a
 * comment, and a comment is not a check: X1b review r1 demonstrated the evasion
 * by adding a non-recording helper here and watching the gate stay green, and
 * `write_frame` already broke the stated rule (it was never an entry point, and
 * it was not needed — its one call site is covered by the conservative
 * assignment immediately above it). `validateRecordingCallees` below now
 * enforces the invariant, with no exceptions.
 */
const RECORDING_CALLEES = new Map([
  [
    "request_blocking",
    "pi/lib.rs request_blocking records at every exit (pre-write TransportLost, post-write StartRejected|PostconditionUnknown, Timeout, Disconnected)",
  ],
  [
    "apply_cycle_control",
    "pi/lib.rs apply_cycle_control records fatal_error=HostAborted and delegates the abort to request_blocking plus its own inspect_err",
  ],
]);

/**
 * Closures an adapter hands to a client, which the client wraps and records.
 *
 * `validateCallerWrapped` checks the half that is mechanically checkable: the
 * site must live inside a body this gate walks. The other half — that the
 * client really does record when it wraps that call — is prose, because the
 * wrapping happens in a different function under a different parameter name
 * (`on_input_accepted` here, `on_accepted` there). This is the one remaining
 * piece of unenforced trust in the gate, and it is named rather than left for a
 * reader to discover: adding an entry here is a deliberate edit, visible in
 * review, and each must cite where the wrapping records.
 */
const CALLER_WRAPPED = new Map([
  [
    "crates/firm-provider-kimi/src/team_runtime.rs::on_input_accepted",
    "wrapped by the client in kimi/lib.rs drive_prompt -> KimiCycleFailure::HostAborted",
  ],
  [
    "crates/firm-provider-kimi/src/team_runtime.rs::control_error",
    "the control closure stashes the fatal error and the adapter re-raises it through self.fail(KimiCycleFailure::HostAborted) right after client.prompt returns",
  ],
]);

/**
 * Tags that classify the statement they appear in, and ONLY that statement.
 * `classify(` / `self.fail(` / `inspect_err` each wrap exactly one call.
 */
const SELF_TAGS = [
  "classify(",
  "self.fail(",
  "inspect_err",
  "last_prompt_failure =",
  "last_cycle_failure =",
  "last_rpc_failure =",
  "last_cycle_ending =",
];

/**
 * Tags that classify every later site on the same path: a field assignment
 * leaves the typed failure set until something overwrites it, which is the
 * conservative-default pattern Pi and Codex use before a write.
 *
 * Deliberately NOT the wrapper helpers. An earlier `self.classify(...)` covers
 * its own call and nothing else — treating it as path-scoped made this gate
 * pass the exact defect it exists to catch (X1a review r1 B1, where a bare
 * `self.receive_event(..)?` sat below an unrelated classified call), which is
 * why the self-test now pins that shape.
 *
 * And a RESET is not a classification. Every adapter opens its cycle with
 * `self.last_cycle_ending = None;`, which sets the field to the absence of an
 * ending; counting it as one let the same defect through a second time. Only an
 * assignment to a value classifies, so `= None` is excluded below.
 */
const PATH_TAGS = [
  "last_prompt_failure =",
  "last_cycle_failure =",
  "last_rpc_failure =",
  "last_cycle_ending =",
];

/** The function name in an ENTRY_POINTS signature, e.g. `fn run_cycle(` -> run_cycle. */
function entryPointName(signature) {
  const match = signature.match(/\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/);
  return match ? match[1] : null;
}

/**
 * Every RECORDING_CALLEES key must name a function this gate itself walks.
 * Otherwise the entry is an unchecked promise that the callee records, which is
 * exactly how a cycle body can exit through a helper that records nothing while
 * the gate stays green.
 */
function validateRecordingCallees(callees, entryPoints) {
  const walked = new Set(
    entryPoints.map(([, , signature]) => entryPointName(signature)).filter(Boolean),
  );
  const problems = [];
  for (const [callee, reason] of callees) {
    if (!walked.has(callee)) {
      problems.push(
        `RECORDING_CALLEES lists \`${callee}\` (${reason}), but it is not an ENTRY_POINTS function, so nothing here verifies that it records at every exit. Add it to ENTRY_POINTS or remove the entry.`,
      );
    }
  }
  return problems;
}

/** Every CALLER_WRAPPED site must live in a body this gate walks. */
function validateCallerWrapped(wrapped, entryPoints) {
  const paths = new Set(entryPoints.map(([, path]) => path));
  const problems = [];
  for (const [key] of wrapped) {
    const [path] = key.split("::");
    if (!paths.has(path)) {
      problems.push(
        `CALLER_WRAPPED names \`${key}\`, but ${path} holds no walked cycle body, so the entry can never apply.`,
      );
    }
  }
  return problems;
}

/**
 * Whether a line assigns the typed failure to a VALUE on this path. A reset to
 * `None` is the absence of an ending, not one, so it never classifies.
 */
function isPathAssignment(code) {
  if (!PATH_TAGS.some((tag) => code.includes(tag))) return false;
  return !/=\s*None\s*;/.test(code);
}

/** Drop line comments and string literals so their contents cannot match. */
function stripNoise(line) {
  let out = "";
  let inString = false;
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i];
    if (inString) {
      if (c === "\\") {
        i += 1;
        continue;
      }
      if (c === '"') inString = false;
      continue;
    }
    if (c === '"') {
      inString = true;
      continue;
    }
    if (line.startsWith("//", i)) break;
    out += c;
  }
  return out;
}

/** Brace-match a function body from its signature line to its closing brace. */
function locateBody(lines, signature) {
  const start = lines.findIndex((line) => line.startsWith(signature));
  if (start < 0) return null;
  let depth = 0;
  let seen = false;
  for (let i = start; i < lines.length; i += 1) {
    for (const c of stripNoise(lines[i])) {
      if (c === "{") {
        depth += 1;
        seen = true;
      } else if (c === "}") {
        depth -= 1;
        if (seen && depth === 0) return { start, end: i };
      }
    }
  }
  return null;
}

/**
 * The whole statement a site terminates: balanced upward, so a `})?;` closing a
 * multi-line `.inspect_err(|_| { ... })` is attributed to the whole call chain
 * and not to its last line.
 */
function statementAt(lines, start, end, index) {
  const balance = (text) =>
    (text.split("(").length - 1 + (text.split("{").length - 1)) -
    (text.split(")").length - 1 + (text.split("}").length - 1));
  let j = index;
  let acc = stripNoise(lines[index]);
  while (j > start && balance(acc) < 0) {
    j -= 1;
    acc = `${stripNoise(lines[j])}\n${acc}`;
  }
  while (j > start) {
    const prev = stripNoise(lines[j - 1]).trimEnd();
    if (prev === "" || /[;{}]$/.test(prev)) break;
    j -= 1;
    acc = `${stripNoise(lines[j])}\n${acc}`;
  }
  // A multi-line opener such as `return Err(` carries its content BELOW it, so
  // extend downward until the call closes. Without this the analyser reads only
  // `return Err(` and cannot see the `self.fail(...)` on the next line.
  //
  // Parentheses only, never braces. `let Some(x) = f()? else {` leaves a brace
  // open, and extending across it swallowed the whole `else` block — so an
  // unrelated classified call INSIDE that block classified the bare `?` above
  // it. That is precisely the X1a r1 B1 defect, and it is why the self-test
  // pins this shape.
  const parenBalance = (text) =>
    text.split("(").length - text.split(")").length;
  let k = index;
  while (k < end && parenBalance(acc) > 0) {
    k += 1;
    acc = `${acc}\n${stripNoise(lines[k])}`;
  }
  return { text: acc, start: j, end: k };
}

/** Brace depth at the start of a line, relative to the body. */
function depthAt(lines, start, index) {
  let depth = 0;
  for (let i = start; i < index; i += 1) {
    for (const c of stripNoise(lines[i])) {
      if (c === "{") depth += 1;
      else if (c === "}") depth -= 1;
    }
  }
  return depth;
}

/** Walk one body and return every site with its classification reason. */
function walkBody(lines, path, start, end) {
  const sites = [];
  for (let i = start; i <= end; i += 1) {
    const code = stripNoise(lines[i]);
    // Any postfix `?`. The narrower form this started as — `?` followed by
    // `;)., ]` or end of line — silently skipped `let Some(x) = f()? else {`,
    // which is the exact shape of the X1a r1 B1 defect: the site was never
    // detected, so it could never be reported unclassified. Comments and string
    // literals are already stripped, so a bare `?` here is the operator.
    const isPropagation = code.includes("?");
    const isReturnErr = /\breturn\s+Err\s*\(/.test(code);
    if (!isPropagation && !isReturnErr) continue;

    const statement = statementAt(lines, start, end, i);
    let reason = null;
    if (SELF_TAGS.some((tag) => statement.text.includes(tag))) {
      reason = "self";
    }
    if (!reason) {
      for (const [fn, why] of RECORDING_CALLEES) {
        if (new RegExp(`\\b${fn}\\s*\\(`).test(statement.text)) {
          reason = `callee: ${why}`;
          break;
        }
      }
    }
    if (!reason) {
      for (const [key, why] of CALLER_WRAPPED) {
        const [wrappedPath, fn] = key.split("::");
        if (wrappedPath === path && new RegExp(`\\b${fn}\\b`).test(statement.text)) {
          reason = `caller: ${why}`;
          break;
        }
      }
    }
    if (!reason) {
      // Guard-then-call: the immediately preceding statement assigns the typed
      // failure, so the site below it cannot escape unrecorded.
      let k = statement.start - 1;
      while (k > start && stripNoise(lines[k]).trim() === "") k -= 1;
      if (k > start && isPathAssignment(stripNoise(lines[k]))) {
        reason = `preceding: line ${k + 1} sets the typed failure before this call`;
      }
    }
    if (!reason) {
      // Enclosing assignment: a typed-failure assignment earlier in this body,
      // at a brace depth no deeper than the site's, is on the site's own path.
      // Scoped by depth deliberately — an assignment inside a SIBLING branch is
      // not on this path and must not count, which the self-test pins.
      const siteDepth = depthAt(lines, start, statement.start);
      for (let k = statement.start - 1; k > start; k -= 1) {
        const code = stripNoise(lines[k]);
        if (!isPathAssignment(code)) continue;
        const candidateDepth = depthAt(lines, start, k);
        if (candidateDepth > siteDepth) continue;
        // Depth alone is not enough: a SIBLING branch can sit at the same depth
        // and still not be on this path. It is on the path only if the block
        // holding the assignment never closes before the site — i.e. the depth
        // between them never drops below the assignment's own.
        let closedBefore = false;
        for (let m = k + 1; m <= statement.start; m += 1) {
          if (depthAt(lines, start, m) < candidateDepth) {
            closedBefore = true;
            break;
          }
        }
        if (closedBefore) continue;
        reason = `enclosing: line ${k + 1} sets the typed failure on this path`;
        break;
      }
    }
    sites.push({
      line: i + 1,
      code: lines[i].trim(),
      reason,
      statement: statement.text.trim(),
    });
  }
  return sites;
}

function analyse(source, path, signature) {
  const lines = source.split("\n");
  const body = locateBody(lines, signature);
  if (!body) return null;
  return walkBody(lines, path, body.start, body.end);
}

// ---------------------------------------------------------------------------

if (process.argv.includes("--self-test")) {
  // The negative control. Each fixture pins one rule, and the two that must be
  // REJECTED matter most: a gate that cannot fail proves nothing.
  const fixtures = [
    {
      name: "a classified `?` is accepted",
      body: [
        "        let sent = self.send_input(input);",
        "        self.classify(sent, Failure::TransportClosed)?;",
      ],
      expect: ["self"],
    },
    {
      name: "a bare `?` is REJECTED",
      body: ["        self.send_input(input)?;"],
      expect: [null],
    },
    {
      name: "a multi-line `return Err(self.fail(..))` is accepted",
      body: [
        "        if bad {",
        "            return Err(",
        "                self.fail(Failure::UnknownTerminalStatus, error)",
        "            );",
        "        }",
      ],
      expect: ["self"],
    },
    {
      name: "a bare multi-line `return Err(..)` is REJECTED",
      body: [
        "        if bad {",
        "            return Err(CliError::Usage(format!(",
        '                "something went wrong"',
        "            )));",
        "        }",
      ],
      expect: [null],
    },
    {
      name: "an assignment on the site's own path classifies it",
      body: [
        "        self.last_cycle_failure = Failure::TransportLost;",
        "        let status = self.probe();",
        "        return Err(CliError::Usage(format!(",
        '            "transport disconnected"',
        "        )));",
      ],
      expect: ["enclosing"],
    },
    {
      name: "a bare let-else `?` is REJECTED even when its else-block classifies",
      body: [
        "        let Some(event) = self.receive_event(POLL)? else {",
        "            let alive = self.ensure_alive();",
        "            self.classify(alive, Failure::TransportClosed)?;",
        "            continue;",
        "        };",
      ],
      expect: [null, "self"],
    },
    {
      name: "a per-cycle reset to None does NOT classify a later bare `?`",
      body: [
        "        self.last_cycle_ending = None;",
        "        loop {",
        "            let sent = self.interrupt();",
        "            self.classify(sent, Failure::TransportClosed)?;",
        "            let Some(event) = self.receive_event(POLL)? else {",
        "                continue;",
        "            };",
        "        }",
      ],
      expect: ["self", null],
    },
    {
      name: "an assignment in a SIBLING branch does NOT classify it",
      body: [
        "        if other {",
        "            self.last_cycle_failure = Failure::TransportLost;",
        "        }",
        "        if bad {",
        "            return Err(CliError::Usage(format!(",
        '                "something else went wrong"',
        "            )));",
        "        }",
      ],
      expect: [null],
    },
  ];

  const problems = [];

  // The allowlist half. X1b review r1 added a non-recording helper to
  // RECORDING_CALLEES and the gate stayed green: the invariant was a comment,
  // so a cycle body could exit through a helper that records nothing. Both
  // shapes must now be rejected.
  const unbackedCallee = validateRecordingCallees(
    new Map([["reviewer_probe_helper", "a helper that records nothing"]]),
    ENTRY_POINTS,
  );
  if (unbackedCallee.length !== 1) {
    problems.push(
      `an allowlisted callee that is not an entry point must be rejected, got ${JSON.stringify(unbackedCallee)}`,
    );
  }
  const backedCallee = validateRecordingCallees(
    new Map([["request_blocking", "a real entry point"]]),
    ENTRY_POINTS,
  );
  if (backedCallee.length !== 0) {
    problems.push(
      `an allowlisted callee that IS an entry point must be accepted, got ${JSON.stringify(backedCallee)}`,
    );
  }
  const unwalkedWrap = validateCallerWrapped(
    new Map([["crates/firm-provider-kimi/src/nowhere.rs::on_input_accepted", "prose"]]),
    ENTRY_POINTS,
  );
  if (unwalkedWrap.length !== 1) {
    problems.push(
      `a caller-wrapped entry in an unwalked file must be rejected, got ${JSON.stringify(unwalkedWrap)}`,
    );
  }
  // The real allowlists must satisfy their own invariants.
  const live = [
    ...validateRecordingCallees(RECORDING_CALLEES, ENTRY_POINTS),
    ...validateCallerWrapped(CALLER_WRAPPED, ENTRY_POINTS),
  ];
  if (live.length !== 0) {
    problems.push(`the gate's own allowlists must validate, got ${JSON.stringify(live)}`);
  }

  for (const fixture of fixtures) {
    const source = [
      "    fn run_cycle(",
      "        &mut self,",
      "    ) -> CliResult<()> {",
      ...fixture.body,
      "        Ok(())",
      "    }",
    ].join("\n");
    const sites = analyse(source, "self-test", "    fn run_cycle(");
    const observed = (sites ?? []).map((site) =>
      site.reason === null ? null : site.reason.split(":")[0],
    );
    if (JSON.stringify(observed) !== JSON.stringify(fixture.expect)) {
      problems.push(
        `${fixture.name}: expected ${JSON.stringify(fixture.expect)}, got ${JSON.stringify(observed)}`,
      );
    }
  }
  if (problems.length > 0) {
    console.error("cycle-ending classification gate self-test FAILED:");
    for (const problem of problems) console.error(`- ${problem}`);
    process.exit(1);
  }
  console.log(
    `cycle-ending classification gate self-test: ${fixtures.length} statement fixtures, including two the analyser must REJECT (a bare \`?\` and a bare multi-line \`return Err\`) and a sibling-branch assignment it must not accept, plus 4 allowlist checks — an un-backed RECORDING_CALLEES entry and an unwalked CALLER_WRAPPED entry must both be rejected.`,
  );
  process.exit(0);
}

const failures = [
  ...validateRecordingCallees(RECORDING_CALLEES, ENTRY_POINTS),
  ...validateCallerWrapped(CALLER_WRAPPED, ENTRY_POINTS),
];
let walked = 0;
let bodies = 0;
for (const [provider, path, signature] of ENTRY_POINTS) {
  const source = readFileSync(resolve(root, path), "utf8");
  const sites = analyse(source, path, signature);
  if (sites === null) {
    failures.push(
      `${path}: cycle entry point \`${signature.trim()}\` not found — the gate must be updated with it, not silently skip it`,
    );
    continue;
  }
  bodies += 1;
  walked += sites.length;
  for (const site of sites) {
    if (site.reason === null) {
      failures.push(
        `${path}:${site.line} (${provider}) leaves the cycle without recording an ADR 0076 CycleEnding:\n    ${site.code}\n  statement: ${site.statement.replace(/\n/g, "\n    ")}`,
      );
    }
  }
}

if (failures.length > 0) {
  console.error("ADR 0076 cycle-ending classification gate failed:\n");
  for (const failure of failures) console.error(`- ${failure}\n`);
  console.error(
    "Every `Err` leaving a cycle body must record exactly one CycleEnding, or the shared loop\nfalls back to an untyped row and the closed table stops being closed.",
  );
  process.exit(1);
}

console.log(
  `ADR 0076 cycle-ending classification: ${bodies} cycle bodies walked, ${walked} \`?\`/return-Err sites, 0 unclassified.`,
);
