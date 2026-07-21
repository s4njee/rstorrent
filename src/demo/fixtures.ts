/**
 * Fixture data for the browser demo (mirrors the Rust MockClient's ten
 * torrents). Dev-only — this module is imported solely by the demo entry, never
 * by the Tauri app.
 */

import type {
  DaemonHealth,
  DetailPayload,
  GlobalStats,
  LogEntry,
  Settings,
  Snapshot,
  Statistics,
  Status,
  TorrentDto,
} from "../ipc/types";

const GIB = 1_073_741_824;
const MIB = 1_048_576;
const KIB = 1_024;
const NOW = Math.floor(Date.UTC(2026, 6, 20, 16, 0, 0) / 1000);

function t(
  hash: string,
  name: string,
  size: number,
  percent: number,
  status: Status,
  down: number,
  up: number,
  ratio: number,
  label: string,
  tracker: string,
  seeds: number,
  peers: number,
  conn: number,
  statusMsg = "",
): TorrentDto {
  const complete = percent >= 100;
  const views = complete && status === "seeding" ? ["seeding"] : [];
  if (!complete && (status === "downloading" || status === "stalled"))
    views.push("leeching");
  return {
    hash,
    name,
    size,
    bytesDone: Math.round((size * percent) / 100),
    percent,
    status,
    statusMsg,
    seedsConnected: status === "downloading" ? Math.round(conn * 0.6) : 0,
    peersConnected: conn,
    seedsSwarm: seeds,
    peersSwarm: peers,
    downRate: down,
    upRate: up,
    etaSeconds:
      down > 0 && !complete
        ? Math.round((size * (100 - percent)) / 100 / down)
        : null,
    ratio,
    label,
    trackerHost: tracker,
    savePath: `/srv/downloads/${name}`,
    priority: 2,
    isPrivate: false,
    throttleName: "",
    downRateLimit: null,
    upRateLimit: null,
    startedAt: NOW - (complete ? 26 : 5) * 3600,
    finishedAt: complete ? NOW - 3 * 3600 : 0,
    views,
  };
}

const torrents: TorrentDto[] = [
  t(
    "A1",
    "ubuntu-24.04.2-desktop-amd64.iso",
    Math.round(5.8 * GIB),
    100,
    "seeding",
    0,
    Math.round(1.2 * MIB),
    2.41,
    "linux-iso",
    "torrent.ubuntu.com",
    142,
    87,
    34,
  ),
  t(
    "B2",
    "debian-12.9.0-amd64-netinst.iso",
    Math.round(631 * MIB),
    100,
    "seeding",
    0,
    Math.round(214 * KIB),
    3.87,
    "linux-iso",
    "bttracker.debian.org",
    98,
    12,
    10,
  ),
  t(
    "C3",
    "Fedora-Workstation-Live-x86_64-41-1.4.iso",
    Math.round(2.3 * GIB),
    67.4,
    "downloading",
    Math.round(8.4 * MIB),
    Math.round(620 * KIB),
    0.19,
    "linux-iso",
    "torrent.fedoraproject.org",
    34,
    12,
    30,
  ),
  t(
    "D4",
    "archlinux-2026.07.01-x86_64.iso",
    Math.round(1.1 * GIB),
    23.1,
    "downloading",
    Math.round(1.1 * MIB),
    Math.round(88 * KIB),
    0.04,
    "linux-iso",
    "tracker.archlinux.org",
    18,
    6,
    12,
  ),
  t(
    "E5",
    "linuxmint-22.1-cinnamon-64bit.iso",
    Math.round(2.8 * GIB),
    45.2,
    "paused",
    0,
    0,
    0.11,
    "linux-iso",
    "linuxtracker.org",
    63,
    28,
    0,
  ),
  t(
    "F6",
    "Big.Buck.Bunny.2008.2160p.mkv",
    Math.round(7.9 * GIB),
    100,
    "seeding",
    0,
    Math.round(980 * KIB),
    5.02,
    "video",
    "tracker.blender.org",
    211,
    140,
    60,
  ),
  t(
    "G7",
    "Sintel.2010.2160p.mkv",
    Math.round(5.1 * GIB),
    91.8,
    "downloading",
    Math.round(2.9 * MIB),
    Math.round(410 * KIB),
    0.44,
    "video",
    "tracker.blender.org",
    26,
    9,
    20,
  ),
  t(
    "H8",
    "openSUSE-Tumbleweed-DVD-x86_64.iso",
    Math.round(4.4 * GIB),
    12.0,
    "stalled",
    0,
    0,
    0.01,
    "linux-iso",
    "opensuse.org",
    0,
    2,
    2,
  ),
  t(
    "I9",
    "raspios-bookworm-arm64-full.img.xz",
    Math.round(2.7 * GIB),
    100,
    "paused",
    0,
    0,
    1.08,
    "sbc",
    "downloads.raspberrypi.org",
    57,
    16,
    0,
  ),
  t(
    "J10",
    "Cosmos.Laundromat.2015.4K.mkv",
    Math.round(3.2 * GIB),
    66.7,
    "error",
    0,
    0,
    0.31,
    "video",
    "tracker.blender.org",
    0,
    0,
    0,
    'Tracker: [Failure reason "unregistered torrent"]',
  ),
];

