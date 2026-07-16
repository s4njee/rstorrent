import { describe, expect, it, vi } from "vitest";
import {
  defaultAddOptions,
  OpenRequestQueue,
  parseOpenRequests,
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
