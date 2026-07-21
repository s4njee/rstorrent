import type { Settings } from "./types";

/**
 * A minimal, type-complete {@link Settings} for the web host.
 *
 * The web UI barely reads these — the server owns the real transport/paths, and
 * delete-data gating uses the backend capability, not `transport`. This exists so
 * the settings store populates and any shared component that peeks at settings
 * doesn't crash. `transport` is a non-local placeholder (the safe default: no
 * delete-data until WE3 wires the capability). Fuller web preferences are v2.
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
    labelDefaults: [],
    watchFolders: [],
    runOnComplete: "",
    seedGoalAction: "stop",
    turtleDownKb: 0,
    turtleUpKb: 0,
    turtleEnabled: false,
    turtleSchedule: { enabled: false, startMin: 0, endMin: 0, days: [] },
    connectionProfiles: [],
    rssFeeds: [],
    rssRules: [],
    rssPollMinutes: 15,
    mock: false,
    ...overrides,
  };
}
