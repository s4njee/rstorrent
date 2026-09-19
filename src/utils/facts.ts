/**
 * The detail panel's facts rail, as data.
 *
 * Pure on purpose: the rail is a list of key/value rows and its arithmetic
 * (notably Uploaded) is worth testing without a DOM.
 *
 * The handoff's eleven keys come first, in its order. Everything after them is
 * ours — per-torrent limits, peer caps and provenance are real rtorrent state the
 * design's prototype had no field for, and dropping them would lose function.
 * They are separated visually by a divider, not a different treatment.
 */

import type { BandwidthRule, PieceInfo, TorrentDto } from "../ipc/types";
import {
  formatBytes,
  formatDate,
  formatEta,
  formatRatio,
  formatRate,
} from "./format";
import { limitSource, ruleFor, sourceWord } from "./bandwidth";

export type Fact = [key: string, value: string, tone?: FactTone];

/** How the rail colours a value: the two rates carry their direction's hue,
 * the way the design's table cells do. */
export type FactTone = "rate" | "up";

/** The handoff's facts, in its order (frame 1a). */
export function primaryFacts(t: TorrentDto, pieces?: PieceInfo): Fact[] {
  const uploaded = uploadedBytes(t);
  return [
    ["Hash", t.hash],
    ["Downloaded", formatBytes(t.bytesDone)],
    ["Uploaded", formatBytes(uploaded)],
    ["Ratio", formatRatio(t.ratio)],
    ["Pieces", piecesText(pieces)],
    ["Peers", String(t.peersConnected)],
    ["Down rate", t.downRate > 0 ? formatRate(t.downRate) : "—", "rate"],
    ["Up rate", t.upRate > 0 ? formatRate(t.upRate) : "—", "up"],
    ["Path", t.savePath || "—"],
    ["Added", addedText(t)],
    // Always shown, unlike the old grid which only mentioned it when true: the
    // design's rail answers "is this private?" in both directions.
    ["Private", t.isPrivate ? "yes" : "no"],
  ];
}

/**
 * Total uploaded bytes.
 *
 * rtorrent exposes `d.up.total` but the DTO carries only `d.ratio`, which *is*
 * uploaded÷downloaded — so this inverts exactly the figure the daemon reports
 * rather than inventing one. A ratio of 0 means "nothing uploaded yet".
 */
export function uploadedBytes(t: TorrentDto): number {
  if (!Number.isFinite(t.ratio) || t.ratio <= 0) return 0;
  return Math.round(t.bytesDone * t.ratio);
}

/** `1,024 × 256 kB`, or a dash until the detail poll delivers the piece map. */
export function piecesText(pieces?: PieceInfo): string {
  if (!pieces || pieces.sizeChunks <= 0) return "—";
  const chunk = pieces.chunkSize > 0 ? formatBytes(pieces.chunkSize) : "?";
  return `${pieces.sizeChunks.toLocaleString()} × ${chunk}`;
}

/** Who added it and when, either of which may be unknown. */
export function addedText(t: TorrentDto): string {
  if (!t.addedBy && !t.addedAt) return "—";
  const who = t.addedBy ?? "unknown";
  return t.addedAt ? `${who} · ${formatDate(t.addedAt)}` : who;
}

/** Our additions to the rail: the per-torrent state the design had no field for. */
export function extraFacts(
  t: TorrentDto,
  globalDown?: number,
  globalUp?: number,
  rules: readonly BandwidthRule[] = [],
  turtleActive = false,
): Fact[] {
  const downLimit = t.downRateLimit ?? globalDown;
  const upLimit = t.upRateLimit ?? globalUp;
  const source = sourceWord(
    limitSource(
      t.throttleName ?? "",
      t.throttleRule,
      t.downRateLimit != null || t.upRateLimit != null,
      ruleFor(t.label ?? "", t.tags ?? [], rules),
      turtleActive,
    ),
  );
  const cap = (value: number | undefined) =>
    value && value > 0 ? String(value) : "default";
  const facts: Fact[] = [
    ["Size", formatBytes(t.size)],
    ["ETA", formatEta(t.etaSeconds, t.status)],
    ["Connections", String(t.peersConnected + t.seedsConnected)],
    [
      "Down limit",
      downLimit ? `${formatRate(downLimit)} · ${source}` : `∞ · ${source}`,
    ],
    [
      "Up limit",
      upLimit ? `${formatRate(upLimit)} · ${source}` : `∞ · ${source}`,
    ],
    ["Peer cap", cap(t.peersMax)],
    ["Peer floor", cap(t.peersMin)],
    ["Upload slots", cap(t.uploadsMax)],
  ];
  if (t.sourcePath) facts.push(["Source", t.sourcePath]);
  if (t.connectionType === "initial_seed")
    facts.push(["Mode", "super-seeding"]);
  if (t.isPrivate) facts.push(["DHT / PEX", "off — private torrent"]);
  return facts;
}
