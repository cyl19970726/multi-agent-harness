# Docs anti-patterns

Check these before final handoff.

- Generic prose only: the page has paragraphs but no page contract, Views, or
  record links.
- Repository markdown treated as live company truth: a design file exists, but
  the project Store lacks corresponding Documents, records, and relations.
- Seed script as product entry: a fixture builder is the only way to create or
  update the project.
- Copied cross-system state: Work status, payment state, actor authority, or
  software delivery state is pasted into Docs instead of linked from the owning
  system.
- UI-only claim: a screenshot looks correct but no Store rows or CLI query prove
  the truth.
- Custom page as database: HTML/React stores business state, bypasses Views, or
  writes directly.
- Missing fallback: custom page has no standard Document/View route.
- GitHub PRD overrides commercial truth: source sync observes software product
  truth; it does not replace the Company OS business model.
- Authority by skill: a skill/tool list is treated as permission or reporting
  authority.
- Hidden finance: purchase, reimbursement, or settlement appears in text but
  no Finance Commitment/Payment record exists.
