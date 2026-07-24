import type { ReactNode } from "react";

/**
 * Read-only presentation contracts for Company OS Docs.  They intentionally do
 * not mirror a database schema: an application adapter owns record lookups,
 * permissions, and action policy before data reaches these components.
 */
export type CompanyOsActorKind = "human" | "agent" | "external" | "service";

export interface CompanyOsActor {
  id: string;
  name: string;
  kind: CompanyOsActorKind;
  role?: string;
  /** Only pass an explicitly reported status; undefined renders no presence claim. */
  reportedStatus?: string;
}

export interface CompanyOsLink {
  id: string;
  label: string;
  href?: string;
  kind?: "document" | "record" | "work" | "approval" | "finance" | "module" | "actor";
  meta?: string;
  actorType?: "Human" | "Standing Agent" | "External" | "Service";
  financialRecordType?: "commitment" | "invoice" | "payment" | "budget";
}

export interface CompanyOsTemplateBlockSnapshot {
  id: string;
  kind: string;
  content: Record<string, unknown>;
  referencedEntities: CompanyOsEntityRef[];
}

export interface CompanyOsTemplateOption extends CompanyOsLink {
  kind: "document";
  templateBlockIds: string[];
  templateBlocks: CompanyOsTemplateBlockSnapshot[];
}

export interface CompanyOsTemplateRecordPolicy {
  status: "declared" | "missing";
  recordTypes: string[];
  relationTypes: string[];
  commandHint: string;
}

export interface CompanyOsActorRef {
  actor_type: "human" | "agent" | "external" | "service";
  actor_id: string;
}

export interface CompanyOsEntityRef {
  kind: string;
  id: string;
}

export interface CompanyOsCorrectiveWorkContext {
  definitionId: string;
  actionPolicyRef: string;
  sourceDocument: CompanyOsLink;
  businessModuleRef?: string;
  sourceRecordRefs: string[];
  requestedBy: CompanyOsActorRef;
  submittedBy: CompanyOsActorRef;
  accountableOwner: CompanyOsActorRef;
  assignees: CompanyOsActorRef[];
  reviewer?: CompanyOsActorRef;
}

export interface CompanyOsCorrectiveWorkCommand {
  id: string;
  command_name: "work_item.append";
  subject_ref: CompanyOsEntityRef;
  requested_by: CompanyOsActorRef;
  payload: {
    definition_id: string;
    record: Record<string, unknown>;
  };
  required_permission: "company.records.write";
  policy_ref: string;
  risk_tier: "r1";
  requires_human_approval: false;
  approval_refs: [];
  status: "requested";
  audit_event_refs: string[];
  requested_at: string;
  completed_at: null;
}

export interface CompanyOsRelationRepairContext {
  definitionId: string;
  actionPolicyRef: string;
  relationType: string;
  from: CompanyOsEntityRef;
  to: CompanyOsEntityRef;
  provenanceRef?: CompanyOsEntityRef;
  requestedBy: CompanyOsActorRef;
  createdBy: CompanyOsActorRef;
}

export interface CompanyOsRelationRepairCommand {
  id: string;
  command_name: "relation.append";
  subject_ref: CompanyOsEntityRef;
  requested_by: CompanyOsActorRef;
  payload: {
    definition_id: string;
    record: Record<string, unknown>;
  };
  required_permission: "company.records.write";
  policy_ref: string;
  risk_tier: "r1";
  requires_human_approval: false;
  approval_refs: [];
  status: "requested";
  audit_event_refs: string[];
  requested_at: string;
  completed_at: null;
}

export interface CompanyOsDocsActionCommand {
  id: string;
  command_name: "document.append" | "block.append" | "typed_record.append" | "view.append" | "relation.append";
  subject_ref: CompanyOsEntityRef;
  requested_by: CompanyOsActorRef;
  payload: {
    definition_id: string;
    record: Record<string, unknown>;
  };
  required_permission: "company.records.write";
  policy_ref: string;
  risk_tier: "r1";
  requires_human_approval: false;
  approval_refs: [];
  status: "requested";
  audit_event_refs: string[];
  requested_at: string;
  completed_at: null;
}

