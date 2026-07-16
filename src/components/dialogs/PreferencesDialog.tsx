/**
 * Preferences window (design screen 04).
 *
 * Left nav selects a section; the right panel edits a working copy of settings.
 * Apply persists via the `apply_settings` command and refreshes the settings
 * store (which also live-reconnects the poller if the transport changed). RSS and
 * Web UI are shown disabled (v2). Some controls are intentionally disabled where
 * the backend doesn't support them yet (see plan.md §10) with an explanatory tip.
 */

import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useUi } from "../../store/ui";
import { useSettings, isLocalhost } from "../../store/settings";
import { useTorrents } from "../../store/torrents";
import { applySettings, testConnection } from "../../ipc/commands";
import type { Settings, Transport } from "../../ipc/types";
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
  { id: "rss", label: "RSS", icon: "⤳", disabled: true },
  { id: "webui", label: "Web UI", icon: "⧉", disabled: true },
  { id: "advanced", label: "Advanced", icon: "⚑" },
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

  // Load the working copy once settings are available.
  useEffect(() => {
    if (settings && !draft) setDraft(settings);
  }, [settings, draft]);

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
      const version = await testConnection(draft.transport);
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
            <Group title="Saving Management">
              <span className={forms.fieldLabel} style={{ width: "auto" }}>
                Default save path
              </span>
              <div className={forms.field}>
                <input
                  className={forms.input}
                  value={draft.defaultSavePath}
                  onChange={(e) =>
                    patch({ defaultSavePath: e.currentTarget.value })
                  }
                  spellCheck={false}
                />
                <button className={forms.browse} onClick={() => void browse()}>
                  Browse…
                </button>
              </div>
              <Checkbox
                checked={false}
                onChange={() => {}}
                disabled
                label="Keep incomplete torrents in a separate folder"
                title="requires rtorrent watch/move configuration (v2)"
              />
              <span
                className={forms.fieldLabel}
                style={{ width: "auto", marginTop: 4 }}
              >
                Watched folder (auto-add .torrent files; takes effect on
                restart)
              </span>
              <div className={forms.field}>
                <input
                  className={forms.input}
                  value={draft.watchFolder}
                  onChange={(e) =>
                    patch({ watchFolder: e.currentTarget.value })
                  }
                  placeholder="(disabled)"
                  spellCheck={false}
                />
                <button
                  className={forms.browse}
                  onClick={async () => {
                    const dir = await open({ directory: true });
                    if (typeof dir === "string") patch({ watchFolder: dir });
                  }}
                >
                  Browse…
                </button>
              </div>
            </Group>
          )}

          {section === "connection" && (
            <ConnectionSection
              transport={draft.transport}
              pollMs={draft.pollMs}
              stallWindowS={draft.stallWindowS}
              onTransport={setTransport}
              onPoll={(pollMs) => patch({ pollMs })}
              onStall={(stallWindowS) => patch({ stallWindowS })}
              onTest={() => void runTest()}
              testMsg={testMsg}
            />
          )}

          {section === "speed" && (
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

          {(section === "rss" || section === "webui") && (
            <Group title={section === "rss" ? "RSS" : "Web UI"}>
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

/** Connection section: transport picker + poll/stall + test button. */
function ConnectionSection({
  transport,
  pollMs,
  stallWindowS,
  onTransport,
  onPoll,
  onStall,
  onTest,
  testMsg,
}: {
  transport: Transport;
  pollMs: number;
  stallWindowS: number;
  onTransport: (t: Transport) => void;
  onPoll: (n: number) => void;
  onStall: (n: number) => void;
  onTest: () => void;
  testMsg: { ok: boolean; text: string } | null;
}) {
  const isUnix = transport.kind === "unixSocket";
  return (
    <Group title="rtorrent Connection">
      <div className={forms.field}>
        <span className={forms.fieldLabel}>Transport</span>
        <Checkbox
          checked={isUnix}
          onChange={() => onTransport({ kind: "unixSocket", path: "" })}
          label="Unix socket"
        />
        <Checkbox
          checked={!isUnix}
          onChange={() =>
            onTransport({ kind: "tcp", host: "127.0.0.1", port: 5000 })
          }
          label="TCP"
        />
      </div>

      {transport.kind === "unixSocket" ? (
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
      ) : (
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
              is insecure.
            </span>
          )}
        </>
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
