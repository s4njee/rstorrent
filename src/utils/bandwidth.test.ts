import { describe, expect, it } from "vitest";

import type { BandwidthRule } from "../ipc/types";
import { describeRule, limitSource, ruleFor, sourceWord } from "./bandwidth";

function tagRule(id: string, tag: string): BandwidthRule {
  return { id, tag, downKb: 512, upKb: 128 };
}

function labelRule(id: string, label: string): BandwidthRule {
  return { id, label, downKb: 1024, upKb: 256 };
}

const rules = [tagRule("archive", "Archive"), labelRule("vid", "video")];

describe("ruleFor", () => {
  it("prefers the first matching tag, then the label", () => {
    expect(ruleFor("video", ["archive"], rules)?.id).toBe("archive");
    expect(ruleFor("VIDEO", [], rules)?.id).toBe("vid");
    expect(ruleFor("other", [], rules)).toBeNull();
  });

  it("ignores rules without ids", () => {
    expect(ruleFor("x", ["archive"], [{ ...rules[0], id: " " }])).toBeNull();
  });
});

describe("limitSource", () => {
  it("orders manual, rule, turtle, global", () => {
    const rule = ruleFor("video", [], rules);
    expect(limitSource("rstorrent_1", "", false, rule, false)).toEqual({
      kind: "manual",
    });
    expect(limitSource("", "", true, null, false)).toEqual({
      kind: "manual",
    });
    expect(limitSource("rule_vid", "vid", true, rule, true)).toEqual({
      kind: "rule",
      describe: 'label "video"',
    });
    expect(limitSource("", "", false, null, true)).toEqual({ kind: "turtle" });
    expect(limitSource("", "", false, null, false)).toEqual({ kind: "global" });
  });

  it("words the precedence for the limit rows", () => {
    expect(sourceWord({ kind: "manual" })).toBe("torrent override");
    expect(sourceWord({ kind: "rule", describe: 'tag "x"' })).toBe(
      'rule tag "x"',
    );
    expect(sourceWord({ kind: "turtle" })).toBe("turtle");
    expect(sourceWord({ kind: "global" })).toBe("global");
  });

  it("describes matches", () => {
    expect(describeRule(rules[0])).toBe('tag "Archive"');
    expect(describeRule(rules[1])).toBe('label "video"');
  });
});
