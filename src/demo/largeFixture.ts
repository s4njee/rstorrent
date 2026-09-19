/**
 * Deterministic large-library fixture for FND-01 (5k torrents).
 *
 * Uses a seeded xorshift so the same count/seed always yields the same list.
 * The distribution loosely mirrors real seedbox data: ~30% seeding, 25%
 * downloading, 10% stalled, 15% paused, 10% completed/paused, 10% error/mixed.
 * All hashes are 40-char hex (sha1-like) and names are deterministic.
 */

import type { GlobalStats, Snapshot, Status, TorrentDto } from "../ipc/types";

const GIB = 1_073_741_824;
const MIB = 1_048_576;

function xorshift(seed: number): () => number {
  let x = seed | 0;
  if (x === 0) x = 0x6a5d39ea;
  return () => {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    // to [0,1)
    return ((x >>> 0) % 1_000_000) / 1_000_000;
  };
}

const STATUSES: Status[] = [
  "downloading",
  "seeding",
  "paused",
  "stalled",
  "checking",
  "error",
  "completed",
];

const TRACKERS = [
  "tracker.ubuntu.com",
  "bttracker.debian.org",
  "tracker.archlinux.org",
  "torrent.fedoraproject.org",
  "opensuse.org",
  "tracker.blender.org",
  "linuxtracker.org",
  "downloads.raspberrypi.org",
  "tracker.example.com",
  "private.tracker.io",
];

const LABELS = ["linux-iso", "video", "sbc", "music", "books", ""];

const ERROR_KINDS = [
  "unregistered",
  "tracker_timeout",
  "tracker_error",
  "missing_files",
  "no_space",
  "permission",
  "disk_error",
  "other",
];

function hexHash(rnd: () => number, index: number): string {
  // deterministic 40-char hex based on index + rnd
  const base = (index * 2654435761) >>> 0;
  let h = "";
  let v = base;
  for (let i = 0; i < 10; i++) {
    v = (v * 1664525 + 1013904223) >>> 0;
    const mix = (v ^ Math.floor(rnd() * 0xffffffff)) >>> 0;
    h += mix.toString(16).padStart(8, "0");
  }
  return h.slice(0, 40);
}

function pick<T>(rnd: () => number, arr: T[]): T {
  return arr[Math.floor(rnd() * arr.length)]!;
}

function torrentName(rnd: () => number, index: number): string {
  const prefixes = [
    "ubuntu-24.04",
    "debian-12",
    "fedora-41",
    "archlinux-2026",
    "opensuse-tumbleweed",
    "linuxmint-22",
    "raspios-bookworm",
    "big-buck-bunny",
    "sintel",
    "cosmos-laundromat",
    "dataset-v3",
    "backup-2026",
    "media-pack",
    "release-",
  ];
  const p = pick(rnd, prefixes);
  const suffix = `${index.toString().padStart(4, "0")}.iso`;
  return `${p}-${suffix}`;
}

export interface LargeFixtureOptions {
  count?: number;
  seed?: number;
  nowSec?: number;
}

export function makeLargeFixture(options: LargeFixtureOptions = {}): Snapshot {
  const count = options.count ?? 5000;
  const seed = options.seed ?? 0x1234abcd;
  const nowSec =
    options.nowSec ?? Math.floor(Date.UTC(2026, 6, 20, 16, 0, 0) / 1000);
  const rnd = xorshift(seed);

  const torrents: TorrentDto[] = [];
  let downRateSum = 0;
  let upRateSum = 0;

  for (let i = 0; i < count; i++) {
    const r = rnd();
    // weighted status pick
    let status: Status;
    if (r < 0.3) status = "seeding";
    else if (r < 0.55) status = "downloading";
    else if (r < 0.65) status = "stalled";
    else if (r < 0.8) status = "paused";
    else if (r < 0.9) status = "completed";
    else if (r < 0.95) status = "error";
    else status = pick(rnd, STATUSES);

    const size = Math.round((0.2 + rnd() * 8) * GIB);
    const percent =
      status === "seeding" || status === "completed"
        ? 100
        : status === "error"
          ? Math.round(rnd() * 80 + 10)
          : status === "paused"
            ? Math.round(rnd() * 90 + 5)
            : Math.round(rnd() * 95);
    const bytesDone = Math.round((size * percent) / 100);
    const downRate = status === "downloading" ? Math.round(rnd() * 5 * MIB) : 0;
    const upRate =
      status === "seeding" || status === "downloading"
        ? Math.round(rnd() * 1.2 * MIB)
        : 0;
    downRateSum += downRate;
    upRateSum += upRate;
    const etaSeconds =
      status === "downloading" && downRate > 0
        ? Math.round((size - bytesDone) / downRate)
        : null;
    const label = pick(rnd, LABELS);
    const trackerHost = pick(rnd, TRACKERS);
    const seedsSwarm = Math.round(rnd() * 200);
    const peersSwarm = Math.round(rnd() * 100);
    const peersConnected =
      status === "downloading" || status === "seeding"
        ? Math.round(rnd() * 40)
        : 0;
    const seedsConnected =
      status === "downloading" ? Math.round(peersConnected * 0.6) : 0;
    const statusMsg =
      status === "error"
        ? `Tracker: [Failure reason "${pick(rnd, ERROR_KINDS)}"]`
        : "";
    const errorKind = status === "error" ? pick(rnd, ERROR_KINDS) : "";
    const hash = hexHash(rnd, i);
    const name = torrentName(rnd, i);
    const isPrivate = rnd() < 0.08;
    const startedAt = nowSec - Math.round(rnd() * 30 * 24 * 3600);
    const finishedAt = percent >= 100 ? startedAt + 3600 : 0;

    const views: string[] = [];
    if (percent >= 100 && (status === "seeding" || status === "completed"))
      views.push("seeding");
    if (percent < 100 && (status === "downloading" || status === "stalled"))
      views.push("leeching");

    torrents.push({
      hash,
      name,
      size,
      bytesDone,
      percent,
      status,
      statusMsg,
      errorKind,
      seedsConnected,
      peersConnected,
      seedsSwarm,
      peersSwarm,
      downRate,
      upRate,
      etaSeconds,
      ratio: Math.round((rnd() * 5 + 0.1) * 100) / 100,
      label,
      trackerHost,
      savePath: `/srv/downloads/${name}`,
      priority: 2,
      isPrivate,
      throttleName: "",
      downRateLimit: null,
      upRateLimit: null,
      startedAt,
      finishedAt,
      views,
    });
  }

  // Sort by hash for deterministic snapshot order before UI sorts
  torrents.sort((a, b) => a.hash.localeCompare(b.hash));

  const freeSpace = 412 * GIB;
  const diskSize = 1114 * GIB;
  const globals: GlobalStats = {
    downRate: downRateSum,
    upRate: upRateSum,
    downRateLimit: 0,
    upRateLimit: 0,
    dhtNodes: 387,
    freeSpace,
    diskSize,
    turtleActive: false,
  };

  return {
    revision: 1,
    torrents,
    globals,
    connection: {
      phase: "connected",
      endpoint: "unix:/Users/you/.rtorrent/rpc.socket",
      daemonVersion: "0.9.8",
      error: null,
      retryInSeconds: null,
    },
  };
}

/** Small helper for benchmarks: time selectVisible + reconcile sort. */
export function largeFixtureHashes(s: Snapshot): string[] {
  return s.torrents.map((t) => t.hash);
}
