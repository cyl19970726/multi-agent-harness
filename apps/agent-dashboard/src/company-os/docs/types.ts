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
  fixtureId?: string;
  updatedLabel?: string;
}

export type CompanyOsViewKind = "table" | "board" | "timeline";

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

export interface CompanyOsStructuredViewData {
  id?: string;
  title: string;
  description?: string;
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
  templates?: CompanyOsLink[];
  databases?: CompanyOsLink[];
  /** Explicit Standing Agent maintainers supplied by organization/document relations. */
  maintainers?: CompanyOsLink[];
  structureNotes?: Array<{ label: string; value: string; tone?: "neutral" | "warning" }>;
  structureLinks?: CompanyOsLink[];
  suggestions?: CompanyOsLink[];
  proposal?: CompanyOsLink;
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
