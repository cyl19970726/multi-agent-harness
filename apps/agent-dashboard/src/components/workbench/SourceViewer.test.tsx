import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SourceViewerContent } from "./SourceViewer";

describe("SourceViewerContent", () => {
  it("renders Markdown and highlights the requested source line", () => {
    const markup = renderToStaticMarkup(<SourceViewerContent document={{ kind: "markdown", path: "/repo/guide.md", size: 18, line: 2, content: "# Guide\nchosen line\nend" }}/>);
    expect(markup).toContain("Guide");
    expect(markup).toContain("chosen line");
    expect(markup).toContain('data-highlighted-line="2"');
  });

  it("renders text with line numbers and a selected row", () => {
    const markup = renderToStaticMarkup(<SourceViewerContent document={{ kind: "text", path: "/repo/log.txt", size: 7, line: 2, content: "one\ntwo" }}/>);
    expect(markup).toContain('data-source-kind="text"');
    expect(markup).toContain('data-highlighted-line="2"');
  });

  for (const kind of ["binary", "missing", "outside_workspace"] as const) {
    it(`renders the ${kind} resolution error state`, () => {
      const markup = renderToStaticMarkup(<SourceViewerContent document={{ kind, path: "evidence", size: 0 }}/>);
      expect(markup).toContain('role="alert"');
      expect(markup).toContain(`data-source-error="${kind}"`);
    });
  }
});
