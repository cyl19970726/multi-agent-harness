import companyLead from "@/assets/company-os/avatars/company-lead.png";
import contentStrategy from "@/assets/company-os/avatars/content-strategy-agent.png";
import documentArchitecture from "@/assets/company-os/avatars/document-architecture-agent.png";
import financeAgent from "@/assets/company-os/avatars/finance-agent.png";
import trademarkAgent from "@/assets/company-os/avatars/trademark-agent.png";

/**
 * Shared actor portrait resolver for Company OS and execution workbench roles.
 * The image is presentational; identity, provider, role, and status always stay
 * in text so a portrait can never become runtime evidence.
 */
const portraits: Array<{ match: RegExp; src: string }> = [
  { match: /document|architecture|governance|runtime|research|worker/i, src: documentArchitecture },
  { match: /trademark|legal|quality|\bqa\b|critic|review|compliance|truth/i, src: trademarkAgent },
  { match: /finance|budget|cost|\bdata\b/i, src: financeAgent },
  { match: /content|strategy|analytics|backend|developer|implementation|engineer/i, src: contentStrategy },
  { match: /ip lead|company lead|brand owner|human|owner|\blead\b|host|coordinator/i, src: companyLead },
];

export function portraitFor(identity: string): string | undefined {
  return portraits.find(({ match }) => match.test(identity))?.src;
}
