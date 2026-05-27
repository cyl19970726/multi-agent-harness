const state = { snapshot: null, liveTimer: null };

const byId = (id) => document.getElementById(id);
const text = (value) => (value == null || value === "" ? "-" : String(value));
const list = (value) => (Array.isArray(value) ? value : []);

const sample = {
  generated_at: "sample",
  teams: [],
  members: [],
  kanban: {
    planned: [],
    assigned: [],
    running: [],
    blocked: [],
    review: [],
    done: [],
    archived: [],
  },
  tasks: [],
  goals: [],
  goal_learning_status: [],
  messages: [],
  events: [],
  proposals: [],
  evidence: [],
  decisions: [],
  provider_sessions: [],
};

function escapeHtml(value) {
  return text(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function setSnapshot(snapshot) {
  state.snapshot = { ...sample, ...snapshot };
  render();
}

async function loadLiveSnapshot() {
  const baseUrl = byId("liveUrlInput").value.trim().replace(/\/$/, "");
  if (!baseUrl) return;
  const response = await fetch(`${baseUrl}/v1/snapshot`);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  setSnapshot(await response.json());
}

function startLive() {
  stopLive();
  loadLiveSnapshot().catch(showLoadError);
  state.liveTimer = window.setInterval(() => loadLiveSnapshot().catch(showLoadError), 5000);
}

function stopLive() {
  if (state.liveTimer) {
    window.clearInterval(state.liveTimer);
    state.liveTimer = null;
  }
}

function showLoadError(error) {
  byId("snapshotMeta").textContent = `Load failed: ${error instanceof Error ? error.message : String(error)}`;
}

function loadJson(raw) {
  try {
    setSnapshot(JSON.parse(raw));
  } catch (error) {
    alert(`Invalid JSON: ${error instanceof Error ? error.message : String(error)}`);
  }
}

function render() {
  const snapshot = state.snapshot ?? sample;
  const tasks = list(snapshot.tasks);
  const teams = list(snapshot.teams);
  const members = list(snapshot.members);
  const messages = list(snapshot.messages);
  const decisions = list(snapshot.decisions);
  const sessions = list(snapshot.provider_sessions);
  const goalLearning = list(snapshot.goal_learning_status);
  byId("snapshotMeta").textContent = `Generated: ${text(snapshot.generated_at)}`;
  byId("metricTasks").textContent = tasks.length;
  byId("metricTeams").textContent = teams.length;
  byId("metricMembers").textContent = members.length;
  byId("metricQueued").textContent = messages.filter((m) => m.delivery_status === "queued").length;
  byId("metricFailed").textContent =
    messages.filter((m) => m.delivery_status === "failed").length +
    sessions.filter((session) => session.status === "failed").length;
  byId("metricSessions").textContent = sessions.length;
  byId("metricDecisions").textContent = decisions.length;
  byId("metricGoalLearning").textContent = goalLearning.reduce(
    (sum, item) => sum + list(item.warnings).length,
    0,
  );
  renderKanban(snapshot);
  renderTeams(snapshot);
  renderMembers(snapshot);
  renderMessages(snapshot);
  renderSessions(snapshot);
  renderGoalLearning(snapshot);
  renderProposals(snapshot);
  renderEvents(snapshot);
  renderEvidence(snapshot);
}

function buildMemberLoad(snapshot) {
  const load = new Map();
  list(snapshot.tasks).forEach((task) => {
    const memberId = task.assignee_agent_id;
    if (!memberId) return;
    if (!load.has(memberId)) {
      load.set(memberId, { assigned: 0, running: 0, review: 0 });
    }
    if (task.status === "assigned" || task.status === "running" || task.status === "review") {
      load.get(memberId)[task.status] += 1;
    }
  });
  return load;
}

function memberTeamIds(member) {
  return new Set(list(member.team_ids));
}

function teamMembers(team, membersById, allMembers) {
  const explicitIds = list(team.member_ids);
  const members = explicitIds.map((id) => membersById.get(id)).filter(Boolean);
  const seen = new Set(members.map((member) => member.id));
  allMembers.forEach((member) => {
    if (!seen.has(member.id) && memberTeamIds(member).has(team.id)) {
      members.push(member);
      seen.add(member.id);
    }
  });
  return members;
}

function renderTeams(snapshot) {
  const members = list(snapshot.members);
  const membersById = new Map(members.map((member) => [member.id, member]));
  const loadByMember = buildMemberLoad(snapshot);
  const teams = list(snapshot.teams);

  byId("teamList").innerHTML =
    teams
      .map((team) => {
        const owner = membersById.get(team.owner_agent_id);
        const rows = teamMembers(team, membersById, members)
          .map((member) => {
            const queued = Number(member.queued_count || 0);
            const runtime = member.runtime_status || member.status || "offline";
            const alive = Boolean(member.runtime_alive);
            const load = loadByMember.get(member.id) || { assigned: 0, running: 0, review: 0 };
            return `<tr>
              <td>
                <strong>${escapeHtml(member.name || member.id)}</strong>
                <span>${escapeHtml(member.id)}</span>
              </td>
              <td>${escapeHtml(member.role)}</td>
              <td>
                <span class="pill ${runtime === "running" && alive ? "good" : "warn"}">${escapeHtml(runtime)}</span>
              </td>
              <td>${escapeHtml(queued)}</td>
              <td>${escapeHtml(load.assigned)}</td>
              <td>${escapeHtml(load.running)}</td>
              <td>${escapeHtml(load.review)}</td>
            </tr>`;
          })
          .join("");

        return `<article class="teamCard">
          <div class="teamHeader">
            <div>
              <h3>${escapeHtml(team.name || team.id)}</h3>
              <div class="meta">id=${escapeHtml(team.id)}</div>
            </div>
            <div class="teamOwner">
              <span>Owner</span>
              <strong>${escapeHtml(owner?.name || team.owner_agent_id)}</strong>
            </div>
          </div>
          <p>${escapeHtml(team.description)}</p>
          <div class="teamTableWrap">
            <table class="teamTable">
              <thead>
                <tr>
                  <th>Member</th>
                  <th>Role</th>
                  <th>Runtime</th>
                  <th>Queued</th>
                  <th>Assigned</th>
                  <th>Running</th>
                  <th>Review</th>
                </tr>
              </thead>
              <tbody>${rows || '<tr><td colspan="7" class="emptyCell">No members</td></tr>'}</tbody>
            </table>
          </div>
        </article>`;
      })
      .join("") || '<div class="empty">No teams</div>';
}

function renderKanban(snapshot) {
  const tasksById = new Map(list(snapshot.tasks).map((task) => [task.id, task]));
  const columns = ["planned", "assigned", "running", "blocked", "review", "done", "archived"];
  byId("kanbanBoard").innerHTML = columns
    .map((column) => {
      const ids = list(snapshot.kanban?.[column]);
      const cards = ids
        .map((id) => tasksById.get(id))
        .filter(Boolean)
        .map(taskCard)
        .join("");
      return `<section class="column">
        <div class="columnHeader"><strong>${escapeHtml(column)}</strong><span>${ids.length}</span></div>
        ${cards || '<div class="empty">No tasks</div>'}
      </section>`;
    })
    .join("");
}

function taskCard(task) {
  return `<article class="taskCard">
    <h3>${escapeHtml(task.title || task.id)}</h3>
    <div class="meta">id=${escapeHtml(task.id)}<br>owner=${escapeHtml(task.owner_agent_id)} assignee=${escapeHtml(task.assignee_agent_id)} reviewer=${escapeHtml(task.reviewer_agent_id)}</div>
    <div class="meta">workspace=${escapeHtml(task.workspace_ref)}<br>branch=${escapeHtml(task.branch_ref)} pr=${escapeHtml(task.pr_ref)}</div>
    <div class="pills">${list(task.owned_paths).map((path) => `<span class="pill">${escapeHtml(path)}</span>`).join("")}</div>
  </article>`;
}

function renderMembers(snapshot) {
  byId("memberGrid").innerHTML =
    list(snapshot.members)
      .map((member) => {
        const queued = Number(member.queued_count || 0);
        const runtime = member.runtime_status || "offline";
        const alive = Boolean(member.runtime_alive);
        return `<article class="memberCard">
          <h3>${escapeHtml(member.name || member.id)}</h3>
          <div class="meta">id=${escapeHtml(member.id)}<br>role=${escapeHtml(member.role)} provider=${escapeHtml(member.provider)}</div>
          <div class="pills">
            <span class="pill ${runtime === "running" && alive ? "good" : "warn"}">${escapeHtml(runtime)}</span>
            <span class="pill ${queued > 0 ? "warn" : "good"}">queued ${queued}</span>
            <span class="pill">${escapeHtml(member.status)}</span>
          </div>
          <div class="meta">runtime=${escapeHtml(member.runtime_id)} pid=${escapeHtml(member.runtime_pid)} alive=${escapeHtml(alive)}<br>thread=${escapeHtml(member.provider_thread_id)}<br>endpoint=${escapeHtml(member.control_endpoint)}</div>
          <div class="meta">task=${escapeHtml(member.current_task_id)}<br>proposal=${escapeHtml(member.current_proposal_id)}<br>prompt=${escapeHtml(member.prompt_ref)}</div>
        </article>`;
      })
      .join("") || '<div class="empty">No members</div>';
}

function renderMessages(snapshot) {
  byId("messageList").innerHTML =
    list(snapshot.messages)
      .slice()
      .reverse()
      .map((message) => item("message", message.id, [
        `kind=${text(message.kind)} status=${text(message.delivery_status)}`,
        `from=${text(message.from_agent_id)} to=${text(message.to_agent_id)} task=${text(message.task_id)}`,
        message.content,
      ]))
      .join("") || '<div class="empty">No messages</div>';
}

function renderSessions(snapshot) {
  byId("sessionList").innerHTML =
    list(snapshot.provider_sessions)
      .slice()
      .reverse()
      .map((session) => {
        const label = session.status === "succeeded" ? "session" : `session:${text(session.status)}`;
        return item(label, session.id, [
          `agent=${text(session.agent_member_id)} task=${text(session.task_id)} exit=${text(session.exit_code)}`,
          `stdout=${text(session.stdout_ref)}`,
          `transcript=${text(session.transcript_ref)}`,
          `evidence=${list(session.evidence_ids).join(", ")}`,
        ]);
      })
      .join("") || '<div class="empty">No provider sessions</div>';
}

function renderGoalLearning(snapshot) {
  byId("goalLearningList").innerHTML =
    list(snapshot.goal_learning_status)
      .map((status) => {
        const warnings = list(status.warnings);
        const ok = Boolean(status.ok);
        const designCount = list(status.goal_design).length;
        const evaluationCount = list(status.goal_evaluation).length;
        const goalCases = list(status.goal_cases);
        const followUps = list(status.follow_up_tasks);
        const reports = list(status.member_reports).length;
        const decisions = list(status.decisions).length;
        const order = status.event_order || {};
        return `<article class="item">
          <h3>${escapeHtml(status.goal_id)}</h3>
          <div class="pills">
            <span class="pill ${ok ? "good" : "warn"}">${ok ? "ok" : "needs review"}</span>
            <span class="pill ${designCount > 0 ? "good" : "bad"}">design ${designCount}</span>
            <span class="pill ${evaluationCount > 0 ? "good" : "warn"}">evaluation ${evaluationCount}</span>
            <span class="pill ${goalCases.length > 0 ? "good" : "warn"}">cases ${goalCases.length}</span>
            <span class="pill ${followUps.length > 0 ? "good" : "warn"}">follow-ups ${followUps.length}</span>
            <span class="pill">reports ${reports}</span>
            <span class="pill">decisions ${decisions}</span>
          </div>
          <div class="meta">tasks=${list(status.task_ids).join(", ")}</div>
          <div class="meta">goal cases=${goalCases.map((item) => escapeHtml(item.source_ref || item.id)).join(", ") || "none"}</div>
          <div class="meta">follow-ups=${followUps.map((task) => escapeHtml(task.id)).join(", ") || "none"}</div>
          <div class="meta">event order: design-before-assignment=${text(order.design_before_assignment)} assignment-before-report=${text(order.assignment_before_report)} report-before-decision=${text(order.report_before_decision)} decision-before-evaluation=${text(order.decision_before_evaluation)}</div>
          <div class="meta">warnings=${warnings.length ? warnings.map(escapeHtml).join("<br>") : "none"}</div>
        </article>`;
      })
      .join("") || '<div class="empty">No goal learning status</div>';
}

function renderProposals(snapshot) {
  byId("proposalList").innerHTML =
    list(snapshot.proposals)
      .slice()
      .reverse()
      .map((proposal) => item("proposal", proposal.title || proposal.id, [
        `id=${text(proposal.id)} status=${text(proposal.status)} task=${text(proposal.task_id)} agent=${text(proposal.agent_member_id)}`,
        proposal.summary,
        `paths=${list(proposal.changed_paths).join(", ")}`,
      ]))
      .join("") || '<div class="empty">No proposals</div>';
}

function renderEvents(snapshot) {
  byId("eventList").innerHTML =
    list(snapshot.events)
      .slice()
      .reverse()
      .map((event) => item(event.event_type, event.summary || event.id, [
        `agent=${text(event.agent_member_id)} runtime=${text(event.provider_runtime_id)} task=${text(event.task_id)}`,
        `payload=${text(event.payload_ref)}`,
        text(event.created_at),
      ]))
      .join("") || '<div class="empty">No events</div>';
}

function renderEvidence(snapshot) {
  byId("evidenceList").innerHTML =
    list(snapshot.evidence)
      .slice()
      .reverse()
      .map((evidence) => item(evidence.source_type, evidence.summary || evidence.id, [
        `id=${text(evidence.id)} task=${text(evidence.task_id)}`,
        evidence.source_ref,
      ]))
      .join("") || '<div class="empty">No evidence</div>';
  byId("decisionList").innerHTML =
    list(snapshot.decisions)
      .slice()
      .reverse()
      .map((decision) => item("decision", decision.decision || decision.id, [
        `id=${text(decision.id)} task=${text(decision.task_id)}`,
        decision.rationale,
        `evidence=${list(decision.evidence_ids).join(", ")}`,
      ]))
      .join("") || '<div class="empty">No decisions</div>';
}

function item(label, title, lines) {
  return `<article class="item">
    <h3>${escapeHtml(title)}</h3>
    <div class="pills"><span class="pill">${escapeHtml(label)}</span></div>
    <div class="meta">${lines.map(escapeHtml).join("<br>")}</div>
  </article>`;
}

byId("fileInput").addEventListener("change", async (event) => {
  const file = event.target.files?.[0];
  if (!file) return;
  loadJson(await file.text());
});

byId("loadPasteButton").addEventListener("click", () => loadJson(byId("jsonInput").value));
byId("loadLiveButton").addEventListener("click", startLive);
byId("stopLiveButton").addEventListener("click", stopLive);

document.querySelectorAll(".tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((item) => item.classList.remove("active"));
    document.querySelectorAll(".view").forEach((item) => item.classList.remove("active"));
    tab.classList.add("active");
    byId(tab.dataset.tab).classList.add("active");
  });
});

render();
