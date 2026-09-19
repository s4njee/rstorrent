/**
 * Preferences window (design screen 04).
 *
 * Left nav selects a section; the right panel edits a working copy of settings.
 * Apply persists via the `apply_settings` command and refreshes the settings
 * store (which also live-reconnects the poller if the transport changed). Web UI
 * is shown disabled (v2). Some controls are intentionally disabled where the
 * backend doesn't support them yet (see plan.md §10) with an explanatory tip.
 */

import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useUi } from "../../store/ui";
import {
  useSettings,
  isLocalhost,
  isInsecureCredentialed,
} from "../../store/settings";
import { useTorrents } from "../../store/torrents";
import {
  applySettings,
  clearHttpPassword,
  hasHttpPassword,
  rssDownload,
  rssExportSeen,
  rssFetch,
  rssImportSeen,
  rssTest,
  setHttpPassword,
  testConnection,
  type RssTestRow,
} from "../../ipc/commands";
import type {
  BandwidthRule,
  CollisionPolicy,
  ConnectionProfile,
  EncryptionMode,
  FeedItem,
  LabelDefault,
  MoveRule,
  RssFeed,
  RssRule,
  SchedWindow,
  Settings,
  TempOverride,
  Transport,
  WatchFolder,
} from "../../ipc/types";
import {
  formatTransition,
  importLegacy,
  nextChange,
} from "../../utils/schedule";
import { credentialStoreName, isWindows } from "../../platform";
import { ModalBase, Button } from "./ModalBase";
import { Checkbox } from "./Checkbox";
import forms from "./forms.module.css";
import styles from "./PreferencesDialog.module.css";

type Section =
  | "behavior"
  | "downloads"
  | "connection"
  | "speed"
  | "bittorrent"
  | "network"
  | "rss"
  | "webui"
  | "advanced";

const NAV: Array<{
  id: Section;
  label: string;
  icon: string;
  disabled?: boolean;
}> = [
  { id: "behavior", label: "Behavior", icon: "⚙" },
  { id: "downloads", label: "Downloads", icon: "⬇" },
  { id: "connection", label: "Connection", icon: "⇄" },
  { id: "speed", label: "Speed", icon: "⏱" },
  { id: "bittorrent", label: "BitTorrent", icon: "⦿" },
  { id: "network", label: "Network", icon: "⇅" },
  { id: "rss", label: "RSS", icon: "⤳" },
  { id: "webui", label: "Web UI", icon: "⧉", disabled: true },
  { id: "advanced", label: "Advanced", icon: "⚑" },
];

/** Encryption presets shown in the Network pane (D7). */
const ENCRYPTION_OPTIONS: Array<{ value: EncryptionMode; label: string }> = [
  { value: "disabled", label: "Disabled (plaintext only)" },
  { value: "allow", label: "Allow (accept encrypted)" },
  { value: "prefer", label: "Prefer (try encrypted, retry)" },
  { value: "require", label: "Require encryption" },
];

