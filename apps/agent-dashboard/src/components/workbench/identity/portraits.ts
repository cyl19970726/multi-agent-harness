import contentCreative from "@/assets/agent-members/avatars/content-creative.png";
import financeOperations from "@/assets/agent-members/avatars/finance-operations.png";
import implementationEngineer from "@/assets/agent-members/avatars/implementation-engineer.png";
import productStrategist from "@/assets/agent-members/avatars/product-strategist.png";
import researchVerifier from "@/assets/agent-members/avatars/research-verifier.png";
import securityReviewer from "@/assets/agent-members/avatars/security-reviewer.png";
import technicalLead from "@/assets/agent-members/avatars/technical-lead.png";
import workspaceArchitect from "@/assets/agent-members/avatars/workspace-architect.png";

/**
 * Shared actor portrait resolver for Company OS and execution workbench roles.
 * The image is presentational; identity, provider, role, and status always stay
 * in text so a portrait can never become runtime evidence.
 */
const portraits: Array<{ match: RegExp; src: string }> = [
  { match: /document|architecture|workspace|governance|runtime|worker/i, src: workspaceArchitect },
  { match: /research|verification|quality|\bqa\b|truth|analyst|observer/i, src: researchVerifier },
  { match: /permission|security|critic|review|compliance|legal|trademark/i, src: securityReviewer },
  { match: /finance|operations|budget|cost|procurement|\bdata\b/i, src: financeOperations },
  { match: /product|strategy|planning|market|growth/i, src: productStrategist },
  { match: /content|creative|brand|media|editorial|design/i, src: contentCreative },
  { match: /backend|developer|implementation|engineer|builder|fixer/i, src: implementationEngineer },
  { match: /ip lead|company lead|brand owner|human|owner|\blead\b|host|coordinator/i, src: technicalLead },
];

const defaultPortraits = [
  workspaceArchitect,
  researchVerifier,
  technicalLead,
  implementationEngineer,
  productStrategist,
  securityReviewer,
  financeOperations,
  contentCreative,
];

/** Resolve a role portrait, or a stable project-default portrait for an
 * otherwise unknown member. The fallback is deterministic so the same agent
 * keeps its visual identity across Mission, Team, and Member views. */
export function portraitFor(identity: string): string {
  const matched = portraits.find(({ match }) => match.test(identity))?.src;
  if (matched) return matched;
  const hash = Array.from(identity).reduce(
    (value, character) => ((value * 31) + character.codePointAt(0)!) >>> 0,
    2166136261,
  );
  return defaultPortraits[hash % defaultPortraits.length];
}
