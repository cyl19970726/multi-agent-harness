import { createContext, useContext, type ReactNode } from "react";
import type { LocalEvidenceTarget } from "./localEvidenceLink";

type OpenEvidence = (target: LocalEvidenceTarget, messageId?: string) => void;
const EvidenceLinkContext = createContext<OpenEvidence | null>(null);

export function EvidenceLinkProvider({ open, children }: { open: OpenEvidence; children: ReactNode }) {
  return <EvidenceLinkContext.Provider value={open}>{children}</EvidenceLinkContext.Provider>;
}

export function useOpenEvidence(): OpenEvidence | null {
  return useContext(EvidenceLinkContext);
}