export function PreferencesDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const settings = useSettings((s) => s.settings);
  const torrents = useTorrents((s) => s.torrents);

  const [section, setSection] = useState<Section>("downloads");
  const [draft, setDraft] = useState<Settings | null>(settings);
  const [testMsg, setTestMsg] = useState<{ ok: boolean; text: string } | null>(
    null,
  );
  // Typed-but-unsaved remote password. Never populated from the Keychain — the
  // secret does not come back into the webview; we only learn whether one exists.
  const [password, setPassword] = useState("");
  const [passwordSaved, setPasswordSaved] = useState(false);

  // Load the working copy once settings are available.
  useEffect(() => {
    if (settings && !draft) setDraft(settings);
  }, [settings, draft]);

  // Ask (only) whether a password is on file for this endpoint, to drive the
  // "saved" hint. Re-runs when the endpoint identity changes.
  const httpUrl = draft?.transport?.kind === "http" ? draft.transport.url : "";
  const httpUser =
    draft?.transport?.kind === "http" ? draft.transport.username : "";
  useEffect(() => {
    if (!httpUrl) {
      setPasswordSaved(false);
      return;
    }
    let cancelled = false;
    void hasHttpPassword(httpUrl, httpUser)
      .then((saved) => !cancelled && setPasswordSaved(saved))
      .catch(() => !cancelled && setPasswordSaved(false));
    return () => {
      cancelled = true;
    };
  }, [httpUrl, httpUser]);

  const labels = useMemo(() => {
    const known = torrents.map((torrent) => torrent.label).filter(Boolean);
    const excluded = draft?.completionNotificationExcludedLabels ?? [];
    const overrides =
      draft?.labelSeedGoals.map((goal) => goal.label).filter(Boolean) ?? [];
    return [...new Set([...known, ...excluded, ...overrides])].sort((a, b) =>
      a.localeCompare(b),
    );
  }, [
    torrents,
    draft?.completionNotificationExcludedLabels,
    draft?.labelSeedGoals,
  ]);

  if (!draft) {
    return (
      <ModalBase
        title="Preferences"
        width={860}
        onCancel={closeDialog}
        footer={
          <Button variant="secondary" onClick={closeDialog}>
            Close
          </Button>
        }
      >
        <div className={forms.meta}>loading…</div>
      </ModalBase>
    );
  }

  const patch = (p: Partial<Settings>) => setDraft({ ...draft, ...p });
  const setTransport = (t: Transport) => patch({ transport: t });

  const apply = async () => {
    // Save the password first: if the Keychain refuses, surface it rather than
    // persisting a transport whose credentials would then be missing.
    if (draft.transport.kind === "http" && password) {
      try {
        await setHttpPassword(
          draft.transport.url,
          draft.transport.username,
          password,
        );
      } catch (err) {
        setSection("connection");
        setTestMsg({ ok: false, text: String(err) });
        return;
      }
    }
    const saved = await applySettings(draft);
    useSettings.getState().set(saved);
    closeDialog();
  };

  const browse = async () => {
    const dir = await open({ directory: true });
    if (typeof dir === "string") patch({ defaultSavePath: dir });
  };

  const runTest = async () => {
    setTestMsg({ ok: true, text: "testing…" });
    try {
      const version = await testConnection(draft.transport, password);
      setTestMsg({ ok: true, text: `connected — rtorrent ${version}` });
    } catch (e) {
      setTestMsg({ ok: false, text: String(e) });
    }
  };

  return (
    <ModalBase
      title="Preferences"
      width={860}
      noPad
      onCancel={closeDialog}
      onPrimary={apply}
      footer={
        <>
          <Button variant="secondary" onClick={closeDialog}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => void apply()}>
            Apply
          </Button>
        </>
      }
    >
      <div className={styles.layout}>
        <div className={styles.nav}>
          {NAV.map((n) => (
            <div
              key={n.id}
              className={`${styles.navItem} ${section === n.id ? styles.active : ""} ${
                n.disabled ? styles.disabled : ""
              }`}
              title={n.disabled ? "planned for v2" : undefined}
              onClick={() => !n.disabled && setSection(n.id)}
            >
              <span className={styles.navIcon}>{n.icon}</span>
              {n.label}
              {n.disabled && <span className={styles.badge}>v2</span>}
            </div>
          ))}
        </div>

        <div className={styles.panel}>
          {section === "behavior" && (
            <Group title="Behavior">
              <Checkbox
                checked={draft.showAddDialog}
                onChange={(v) => patch({ showAddDialog: v })}
                label="Show the add-torrent dialog (uncheck to add instantly with defaults)"
              />
              <Checkbox
                checked={draft.confirmOnRemove}
                onChange={(v) => patch({ confirmOnRemove: v })}
                label="Confirm before removing torrents"
              />
              <div className={styles.notificationLabels}>
                <span className={forms.fieldLabel} style={{ width: "auto" }}>
                  Exclude labels from completion notifications
                </span>
                <span className={forms.meta}>
                  Completed torrents with checked labels will stay silent.
                </span>
                {labels.length === 0 ? (
                  <span className={forms.meta}>
                    No labels are currently known.
                  </span>
                ) : (
                  <div className={styles.labelGrid}>
                    {labels.map((label) => (
                      <Checkbox
                        key={label}
                        checked={draft.completionNotificationExcludedLabels.includes(
                          label,
                        )}
                        onChange={(excluded) => {
                          const next = new Set(
                            draft.completionNotificationExcludedLabels,
                          );
                          if (excluded) next.add(label);
                          else next.delete(label);
                          patch({
                            completionNotificationExcludedLabels: [
                              ...next,
                            ].sort((a, b) => a.localeCompare(b)),
                          });
                        }}
                        label={label}
                      />
                    ))}
                  </div>
                )}
              </div>
            </Group>
          )}

          {section === "downloads" && (
            <DownloadsSection draft={draft} patch={patch} browse={browse} />
          )}

          {section === "connection" && (
            <ConnectionSection
              transport={draft.transport}
              profiles={draft.connectionProfiles}
              onProfiles={(connectionProfiles) => patch({ connectionProfiles })}
              pollMs={draft.pollMs}
              stallWindowS={draft.stallWindowS}
              onTransport={setTransport}
              onPoll={(pollMs) => patch({ pollMs })}
              onStall={(stallWindowS) => patch({ stallWindowS })}
              onTest={() => void runTest()}
              testMsg={testMsg}
              password={password}
              onPassword={setPassword}
              passwordSaved={passwordSaved}
              onForgetPassword={() => {
                void clearHttpPassword(httpUrl, httpUser)
                  .then(() => {
                    setPasswordSaved(false);
                    setPassword("");
                  })
                  .catch((err) => setTestMsg({ ok: false, text: String(err) }));
              }}
            />
          )}

          {section === "speed" && (
            <>
              <Group title="Speed Limits (KiB/s, 0 = unlimited)">
                <NumberRow
                  label="Global download limit"
                  value={draft.downLimitKb}
                  onChange={(downLimitKb) => patch({ downLimitKb })}
                />
                <NumberRow
                  label="Global upload limit"
                  value={draft.upLimitKb}
                  onChange={(upLimitKb) => patch({ upLimitKb })}
                />
              </Group>

              <Group title="Connection Limits (0 = default / unlimited)">
                <NumberRow
                  label="Max peers per torrent"
                  value={draft.maxPeers}
                  onChange={(maxPeers) => patch({ maxPeers })}
                />
                <NumberRow
                  label="Global upload slots"
                  value={draft.maxUploadsGlobal}
                  onChange={(maxUploadsGlobal) => patch({ maxUploadsGlobal })}
                />
                <NumberRow
                  label="Global download slots"
                  value={draft.maxDownloadsGlobal}
                  onChange={(maxDownloadsGlobal) =>
                    patch({ maxDownloadsGlobal })
                  }
                />
              </Group>

              <Group title="Queue">
                <NumberRow
                  label="Max active downloads"
                  value={draft.maxActiveDownloads}
                  onChange={(maxActiveDownloads) =>
                    patch({ maxActiveDownloads })
                  }
                />
                <NumberRow
                  label="Max active uploads"
                  value={draft.maxActiveUploads}
                  onChange={(maxActiveUploads) =>
                    patch({ maxActiveUploads })
                  }
                />
                <NumberRow
                  label="Max active torrents"
                  value={draft.maxActiveTorrents}
                  onChange={(maxActiveTorrents) =>
                    patch({ maxActiveTorrents })
                  }
                />
                <NumberRow
                  label="Ignore slow below (KiB/s)"
                  value={draft.queueSlowLimitKbs}
                  onChange={(queueSlowLimitKbs) =>
                    patch({ queueSlowLimitKbs })
                  }
                />
                <span className={forms.meta}>
                  0 = no limit. Held-back torrents pause loaded and read
                  "Queued"; force-started torrents and ones slower than the
                  floor are never held back and never counted.
                </span>
              </Group>

              <TurtleGroup draft={draft} patch={patch} />
              <SchedulerGroup draft={draft} patch={patch} />
              <BandwidthRulesGroup draft={draft} patch={patch} />
            </>
          )}

          {section === "network" && (
            <NetworkSection draft={draft} patch={patch} />
          )}

          {section === "bittorrent" && (
            <>
              <Group title="BitTorrent">
                <div className={forms.field}>
                  <span className={forms.fieldLabel} style={{ width: 120 }}>
                    Listen port range
                  </span>
                  <input
                    className={forms.input}
                    value={draft.portRange}
                    onChange={(e) =>
                      patch({ portRange: e.currentTarget.value })
                    }
                    placeholder="6881-6899"
                    spellCheck={false}
                  />
                </div>
                <Checkbox
                  checked={draft.dhtEnabled}
                  onChange={(dhtEnabled) => patch({ dhtEnabled })}
                  label="Enable DHT (distributed hash table)"
                />
                <span className={styles.warn}>
                  Port and DHT changes may require an rtorrent restart to take
                  full effect.
                </span>
              </Group>

              <Group title="Seeding limits">
                <div className={forms.field}>
                  <span className={forms.fieldLabel} style={{ width: 140 }}>
                    When goal reached
                  </span>
                  <select
                    className={forms.input}
                    value={draft.seedGoalAction}
                    aria-label="Action when the seed goal is reached"
                    onChange={(e) =>
                      patch({
                        seedGoalAction: e.currentTarget
                          .value as Settings["seedGoalAction"],
                      })
                    }
                  >
                    <option value="stop">Stop seeding</option>
                    <option value="remove">Remove torrent</option>
                    <option value="removeData">Remove torrent and data</option>
                  </select>
                </div>
                <div className={styles.seedLimitLine}>
                  <span>Stop at ratio</span>
                  <SeedLimitInput
                    ariaLabel="Global stop ratio"
                    value={draft.globalSeedGoal.stopRatio}
                    onChange={(stopRatio) =>
                      patch({
                        globalSeedGoal: {
                          ...draft.globalSeedGoal,
                          stopRatio,
                        },
                      })
                    }
                  />
                  <span>or after</span>
                  <SeedLimitInput
                    ariaLabel="Global seeding hours"
                    value={draft.globalSeedGoal.seedHours}
                    onChange={(seedHours) =>
                      patch({
                        globalSeedGoal: {
                          ...draft.globalSeedGoal,
                          seedHours,
                        },
                      })
                    }
                  />
                  <span>hours seeding</span>
                </div>
                <span className={forms.meta}>
                  Empty or 0 disables a rule. The first reached rule stops the
                  torrent.
                </span>

                <div className={styles.seedOverrideBlock}>
                  <div className={styles.seedOverrideHeader}>
                    <span>Label override</span>
                    <span>Ratio</span>
                    <span>Hours</span>
                    <span />
                  </div>
                  {draft.labelSeedGoals.map((goal, index) => {
                    const usedLabels = new Set(
                      draft.labelSeedGoals
                        .filter((_, row) => row !== index)
                        .map((row) => row.label),
                    );
                    return (
                      <div
                        className={styles.seedOverrideRow}
                        key={`${goal.label}-${index}`}
                      >
                        <select
                          className={forms.input}
                          value={goal.label}
                          aria-label="Seed goal override label"
                          onChange={(event) => {
                            const next = [...draft.labelSeedGoals];
                            next[index] = {
                              ...goal,
                              label: event.currentTarget.value,
                            };
                            patch({ labelSeedGoals: next });
                          }}
                        >
                          {labels
                            .filter(
                              (label) =>
                                label === goal.label || !usedLabels.has(label),
                            )
                            .map((label) => (
                              <option key={label} value={label}>
                                {label}
                              </option>
                            ))}
                        </select>
                        <SeedLimitInput
                          ariaLabel={`${goal.label} stop ratio`}
                          value={goal.stopRatio}
                          onChange={(stopRatio) => {
                            const next = [...draft.labelSeedGoals];
                            next[index] = { ...goal, stopRatio };
                            patch({ labelSeedGoals: next });
                          }}
                        />
                        <SeedLimitInput
                          ariaLabel={`${goal.label} seeding hours`}
                          value={goal.seedHours}
                          onChange={(seedHours) => {
                            const next = [...draft.labelSeedGoals];
                            next[index] = { ...goal, seedHours };
                            patch({ labelSeedGoals: next });
                          }}
                        />
                        <button
                          className={styles.removeOverride}
                          title={`Remove ${goal.label} override`}
                          aria-label={`Remove ${goal.label} override`}
                          onClick={() =>
                            patch({
                              labelSeedGoals: draft.labelSeedGoals.filter(
                                (_, row) => row !== index,
                              ),
                            })
                          }
                        >
                          ×
                        </button>
                      </div>
                    );
                  })}
                  {draft.labelSeedGoals.length === 0 && (
                    <span className={forms.meta}>No label overrides.</span>
                  )}
                </div>

                {(() => {
                  const used = new Set(
                    draft.labelSeedGoals.map((goal) => goal.label),
                  );
                  const nextLabel = labels.find((label) => !used.has(label));
                  return (
                    <button
                      className={styles.addOverride}
                      disabled={!nextLabel}
                      title={
                        nextLabel
                          ? undefined
                          : "No unused torrent labels are currently known"
                      }
                      onClick={() => {
                        if (!nextLabel) return;
                        patch({
                          labelSeedGoals: [
                            ...draft.labelSeedGoals,
                            { label: nextLabel, stopRatio: 0, seedHours: 0 },
                          ],
                        });
                      }}
                    >
                      + Add label override
                    </button>
                  );
                })()}
                <span className={forms.meta}>
                  A row replaces the global goal; leave both fields empty for an
                  explicit no-limit label.
                </span>
              </Group>
            </>
          )}

          {section === "rss" && (
            <RssSection draft={draft} patch={patch} labels={labels} />
          )}

          {section === "webui" && (
            <Group title="Web UI">
              <span className={forms.meta}>Planned for a future version.</span>
            </Group>
          )}

          {section === "advanced" && (
            <Group title="Advanced">
              <Checkbox
                checked={draft.mock}
                onChange={(mock) => patch({ mock })}
                label="Mock mode (use built-in fixture torrents, no daemon)"
              />
              <span className={styles.warn}>
                Toggling mock mode reconnects the backend on Apply.
              </span>
            </Group>
          )}
        </div>
      </div>
    </ModalBase>
  );
}

