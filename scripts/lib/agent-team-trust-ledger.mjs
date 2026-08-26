function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

const TRANSCRIPT_MIRROR_FIELDS = new Set([
  "command_history",
  "command_output",
  "events",
  "file_events",
  "messages",
  "reasoning",
  "stderr",
  "stdout",
  "thinking",
  "tool_calls",
  "tool_results",
  "transcript",
  "transcript_items",
  "turns",
]);

const TRANSCRIPT_MIRROR_AGGREGATES = new Set([
  "command_output",
  "file_event",
  "provider_event",
  "provider_message",
  "provider_transcript",
  "provider_turn",
  "tool_call",
  "tool_result",
  "transcript",
]);

function addFailure(failures, message) {
  failures.push(`trust ledger: ${message}`);
}

function exactlyOne(records, description, failures) {
  if (records.length !== 1) {
    addFailure(failures, `expected exactly one ${description}, found ${records.length}`);
    return null;
  }
  return records[0];
}

function eventOf(record) {
  return record?.operation?.event;
}

function projectionOf(record) {
  return record?.operation?.resulting_projection;
}

function hasEvent(record, aggregateKind, transition) {
  const event = eventOf(record);
  return event?.aggregate_kind === aggregateKind && event?.transition === transition;
}

function checkEqual(failures, actual, expected, description) {
  if (actual !== expected) {
    addFailure(
      failures,
      `${description} mismatch: expected ${JSON.stringify(expected)}, found ${JSON.stringify(actual)}`,
    );
  }
}

function normalizedFieldName(value) {
  return value.toLowerCase().replaceAll("-", "_");
}

function transcriptMirrorPath(value, path = [], seen = new WeakSet()) {
  if (!isObject(value) && !Array.isArray(value)) return null;
  if (seen.has(value)) return null;
  seen.add(value);

  for (const [key, child] of Object.entries(value)) {
    const childPath = [...path, key];
    if (TRANSCRIPT_MIRROR_FIELDS.has(normalizedFieldName(key))) return childPath.join(".");
    const nested = transcriptMirrorPath(child, childPath, seen);
    if (nested) return nested;
  }
  return null;
}

function isCanonicalPassReview(body) {
  if (typeof body !== "string") return false;
  const lines = body.split(/\r?\n/u).map((line) => line.trim()).filter(Boolean);
  if (lines[0] !== "REVIEW_RESULT") return false;

  const verdicts = lines.filter((line) => /^Verdict\s*:/u.test(line));
  if (verdicts.length !== 1 || verdicts[0] !== "Verdict: Pass") return false;

  const remainder = lines.filter((line) => line !== "REVIEW_RESULT" && line !== "Verdict: Pass");
  return !remainder.some((line) =>
    /\b(?:not\s+pass|changes?\s+required|fail(?:ed|ure)?|reject(?:ed|ion)?)\b/iu.test(line));
}

function checkExecutionSpaces(failures, matchedRecords) {
  const spaces = new Set();
  for (const [description, record] of matchedRecords) {
    const space = record?.execution_space_id;
    if (typeof space !== "string" || !space.trim()) {
      addFailure(failures, `${description} has no non-empty execution_space_id`);
    } else {
      spaces.add(space);
    }
  }
  if (spaces.size > 1) {
    addFailure(failures, `matched records span multiple execution spaces: ${JSON.stringify([...spaces].sort())}`);
  }
}

export function parseTrustOperationJsonl(jsonl) {
  if (typeof jsonl !== "string") {
    throw new TypeError("trust ledger JSONL must be a string");
  }

  const records = [];
  for (const [index, line] of jsonl.split(/\r?\n/u).entries()) {
    if (!line.trim()) continue;
    let record;
    try {
      record = JSON.parse(line);
    } catch (error) {
      throw new Error(`trust ledger line ${index + 1} is malformed JSON: ${error.message}`);
    }
    if (!isObject(record)) {
      throw new Error(`trust ledger line ${index + 1} must contain a JSON object`);
    }
    if (!isObject(record.operation) || !isObject(record.operation.event)) {
      throw new Error(`trust ledger line ${index + 1} has no canonical operation.event envelope`);
    }
    records.push(record);
  }
  return records;
}

