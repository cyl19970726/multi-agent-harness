/**
 * Keep Company OS in-app links inside the same live workbench context.
 *
 * Store-live URLs carry independent Company Store, Execution Space, Project
 * Binding, and API selections. Internal links that drop one can silently route
 * users to another operating context or fixture fallback.
 */
export function preserveCompanyOsWorkbenchContext(href?: string): string | undefined {
  if (!href || typeof window === "undefined") return href;
  try {
    const current = new URL(window.location.href);
    const target = new URL(href, current);
    for (const key of ["api", "project", "space", "company"]) {
      const value = current.searchParams.get(key);
      if (value && !target.searchParams.has(key)) target.searchParams.set(key, value);
    }
    return `${target.pathname}${target.search}${target.hash}`;
  } catch {
    return href;
  }
}
