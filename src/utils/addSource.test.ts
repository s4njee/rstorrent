import { describe, expect, it } from "vitest";
import { isLoadableSource, parseSourceLines } from "./addSource";

describe("isLoadableSource", () => {
  it("accepts magnets with a btih hash and http(s) URLs", () => {
    expect(
      isLoadableSource(
        "magnet:?xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01&dn=x",
      ),
    ).toBe(true);
    expect(isLoadableSource("https://example.test/a.torrent")).toBe(true);
    expect(isLoadableSource("http://example.test/a.torrent")).toBe(true);
  });

  it("rejects anything else", () => {
    expect(isLoadableSource("magnet:?xt=urn:sha1:abc")).toBe(false);
    expect(isLoadableSource("not a link")).toBe(false);
    expect(isLoadableSource("ftp://example.test/a.torrent")).toBe(false);
    expect(isLoadableSource("")).toBe(false);
  });
});

describe("parseSourceLines", () => {
  it("keeps one entry per non-blank line, in order", () => {
    const parsed = parseSourceLines(
      "magnet:?xt=urn:btih:aaaa\n\n  https://example.test/b.torrent  \nnonsense\n",
    );
    expect(parsed).toEqual([
      { value: "magnet:?xt=urn:btih:aaaa", valid: true },
      { value: "https://example.test/b.torrent", valid: true },
      { value: "nonsense", valid: false },
    ]);
  });

  it("ignores blank input entirely", () => {
    expect(parseSourceLines("  \n \n")).toEqual([]);
  });
});