export function verifyCanonicalTrustLedger(evidence, records) {
  const failures = [];
  if (!isObject(evidence) || !isObject(evidence.team) || !isObject(evidence.revision)
      || !isObject(evidence.work) || !Array.isArray(evidence.sessions)) {
    return ["trust ledger: coding dogfood evidence is missing team, revision, work, or sessions"];
  }
  if (!Array.isArray(records)) {
    return ["trust ledger: parsed records must be an array"];
  }

  for (const [index, record] of records.entries()) {
    const aggregateKind = eventOf(record)?.aggregate_kind;
    if (typeof aggregateKind === "string"
        && TRANSCRIPT_MIRROR_AGGREGATES.has(normalizedFieldName(aggregateKind))) {
      addFailure(failures, `record ${index + 1} uses forbidden transcript-mirror aggregate ${JSON.stringify(aggregateKind)}`);
    }
    const mirrorPath = transcriptMirrorPath(record);
    if (mirrorPath) {
      addFailure(failures, `record ${index + 1} contains forbidden transcript-mirror field ${mirrorPath}`);
    }
  }

  const { team, revision, work, sessions } = evidence;
  const matchedRecords = [];
  const reportRecord = exactlyOne(
    records.filter((record) => hasEvent(record, "work_report", "created")
      && eventOf(record).aggregate_id === work.work_report_id),
    `WorkReport ${JSON.stringify(work.work_report_id)}`,
    failures,
  );
  if (reportRecord) matchedRecords.push(["WorkReport", reportRecord]);
  const report = reportRecord && projectionOf(reportRecord);
  if (reportRecord && !isObject(report)) {
    addFailure(failures, `WorkReport ${JSON.stringify(work.work_report_id)} has no resulting projection`);
  } else if (report) {
    checkEqual(failures, report.id, work.work_report_id, "WorkReport id");
    checkEqual(failures, report.work_id, work.work_id, "WorkReport work id");
    checkEqual(failures, report.work_revision, work.accepted_version - 1, "WorkReport work revision");
    checkEqual(failures, report.authored_by?.kind, "agent_member", "WorkReport author kind");
    checkEqual(
      failures,
      report.authored_by?.id,
      team.implementer_agent_member_id,
      "WorkReport author",
    );
    checkEqual(failures, report.candidate?.kind, "git_commit", "WorkReport candidate kind");
    checkEqual(failures, report.candidate?.value, revision.candidate, "WorkReport candidate revision");
  }

  const acceptanceRecord = exactlyOne(
    records.filter((record) => hasEvent(record, "work", "accepted")
      && eventOf(record).id === work.acceptance_event_id),
    `acceptance event ${JSON.stringify(work.acceptance_event_id)}`,
    failures,
  );
  if (acceptanceRecord) matchedRecords.push(["acceptance event", acceptanceRecord]);
  if (acceptanceRecord) {
    const event = eventOf(acceptanceRecord);
    const acceptedWork = projectionOf(acceptanceRecord);
    checkEqual(failures, event.aggregate_id, work.work_id, "acceptance Work id");
    checkEqual(failures, event.resulting_version, work.accepted_version, "accepted Work version");
    checkEqual(failures, event.performed_by_actor?.kind, "agent_member", "acceptance actor kind");
    checkEqual(
      failures,
      event.performed_by_actor?.id,
      work.acceptance_actor_agent_member_id,
      "acceptance actor",
    );
    checkEqual(
      failures,
      work.acceptance_actor_agent_member_id,
      team.host_agent_member_id,
      "acceptance evidence Host",
    );
    checkEqual(failures, event.payload?.work_report_id, work.work_report_id, "accepted WorkReport");
    if (report) {
      checkEqual(
        failures,
        event.payload?.candidate_fingerprint,
        report.candidate_fingerprint,
        "accepted candidate fingerprint",
      );
    }
    if (!isObject(acceptedWork)) {
      addFailure(failures, `acceptance event ${JSON.stringify(work.acceptance_event_id)} has no resulting projection`);
    } else {
      checkEqual(failures, acceptedWork.id, work.work_id, "accepted projection Work id");
      checkEqual(failures, acceptedWork.version, work.accepted_version, "accepted projection version");
      checkEqual(failures, acceptedWork.phase, "closed", "accepted projection phase");
      checkEqual(failures, acceptedWork.resolution, "accepted", "accepted projection resolution");
    }
  }

  const reviewRecord = exactlyOne(
    records.filter((record) => hasEvent(record, "message", "authored")
      && eventOf(record).aggregate_id === work.review_message_id),
    `review Message ${JSON.stringify(work.review_message_id)}`,
    failures,
  );
  if (reviewRecord) matchedRecords.push(["review Message", reviewRecord]);
  if (reviewRecord) {
    const review = projectionOf(reviewRecord);
    if (!isObject(review)) {
      addFailure(failures, `review Message ${JSON.stringify(work.review_message_id)} has no resulting projection`);
    } else {
      checkEqual(failures, review.id, work.review_message_id, "review Message id");
      checkEqual(failures, review.work_id, work.work_id, "review Message Work id");
      checkEqual(failures, review.team_run_id, team.team_run_id, "review Message TeamRun");
      checkEqual(
        failures,
        review.sender_agent_member_id,
        work.reviewer_agent_member_id,
        "review Message author",
      );
      if (work.reviewer_agent_member_id === team.implementer_agent_member_id) {
        addFailure(failures, "review Message author must be independent from the implementer");
      }
      if (!isCanonicalPassReview(review.body)) {
        addFailure(failures, "review Message must be canonical REVIEW_RESULT with exactly one unambiguous Verdict: Pass");
      }
    }
  }

  const seenMemberIds = new Set();
  const seenSessionIds = new Set();
  for (const session of sessions) {
    if (!isObject(session)) {
      addFailure(failures, "Session evidence rows must be objects");
      continue;
    }
    if (seenMemberIds.has(session.agent_member_id)) {
      addFailure(failures, `duplicate Session evidence for AgentMember ${JSON.stringify(session.agent_member_id)}`);
    }
    if (seenSessionIds.has(session.agent_session_id)) {
      addFailure(failures, `duplicate AgentSession evidence id ${JSON.stringify(session.agent_session_id)}`);
    }
    seenMemberIds.add(session.agent_member_id);
    seenSessionIds.add(session.agent_session_id);

    const bindingRecord = exactlyOne(
      records.filter((record) => {
        if (!hasEvent(record, "agent_session", "native_session_bound")) return false;
        const event = eventOf(record);
        const native = event.payload?.native_session_ref;
        return event.aggregate_id === session.agent_session_id
          && event.payload?.session_id === session.agent_session_id
          && native?.native_session_id === session.native_session_id;
      }),
      `native-session binding for AgentSession ${JSON.stringify(session.agent_session_id)}`,
      failures,
    );
    if (!bindingRecord) continue;
    matchedRecords.push([`AgentSession ${JSON.stringify(session.agent_session_id)} binding`, bindingRecord]);

    const event = eventOf(bindingRecord);
    const boundSession = projectionOf(bindingRecord);
    const eventNative = event.payload?.native_session_ref;
    const projectedNative = boundSession?.native_session_ref;
    if (!isObject(boundSession) || !isObject(eventNative) || !isObject(projectedNative)) {
      addFailure(failures, `AgentSession ${JSON.stringify(session.agent_session_id)} has an incomplete native-session binding`);
      continue;
    }
    checkEqual(failures, boundSession.id, session.agent_session_id, "AgentSession id");
    checkEqual(failures, boundSession.agent_member_id, session.agent_member_id, "AgentSession member");
    checkEqual(failures, boundSession.provider_kind, session.provider, "AgentSession provider");
    checkEqual(failures, eventNative.provider, session.provider, "native-session event provider");
    checkEqual(failures, projectedNative.provider, session.provider, "native-session projection provider");
    checkEqual(
      failures,
      projectedNative.native_session_id,
      session.native_session_id,
      "native-session projection id",
    );
    checkEqual(
      failures,
      projectedNative.native_session_id,
      eventNative.native_session_id,
      "native-session event/projection binding",
    );
  }

  checkExecutionSpaces(failures, matchedRecords);

  return failures;
}

export function verifyCanonicalTrustLedgerJsonl(evidence, jsonl) {
  try {
    return verifyCanonicalTrustLedger(evidence, parseTrustOperationJsonl(jsonl));
  } catch (error) {
    return [
      error.message.startsWith("trust ledger") ? error.message : `trust ledger: ${error.message}`,
    ];
  }
}
