# AI Company OS V1 page matrix

All expected designs use desktop `1536x1024`. The canonical set contains twelve
non-redundant pages: each answers a distinct operator question and together they
prove one cross-module business case.

## Shared cross-module fixture

Every relevant image uses the same records and participants so visual review proves real linkage rather than coincidental repetition.

```text
Trademark application: CN-2026-018
Brand: Brand A
Work item: Trademark filing for Brand A
Financial record: Trademark filing fee · Commitment · ¥3,000 · Pending approval
Human owner: Brand Owner · Human
Standing agents: Trademark Agent; Document Architecture Agent; Finance Agent
External participant: External Lawyer · External
```

| # | Page | Representative state | Operator question | Expected image | Explicit exclusions |
| --- | --- | --- | --- | --- | --- |
| 1 | Company Home | morning review; two decisions need attention | What needs my decision today, and is the company healthy? | `expected/home/morning-review--desktop-concept-1536x1024.png` | no full-company chat stream, raw runtime, or analytics-only wall |
| 2 | Docs workspace | company knowledge structure with one suggested new domain | Where does company knowledge and business structure live? | `expected/docs-workspace/company-knowledge-home--desktop-concept-1536x1024.png` | no file-browser-first UI, Git paths, or raw Markdown framing |
| 3 | Document Focus | Brand A content operating plan is on track | What is this project doing, why, and what changes next? | `expected/document-focus/brand-a-content-plan--desktop-concept-1536x1024.png` | no copied metrics, raw execution logs, or autonomous unreviewed decisions |
| 4 | Workboard | mixed work across intake through review | Who submitted, owns, executes, and reviews company work? | `expected/workboard/company-workboard--desktop-concept-1536x1024.png` | no single ambiguous assignee or chat-as-task |
| 5 | Work Item Focus | trademark filing waits for human fee approval | What is the accountable chain, evidence, execution, and financial consequence? | `expected/work-item-focus/trademark-filing--desktop-concept-1536x1024.png` | no invisible requester/approver, payment bypass, or external person rendered as agent |
| 6 | Finance | July operating view; trademark fee pending approval | Where is money committed or spent and what business record caused it? | `expected/finance/july-operating-view--desktop-concept-1536x1024.png` | no document text acting as ledger, unapproved payment, or unrestricted access |
| 7 | Organization | mixed company org with a proposed Trademark Agent role | Which people, agents, and external contributors make up the company and where are gaps? | `expected/agents-organization/mixed-company-org--desktop-concept-1536x1024.png` | no online-status roster, inferred availability, or automatic high-privilege role creation |
| 8 | Standing Agent Focus | Document Architecture Agent available with non-exclusive work | What contexts does this durable agent serve and can it safely receive more work? | `expected/standing-agent-focus/document-architect-available--desktop-concept-1536x1024.png` | no MemberRun ownership, fake capacity, persistent thinking, or child-thread-as-agent |
| 9 | Governance proposal | new Trademark Management module awaiting final approval | Where should a new business domain live and what changes does it require? | `expected/governance-proposal/trademark-management-module--desktop-concept-1536x1024.png` | no directory-only change, bypassed impact analysis, or automatic permission/agent creation |
| 10 | Approval Focus | filing fee and legal submission require a human decision | What exactly am I authorizing, based on which evidence and policy? | `expected/approval-focus/trademark-filing-fee--desktop-concept-1536x1024.png` | no agent impersonating a human approver, hidden financial effect, or one-click blind approval |
| 11 | Business Module Focus | Trademark Management operating home | How does one new business domain compose records, work, finance, people, and knowledge? | `expected/business-module-focus/trademark-management--desktop-concept-1536x1024.png` | no folder-only module, duplicated ledger values, or runtime-first layout |
| 12 | Human Member Focus | Brand Owner owns Brand A decisions and reviews | What does this human own, which documents/work/approvals need attention, and where do they participate? | `expected/human-member-focus/brand-owner--desktop-concept-1536x1024.png` | no runtime/provider/session sections, fake availability, or agent-like execution telemetry |

## Coverage map

| Core capability | Primary proof page | Supporting proof |
| --- | --- | --- |
| Company-level decisions and health | Home | Governance proposal, Finance |
| Notion-like document system | Docs workspace, Document Focus | Governance proposal |
| Typed work and explicit responsibility | Workboard, Work Item Focus | Standing Agent Focus |
| Cross-module trademark-to-finance link | Work Item Focus, Finance | Governance proposal |
| Mixed human/agent/external organization | Organization | Work Item Focus, Standing Agent Focus |
| Long-lived agent activity and availability | Standing Agent Focus | Agents organization |
| Governed architecture growth | Governance proposal | Docs workspace, Organization |
| Human-sensitive action gate | Approval Focus | Home, Work Item Focus, Finance |
| Complete business-domain composition | Business Module Focus | Docs workspace, Governance proposal |
| Human responsibility without agent-runtime conflation | Human Member Focus | Work Item Focus, Approval Focus, Organization |