const globals: GlobalStats = {
  downRate: torrents.reduce((n, x) => n + x.downRate, 0),
  upRate: torrents.reduce((n, x) => n + x.upRate, 0),
  downRateLimit: 0,
  upRateLimit: Math.round(5 * MIB),
  dhtNodes: 387,
  freeSpace: 412 * GIB,
  turtleActive: false,
};

export const snapshot: Snapshot = {
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

/** A partial bitfield for the pieces bar (leading run + a scattered edge). */
function bitfield(chunks: number, done: number): string {
  const bits: boolean[] = Array.from({ length: chunks }, (_, i) => {
    const solid = Math.floor(done * 0.85);
    if (i < solid) return true;
    return i < done && i % 3 === 0;
  });
  let hex = "";
  for (let byte = 0; byte < bits.length; byte += 8) {
    let v = 0;
    for (let o = 0; o < 8; o++) if (bits[byte + o]) v |= 0x80 >> o;
    hex += v.toString(16).padStart(2, "0");
  }
  return hex;
}

/** Detail payload (pieces bar) for a torrent's General tab, sized to its own
 *  completion so the bar matches the row. */
export function piecesDetail(hash: string): DetailPayload {
  const percent = torrents.find((x) => x.hash === hash)?.percent ?? 67.4;
  const chunks = 4700;
  const done = Math.round((chunks * percent) / 100);
  return {
    hash,
    tab: "general",
    pieces: {
      sizeChunks: chunks,
      completedChunks: done,
      chunkSize: 512 * KIB,
      bitfield: bitfield(chunks, done),
    },
  };
}

export const statistics: Statistics = {
  sessionDown: Math.round(1.6 * GIB),
  sessionUp: Math.round(312 * MIB),
  allTimeDown: Math.round(184 * GIB),
  allTimeUp: Math.round(302 * GIB),
  allTimeRatio: 1.64,
  sessionWaste: Math.round(184 * MIB),
  connectedPeers: 168,
  cacheHitPct: 96.4,
  bufferSize: Math.round(128 * MIB),
  cacheOverloadPct: 0,
  queuedIo: 3,
};

export const daemonHealth: DaemonHealth = {
  clientVersion: "0.9.8",
  apiVersion: "11",
  sessionPath: "/Users/you/.rtorrent/session",
  memoryMax: 4 * GIB,
  memoryCurrent: Math.round(128 * MIB),
  openSockets: 214,
  maxOpenSockets: 3000,
  maxOpenFiles: 1024,
  httpMaxOpen: 128,
};

export const log: LogEntry[] = [
  {
    time: Date.now() - 60000,
    level: "info",
    message: "connected to rtorrent",
    hash: null,
  },
  {
    time: Date.now() - 40000,
    level: "info",
    message: "added torrent from Fedora-Workstation-Live.iso",
    hash: "C3",
  },
  {
    time: Date.now() - 20000,
    level: "warn",
    message: 'Tracker: [Failure reason "unregistered torrent"]',
    hash: "J10",
  },
];

/** A full Settings object — a clean HTTP endpoint stand-in for the demo. */
export const settings: Settings = {
  transport: {
    kind: "http",
    url: "https://seedbox.example.com/RPC2",
    username: "demo",
  },
  pollMs: 1000,
  stallWindowS: 30,
  defaultSavePath: "/srv/downloads",
  showAddDialog: true,
  confirmOnRemove: true,
  downLimitKb: 0,
  upLimitKb: 0,
  portRange: "6881-6899",
  dhtEnabled: false,
  watchFolder: "",
  completionNotificationExcludedLabels: [],
  torrentThrottles: [],
  globalSeedGoal: { stopRatio: 2, seedHours: 0 },
  labelSeedGoals: [],
  encryption: "prefer",
  pexEnabled: true,
  proxyAddress: "",
  proxyTrackerHttp: false,
  bindAddress: "",
  localAddress: "",
  maxPeers: 0,
  maxUploadsGlobal: 0,
  maxDownloadsGlobal: 0,
  maxActiveDownloads: 0,
  labelDefaults: [],
  watchFolders: [],
  runOnComplete: "",
  seedGoalAction: "stop",
  turtleDownKb: 500,
  turtleUpKb: 100,
  turtleEnabled: false,
  turtleSchedule: { enabled: false, startMin: 120, endMin: 480, days: [] },
  connectionProfiles: [
    {
      name: "seedbox",
      transport: {
        kind: "http",
        url: "https://seedbox.example.com/RPC2",
        username: "demo",
      },
    },
    {
      name: "local",
      transport: {
        kind: "unixSocket",
        path: "/Users/you/.rtorrent/rpc.socket",
      },
    },
  ],
  rssFeeds: [],
  rssRules: [],
  rssPollMinutes: 15,
  mock: false,
};
