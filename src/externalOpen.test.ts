import { describe, expect, it, vi } from "vitest";
import {
  defaultAddOptions,
  OpenRequestQueue,
  parseDroppedFiles,
  parsePastedText,
} from "./externalOpen";
import type { Settings } from "./ipc/types";

describe("parseDroppedFiles", () => {
  it("keeps only .torrent Files as upload sources", () => {
    const torrent = new File(["d4:infoe"], "movie.torrent", {
      type: "application/x-bittorrent",
    });
    const noise = new File(["x"], "readme.txt", { type: "text/plain" });
    expect(parseDroppedFiles([noise, torrent])).toEqual([
      { kind: "upload", file: torrent },
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
          started.push(
            source.kind === "magnet" ? source.uri : source.file.name,
          );
          releases.push(resolve);
        }),
    );

    queue.enqueue(parsePastedText("magnet:?xt=one\nmagnet:?xt=two"));
    expect(started).toEqual(["magnet:?xt=one"]);

    releases.shift()!();
    await Promise.resolve();
    expect(started).toEqual(["magnet:?xt=one", "magnet:?xt=two"]);

    releases.shift()!();
    await queue.whenIdle();
  });

  it("reports an error and continues with the remaining queue", async () => {
    const handled: string[] = [];
    const onError = vi.fn();
    const queue = new OpenRequestQueue(async (source) => {
      const name = source.kind === "magnet" ? source.uri : source.file.name;
      handled.push(name);
      if (name.includes("bad")) throw new Error("bad request");
    }, onError);

    queue.enqueue(parsePastedText("magnet:?xt=bad\nmagnet:?xt=good"));
    await queue.whenIdle();

    expect(handled).toEqual(["magnet:?xt=bad", "magnet:?xt=good"]);
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

  it("ignores local paths, which the server cannot read from the browser", () => {
    expect(parsePastedText("/tmp/a.torrent\nfile:///tmp/b.torrent")).toEqual(
      [],
    );
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
