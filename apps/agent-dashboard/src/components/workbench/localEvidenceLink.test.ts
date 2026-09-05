import { describe, expect, it } from "vitest";
import { localEvidenceTarget } from "./localEvidenceLink";

describe("localEvidenceTarget", () => {
  it("recognizes absolute and line-addressed local citations", () => {
    expect(localEvidenceTarget("/Users/example/repo/docs/foo.md:11"))
      .toEqual({ path: "/Users/example/repo/docs/foo.md", line: 11 });
    expect(localEvidenceTarget("docs/current/operations.md:27"))
      .toEqual({ path: "docs/current/operations.md", line: 27 });
    expect(localEvidenceTarget("C:\\repo\\evidence.txt:3"))
      .toEqual({ path: "C:\\repo\\evidence.txt", line: 3 });
  });

  it("leaves external, hash, and unaddressed relative links alone", () => {
    expect(localEvidenceTarget("https://example.com/a.md:3")).toBeNull();
    expect(localEvidenceTarget("http://example.com")).toBeNull();
    expect(localEvidenceTarget("#section")).toBeNull();
    expect(localEvidenceTarget("README.md")).toBeNull();
  });
});