/** A labeled group with the uppercase-dim section header. */
function Group({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className={forms.section} style={{ marginBottom: 10 }}>
        {title}
      </div>
      <div className={forms.col}>{children}</div>
    </div>
  );
}

/** A label + number input row. */
function NumberRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (n: number) => void;
}) {
  return (
    <div className={forms.field}>
      <span className={forms.fieldLabel} style={{ width: 180 }}>
        {label}
      </span>
      <input
        className={forms.input}
        type="number"
        min={0}
        value={value}
        onChange={(e) =>
          onChange(Math.max(0, Number(e.currentTarget.value) || 0))
        }
      />
    </div>
  );
}

function SeedLimitInput({
  ariaLabel,
  value,
  onChange,
}: {
  ariaLabel: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <input
      className={`${forms.input} ${styles.limitInput}`}
      type="number"
      min={0}
      step="0.1"
      inputMode="decimal"
      aria-label={ariaLabel}
      placeholder="—"
      value={value > 0 ? value : ""}
      onChange={(event) =>
        onChange(Math.max(0, Number(event.currentTarget.value) || 0))
      }
    />
  );
}

/** Network pane: encryption/PEX (D7), proxy (D8), bind/listen (D9). */
function NetworkSection({
  draft,
  patch,
}: {
  draft: Settings;
  patch: (p: Partial<Settings>) => void;
}) {
  return (
    <>
      <Group title="Encryption">
        <div className={forms.field}>
          <span className={forms.fieldLabel} style={{ width: 120 }}>
            Protocol encryption
          </span>
          <select
            className={forms.input}
            value={draft.encryption}
            aria-label="Protocol encryption preset"
            onChange={(e) =>
              patch({ encryption: e.currentTarget.value as EncryptionMode })
            }
          >
            {ENCRYPTION_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </div>
        <span className={forms.meta}>
          rtorrent doesn’t report its current encryption setting, so this shows
          the preset rstorrent last applied on Apply.
        </span>
        <Checkbox
          checked={draft.pexEnabled}
          onChange={(pexEnabled) => patch({ pexEnabled })}
          label="Enable peer exchange (PEX)"
        />
      </Group>

      <Group title="Proxy">
        <Checkbox
          checked={draft.proxyTrackerHttp}
          onChange={(proxyTrackerHttp) => patch({ proxyTrackerHttp })}
          label="Route tracker HTTP requests through a proxy"
        />
        <div className={forms.field}>
          <span className={forms.fieldLabel} style={{ width: 120 }}>
            Proxy address
          </span>
          <input
            className={forms.input}
            value={draft.proxyAddress}
            disabled={!draft.proxyTrackerHttp}
            onChange={(e) => patch({ proxyAddress: e.currentTarget.value })}
            placeholder="host:port"
            spellCheck={false}
          />
        </div>
        <span className={styles.warn}>
          Only HTTP tracker announces are proxied — UDP trackers and peer
          connections bypass it.
        </span>
      </Group>

      <Group title="Binding">
        <div className={forms.field}>
          <span className={forms.fieldLabel} style={{ width: 120 }}>
            Bind address
          </span>
          <input
            className={forms.input}
            value={draft.bindAddress}
            onChange={(e) => patch({ bindAddress: e.currentTarget.value })}
            placeholder="(all interfaces)"
            spellCheck={false}
          />
        </div>
        <div className={forms.field}>
          <span className={forms.fieldLabel} style={{ width: 120 }}>
            Reported address
          </span>
          <input
            className={forms.input}
            value={draft.localAddress}
            onChange={(e) => patch({ localAddress: e.currentTarget.value })}
            placeholder="(automatic)"
            spellCheck={false}
          />
        </div>
        <span className={forms.meta}>
          Bind to a VPN interface’s address so traffic dies with the tunnel
          instead of leaking. Clearing a bind takes effect after a daemon
          restart.
        </span>
      </Group>
    </>
  );
}

/** A bordered block grouping one watch folder's fields. */
const rowCard: React.CSSProperties = {
  border: "1px solid var(--border-row)",
  borderRadius: 4,
  padding: 8,
  display: "flex",
  flexDirection: "column",
  gap: 4,
};

const DAY_LABELS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/** Minutes-since-midnight ↔ an <input type="time"> "HH:MM" value. */
function minToTime(min: number): string {
  const m = ((min % 1440) + 1440) % 1440;
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}
function timeToMin(t: string): number {
  const [h, m] = t.split(":").map(Number);
  return (h || 0) * 60 + (m || 0);
}

/** Downloads pane: default path, incomplete dir + move rules (V3-14),
 *  run-on-complete (C13), watch folders (C12), per-label defaults (C11). */
function DownloadsSection({
  draft,
  patch,
  browse,
}: {
  draft: Settings;
  patch: (p: Partial<Settings>) => void;
  browse: () => void;
}) {
  const pickInto = async (apply: (dir: string) => void) => {
    const dir = await open({ directory: true });
    if (typeof dir === "string") apply(dir);
  };

  const setFolder = (i: number, next: Partial<WatchFolder>) =>
    patch({
      watchFolders: draft.watchFolders.map((f, idx) =>
        idx === i ? { ...f, ...next } : f,
      ),
    });
  const setDefault = (i: number, next: Partial<LabelDefault>) =>
    patch({
      labelDefaults: draft.labelDefaults.map((d, idx) =>
        idx === i ? { ...d, ...next } : d,
      ),
    });
  const setRule = (i: number, next: Partial<MoveRule>) =>
    patch({
      moveRules: draft.moveRules.map((r, idx) =>
        idx === i ? { ...r, ...next } : r,
      ),
    });

  return (
    <>
      <Group title="Saving Management">
        <span className={forms.fieldLabel} style={{ width: "auto" }}>
          Default save path
        </span>
        <div className={forms.field}>
          <input
            className={forms.input}
            value={draft.defaultSavePath}
            onChange={(e) => patch({ defaultSavePath: e.currentTarget.value })}
            spellCheck={false}
            aria-label="Default save path"
          />
          <button className={forms.browse} onClick={() => void browse()}>
            Browse…
          </button>
        </div>
      </Group>

      <Group title="On Completion">
        <span className={forms.fieldLabel} style={{ width: "auto" }}>
          Run a command when a torrent completes
        </span>
        <input
          className={forms.input}
          value={draft.runOnComplete}
          onChange={(e) => patch({ runOnComplete: e.currentTarget.value })}
          placeholder="(disabled)   e.g.  /usr/local/bin/on-done %N %F"
          spellCheck={false}
        />
        <span className={forms.meta}>
          Runs on this machine, directly (no shell). Tokens: %N name · %F save
          path · %H hash. Point it at a script for pipes or redirects.
        </span>
      </Group>

      <Group title="Incomplete Torrents + Move on Complete">
        <span className={forms.fieldLabel} style={{ width: "auto" }}>
          Keep incomplete torrents in
        </span>
        <div className={forms.field}>
          <input
            className={forms.input}
            value={draft.incompleteDir}
            onChange={(e) => patch({ incompleteDir: e.currentTarget.value })}
            placeholder="(disabled — downloads go straight to their save path)"
            spellCheck={false}
          />
          <button
            className={forms.browse}
            onClick={() => void pickInto((d) => patch({ incompleteDir: d }))}
          >
            Browse…
          </button>
        </div>
        <span className={forms.meta}>
          New downloads land here and move home on completion (recorded per
          torrent, crash-safe, local daemons only). Empty disables it.
        </span>
        <span className={forms.fieldLabel} style={{ width: "auto" }}>
          When the destination is taken
        </span>
        <select
          className={forms.input}
          style={{ maxWidth: 280 }}
          value={draft.collisionPolicy}
          aria-label="When the move destination is taken"
          onChange={(e) =>
            patch({ collisionPolicy: e.currentTarget.value as CollisionPolicy })
          }
        >
          <option value="error">Refuse the move (keep seeding in place)</option>
          <option value="auto-rename">Rename with (1), (2), …</option>
        </select>
      </Group>

      <Group title="Move-on-Complete Rules">
        {draft.moveRules.length === 0 && (
          <span className={forms.meta}>
            No rules — completions move to their recorded save path (or stay
            put). First matching tag wins, then label.
          </span>
        )}
        {draft.moveRules.map((r, i) => (
          <div className={forms.field} key={i}>
            <select
              className={forms.input}
              style={{ maxWidth: 96 }}
              value={r.tag != null ? "tag" : "label"}
              aria-label="Rule matches a tag or a label"
              onChange={(e) => {
                const kind = e.currentTarget.value;
                setRule(
                  i,
                  kind === "tag"
                    ? { tag: r.tag ?? r.label ?? "", label: undefined }
                    : { label: r.label ?? r.tag ?? "", tag: undefined },
                );
              }}
            >
              <option value="tag">tag</option>
              <option value="label">label</option>
            </select>
            <input
              className={forms.input}
              style={{ maxWidth: 140 }}
              value={r.tag ?? r.label ?? ""}
              onChange={(e) =>
                setRule(
                  i,
                  r.tag != null
                    ? { tag: e.currentTarget.value }
                    : { label: e.currentTarget.value },
                )
              }
              placeholder={r.tag != null ? "tag" : "label"}
              spellCheck={false}
            />
            <input
              className={forms.input}
              value={r.destination}
              onChange={(e) => setRule(i, { destination: e.currentTarget.value })}
              placeholder="destination"
              aria-label="Rule destination"
              spellCheck={false}
            />
            <button
              className={forms.browse}
              onClick={() =>
                void pickInto((dir) => setRule(i, { destination: dir }))
              }
            >
              Browse…
            </button>
            <button
              className={styles.removeOverride}
              title="Remove rule"
              aria-label="Remove move rule"
              onClick={() =>
                patch({ moveRules: draft.moveRules.filter((_, idx) => idx !== i) })
              }
            >
              ×
            </button>
          </div>
        ))}
        <button
          className={styles.addOverride}
          onClick={() =>
            patch({
              moveRules: [
                ...draft.moveRules,
                { tag: "", destination: "" },
              ],
            })
          }
        >
          + Add move rule
        </button>
      </Group>

      <Group title="Watch Folders (auto-add; takes effect on restart)">
        {draft.watchFolders.length === 0 && (
          <span className={forms.meta}>No watch folders.</span>
        )}
        {draft.watchFolders.map((f, i) => (
          <div key={i} style={rowCard}>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Folder
              </span>
              <input
                className={forms.input}
                value={f.path}
                onChange={(e) => setFolder(i, { path: e.currentTarget.value })}
                placeholder="folder to watch"
                spellCheck={false}
              />
              <button
                className={forms.browse}
                onClick={() => void pickInto((d) => setFolder(i, { path: d }))}
              >
                Browse…
              </button>
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Label
              </span>
              <input
                className={forms.input}
                value={f.label}
                onChange={(e) => setFolder(i, { label: e.currentTarget.value })}
                placeholder="(none)"
                spellCheck={false}
              />
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Save to
              </span>
              <input
                className={forms.input}
                value={f.savePath}
                onChange={(e) =>
                  setFolder(i, { savePath: e.currentTarget.value })
                }
                placeholder="(label / global default)"
                spellCheck={false}
              />
              <button
                className={forms.browse}
                onClick={() =>
                  void pickInto((d) => setFolder(i, { savePath: d }))
                }
              >
                Browse…
              </button>
              <button
                className={styles.removeOverride}
                title="Remove folder"
                aria-label="Remove watch folder"
                onClick={() =>
                  patch({
                    watchFolders: draft.watchFolders.filter(
                      (_, idx) => idx !== i,
                    ),
                  })
                }
              >
                ×
              </button>
            </div>
          </div>
        ))}
        <button
          className={styles.addOverride}
          onClick={() =>
            patch({
              watchFolders: [
                ...draft.watchFolders,
                { path: "", label: "", savePath: "" },
              ],
            })
          }
        >
          + Add watch folder
        </button>
      </Group>

      <Group title="Per-label Default Save Paths">
        {draft.labelDefaults.length === 0 && (
          <span className={forms.meta}>No per-label defaults.</span>
        )}
        {draft.labelDefaults.map((d, i) => (
          <div className={forms.field} key={i}>
            <input
              className={forms.input}
              style={{ maxWidth: 160 }}
              value={d.label}
              onChange={(e) => setDefault(i, { label: e.currentTarget.value })}
              placeholder="label"
              spellCheck={false}
            />
            <input
              className={forms.input}
              value={d.savePath}
              onChange={(e) =>
                setDefault(i, { savePath: e.currentTarget.value })
              }
              placeholder="save path"
              spellCheck={false}
            />
            <button
              className={forms.browse}
              onClick={() =>
                void pickInto((dir) => setDefault(i, { savePath: dir }))
              }
            >
              Browse…
            </button>
            <button
              className={styles.removeOverride}
              title="Remove default"
              aria-label="Remove label default"
              onClick={() =>
                patch({
                  labelDefaults: draft.labelDefaults.filter(
                    (_, idx) => idx !== i,
                  ),
                })
              }
            >
              ×
            </button>
          </div>
        ))}
        <button
          className={styles.addOverride}
          onClick={() =>
            patch({
              labelDefaults: [
                ...draft.labelDefaults,
                { label: "", savePath: "" },
              ],
            })
          }
        >
          + Add label default
        </button>
        <span className={forms.meta}>
          Adding a torrent with a matching label pre-fills its save path from
          here.
        </span>
      </Group>
    </>
  );
}

/** Speed pane: turtle-mode alternative limits + optional daily schedule (B14). */
function TurtleGroup({
  draft,
  patch,
}: {
  draft: Settings;
  patch: (p: Partial<Settings>) => void;
}) {
  return (
    <Group title="Turtle Mode (alternative limits, KiB/s)">
      <NumberRow
        label="Turtle download limit"
        value={draft.turtleDownKb}
        onChange={(turtleDownKb) => patch({ turtleDownKb })}
      />
      <NumberRow
        label="Turtle upload limit"
        value={draft.turtleUpKb}
        onChange={(turtleUpKb) => patch({ turtleUpKb })}
      />
      <Checkbox
        checked={draft.turtleEnabled}
        onChange={(turtleEnabled) => patch({ turtleEnabled })}
        label="Turtle mode on now (manual)"
      />
      <span className={forms.meta}>
        The manual toggle applies these rates whenever no pause window covers
        the clock.
      </span>
    </Group>
  );
}

/** Weekly scheduler grid (V3-18 / QUE-05). */
function SchedulerGroup({
  draft,
  patch,
}: {
  draft: Settings;
  patch: (p: Partial<Settings>) => void;
}) {
  const grid = draft.schedule.windows;
  const setGrid = (windows: SchedWindow[]) =>
    patch({ schedule: { ...draft.schedule, windows } });
  const setWindow = (i: number, next: Partial<SchedWindow>) =>
    setGrid(grid.map((w, idx) => (idx === i ? { ...w, ...next } : w)));
  const toggleDay = (i: number, d: number) => {
    const days = grid[i].days.includes(d)
      ? grid[i].days.filter((x) => x !== d)
      : [...grid[i].days, d].sort((a, b) => a - b);
    setWindow(i, { days });
  };

  const now = new Date();
  const weekday = now.getDay();
  const minute = now.getHours() * 60 + now.getMinutes();
  const nowMs = now.getTime();
  const manual = draft.turtleEnabled
    ? { downKb: draft.turtleDownKb, upKb: draft.turtleUpKb }
    : null;
  const over = draft.schedule.tempOverride;
  const overLive = over != null && over.untilMs > nowMs;
  const change = nextChange(grid, overLive ? over : null, manual, weekday, minute, nowMs);
  const legacyActive =
    grid.length === 0 && draft.turtleSchedule.enabled;

  const setOverride = (next: TempOverride | null) =>
    patch({ schedule: { ...draft.schedule, tempOverride: next } });
  const overMinutes =
    over != null ? Math.max(0, Math.round((over.untilMs - nowMs) / 60000)) : 30;

  return (
    <Group title="Weekly Scheduler">
      {legacyActive && (
        <span className={forms.meta}>
          Showing the imported turtle window (
          {minToTime(draft.turtleSchedule.startMin)}–
          {minToTime(draft.turtleSchedule.endMin)}).{" "}
          <button
            type="button"
            className={styles.addOverride}
            onClick={() =>
              patch({
                schedule: {
                  ...draft.schedule,
                  windows: importLegacy(
                    true,
                    draft.turtleSchedule.startMin,
                    draft.turtleSchedule.endMin,
                    draft.turtleSchedule.days,
                    draft.turtleDownKb,
                    draft.turtleUpKb,
                  ),
                },
                turtleSchedule: { ...draft.turtleSchedule, enabled: false },
              })
            }
          >
            Import as editable windows
          </button>
        </span>
      )}
      {grid.length === 0 && !legacyActive && (
        <span className={forms.meta}>
          No windows — limits follow the manual toggle and the globals. First
          matching window wins; pause beats limits.
        </span>
      )}
      {grid.map((w, i) => (
        <div key={i} style={rowCard}>
          <div className={forms.field}>
            <select
              className={forms.input}
              style={{ maxWidth: 110 }}
              value={w.pause ? "pause" : "limit"}
              aria-label="Window mode"
              onChange={(e) =>
                setWindow(i, { pause: e.currentTarget.value === "pause" })
              }
            >
              <option value="limit">Limit</option>
              <option value="pause">Pause</option>
            </select>
            {!w.pause && (
              <>
                <input
                  className={forms.input}
                  style={{ maxWidth: 80 }}
                  value={w.downKb}
                  onChange={(e) =>
                    setWindow(i, {
                      downKb: Number(e.currentTarget.value) || 0,
                    })
                  }
                  inputMode="numeric"
                  aria-label="Window download cap KiB/s"
                />
                <input
                  className={forms.input}
                  style={{ maxWidth: 80 }}
                  value={w.upKb}
                  onChange={(e) =>
                    setWindow(i, { upKb: Number(e.currentTarget.value) || 0 })
                  }
                  inputMode="numeric"
                  aria-label="Window upload cap KiB/s"
                />
              </>
            )}
            <button
              className={styles.removeOverride}
              title="Remove window"
              aria-label="Remove schedule window"
              onClick={() => setGrid(grid.filter((_, idx) => idx !== i))}
            >
              ×
            </button>
          </div>
          <div className={forms.field}>
            <span className={forms.fieldLabel} style={{ width: 64 }}>
              From
            </span>
            <input
              className={forms.input}
              type="time"
              aria-label="Window start"
              value={minToTime(w.startMin)}
              onChange={(e) =>
                setWindow(i, { startMin: timeToMin(e.currentTarget.value) })
              }
            />
            <span style={{ margin: "0 6px" }}>to</span>
            <input
              className={forms.input}
              type="time"
              aria-label="Window end"
              value={minToTime(w.endMin)}
              onChange={(e) =>
                setWindow(i, { endMin: timeToMin(e.currentTarget.value) })
              }
            />
          </div>
          <div className={forms.field}>
            <span className={forms.fieldLabel} style={{ width: 64 }}>
              Days
            </span>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
              {DAY_LABELS.map((label, d) => (
                <label
                  key={label}
                  style={{
                    display: "flex",
                    gap: 3,
                    alignItems: "center",
                    fontSize: 11,
                  }}
                >
                  <input
                    type="checkbox"
                    checked={w.days.length === 0 || w.days.includes(d)}
                    onChange={() => toggleDay(i, d)}
                  />
                  {label}
                </label>
              ))}
            </div>
          </div>
        </div>
      ))}
      <button
        className={styles.addOverride}
        onClick={() =>
          setGrid([
            ...grid,
            {
              days: [],
              startMin: 1320,
              endMin: 360,
              pause: false,
              downKb: 0,
              upKb: 0,
            },
          ])
        }
      >
        + Add window
      </button>
      <span className={forms.meta}>
        No days selected = every day. An end time earlier than the start wraps
        past midnight. Wall-clock time, so daylight-saving shifts need no
        configuration.
      </span>
      <div className={forms.field}>
        <span className={forms.fieldLabel} style={{ width: 64 }}>
          Override
        </span>
        <select
          className={forms.input}
          style={{ maxWidth: 130 }}
          value={overLive ? (over.pause ? "pause" : "limit") : "off"}
          aria-label="Temporary override mode"
          onChange={(e) => {
            const mode = e.currentTarget.value;
            if (mode === "off") {
              setOverride(null);
            } else {
              setOverride({
                pause: mode === "pause",
                downKb: over?.downKb ?? 0,
                upKb: over?.upKb ?? 0,
                untilMs: Date.now() + overMinutes * 60000,
              });
            }
          }}
        >
          <option value="off">Off</option>
          <option value="pause">Pause for…</option>
          <option value="limit">Limit for…</option>
        </select>
        <input
          className={forms.input}
          style={{ maxWidth: 70 }}
          value={overMinutes}
          onChange={(e) => {
            const minutes = Math.max(1, Number(e.currentTarget.value) || 0);
            if (overLive && over) {
              setOverride({ ...over, untilMs: Date.now() + minutes * 60000 });
            } else {
              setOverride({
                pause: true,
                downKb: 0,
                upKb: 0,
                untilMs: Date.now() + minutes * 60000,
              });
            }
          }}
          inputMode="numeric"
          aria-label="Override minutes"
        />
        <span className={forms.meta}>min</span>
      </div>
      {change ? (
        <span className={forms.meta}>
          Next change: {formatTransition(change)}.
        </span>
      ) : (
        <span className={forms.meta}>No scheduled changes coming up.</span>
      )}
    </Group>
  );
}

let rssIdCounter = 0;
/** Unique-enough id for a new feed/rule (only needs to be stable in this list). */
function rssId(prefix: string): string {
  rssIdCounter += 1;
  return `${prefix}_${Date.now().toString(36)}_${rssIdCounter}`;
}

/** One rule's test report: every item with its verdict and first miss reason. */
function TestReportView({
  report,
}: {
  report: {
    feedName: string;
    rows: RssTestRow[];
    loading: boolean;
    error: string | null;
  };
}) {
  if (report.loading) {
    return <span className={forms.meta}>testing…</span>;
  }
  if (report.error) {
    return <span className={forms.error}>{report.error}</span>;
  }
  const hits = report.rows.filter((r) => r.matched).length;
  return (
    <div>
      <span className={forms.meta}>
        {report.feedName}: {hits} of {report.rows.length} match.
      </span>
      {report.rows.slice(0, 12).map((row) => {
        const miss = row.clauses.find((c) => !c.passed);
        return (
          <div key={row.guid} className={forms.meta}>
            {row.matched ? "✓" : "✗"} {row.title}
            {!row.matched && miss && (
              <span>
                {" "}
                — {miss.label}: {miss.detail}
              </span>
            )}
          </div>
        );
      })}
      {report.rows.length > 12 && (
        <span className={forms.meta}>
          …and {report.rows.length - 12} more
        </span>
      )}
    </div>
  );
}

/** Bandwidth rules (V3-18 / QUE-04): named rate profiles by tag/label. */
function BandwidthRulesGroup({
  draft,
  patch,
}: {
  draft: Settings;
  patch: (p: Partial<Settings>) => void;
}) {
  const setRule = (i: number, next: Partial<BandwidthRule>) =>
    patch({
      bandwidthRules: draft.bandwidthRules.map((r, idx) =>
        idx === i ? { ...r, ...next } : r,
      ),
    });
  return (
    <Group title="Bandwidth Rules (KiB/s)">
      {draft.bandwidthRules.length === 0 && (
        <span className={forms.meta}>
          No rules — every torrent follows turtle/global. First matching tag
          wins, then label; hand-set limits always win over rules.
        </span>
      )}
      {draft.bandwidthRules.map((r, i) => (
        <div key={r.id || i} style={rowCard}>
          <div className={forms.field}>
            <select
              className={forms.input}
              style={{ maxWidth: 92 }}
              value={r.tag != null ? "tag" : "label"}
              aria-label="Rule matches a tag or a label"
              onChange={(e) =>
                setRule(
                  i,
                  e.currentTarget.value === "tag"
                    ? { tag: r.tag ?? r.label ?? "", label: undefined }
                    : { label: r.label ?? r.tag ?? "", tag: undefined },
                )
              }
            >
              <option value="tag">tag</option>
              <option value="label">label</option>
            </select>
            <input
              className={forms.input}
              style={{ maxWidth: 140 }}
              value={r.tag ?? r.label ?? ""}
              onChange={(e) =>
                setRule(
                  i,
                  r.tag != null
                    ? { tag: e.currentTarget.value }
                    : { label: e.currentTarget.value },
                )
              }
              placeholder={r.tag != null ? "tag" : "label"}
              spellCheck={false}
            />
            <button
              className={styles.removeOverride}
              title="Remove rule"
              aria-label="Remove bandwidth rule"
              onClick={() =>
                patch({
                  bandwidthRules: draft.bandwidthRules.filter(
                    (_, idx) => idx !== i,
                  ),
                })
              }
            >
              ×
            </button>
          </div>
          <div className={forms.field}>
            <span className={forms.fieldLabel} style={{ width: 64 }}>
              Down
            </span>
            <input
              className={forms.input}
              style={{ maxWidth: 90 }}
              value={r.downKb}
              onChange={(e) =>
                setRule(i, { downKb: Number(e.currentTarget.value) || 0 })
              }
              inputMode="numeric"
              aria-label="Download cap KiB/s"
            />
            <span className={forms.fieldLabel} style={{ width: 48 }}>
              Up
            </span>
            <input
              className={forms.input}
              style={{ maxWidth: 90 }}
              value={r.upKb}
              onChange={(e) =>
                setRule(i, { upKb: Number(e.currentTarget.value) || 0 })
              }
              inputMode="numeric"
              aria-label="Upload cap KiB/s"
            />
          </div>
          <div className={forms.field}>
            <span className={forms.fieldLabel} style={{ width: 64 }}>
              Peers
            </span>
            <input
              className={forms.input}
              style={{ maxWidth: 70 }}
              value={r.peersMax ?? 0}
              onChange={(e) =>
                setRule(i, { peersMax: Number(e.currentTarget.value) || 0 })
              }
              inputMode="numeric"
              aria-label="Max peers"
            />
            <input
              className={forms.input}
              style={{ maxWidth: 70 }}
              value={r.peersMin ?? 0}
              onChange={(e) =>
                setRule(i, { peersMin: Number(e.currentTarget.value) || 0 })
              }
              inputMode="numeric"
              aria-label="Min peers"
            />
            <span className={forms.fieldLabel} style={{ width: 48 }}>
              Slots
            </span>
            <input
              className={forms.input}
              style={{ maxWidth: 70 }}
              value={r.uploadsMax ?? 0}
              onChange={(e) =>
                setRule(i, { uploadsMax: Number(e.currentTarget.value) || 0 })
              }
              inputMode="numeric"
              aria-label="Upload slots"
            />
          </div>
          <span className={forms.meta}>
            0 = unlimited rates, daemon-default caps. Applied on the next poll;
            deleting a rule returns its torrents to global limits.
          </span>
        </div>
      ))}
      <button
        className={styles.addOverride}
        onClick={() =>
          patch({
            bandwidthRules: [
              ...draft.bandwidthRules,
              { id: rssId("bw"), tag: "", downKb: 0, upKb: 0 },
            ],
          })
        }
      >
        + Add bandwidth rule
      </button>
    </Group>
  );
}

/** RSS pane (B11): poll interval, feeds (with live preview), auto-download rules. */
function RssSection({
  draft,
  patch,
  labels,
}: {
  draft: Settings;
  patch: (p: Partial<Settings>) => void;
  labels: string[];
}) {
  const [preview, setPreview] = useState<{
    feedId: string;
    items: FeedItem[];
    loading: boolean;
    error: string | null;
  } | null>(null);
  const [testReport, setTestReport] = useState<{
    ruleId: string;
    feedName: string;
    rows: RssTestRow[];
    loading: boolean;
    error: string | null;
  } | null>(null);
  const [seenMsg, setSeenMsg] = useState<string | null>(null);

  const setFeed = (i: number, next: Partial<RssFeed>) =>
    patch({
      rssFeeds: draft.rssFeeds.map((f, idx) =>
        idx === i ? { ...f, ...next } : f,
      ),
    });
  const setRule = (i: number, next: Partial<RssRule>) =>
    patch({
      rssRules: draft.rssRules.map((r, idx) =>
        idx === i ? { ...r, ...next } : r,
      ),
    });

  const previewFeed = async (feed: RssFeed) => {
    setPreview({ feedId: feed.id, items: [], loading: true, error: null });
    try {
      const items = await rssFetch(feed.url);
      setPreview({ feedId: feed.id, items, loading: false, error: null });
    } catch (e) {
      setPreview({
        feedId: feed.id,
        items: [],
        loading: false,
        error: String(e),
      });
    }
  };

  /** Test one rule against its feed (its own, else the first enabled one),
   *  with per-clause verdicts. */
  const testRule = async (i: number) => {
    const rule = draft.rssRules[i];
    if (!rule) return;
    const feed = draft.rssFeeds.find((f) => f.id === rule.feedId && f.enabled)
      ?? draft.rssFeeds.find((f) => f.enabled);
    if (!feed) {
      setTestReport({
        ruleId: rule.id,
        feedName: "",
        rows: [],
        loading: false,
        error: "no enabled feed to test against",
      });
      return;
    }
    setTestReport({
      ruleId: rule.id,
      feedName: feed.name || feed.url,
      rows: [],
      loading: true,
      error: null,
    });
    try {
      const rows = await rssTest(rule, feed.url);
      setTestReport({
        ruleId: rule.id,
        feedName: feed.name || feed.url,
        rows,
        loading: false,
        error: null,
      });
    } catch (e) {
      setTestReport({
        ruleId: rule.id,
        feedName: feed.name || feed.url,
        rows: [],
        loading: false,
        error: String(e),
      });
    }
  };

  const exportSeen = async () => {
    try {
      const json = await rssExportSeen();
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "rss_seen.json";
      a.click();
      URL.revokeObjectURL(url);
      setSeenMsg(
        `exported ${JSON.parse(json).length} remembered download(s)`,
      );
    } catch (e) {
      setSeenMsg(`export failed: ${String(e)}`);
    }
  };

  const importSeen = async (file: File) => {
    try {
      const json = await file.text();
      const added = await rssImportSeen(json);
      setSeenMsg(`imported ${added} new remembered download(s)`);
    } catch (e) {
      setSeenMsg(`import failed: ${String(e)}`);
    }
  };

  return (
    <>
      <Group title="RSS">
        <NumberRow
          label="Poll interval (minutes)"
          value={draft.rssPollMinutes}
          onChange={(rssPollMinutes) => patch({ rssPollMinutes })}
        />
        <span className={forms.meta}>
          How often to check feeds and auto-add rule matches. Rules with
          their own interval keep their own cadence. 0 disables background
          polling (Preview, Test and Download still work).
        </span>
        <div className={forms.field}>
          <button
            type="button"
            className={styles.addOverride}
            onClick={() => void exportSeen()}
          >
            Export seen-set
          </button>
          <label className={styles.addOverride} style={{ cursor: "pointer" }}>
            Import seen-set
            <input
              type="file"
              accept="application/json"
              hidden
              onChange={(e) => {
                const file = e.currentTarget.files?.[0];
                if (file) void importSeen(file);
                e.currentTarget.value = "";
              }}
            />
          </label>
          {seenMsg && <span className={forms.meta}>{seenMsg}</span>}
        </div>
      </Group>

      <Group title="Feeds">
        {draft.rssFeeds.length === 0 && (
          <span className={forms.meta}>No feeds.</span>
        )}
        {draft.rssFeeds.map((f, i) => (
          <div key={f.id} style={rowCard}>
            <div className={forms.field}>
              <input
                type="checkbox"
                checked={f.enabled}
                aria-label="Feed enabled"
                onChange={(e) =>
                  setFeed(i, { enabled: e.currentTarget.checked })
                }
              />
              <input
                className={forms.input}
                style={{ maxWidth: 150 }}
                value={f.name}
                onChange={(e) => setFeed(i, { name: e.currentTarget.value })}
                placeholder="name"
                spellCheck={false}
              />
              <input
                className={forms.input}
                value={f.url}
                onChange={(e) => setFeed(i, { url: e.currentTarget.value })}
                placeholder="https://…/rss"
                spellCheck={false}
              />
              <button
                className={forms.browse}
                disabled={!f.url}
                onClick={() => void previewFeed(f)}
              >
                Preview
              </button>
              <button
                className={styles.removeOverride}
                aria-label="Remove feed"
                onClick={() =>
                  patch({
                    rssFeeds: draft.rssFeeds.filter((_, idx) => idx !== i),
                  })
                }
              >
                ×
              </button>
            </div>
            {preview?.feedId === f.id && (
              <div style={{ marginTop: 2 }}>
                {preview.loading && (
                  <span className={forms.meta}>loading…</span>
                )}
                {preview.error && (
                  <span
                    className={forms.meta}
                    style={{ color: "var(--accent-red-soft)" }}
                  >
                    {preview.error}
                  </span>
                )}
                {!preview.loading &&
                  !preview.error &&
                  preview.items.length === 0 && (
                    <span className={forms.meta}>no items</span>
                  )}
                {preview.items.slice(0, 30).map((it) => (
                  <div className={forms.field} key={it.guid} style={{ gap: 8 }}>
                    <span
                      style={{
                        flex: 1,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                      title={it.title}
                    >
                      {it.title}
                    </span>
                    <button
                      className={forms.browse}
                      onClick={() => void rssDownload(it.link, "", "")}
                    >
                      Download
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
        <button
          className={styles.addOverride}
          onClick={() =>
            patch({
              rssFeeds: [
                ...draft.rssFeeds,
                { id: rssId("feed"), name: "", url: "", enabled: true },
              ],
            })
          }
        >
          + Add feed
        </button>
      </Group>

      <Group title="Auto-download Rules">
        {draft.rssRules.length === 0 && (
          <span className={forms.meta}>
            No rules. A feed with no matching rule is only fetched when you
            press Preview.
          </span>
        )}
        {draft.rssRules.map((r, i) => (
          <div key={r.id} style={rowCard}>
            <div className={forms.field}>
              <input
                type="checkbox"
                checked={r.enabled}
                aria-label="Rule enabled"
                onChange={(e) =>
                  setRule(i, { enabled: e.currentTarget.checked })
                }
              />
              <input
                className={forms.input}
                value={r.name}
                onChange={(e) => setRule(i, { name: e.currentTarget.value })}
                placeholder="rule name"
                spellCheck={false}
              />
              <button
                className={styles.removeOverride}
                aria-label="Remove rule"
                onClick={() =>
                  patch({
                    rssRules: draft.rssRules.filter((_, idx) => idx !== i),
                  })
                }
              >
                ×
              </button>
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Feed
              </span>
              <select
                className={forms.input}
                value={r.feedId}
                aria-label="Rule feed"
                onChange={(e) => setRule(i, { feedId: e.currentTarget.value })}
              >
                <option value="">All feeds</option>
                {draft.rssFeeds.map((f) => (
                  <option key={f.id} value={f.id}>
                    {f.name || f.url}
                  </option>
                ))}
              </select>
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Must contain
              </span>
              <input
                className={forms.input}
                value={r.mustContain}
                onChange={(e) =>
                  setRule(i, { mustContain: e.currentTarget.value })
                }
                placeholder="e.g.  1080p x265"
                spellCheck={false}
              />
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Exclude
              </span>
              <input
                className={forms.input}
                value={r.mustNotContain}
                onChange={(e) =>
                  setRule(i, { mustNotContain: e.currentTarget.value })
                }
                placeholder="e.g.  cam hdcam"
                spellCheck={false}
              />
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Regex
              </span>
              <input
                className={forms.input}
                value={r.regexMatch ?? ""}
                onChange={(e) =>
                  setRule(i, { regexMatch: e.currentTarget.value })
                }
                placeholder="optional, e.g. ^Show\.S0[12]"
                spellCheck={false}
              />
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Seasons
              </span>
              <input
                className={forms.input}
                style={{ maxWidth: 110 }}
                value={r.seasonRange ?? ""}
                onChange={(e) =>
                  setRule(i, { seasonRange: e.currentTarget.value })
                }
                placeholder="e.g. 1-3"
                spellCheck={false}
              />
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Episodes
              </span>
              <input
                className={forms.input}
                style={{ maxWidth: 110 }}
                value={r.episodeRange ?? ""}
                onChange={(e) =>
                  setRule(i, { episodeRange: e.currentTarget.value })
                }
                placeholder="e.g. 4-12"
                spellCheck={false}
              />
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Size MiB
              </span>
              <input
                className={forms.input}
                style={{ maxWidth: 90 }}
                value={r.minSizeMb ?? 0}
                onChange={(e) =>
                  setRule(i, { minSizeMb: Number(e.currentTarget.value) || 0 })
                }
                inputMode="numeric"
                aria-label="Minimum size MiB"
              />
              <span style={{ margin: "0 6px" }}>–</span>
              <input
                className={forms.input}
                style={{ maxWidth: 90 }}
                value={r.maxSizeMb ?? 0}
                onChange={(e) =>
                  setRule(i, { maxSizeMb: Number(e.currentTarget.value) || 0 })
                }
                inputMode="numeric"
                aria-label="Maximum size MiB"
              />
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Prefer
              </span>
              <input
                className={forms.input}
                style={{ maxWidth: 110 }}
                value={r.preferQuality ?? ""}
                onChange={(e) =>
                  setRule(i, { preferQuality: e.currentTarget.value })
                }
                placeholder="e.g. 1080p"
                spellCheck={false}
              />
              <label
                style={{
                  display: "flex",
                  gap: 3,
                  alignItems: "center",
                  fontSize: 11,
                }}
              >
                <input
                  type="checkbox"
                  checked={r.smartEpisode ?? false}
                  onChange={(e) =>
                    setRule(i, { smartEpisode: e.currentTarget.checked })
                  }
                />
                Smart episode
              </label>
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Label
              </span>
              <input
                className={forms.input}
                style={{ maxWidth: 150 }}
                value={r.label}
                list="rss-known-labels"
                onChange={(e) => setRule(i, { label: e.currentTarget.value })}
                placeholder="(none)"
                spellCheck={false}
              />
              <span className={forms.fieldLabel} style={{ width: 56 }}>
                Save to
              </span>
              <input
                className={forms.input}
                value={r.savePath}
                onChange={(e) =>
                  setRule(i, { savePath: e.currentTarget.value })
                }
                placeholder="(label / global default)"
                spellCheck={false}
              />
            </div>
            <div className={forms.field}>
              <span className={forms.fieldLabel} style={{ width: 64 }}>
                Tags
              </span>
              <input
                className={forms.input}
                value={(r.tags ?? []).join(", ")}
                onChange={(e) =>
                  setRule(i, {
                    tags: e.currentTarget.value
                      .split(",")
                      .map((t) => t.trim())
                      .filter(Boolean),
                  })
                }
                placeholder="(none)"
                spellCheck={false}
              />
            </div>
            <div className={forms.field}>
              <label
                style={{
                  display: "flex",
                  gap: 3,
                  alignItems: "center",
                  fontSize: 11,
                }}
              >
                <input
                  type="checkbox"
                  checked={r.start ?? true}
                  onChange={(e) => setRule(i, { start: e.currentTarget.checked })}
                />
                Start on add
              </label>
              <label
                style={{
                  display: "flex",
                  gap: 3,
                  alignItems: "center",
                  fontSize: 11,
                }}
              >
                <input
                  type="checkbox"
                  checked={r.topOfQueue ?? false}
                  onChange={(e) =>
                    setRule(i, { topOfQueue: e.currentTarget.checked })
                  }
                />
                Top of queue
              </label>
              <span className={forms.fieldLabel} style={{ width: 90 }}>
                Every (min)
              </span>
              <input
                className={forms.input}
                style={{ maxWidth: 70 }}
                value={r.pollMinutes ?? 0}
                onChange={(e) =>
                  setRule(i, { pollMinutes: Number(e.currentTarget.value) || 0 })
                }
                inputMode="numeric"
                aria-label="Rule poll interval minutes, 0 means global"
              />
              <button
                className={styles.addOverride}
                onClick={() => void testRule(i)}
              >
                Test
              </button>
            </div>
            {testReport?.ruleId === r.id && (
              <TestReportView report={testReport} />
            )}
          </div>
        ))}
        <datalist id="rss-known-labels">
          {labels.map((l) => (
            <option key={l} value={l} />
          ))}
        </datalist>
        <button
          className={styles.addOverride}
          onClick={() =>
            patch({
              rssRules: [
                ...draft.rssRules,
                {
                  id: rssId("rule"),
                  name: "",
                  enabled: true,
                  feedId: "",
                  mustContain: "",
                  mustNotContain: "",
                  regexMatch: "",
                  seasonRange: "",
                  episodeRange: "",
                  minSizeMb: 0,
                  maxSizeMb: 0,
                  preferQuality: "",
                  smartEpisode: false,
                  label: "",
                  tags: [],
                  savePath: "",
                  start: true,
                  topOfQueue: false,
                  pollMinutes: 0,
                },
              ],
            })
          }
        >
          + Add rule
        </button>
        <span className={forms.meta}>
          Tokens are space-separated: “must contain” needs all of them,
          “exclude” rejects if any appears. Case-insensitive.
        </span>
      </Group>
    </>
  );
}

/** Connection section: profiles (B10) + transport picker + poll/stall + test. */
function ConnectionSection({
  transport,
  profiles,
  onProfiles,
  pollMs,
  stallWindowS,
  onTransport,
  onPoll,
  onStall,
  onTest,
  testMsg,
  password,
  onPassword,
  passwordSaved,
  onForgetPassword,
}: {
  transport: Transport;
  profiles: ConnectionProfile[];
  onProfiles: (p: ConnectionProfile[]) => void;
  pollMs: number;
  stallWindowS: number;
  onTransport: (t: Transport) => void;
  onPoll: (n: number) => void;
  onStall: (n: number) => void;
  onTest: () => void;
  testMsg: { ok: boolean; text: string } | null;
  password: string;
  onPassword: (p: string) => void;
  passwordSaved: boolean;
  onForgetPassword: () => void;
}) {
  const [savingProfile, setSavingProfile] = useState(false);
  const [profileName, setProfileName] = useState("");

  // The active profile is whichever one's transport matches the editor's.
  const sameTransport = (t: Transport) =>
    JSON.stringify(t) === JSON.stringify(transport);
  const activeName =
    profiles.find((p) => sameTransport(p.transport))?.name ?? "";

  const saveProfile = () => {
    const name = profileName.trim();
    if (!name) return;
    const others = profiles.filter((p) => p.name !== name);
    onProfiles(
      [...others, { name, transport }].sort((a, b) =>
        a.name.localeCompare(b.name),
      ),
    );
    setProfileName("");
    setSavingProfile(false);
  };

  return (
    <Group title="rtorrent Connection">
      <div className={forms.field}>
        <span className={forms.fieldLabel}>Profile</span>
        <select
          className={forms.input}
          value={activeName}
          aria-label="Connection profile"
          onChange={(e) => {
            const p = profiles.find((x) => x.name === e.currentTarget.value);
            if (p) onTransport(p.transport);
          }}
        >
          <option value="">(unsaved)</option>
          {profiles.map((p) => (
            <option key={p.name} value={p.name}>
              {p.name}
            </option>
          ))}
        </select>
        <button
          className={forms.browse}
          onClick={() => {
            setProfileName(activeName);
            setSavingProfile(true);
          }}
        >
          Save as…
        </button>
        {activeName && (
          <button
            className={forms.browse}
            onClick={() =>
              onProfiles(profiles.filter((p) => p.name !== activeName))
            }
          >
            Delete
          </button>
        )}
      </div>
      {savingProfile && (
        <div className={forms.field}>
          <span className={forms.fieldLabel} />
          <input
            className={forms.input}
            autoFocus
            placeholder="profile name…"
            value={profileName}
            onChange={(e) => setProfileName(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") saveProfile();
              if (e.key === "Escape") {
                e.stopPropagation();
                setSavingProfile(false);
              }
            }}
          />
          <button className={forms.browse} onClick={saveProfile}>
            Save
          </button>
        </div>
      )}
      <span className={forms.meta}>
        Save the current connection as a named profile, then switch daemons from
        the dropdown. Applying reconnects.
      </span>

      <div className={forms.field}>
        <span className={forms.fieldLabel}>Transport</span>
        {/* Windows cannot reach a socket inside the WSL VM, so the option is
            hidden rather than offered and left to fail. */}
        {!isWindows && (
          <Checkbox
            checked={transport.kind === "unixSocket"}
            onChange={() => onTransport({ kind: "unixSocket", path: "" })}
            label="Unix socket"
          />
        )}
        <Checkbox
          checked={transport.kind === "tcp"}
          onChange={() =>
            onTransport({ kind: "tcp", host: "127.0.0.1", port: 5000 })
          }
          label="TCP"
        />
        <Checkbox
          checked={transport.kind === "http"}
          onChange={() =>
            onTransport({ kind: "http", url: "https://", username: "" })
          }
          label="HTTP(S)"
        />
      </div>

      {transport.kind === "unixSocket" && (
        <div className={forms.field}>
          <span className={forms.fieldLabel}>Socket</span>
          <input
            className={forms.input}
            value={transport.path}
            onChange={(e) =>
              onTransport({ kind: "unixSocket", path: e.currentTarget.value })
            }
            placeholder="~/.rtorrent/rpc.socket"
            spellCheck={false}
          />
        </div>
      )}

      {transport.kind === "tcp" && (
        <>
          <div className={forms.field}>
            <span className={forms.fieldLabel}>Host</span>
            <input
              className={forms.input}
              value={transport.host}
              onChange={(e) =>
                onTransport({
                  kind: "tcp",
                  host: e.currentTarget.value,
                  port: transport.port,
                })
              }
              spellCheck={false}
            />
          </div>
          <div className={forms.field}>
            <span className={forms.fieldLabel}>Port</span>
            <input
              className={forms.input}
              type="number"
              value={transport.port}
              onChange={(e) =>
                onTransport({
                  kind: "tcp",
                  host: transport.host,
                  port: Number(e.currentTarget.value) || 0,
                })
              }
            />
          </div>
          {!isLocalhost(transport) && (
            <span className={styles.warn}>
              SCGI has no authentication — exposing it to a non-localhost host
              is insecure. Prefer HTTP(S) with a username and password.
            </span>
          )}
        </>
      )}

      {transport.kind === "http" && (
        <HttpFields
          transport={transport}
          onTransport={onTransport}
          password={password}
          onPassword={onPassword}
          passwordSaved={passwordSaved}
          onForgetPassword={onForgetPassword}
        />
      )}

      <div className={forms.field}>
        <span className={forms.fieldLabel} style={{ width: 120 }}>
          Poll interval (ms)
        </span>
        <input
          className={forms.input}
          type="number"
          min={250}
          value={pollMs}
          onChange={(e) =>
            onPoll(Math.max(250, Number(e.currentTarget.value) || 1000))
          }
        />
      </div>
      <div className={forms.field}>
        <span className={forms.fieldLabel} style={{ width: 120 }}>
          Stall window (s)
        </span>
        <input
          className={forms.input}
          type="number"
          min={0}
          value={stallWindowS}
          onChange={(e) =>
            onStall(Math.max(0, Number(e.currentTarget.value) || 30))
          }
        />
      </div>

      <div className={forms.field}>
        <button className={forms.browse} onClick={onTest}>
          Test connection
        </button>
        {testMsg && (
          <span
            className={`${styles.testResult} ${testMsg.ok ? styles.ok : styles.err}`}
          >
            {testMsg.text}
          </span>
        )}
      </div>
    </Group>
  );
}

/**
 * HTTP(S) endpoint fields (B9): URL, username, and a password that goes to the
 * Keychain rather than settings.json.
 *
 * The password box is write-only by design — a saved secret is never read back
 * into the webview, so it shows a "saved" hint with a Forget button instead of
 * the value. Leaving it blank keeps whatever is already saved.
 */
function HttpFields({
  transport,
  onTransport,
  password,
  onPassword,
  passwordSaved,
  onForgetPassword,
}: {
  transport: Extract<Transport, { kind: "http" }>;
  onTransport: (t: Transport) => void;
  password: string;
  onPassword: (p: string) => void;
  passwordSaved: boolean;
  onForgetPassword: () => void;
}) {
  const insecure = isInsecureCredentialed(transport);
  return (
    <>
      <div className={forms.field}>
        <span className={forms.fieldLabel}>URL</span>
        <input
          className={forms.input}
          value={transport.url}
          onChange={(e) =>
            onTransport({ ...transport, url: e.currentTarget.value })
          }
          placeholder="https://seedbox.example.com/RPC2"
          spellCheck={false}
        />
      </div>
      <div className={forms.field}>
        <span className={forms.fieldLabel}>Username</span>
        <input
          className={forms.input}
          value={transport.username}
          onChange={(e) =>
            onTransport({ ...transport, username: e.currentTarget.value })
          }
          placeholder="(none)"
          spellCheck={false}
          autoComplete="off"
        />
      </div>
      <div className={forms.field}>
        <span className={forms.fieldLabel}>Password</span>
        <input
          className={forms.input}
          type="password"
          value={password}
          onChange={(e) => onPassword(e.currentTarget.value)}
          placeholder={passwordSaved ? "•••••••• (saved)" : "(none)"}
          autoComplete="new-password"
        />
        {passwordSaved && (
          <button className={forms.browse} onClick={onForgetPassword}>
            Forget
          </button>
        )}
      </div>
      <span className={forms.meta}>
        Stored in your {credentialStoreName}, not in the settings file.
        {passwordSaved && " Leave blank to keep the saved password."}
      </span>
      {insecure && (
        <span className={styles.warn}>
          http:// sends your password in the clear — anything on the network
          path can read it. Use https:// for a remote daemon.
        </span>
      )}
    </>
  );
}
