import type { CompanyOsWorkspaceTreeItem } from "./types";

/** The workspace tree is a real hierarchy, so every walk must recurse. */
export function flattenDocumentTree(items: CompanyOsWorkspaceTreeItem[] = []): CompanyOsWorkspaceTreeItem[] {
  return items.flatMap((item) => [item, ...flattenDocumentTree(item.children)]);
}

/**
 * Tree nodes carry either a Document route or a BusinessModule route.  Space nodes
 * hold both kinds, so raw child count is not a usable signal for "which node holds
 * the pages".
 */
export function isDocumentTreeNode(item: CompanyOsWorkspaceTreeItem): boolean {
  return Boolean(item.href?.includes("document="));
}

export function documentChildCount(item: CompanyOsWorkspaceTreeItem): number {
  return (item.children ?? []).filter(isDocumentTreeNode).length;
}

/**
 * Picks the node that actually holds a named area's child pages.
 *
 * Counting raw children picks the wrong node: a space node's children are its root
 * Documents *plus* every BusinessModule attached to that space, so a space holding
 * one root Document and eleven modules (twelve children) outranks the root Document
 * that holds the eleven real pages.  Rank by Document children only; on a tie prefer
 * the deeper Document node over a grouping node, and otherwise keep the first
 * pre-order match so a match set with no Document children stays at the top.
 */
export function selectDocumentDirectoryAnchor(
  tree: CompanyOsWorkspaceTreeItem[] | undefined,
  pattern: RegExp,
): CompanyOsWorkspaceTreeItem | undefined {
  const matches = flattenDocumentTree(tree).filter((item) => pattern.test(`${item.label} ${item.ref ?? ""}`));
  const best = matches.reduce<CompanyOsWorkspaceTreeItem | undefined>((winner, item) => {
    if (!winner) return item;
    const delta = documentChildCount(item) - documentChildCount(winner);
    if (delta !== 0) return delta > 0 ? item : winner;
    if (isDocumentTreeNode(item) !== isDocumentTreeNode(winner)) return isDocumentTreeNode(item) ? item : winner;
    return winner;
  }, undefined);
  return best ?? tree?.[0];
}
