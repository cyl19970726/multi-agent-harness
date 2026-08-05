/// <reference types="vite/client" />

interface ImportMetaEnv {
  /**
   * The commit this frontend bundle was built/dev-served from, injected by
   * `vite.config.ts` at build/dev-server start (issue #307 provenance
   * surface). "unknown" when the build environment had no git available.
   */
  readonly VITE_DASHBOARD_GIT_REV: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
