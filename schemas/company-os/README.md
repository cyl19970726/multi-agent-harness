# Company OS schemas

These Draft 2020-12 schemas describe the additive Company OS product records.
They intentionally do not import, project, or convert legacy Goal/Task records.

| File | Canonical records |
| --- | --- |
| `actors.schema.json` | ActorRef, HumanMember, StandingAgent, ExternalParticipant, ServiceActor, OrgUnit, OrganizationMembership |
| `knowledge.schema.json` | Document, Block, TypedRecord, Relation, View, BusinessModule |
| `work.schema.json` | Milestone, WorkType-bearing WorkItem, Assignment, Approval |
| `finance.schema.json` | Commitment, Payment |
| `programmable-page.schema.json` | CustomPageDefinition, CustomPagePackage, ActionPolicyDefinition, ActionCommand, AuditEvent |

Cross-record authorization and referential checks belong at the governed store
and Action Command boundary. The Rust models enforce local invariants before a
record reaches that boundary.
