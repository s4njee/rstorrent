import type { BandwidthRule, Settings } from "./types";

/**
 * A minimal, type-complete {@link Settings} for the web host.
 *
 * The web UI barely reads these — the server owns the real transport/paths, and
 * delete-data gating uses the backend capability, not `transport`. This exists so
 * the settings store populates and any shared component that peeks at settings
 * doesn't crash. `transport` is a non-local placeholder (the safe default: no
 * delete-data). The Settings route (WC7) reads the daemon's real values through
 * [`getConfig`] instead.
 */
export function webSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    transport: { kind: "http", url: "", username: "" },
    pollMs: 1000,
    stallWindowS: 30,
    defaultSavePath: "",
    showAddDialog: true,
    confirmOnRemove: true,
    downLimitKb: 0,
    upLimitKb: 0,
    portRange: "6881-6899",
    dhtEnabled: false,
    watchFolder: "",
    completionNotificationExcludedLabels: [],
    torrentThrottles: [],
    globalSeedGoal: { stopRatio: 0, seedHours: 0 },
    labelSeedGoals: [],
    encryption: "allow",
    pexEnabled: true,
    proxyAddress: "",
    proxyTrackerHttp: false,
    bindAddress: "",
    localAddress: "",
    maxPeers: 0,
    maxUploadsGlobal: 0,
    maxDownloadsGlobal: 0,
    maxActiveDownloads: 0,
    maxActiveUploads: 0,
    maxActiveTorrents: 0,
    queueSlowLimitKbs: 0,
    labelDefaults: [],
    incompleteDir: "",
    moveRules: [],
    collisionPolicy: "error",
    bandwidthRules: [],
    watchFolders: [],
    runOnComplete: "",
    seedGoalAction: "stop",
    turtleDownKb: 0,
    turtleUpKb: 0,
    turtleEnabled: false,
    turtleSchedule: { enabled: false, startMin: 0, endMin: 0, days: [] },
    schedule: { windows: [], tempOverride: null },
    connectionProfiles: [],
    rssFeeds: [],
    rssRules: [],
    rssPollMinutes: 15,
    mock: false,
    ...overrides,
  };
}

// --- The Settings route's data (WC7) -----------------------------------------

/** A Settings-route section, in the design's order. */
export type SettingsSection =
  | "general"
  | "connection"
  | "bandwidth"
  | "queue"
  | "directories"
  | "labels"
  | "interface"
  | "advanced";

/** How a daemon key is edited, mirroring the server's `KindDesc`. */
export type ConfigKind =
  | { type: "int"; unit: string; min: number; max: number }
  | { type: "str"; maxLen: number }
  | { type: "choice"; options: string[] }
  | { type: "bool" }
  | { type: "readonly" };

/** One daemon key with its current value, as `GET /api/config` returns it. */
export interface ConfigRow {
  id: string;
  label: string;
  /** The rtorrent key, shown in faint text under the label. */
  key: string;
  section: SettingsSection;
  kind: ConfigKind;
  hint: string;
  /** `null` when this daemon build does not expose the key. */
  value: string | null;
  available: boolean;
}

/** The server-owned interface preferences. */
export interface UiPreferences {
  theme?: string;
  accent?: string;
  density?: string;
  dateFormat?: string;
  rateFormat?: string;
  columns?: unknown;
}

/** Facts about the server, shown in the General section. */
export interface ServerInfo {
  displayName: string;
  savePath: string;
  listen: string;
  pollMs: number;
  authMode: "password" | "none";
  mock: boolean;
  /** Bandwidth rules for the precedence display (V3-18); enforcement is desktop-side. */
  bandwidthRules?: BandwidthRule[];
}

export interface SettingsResponse {
  ui: UiPreferences;
  server: ServerInfo;
}

/** The `.rtorrent.rc` the daemon runs, and its managed block if any. */
export interface AdvancedInfo {
  path: string;
  exists: boolean;
  block: string | null;
}

/** The outcome of a config save: what took, and what did not. */
export interface SaveReport {
  applied: string[];
  failed: Array<{ id: string; error: string }>;
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      "X-Rstorrent": "1",
      ...(init?.headers ?? {}),
    },
  });
  const text = await res.text();
  const body = text ? (JSON.parse(text) as unknown) : null;
  if (!res.ok) {
    const message =
      body && typeof body === "object" && "error" in body
        ? String((body as { error: unknown }).error)
        : `request failed (${res.status})`;
    throw new Error(message);
  }
  return body as T;
}

/** Every daemon key the Settings route shows, with its live value. */
export function getConfig(): Promise<ConfigRow[]> {
  return api<ConfigRow[]>("/api/config");
}

/** Write the changed keys; resolves with per-key outcomes, never a false success. */
export function saveConfig(
  values: Record<string, string>,
): Promise<SaveReport> {
  return api<SaveReport>("/api/config", {
    method: "POST",
    body: JSON.stringify({ values }),
  });
}

/** The server-owned interface preferences and the server facts. */
export function getSettings(): Promise<SettingsResponse> {
  return api<SettingsResponse>("/api/settings");
}

/** Merge and persist interface preferences. */
export function saveSettings(ui: UiPreferences): Promise<UiPreferences> {
  return api<UiPreferences>("/api/settings", {
    method: "PUT",
    body: JSON.stringify(ui),
  });
}

/** Change the login password (takes effect without a restart). */
export async function changePassword(
  current: string,
  next: string,
): Promise<void> {
  await api<null>("/api/settings/password", {
    method: "POST",
    body: JSON.stringify({ current, new: next }),
  });
}

/** The daemon's managed `.rtorrent.rc` block. */
export function getAdvanced(): Promise<AdvancedInfo> {
  return api<AdvancedInfo>("/api/settings/advanced");
}

// --- The Stats route's data (WC8) --------------------------------------------

/** One rate sample in the server's 60-minute window. */
export interface StatsSample {
  /** Unix milliseconds. */
  at: number;
  down: number;
  up: number;
}

/** Torrent counts by lifecycle state. */
export interface StateCounts {
  total: number;
  downloading: number;
  seeding: number;
  completed: number;
  stopped: number;
  checking: number;
  errored: number;
  stalled: number;
}

/** One configured volume; `free`/`total` are null when it cannot be read. */
export interface VolumeStat {
  path: string;
  free: number | null;
  total: number | null;
}

export interface StatsPayload {
  history: StatsSample[];
  pollMs: number;
  sessionDown: number;
  sessionUp: number;
  sessionRatio: number | null;
  counts: StateCounts;
  uptimeSeconds: number;
  dhtNodes: number;
  portRange: string | null;
  /** The external port check (WC11) owns this; null means "not checked". */
  portStatus: boolean | null;
  volumes: VolumeStat[];
}

/** The Stats route's data: history, session totals, counts and volumes. */
export function getStats(): Promise<StatsPayload> {
  return api<StatsPayload>("/api/stats");
}

// --- Library search (V3-12) --------------------------------------------------

export interface FileSearchResults {
  /** Hashes whose contained filenames match the query. */
  hashes: string[];
  /** How many torrents the server has indexed so far. */
  indexed: number;
}

/**
 * Filename search across the server's lazily built index. The browser already
 * matches name/label/tags/tracker/path locally; this covers the one field it
 * cannot compute without a daemon call per torrent.
 */
export function searchFiles(query: string): Promise<FileSearchResults> {
  return api<FileSearchResults>(`/api/search?q=${encodeURIComponent(query)}`);
}
