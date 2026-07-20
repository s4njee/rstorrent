import { describe, expect, it, vi } from "vitest";
import {
  defaultAddOptions,
  OpenRequestQueue,
  parseDroppedPaths,
  parseOpenRequests,
  parsePastedText,
} from "./externalOpen";
import type { Settings } from "./ipc/types";

describe("parseOpenRequests", () => {
  it("preserves Finder file order and decodes file URLs", () => {
    expect(
      parseOpenRequests([
        "file:///Users/me/One%20File.torrent",
        "file://localhost/tmp/TWO.TORRENT",
      ]),
    ).toEqual([
      { kind: "file", path: "/Users/me/One File.torrent" },
      { kind: "file", path: "/tmp/TWO.TORRENT" },
    ]);
  });

  it("accepts magnets and filters unrelated or remote files", () => {
    expect(
      parseOpenRequests([
        "magnet:?xt=urn:btih:ABC&dn=Example",
        "file:///tmp/readme.txt",
        "file://server/share/test.torrent",
        "https://example.com/test.torrent",
      ]),
    ).toEqual([{ kind: "magnet", uri: "magnet:?xt=urn:btih:ABC&dn=Example" }]);
  });

  it("accepts the Windows shapes: bare drive paths and file: URLs", () => {
    expect(
      parseOpenRequests([
        // What a double-clicked .torrent looks like in argv.
        "C:\\Users\\me\\Downloads\\One.torrent",
        // `pathname` on Windows keeps a leading slash and POSIX separators.
        "file:///C:/Users/me/Two%20Files.torrent",
      ]),
    ).toEqual([
      { kind: "file", path: "C:\\Users\\me\\Downloads\\One.torrent" },
      { kind: "file", path: "C:\\Users\\me\\Two Files.torrent" },
    ]);
  });

  it("accepts a WSL share path but still rejects other UNC hosts", () => {
    expect(
      parseOpenRequests([
        "\\\\wsl.localhost\\Ubuntu\\home\\me\\x.torrent",
        "file://server/share/test.torrent",
      ]),
    ).toEqual([
      { kind: "file", path: "\\\\wsl.localhost\\Ubuntu\\home\\me\\x.torrent" },
    ]);
  });
});

describe("OpenRequestQueue", () => {
  it("waits for each request before starting the next", async () => {
    const releases: Array<() => void> = [];
    const started: string[] = [];
    const queue = new OpenRequestQueue(
      (source) =>
        new Promise<void>((resolve) => {
          started.push(source.kind === "file" ? source.path : source.uri);
          releases.push(resolve);
        }),
    );

    queue.enqueue(
      parseOpenRequests(["file:///tmp/one.torrent", "file:///tmp/two.torrent"]),
    );
    expect(started).toEqual(["/tmp/one.torrent"]);

    releases.shift()!();
    await Promise.resolve();
    expect(started).toEqual(["/tmp/one.torrent", "/tmp/two.torrent"]);

    releases.shift()!();
    await queue.whenIdle();
  });

  it("reports an error and continues with the remaining queue", async () => {
    const handled: string[] = [];
    const onError = vi.fn();
    const queue = new OpenRequestQueue(async (source) => {
      const path = source.kind === "file" ? source.path : source.uri;
      handled.push(path);
      if (path.includes("bad")) throw new Error("bad request");
    }, onError);

    queue.enqueue(parseOpenRequests(["/tmp/bad.torrent", "/tmp/good.torrent"]));
    await queue.whenIdle();

    expect(handled).toEqual(["/tmp/bad.torrent", "/tmp/good.torrent"]);
    expect(onError).toHaveBeenCalledOnce();
  });
});

it("builds instant-add defaults from preferences", () => {
  const settings = {
    defaultSavePath: "/Volumes/Downloads",
  } as Settings;
  expect(defaultAddOptions(settings)).toEqual({
    savePath: "/Volumes/Downloads",
    label: "",
    start: true,
    topOfQueue: false,
    sequential: false,
    skipHashCheck: false,
    unselectedIndexes: [],
  });
});

describe("parseDroppedPaths (C1)", () => {
  it("keeps .torrent files and ignores everything else in the drop", () => {
    expect(
      parseDroppedPaths([
        "/Users/me/a.torrent",
        "/Users/me/holiday.jpg",
        "/Users/me/B.TORRENT",
      ]),
    ).toEqual([
      { kind: "file", path: "/Users/me/a.torrent" },
      { kind: "file", path: "/Users/me/B.TORRENT" },
    ]);
  });

  it("keeps paths containing spaces intact", () => {
    expect(parseDroppedPaths(["/Users/me/Some File.torrent"])).toEqual([
      { kind: "file", path: "/Users/me/Some File.torrent" },
    ]);
  });

  it("returns nothing for a drop with no torrents", () => {
    expect(parseDroppedPaths(["/Users/me/notes.txt"])).toEqual([]);
  });
});

describe("parsePastedText (C2)", () => {
  it("accepts a magnet link", () => {
    const uri = "magnet:?xt=urn:btih:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b";
    expect(parsePastedText(uri)).toEqual([{ kind: "magnet", uri }]);
  });

  it("accepts an http(s) .torrent URL, ignoring a query string", () => {
    const uri = "https://example.org/files/x.torrent?id=42";
    expect(parsePastedText(uri)).toEqual([{ kind: "magnet", uri }]);
  });

  it("ignores an ordinary web link, so pasting a URL is a no-op", () => {
    expect(parsePastedText("https://example.org/news/article")).toEqual([]);
  });

  it("ignores prose and empty input", () => {
    expect(parsePastedText("just some copied words")).toEqual([]);
    expect(parsePastedText("   ")).toEqual([]);
    expect(parsePastedText("")).toEqual([]);
  });

  it("splits on newlines only, so pasted paths keep their spaces", () => {
    expect(parsePastedText("/tmp/Two Words.torrent\n/tmp/b.torrent")).toEqual([
      { kind: "file", path: "/tmp/Two Words.torrent" },
      { kind: "file", path: "/tmp/b.torrent" },
    ]);
  });

  it("takes every magnet from a multi-line paste, skipping junk lines", () => {
    const text = [
      "magnet:?xt=urn:btih:aaaa",
      "not a torrent",
      "",
      "magnet:?xt=urn:btih:bbbb",
    ].join("\n");
    expect(parsePastedText(text)).toEqual([
      { kind: "magnet", uri: "magnet:?xt=urn:btih:aaaa" },
      { kind: "magnet", uri: "magnet:?xt=urn:btih:bbbb" },
    ]);
  });

  it("trims surrounding whitespace from a copied magnet", () => {
    expect(parsePastedText("  magnet:?xt=urn:btih:cccc  ")).toEqual([
      { kind: "magnet", uri: "magnet:?xt=urn:btih:cccc" },
    ]);
  });
});

describe("parseOpenRequests still rejects http URLs", () => {
  it("does not accept http(s) torrents from LaunchServices", () => {
    // Only pasted/dropped text may carry an http URL; deep links never do.
    expect(parseOpenRequests(["https://example.org/x.torrent"])).toEqual([]);
  });
});
