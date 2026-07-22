(() => {
  "use strict";

  const validViews = new Set(["overview", "journey", "architecture"]);
  const asset = (path) => `../design/${path}`;

  const images = {
    home: asset("company-os-v2/store-live-actual/home--morning-operating-review--desktop.png"),
    docs: asset("company-os-v2/store-live-actual/docs--company-knowledge-workspace--desktop.png"),
    organization: asset("company-os-v2/company-os-four-systems-v1/expected/organization-overview-ui--1536x1024.png"),
    work: asset("company-os-v2/work-operating-system-v1/actual/work-board--desktop-1536x1024.png"),
    approval: asset("company-os-v1/actual/approval-focus--trademark-filing-fee--desktop-1440x1000.png"),
    finance: asset("company-os-v1/actual/finance--july-operating-view--desktop-1440x1000.png"),
    module: asset("company-os-v3/trademark-native-closure-v1/comparisons/business-module--native-trademark--desktop-1536x1024.png"),
    docsResult: asset("company-os-v3/trademark-native-closure-v1/comparisons/docs-workspace--native-trademark--desktop-1536x1024.png"),
    workComparison: asset("company-os-v3/trademark-native-closure-v1/comparisons/work-board--native-trademark--desktop-1536x1024.png"),
  };

  const commonAuthority = ["Accountable Human", "Assigned Standing Agent", "External collaborator", "Named Human approver"];

  const businessLines = [
    {
      id: "company-governance",
      name: "Company governance",
      short: "Governance",
      journey: "Policy & organization change",
      status: "Expected · substrate partial",
      summary: "A company policy becomes governed work, receives Human authority, and returns as a durable decision.",
      steps: [
        step("Home", "Needs company attention", images.home, "Actual page", "/?surface=home", "Attention projection", ["approval_ref", "document_ref"], "Human Owner", "Open the source policy", "Urgent policy pressure stays linked to its source."),
        step("Docs", "Policy source", images.docs, "Actual page", "/?surface=docs&documentId=policy", "Document", ["document_ref", "module_ref"], "Docs Governance Agent", "Create governed WorkItem", "Docs owns policy context; Work will own execution lifecycle."),
        step("Organization", "Authority & impact", images.organization, "Expected page", "/?surface=organization&proposalId=…", "OrgChangeProposal", ["actor_refs", "org_unit_refs"], "Org / HR Governance Agent", "Confirm roles and approver", "Organization owns membership, permission, and authority."),
        step("Work", "Commitment & owner", images.work, "Actual page", "/?surface=work&workItemId=…", "WorkItem", ["source_document_ref", "accountable_ref"], "Lead Agent", "Submit for approval", "Work owns lifecycle and explicit responsibility."),
        step("Approval", "Human gate", images.approval, "Actual page", "/?surface=approvals&approvalId=…", "Approval", ["work_item_ref", "required_approver_refs"], "Human Owner", "Approve or reject change", "Approval is a governed business bridge, never a comment."),
        step("Docs result", "Decision returned", images.docsResult, "Actual comparison", "/?surface=docs&documentId=policy", "Decision Block", ["result_document_ref", "evidence_refs"], "Docs Governance Agent", "Publish accepted policy", "The originating document receives the durable decision."),
      ],
      authorities: commonAuthority,
    },
    {
      id: "brand-ip",
      name: "Brand & IP",
      short: "Brand & IP",
      journey: "CN trademark filing",
      status: "Verified native slice",
      summary: "One operation, one linked truth across product pages.",
      steps: [
        step("Home", "Needs my attention", images.home, "Actual page", "/?surface=home", "Approval pressure", ["approval_ref", "application_ref"], "Brand Owner Human", "Open trademark context", "The attention card links to the real application and decision."),
        step("Docs", "Source & module", images.docs, "Actual page", "/?surface=docs&module=module-trademark-management", "Document + BusinessModule", ["source_document_ref", "typed_record_ref", "module_ref"], "Docs Governance Agent", "Open filing WorkItem", "Docs owns the strategy, application record, and returned memory."),
        step("Work", "Commitment & owner", images.work, "Actual page", "/?surface=work&workItemId=workitem-trademark-filing-brand-a", "WorkItem", ["source_document_ref", "assignment_ref", "milestone_ref"], "Brand Owner + Trademark Agent", "Request Human approval", "Work owns commitment, assignment, lifecycle, evidence, and result routing."),
        step("Approval", "Human authority", images.approval, "Actual page", "/?surface=approvals&approvalId=approval-trademark-filing-fee-cn-2026-018", "Approval", ["work_item_ref", "financial_record_ref", "required_approver_refs"], "Brand Owner Human", "Review ¥3,000 commitment", "Human authority is required before the sensitive financial effect proceeds."),
        step("Finance", "¥3,000 Commitment", images.finance, "Actual page", "/?surface=finance&recordId=financial-commitment-trademark-filing-fee-cn-2026-018", "FinancialRecord(commitment)", ["source_document_ref", "work_item_ref", "approval_refs"], "Finance Governance Agent", "Record authorized commitment", "Commitment exists; Payment does not exist without settlement evidence."),
        step("Docs result", "Evidence returned", images.docsResult, "Actual comparison", "/?surface=docs&documentId=document-trademark-application-cn-2026-018", "Result Block + TypedRecord", ["result_document_ref", "evidence_refs", "work_item_ref"], "Trademark Agent + accountable Human", "Review durable filing result", "Evidence and accepted outcome update the originating company memory."),
      ],
      authorities: ["Brand Owner Human", "Trademark Agent", "External counsel", "Human approver"],
    },
    {
      id: "content-media",
      name: "Content & media",
      short: "Content",
      journey: "Campaign planning & learning",
      status: "Expected journey",
      summary: "A content plan becomes assigned work; published evidence and metrics return to the plan.",
      steps: [
        step("Home", "Campaign pressure", images.home, "Actual page type", "/?surface=home", "Attention projection", ["document_ref", "work_item_refs"], "Content Lead Human", "Open campaign plan", "Home composes source-linked attention; it owns no second task list."),
        step("Docs", "Plan & brief", images.docs, "Actual page type", "/?surface=docs&documentId=content-plan", "Document", ["document_ref", "module_ref", "metric_refs"], "Content Agent", "Create delivery WorkItems", "The brief, channel rules, and learning history live in Docs."),
        step("Organization", "Roles & capacity", images.organization, "Expected page", "/?surface=organization&agent=content-agent", "ActorRef", ["accountable_ref", "assignee_refs"], "Org / HR Governance Agent", "Confirm available owners", "Availability and capacity must be explicit, never inferred from runtime."),
        step("Work", "Production board", images.work, "Actual page type", "/?surface=work&businessLine=content", "WorkItem set", ["source_document_ref", "milestone_ref", "assignment_refs"], "Content Lead + Agents", "Review deliverables", "Each deliverable keeps requester, submitter, owner, and evidence."),
        step("Approval", "Brand / legal gate", images.approval, "Actual page type", "/?surface=approvals&businessLine=content", "Approval", ["work_item_ref", "evidence_refs"], "Named Human reviewer", "Approve publication", "Only risky publications require this bridge; it is not universal."),
        step("Docs result", "Metrics & learning", images.docsResult, "Actual page type", "/?surface=docs&documentId=content-plan", "MetricObservation + Result Block", ["result_document_ref", "metric_refs"], "Content Agent", "Update next-stage plan", "Durable outcomes and meaningful metrics improve the source plan."),
      ],
      authorities: ["Content Lead Human", "Content Agent", "Brand reviewer", "External platform"],
    },
    {
      id: "product-development",
      name: "Product & development",
      short: "Product",
      journey: "Feature delivery",
      status: "Expected journey",
      summary: "A product requirement becomes owned work; review evidence returns to the specification and milestone.",
      steps: [
        step("Docs", "Requirement source", images.docs, "Actual page type", "/?surface=docs&documentId=prd", "Document", ["document_ref", "module_ref"], "Product Lead", "Create scoped WorkItem", "Docs owns the requirement, rationale, and acceptance narrative."),
        step("Organization", "Capability & ownership", images.organization, "Expected page", "/?surface=organization&agent=development-agent", "ActorRef", ["accountable_ref", "assignee_refs"], "Org / HR Governance Agent", "Confirm owner and permissions", "Organization owns identity, capability, and authority."),
        step("Work", "Milestone & delivery", images.work, "Actual page type", "/?surface=work&businessLine=product", "WorkItem + Milestone", ["source_document_ref", "milestone_ref", "assignment_refs"], "Product Lead + Development Agent", "Submit implementation evidence", "Work owns lifecycle; repository links are evidence, not a second task model."),
        step("Approval", "Release / security gate", images.approval, "Actual page type", "/?surface=approvals&businessLine=product", "Approval", ["work_item_ref", "evidence_refs"], "Named Human authority", "Authorize sensitive release", "Human gate appears only when policy requires it."),
        step("Finance", "Cost effect when present", images.finance, "Actual page type", "/?surface=finance&businessLine=product", "Commitment", ["work_item_ref", "approval_refs"], "Finance Governance Agent", "Record actual monetary effect", "A WorkItem may have no financial effect; this step is conditional."),
        step("Docs result", "Accepted result", images.docsResult, "Actual page type", "/?surface=docs&documentId=prd", "Result Block", ["result_document_ref", "evidence_refs"], "Product Lead", "Update specification and decision", "The PRD records the accepted outcome, not raw execution transcript."),
      ],
      authorities: ["Product Lead Human", "Development Agent", "Security reviewer", "Release approver"],
    },
    {
      id: "finance-admin",
      name: "Finance & admin",
      short: "Finance",
      journey: "Purchase & settlement",
      status: "Expected journey",
      summary: "A documented purchase request becomes governed work, Human approval, and auditable monetary records.",
      steps: [
        step("Docs", "Request & policy", images.docs, "Actual page type", "/?surface=docs&documentId=purchase-request", "Document", ["document_ref", "policy_ref"], "Requesting Human", "Create procurement WorkItem", "Docs owns need, context, and policy references."),
        step("Work", "Procurement commitment", images.work, "Actual page type", "/?surface=work&type=purchase", "WorkItem", ["source_document_ref", "accountable_ref"], "Operations owner", "Request budget approval", "Work owns the procurement lifecycle and evidence routing."),
        step("Organization", "Authority & separation", images.organization, "Expected page", "/?surface=organization&role=finance-approver", "Role + Authority", ["requester_ref", "approver_ref"], "Org / HR Governance Agent", "Resolve authorized Human", "Requester and approver remain distinct when policy requires it."),
        step("Approval", "Human spend gate", images.approval, "Actual page type", "/?surface=approvals&type=financial", "Approval", ["work_item_ref", "financial_record_ref"], "Authorized Human", "Approve exact amount and scope", "Approval authorizes a bounded effect; it does not prove settlement."),
        step("Finance", "Commitment → payment", images.finance, "Actual page type", "/?surface=finance", "Commitment / Invoice / Payment", ["commitment_ref", "invoice_ref", "payment_ref"], "Finance Governance Agent", "Record evidence-backed transition", "Each financial state remains a distinct immutable record."),
        step("Docs result", "Receipt & decision", images.docsResult, "Actual page type", "/?surface=docs&documentId=purchase-request", "Evidence + Result Block", ["result_document_ref", "financial_record_refs"], "Operations owner", "Close request with evidence", "Company memory reflects what actually happened."),
      ],
      authorities: ["Requesting Human", "Operations owner", "Finance reviewer", "Authorized payer"],
    },
  ];

  function step(page, question, image, truth, route, object, refs, authority, action, summary) {
    return { page, question, image, truth, route, object, refs, authority, action, summary };
  }

  const state = {
    view: "overview",
    lineId: "brand-ip",
    stepIndex: 3,
    inspectorTab: "jump",
    objectView: false,
  };

  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => Array.from(root.querySelectorAll(selector));
  const escapeHtml = (value) => String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");

  function currentLine() {
    return businessLines.find((line) => line.id === state.lineId) || businessLines[1];
  }

  function readUrl() {
    const params = new URLSearchParams(location.search);
    state.view = validViews.has(params.get("view")) ? params.get("view") : "overview";
    if (businessLines.some((line) => line.id === params.get("line"))) state.lineId = params.get("line");
    const parsedStep = Number(params.get("step"));
    if (Number.isInteger(parsedStep) && parsedStep >= 0 && parsedStep < currentLine().steps.length) state.stepIndex = parsedStep;
  }

  function writeUrl({ replace = false } = {}) {
    const url = new URL(location.href);
    url.searchParams.set("view", state.view);
    if (state.view === "journey") {
      url.searchParams.set("line", state.lineId);
      url.searchParams.set("step", String(state.stepIndex));
    } else {
      url.searchParams.delete("line");
      url.searchParams.delete("step");
    }
    history[replace ? "replaceState" : "pushState"]({}, "", url);
  }

  function applyView({ focus = false } = {}) {
    document.body.dataset.view = state.view;
    $$('[data-view-panel]').forEach((panel) => { panel.hidden = panel.dataset.viewPanel !== state.view; });
    $$('[data-view-link]').forEach((link) => {
      const active = link.dataset.viewLink === state.view;
      if (active) link.setAttribute("aria-current", "page");
      else link.removeAttribute("aria-current");
    });
    document.title = `${state.view === "overview" ? "Company OS" : state.view === "journey" ? "Product Journeys" : "Product Architecture"} · Live PRD`;
    if (state.view === "journey") renderJourney();
    if (focus) {
      const heading = $(`[data-view-panel="${state.view}"] h1`);
      heading?.setAttribute("tabindex", "-1");
      heading?.focus({ preventScroll: true });
    }
    window.scrollTo({ top: 0, behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  }

  function activateView(view, { push = true, focus = true } = {}) {
    if (!validViews.has(view)) return;
    state.view = view;
    if (push) writeUrl();
    applyView({ focus });
  }

  function renderBusinessLines() {
    const markup = businessLines.map((line) => `
      <button type="button" data-line-id="${line.id}" aria-pressed="${line.id === state.lineId}">
        <strong>${escapeHtml(line.name)}</strong><small>${escapeHtml(line.journey)}</small>
      </button>`).join("");
    $("#businessLineNav").innerHTML = markup;
    $("#mobileBusinessLines").innerHTML = businessLines.map((line) => `
      <button type="button" data-line-id="${line.id}" aria-pressed="${line.id === state.lineId}">${escapeHtml(line.short)}</button>`).join("");
    $$('[data-line-id]').forEach((button) => button.addEventListener("click", () => selectBusinessLine(button.dataset.lineId)));
  }

  function selectBusinessLine(lineId) {
    if (!businessLines.some((line) => line.id === lineId)) return;
    state.lineId = lineId;
    state.stepIndex = Math.min(3, currentLine().steps.length - 1);
    state.inspectorTab = "jump";
    writeUrl();
    renderJourney();
    $("#journeyTitle")?.focus({ preventScroll: true });
  }

  function renderJourney() {
    renderBusinessLines();
    const line = currentLine();
    $("#journeyTitle").textContent = `${line.name} · ${line.journey}`;
    $("#journeySummary").textContent = line.summary;
    $("#journeyStatus").textContent = line.status;

    $("#journeySteps").innerHTML = line.steps.map((item, index) => `
      <button class="journey-step" type="button" aria-pressed="${index === state.stepIndex}" data-step-index="${index}" aria-label="步骤 ${index + 1}: ${escapeHtml(item.page)}, ${escapeHtml(item.question)}">
        <span class="journey-step-head"><span class="journey-step-num">${index + 1}</span><span><h2>${escapeHtml(item.page)}</h2><p>${escapeHtml(item.question)}</p></span></span>
        <span class="journey-preview"><img src="${escapeHtml(item.image)}" alt="${escapeHtml(item.page)} 产品页面参考" /><span class="preview-truth">${escapeHtml(item.truth)}</span></span>
        <span class="journey-step-footer">${escapeHtml(item.summary)}</span>
      </button>`).join("");

    $$('[data-step-index]').forEach((button) => button.addEventListener("click", () => {
      state.stepIndex = Number(button.dataset.stepIndex);
      state.inspectorTab = "jump";
      writeUrl();
      renderJourney();
      $("#inspectorContent")?.focus({ preventScroll: true });
    }));

    renderGraph(line);
    renderInspector(line);
    installImageFallbacks();
  }

  function renderGraph(line) {
    const nav = line.steps.map((item) => item.page);
    const objects = line.steps.map((item) => item.object);
    const authorities = line.steps.map((_, index) => line.authorities[Math.min(index, line.authorities.length - 1)] || "Explicit ActorRef");
    $(".handoff-graph").classList.toggle("object-mode", state.objectView);
    $$('.view-toggle button').forEach((button, index) => {
      const active = state.objectView === (index === 1);
      button.classList.toggle("is-active", active);
      button.setAttribute("aria-pressed", String(active));
    });
    $("#handoffGraphContent").innerHTML = `
      ${graphLane("Navigation", nav, "navigation")}
      ${graphLane("Canonical objects", objects, "objects")}
      ${graphLane("Organization authority", authorities, "authority")}
      <p class="graph-note">Solid line = navigable page transition · dotted blue = canonical relation · dotted sage = authority relationship. This is not a Task Graph.</p>`;
  }

  function graphLane(label, nodes, className) {
    return `<div class="graph-lane ${className}"><div class="lane-label">${escapeHtml(label)}</div>${nodes.map((node) => `<div class="graph-node">${escapeHtml(node)}</div>`).join("")}</div>`;
  }

  function renderInspector(line) {
    const item = line.steps[state.stepIndex];
    $$('.inspector-tabs button').forEach((button) => button.setAttribute("aria-selected", String(button.dataset.inspectorTab === state.inspectorTab)));
    const title = state.stepIndex === 0 ? `Open ${item.page}` : `${line.steps[state.stepIndex - 1].page} → ${item.page}`;
    let body = "";
    if (state.inspectorTab === "jump") {
      body = `<h2 id="inspectorTitle">${escapeHtml(title)}</h2><p>${escapeHtml(item.action)}</p>
        <ul class="contract-list">
          <li><small>Target route</small><code>${escapeHtml(item.route)}</code></li>
          <li><small>Destination object</small><code>${escapeHtml(item.object)}</code></li>
          <li><small>Preserved refs</small><span class="ref-list">${item.refs.map((ref) => `<code>${escapeHtml(ref)}</code>`).join("")}</span></li>
          <li><small>Authority</small><code>${escapeHtml(item.authority)}</code></li>
          <li><small>Return</small><code>Browser Back + originating Document / WorkItem link</code></li>
        </ul><div class="truth-summary"><strong>Truth boundary</strong><span>${escapeHtml(item.summary)}</span></div>`;
    } else if (state.inspectorTab === "objects") {
      body = `<h2 id="inspectorTitle">Canonical objects</h2><p>页面只投影这些记录，不创建副本。</p><ul class="contract-list"><li><small>Primary</small><code>${escapeHtml(item.object)}</code></li><li><small>Stable references</small><span class="ref-list">${item.refs.map((ref) => `<code>${escapeHtml(ref)}</code>`).join("")}</span></li><li><small>Owning rule</small><code>${escapeHtml(ownerFor(item.page))}</code></li></ul>`;
    } else if (state.inspectorTab === "authority") {
      body = `<h2 id="inspectorTitle">Organization authority</h2><p>Actor identity and permission come from Organization.</p><ul class="contract-list"><li><small>Current authority</small><code>${escapeHtml(item.authority)}</code></li><li><small>Business-line actors</small><span class="ref-list">${line.authorities.map((actor) => `<code>${escapeHtml(actor)}</code>`).join("")}</span></li><li><small>Invariant</small><code>Runtime health never implies availability, authority, or Human consent.</code></li></ul>`;
    } else {
      body = `<h2 id="inspectorTitle">Visual evidence</h2><p>${escapeHtml(item.truth)} · source-labelled product page.</p><button class="inspector-evidence" type="button" data-image-src="${escapeHtml(item.image)}" data-image-title="${escapeHtml(item.page)} · ${escapeHtml(line.journey)}" data-image-truth="${escapeHtml(item.truth)}"><img src="${escapeHtml(item.image)}" alt="${escapeHtml(item.page)} evidence" /></button><div class="truth-summary"><strong>Review rule</strong><span>Expected defines intent. Only authority-labelled Store-live browser evidence can claim Actual.</span></div>`;
    }
    $("#inspectorContent").innerHTML = body;
    installDialogTriggers($("#inspectorContent"));
  }

  function ownerFor(page) {
    if (page.includes("Docs")) return "Docs owns context and durable result.";
    if (page === "Organization") return "Organization owns identity, membership, permission, and authority.";
    if (page === "Finance") return "Finance owns every monetary state and effect.";
    if (page === "Approval") return "Approval is a governed bridge linked from Work or Finance.";
    if (page === "Work") return "Work owns commitment, responsibility, lifecycle, evidence, and result routing.";
    return "Home composes linked projections; it owns no duplicate business records.";
  }

  function installDialogTriggers(root = document) {
    $$('[data-image-src]', root).forEach((button) => {
      if (button.dataset.dialogBound === "true") return;
      button.dataset.dialogBound = "true";
      if (!button.getAttribute("aria-label")) {
        button.setAttribute("aria-label", `放大查看：${button.dataset.imageTitle || button.querySelector("img")?.alt || "视觉证据"}`);
      }
      button.addEventListener("click", () => openImageDialog(button));
    });
  }

  function openImageDialog(trigger) {
    const dialog = $("#imageDialog");
    const image = $("#dialogImage");
    const src = trigger.dataset.imageSrc;
    $("#imageDialogTitle").textContent = trigger.dataset.imageTitle || "Visual evidence";
    $("#imageDialogTruth").textContent = trigger.dataset.imageTruth || "Source image";
    $("#imageDialogTruth").className = `truth-tag ${String(trigger.dataset.imageTruth || "").toLowerCase().includes("actual") ? "actual" : "expected"}`;
    image.src = src;
    image.alt = trigger.querySelector("img")?.alt || trigger.dataset.imageTitle || "Visual evidence";
    $("#dialogSourceLink").href = src;
    dialog.showModal();
    $("#closeImageDialog").focus();
  }

  function installImageFallbacks() {
    $$("img").forEach((image) => {
      if (image.dataset.fallbackBound === "true") return;
      image.dataset.fallbackBound = "true";
      image.addEventListener("error", () => {
        image.alt = `${image.alt || "Image"} — source unavailable`;
        image.closest("figure, .journey-preview, .inspector-evidence")?.classList.add("image-unavailable");
      });
    });
  }

  function updateProgress() {
    const max = document.documentElement.scrollHeight - innerHeight;
    const value = max > 0 ? Math.min(100, Math.max(0, scrollY / max * 100)) : 0;
    $("#readingProgress").style.width = `${value}%`;
  }

  $$('[data-view-link]').forEach((link) => link.addEventListener("click", (event) => {
    event.preventDefault();
    activateView(link.dataset.viewLink);
  }));

  $$('.inspector-tabs button').forEach((button) => button.addEventListener("click", () => {
    state.inspectorTab = button.dataset.inspectorTab;
    renderInspector(currentLine());
    $("#inspectorContent").focus({ preventScroll: true });
  }));

  $$('.view-toggle button').forEach((button, index) => button.addEventListener("click", () => {
    state.objectView = index === 1;
    renderGraph(currentLine());
    $("#handoffGraphContent").scrollIntoView({ block: "nearest", behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  }));

  $("#closeImageDialog").addEventListener("click", () => $("#imageDialog").close());
  $("#imageDialog").addEventListener("click", (event) => {
    if (event.target === $("#imageDialog")) $("#imageDialog").close();
  });

  addEventListener("popstate", () => { readUrl(); applyView({ focus: true }); });
  addEventListener("scroll", updateProgress, { passive: true });
  addEventListener("resize", updateProgress, { passive: true });

  const sectionObserver = "IntersectionObserver" in window ? new IntersectionObserver((entries) => {
    const active = entries.filter((entry) => entry.isIntersecting).sort((a,b) => b.intersectionRatio - a.intersectionRatio)[0];
    if (!active || state.view !== "overview") return;
    $$(".section-nav a").forEach((link) => link.classList.toggle("is-current", link.hash === `#${active.target.id}`));
  }, { rootMargin: "-15% 0px -70%", threshold: [0,.2,.5] }) : null;
  if (sectionObserver) $$(".overview-view section[id]").forEach((section) => sectionObserver.observe(section));

  readUrl();
  renderBusinessLines();
  applyView();
  installDialogTriggers();
  installImageFallbacks();
  updateProgress();
})();