export interface CompanyOsDocumentAuthoringContext {
  definitionId: string;
  documentPolicyRef: string;
  blockPolicyRef: string;
  documentId: string;
  spaceId: string;
  parentDocumentId: string | null;
  documentKind: string;
  lifecycleStatus: string;
  blockIds: string[];
  permissionPolicyRefs: string[];
  referenceRefs: CompanyOsEntityRef[];
  templateRef?: string | null;
  templateOptions?: CompanyOsTemplateOption[];
  templateRecordPolicy?: CompanyOsTemplateRecordPolicy;
  createdBy: CompanyOsActorRef;
  createdAt: string;
  requestedBy: CompanyOsActorRef;
}

export interface CompanyOsModuleAuthoringContext {
  definitionId: string;
  moduleId: string;
  sourceDocumentId: string;
  typedRecordPolicyRef: string;
  relationPolicyRef: string;
  viewPolicyRef: string;
  requestedBy: CompanyOsActorRef;
}

export interface CompanyOsProperty {
  label: string;
  value: ReactNode;
  ref?: string;
  actorType?: CompanyOsLink["actorType"];
}

export interface CompanyOsSimpleTable {
  columns: string[];
  rows: Array<Array<ReactNode>>;
  caption?: string;
}

export type CompanyOsDocumentBlock =
  | { id: string; type: "paragraph"; content: ReactNode }
  | { id: string; type: "heading"; level?: 2 | 3; content: ReactNode }
  | { id: string; type: "bullets"; items: ReactNode[] }
  | { id: string; type: "callout"; title?: string; content: ReactNode; tone?: "neutral" | "warning" | "success" }
  | { id: string; type: "table"; table: CompanyOsSimpleTable }
  | { id: string; type: "relations"; label?: string; links: CompanyOsLink[] }
  | { id: string; type: "custom"; content: ReactNode };

export interface CompanyOsDocumentPageData {
  id?: string;
  title: string;
  breadcrumb?: string[];
  space?: string;
  description?: string;
  properties?: CompanyOsProperty[];
  blocks: CompanyOsDocumentBlock[];
  sourceLinks?: CompanyOsLink[];
  resultLinks?: CompanyOsLink[];
  connectedRecords?: CompanyOsLink[];
  activity?: Array<{ id: string; label: string; detail?: string; at?: string }>;
  authoring?: CompanyOsDocumentAuthoringContext;
  fixtureId?: string;
  updatedLabel?: string;
}

export type CompanyOsViewKind = "table" | "board" | "timeline";

export interface CompanyOsStandardViewConfig {
  mode?: CompanyOsViewKind;
  sourceKinds?: string[];
  filters?: Array<{ field: string; value: string }>;
  groupBy?: string;
  sortBy?: string;
  query?: Record<string, unknown>;
}

export interface CompanyOsViewColumn {
  id: string;
  label: string;
  cell: (record: CompanyOsRecord) => ReactNode;
}

export interface CompanyOsRecord {
  id: string;
  title: string;
  type?: string;
  status?: string;
  group?: string;
  date?: string;
  links?: CompanyOsLink[];
  fields?: Record<string, ReactNode>;
}

export interface CompanyOsCustomPageStatus {
  definitionId: string;
  moduleId?: string;
  purpose?: string;
  ownerLabel?: string;
  activePackageId?: string;
  activeVersion?: string;
  latestPackageId?: string;
  latestVersion?: string;
  artifactRef?: string;
  entrypoint?: string;
  integrityDigest?: string;
  fixtureRef?: string;
  visualContractRef?: string;
  fallbackViewId?: string;
  allowedQueries: string[];
  declaredActions: string[];
  approvedComponents: string[];
  policyRefs: string[];
  status: "active" | "candidate_recorded" | "definition_only" | "fallback_only";
  statusLabel: string;
  boundaryNote: string;
}

