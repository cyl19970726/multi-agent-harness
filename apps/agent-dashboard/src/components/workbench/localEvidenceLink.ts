export interface LocalEvidenceTarget {
  path: string;
  line?: number;
}

/** Classify only filesystem evidence. Web and hash links remain ordinary links. */
export function localEvidenceTarget(rawHref: string): LocalEvidenceTarget | null {
  const href = rawHref.trim();
  if (!href || /^(?:https?:|mailto:|#)/i.test(href)) return null;
  const citation = /^(.*):(\d+)$/.exec(href);
  if (citation) {
    const line = Number(citation[2]);
    if (line > 0 && isLocalPath(citation[1])) return { path: citation[1], line };
  }
  return isAbsoluteLocalPath(href) ? { path: href } : null;
}

function isAbsoluteLocalPath(value: string): boolean {
  return value.startsWith("/") || /^[A-Za-z]:[\\/]/.test(value);
}

function isLocalPath(value: string): boolean {
  return isAbsoluteLocalPath(value) || value.startsWith("./") || /^[^?#]+[\\/][^?#]+$/.test(value);
}