export interface CompanyOsStructuredViewData {
  id?: string;
  title: string;
  description?: string;
  provenance?: {
    moduleId?: string;
    moduleLabel?: string;
    viewId?: string;
    viewTitle?: string;
    sourceKinds?: string[];
    querySummary?: string;
    recordCount?: number;
  };
  configuration?: CompanyOsStandardViewConfig;
  records: CompanyOsRecord[];
  columns: CompanyOsViewColumn[];
  availableViews?: CompanyOsViewKind[];
  fallback?: {
    label: string;
    href?: string;
    description?: string;
  };
  sourceLinks?: CompanyOsLink[];
  resultLinks?: CompanyOsLink[];
  authoring?: CompanyOsModuleAuthoringContext;
  customPage?: CompanyOsCustomPageStatus;
  fixtureId?: string;
}

export interface CompanyOsWorkspaceSpace {
  id: string;
  name: string;
  summary?: string;
  countLabel?: string;
  status?: string;
  href?: string;
}

export interface CompanyOsWorkspaceTreeItem {
  id: string;
  /** Canonical store ref for a visible durable object; absent for UI-only grouping nodes. */
  ref?: string;
  label: string;
  href?: string;
  selected?: boolean;
  meta?: string;
  children?: CompanyOsWorkspaceTreeItem[];
}

export interface CompanyOsWorkspaceData {
  title?: string;
  description?: string;
  rootSelected?: boolean;
  tree: CompanyOsWorkspaceTreeItem[];
  spaces: CompanyOsWorkspaceSpace[];
  recentlyUpdated?: CompanyOsLink[];
  templates?: CompanyOsTemplateOption[];
  templateRecordPolicy?: CompanyOsTemplateRecordPolicy;
  databases?: CompanyOsLink[];
  /** Explicit Standing Agent maintainers supplied by organization/document relations. */
  maintainers?: CompanyOsLink[];
  structureNotes?: Array<{ label: string; value: string; tone?: "neutral" | "warning" }>;
  structureLinks?: CompanyOsLink[];
  suggestions?: CompanyOsLink[];
  proposal?: CompanyOsLink;
  authoringCommands?: Array<{
    id: string;
    label: string;
    command: string;
    scope: "governance" | "module_action";
    disabledReason?: string;
  }>;
  fixtureId?: string;
}

export interface CompanyOsHealthFinding {
  id: string;
  kind: string;
  severity: "critical" | "warning" | "info" | "good";
  title: string;
  detail: string;
  subject?: CompanyOsLink;
  related?: CompanyOsLink;
  affected?: CompanyOsLink[];
  recommendedAction: string;
  directActionLabel?: string;
  correctiveWorkLabel?: string;
  correctiveWorkContext?: CompanyOsCorrectiveWorkContext;
  relationRepairContext?: CompanyOsRelationRepairContext;
}

export interface CompanyOsDocumentHealthData {
  title: string;
  description?: string;
  status: "pass" | "issues";
  counts: {
    documents: number;
    blocks: number;
    typedRecords: number;
    relations: number;
    businessModules: number;
    findings: number;
    critical: number;
    warning: number;
  };
  findings: CompanyOsHealthFinding[];
  selectedFindingId?: string;
  cleanupQueue?: Array<{
    id: string;
    operation: "rename" | "split" | "merge" | "archive" | "migrate";
    label: string;
    detail: string;
    findingId: string;
    subject?: CompanyOsLink;
    route: "corrective_work_item";
    disabledReason?: string;
  }>;
  governanceAgent?: CompanyOsLink;
  structureLinks?: CompanyOsLink[];
  actionHints?: Array<{
    id: string;
    label: string;
    command: string;
    tone?: "primary" | "neutral" | "warning";
    disabledReason?: string;
  }>;
  fixtureId?: string;
}

export interface CompanyOsHomeData {
  title: string;
  subtitle?: string;
  decisionRequired?: CompanyOsLink;
  decisionSummary?: string;
  decisionActor?: CompanyOsActor;
  decisionRequester?: CompanyOsLink;
  decisionCollaborators?: CompanyOsLink[];
  changes: CompanyOsLink[];
  participants?: CompanyOsLink[];
  workSummary: Array<{ id?: string; label: string; value: string; detail?: string }>;
  financeSummary: Array<{ id?: string; label: string; value: string; detail?: string; financialRecordType?: CompanyOsLink["financialRecordType"] }>;
  fixtureId?: string;
}
