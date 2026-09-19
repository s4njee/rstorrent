/**
 * The Settings route (WC7).
 *
 * Three kinds of row share one 230px/1fr grid: daemon keys read live from
 * `/api/config` (Connection/Bandwidth/Queue/Directories), server facts the
 * browser cannot change (General), and the server-owned interface preferences
 * (Interface). Save is dirty-tracked and reports per-key outcomes, so a partial
 * failure never reads as success; Revert restores the daemon's values.
 *
 * Security (password change) and Advanced (the daemon's managed `.rtorrent.rc`
 * block, read-only) sit at the end, mirroring the desktop Preferences order.
 */

import { useEffect, useMemo, useState } from "react";
import {
  changePassword,
  getAdvanced,
  getConfig,
  getSettings,
  saveConfig,
  saveSettings,
  type AdvancedInfo,
  type ConfigKind,
  type ConfigRow,
  type ServerInfo,
  type SettingsSection,
  type SaveReport,
  type UiPreferences,
} from "../ipc/webSettings";
import { useNotices } from "../store/notices";
import { refetchSnapshotNow } from "../ipc/web";
import {
  applyDensity,
  applyTheme,
  clearAccent,
  loadThemeChoice,
  saveDensity,
  saveThemeChoice,
  THEME_CHOICES,
  DENSITIES,
  type Density,
  type ThemeChoice,
} from "../theme/theme";
import styles from "./SettingsPage.module.css";

const SECTIONS: Array<{ id: SettingsSection; label: string }> = [
  { id: "general", label: "General" },
  { id: "connection", label: "Connection" },
  { id: "bandwidth", label: "Bandwidth" },
  { id: "queue", label: "Queue" },
  { id: "directories", label: "Directories" },
  { id: "labels", label: "Labels" },
  { id: "interface", label: "Interface" },
  { id: "advanced", label: "Advanced" },
];

/** Mirror the server's validation so bad input is caught before the round-trip. */
export function validateConfigValue(
  kind: ConfigKind,
  value: string,
): string | null {
  const trimmed = value.trim();
  switch (kind.type) {
    case "int": {
      if (trimmed === "") return "a number is required";
      const number = Number(trimmed);
      if (!Number.isInteger(number)) return "must be a whole number";
      if (number < kind.min || number > kind.max) {
        return `must be between ${kind.min} and ${kind.max}`;
      }
      return null;
    }
    case "str":
      return trimmed.length > kind.maxLen
        ? `must be at most ${kind.maxLen} characters`
        : null;
    case "choice":
      return kind.options.includes(trimmed)
        ? null
        : `must be one of ${kind.options.join(", ")}`;
    case "bool":
      return trimmed === "0" || trimmed === "1" ? null : "must be 0 or 1";
    case "readonly":
      return "this setting is read-only";
  }
}

export function SettingsPage({ onBack }: { onBack: () => void }) {
  const [section, setSection] = useState<SettingsSection>("general");
  const [config, setConfig] = useState<ConfigRow[] | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [server, setServer] = useState<ServerInfo | null>(null);
  const [ui, setUi] = useState<UiPreferences>({});
  const [advanced, setAdvanced] = useState<AdvancedInfo | null>(null);
  const [saving, setSaving] = useState(false);
  const [report, setReport] = useState<SaveReport | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const pushNotice = useNotices((s) => s.push);

  useEffect(() => {
    let cancelled = false;
    Promise.all([getConfig(), getSettings(), getAdvanced()])
      .then(([rows, settings, adv]) => {
        if (cancelled) return;
        setConfig(rows);
        setServer(settings.server);
        setUi(settings.ui);
        setAdvanced(adv);
        setDraft(
          Object.fromEntries(rows.map((row) => [row.id, row.value ?? ""])),
        );
        // The operator default applies only when this browser has no choice of
        // its own (WC1-S4).
        if (loadThemeChoice() === null) {
          applyTheme(
            (settings.ui.theme as ThemeChoice | undefined) ?? "dark",
            settings.ui.accent || null,
          );
          if (settings.ui.density) {
            applyDensity(settings.ui.density as Density);
          }
        }
      })
      .catch((error: unknown) =>
        !cancelled ? setLoadError(String(error)) : undefined,
      );
    return () => {
      cancelled = true;
    };
  }, []);

  const changed = useMemo(() => {
    if (!config) return [] as string[];
    return config
      .filter((row) => (draft[row.id] ?? "") !== (row.value ?? ""))
      .map((row) => row.id);
  }, [config, draft]);

  const errors = useMemo(() => {
    const out: Record<string, string> = {};
    if (!config) return out;
    for (const row of config) {
      if (!changed.includes(row.id)) continue;
      const message = validateConfigValue(row.kind, draft[row.id] ?? "");
      if (message) out[row.id] = message;
    }
    return out;
  }, [config, draft, changed]);

  const dirty = changed.length > 0;
  const blocked = Object.keys(errors).length > 0;

  const onSave = async () => {
    if (!config || blocked || !dirty) return;
    setSaving(true);
    setReport(null);
    try {
      const values: Record<string, string> = {};
      for (const id of changed) values[id] = (draft[id] ?? "").trim();
      const outcome = await saveConfig(values);
      setReport(outcome);
      // Fold the accepted writes into the truth so the row stops reading dirty;
      // a failed key keeps its draft and stays highlighted.
      setConfig(
        (rows) =>
          rows?.map((row) =>
            outcome.applied.includes(row.id)
              ? { ...row, value: (draft[row.id] ?? "").trim(), available: true }
              : row,
          ) ?? rows,
      );
      if (outcome.applied.length > 0) {
        refetchSnapshotNow();
      }
    } catch (error: unknown) {
      setReport({ applied: [], failed: [{ id: "*", error: String(error) }] });
    } finally {
      setSaving(false);
    }
  };

  const onRevert = () => {
    if (!config) return;
    setDraft(
      Object.fromEntries(config.map((row) => [row.id, row.value ?? ""])),
    );
    setReport(null);
  };

  const persistUi = (patch: UiPreferences) => {
    const next = { ...ui, ...patch };
    setUi(next);
    void saveSettings(patch)
      .then((saved) => setUi((current) => ({ ...current, ...saved })))
      .catch((error: unknown) => pushNotice("error", String(error)));
  };

  const daemonRows = (id: SettingsSection) =>
    (config ?? []).filter((row) => row.section === id);

  return (
    <div className={styles.page} data-testid="settings-page">
      <nav className={styles.nav} aria-label="Settings sections">
        {SECTIONS.map((item) => (
          <button
            key={item.id}
            type="button"
            className={styles.navItem}
            aria-current={section === item.id ? "page" : undefined}
            onClick={() => setSection(item.id)}
          >
            {item.label}
          </button>
        ))}
      </nav>

      <div className={styles.panel}>
        {loadError && (
          <p className={styles.error}>
            Could not load settings: {loadError}.{" "}
            <button type="button" className={styles.button} onClick={onBack}>
              Back to console
            </button>
          </p>
        )}

        {section === "general" && (
          <Section title="General">
            <StaticRow
              label="Display name"
              value={server?.displayName ?? "—"}
            />
            <StaticRow label="Daemon endpoint" value={server?.listen ?? "—"} />
            <StaticRow
              label="Default save path"
              value={server?.savePath || "—"}
            />
            <StaticRow
              label="Poll cadence"
              value={server ? `${server.pollMs} ms` : "—"}
            />
            <StaticRow
              label="Authentication"
              value={
                server
                  ? server.authMode === "none"
                    ? "disabled (loopback)"
                    : "password"
                  : "—"
              }
            />
            <StaticRow
              label="Daemon"
              value={server?.mock ? "mock fixtures" : "live"}
            />
          </Section>
        )}

        {(section === "connection" ||
          section === "bandwidth" ||
          section === "queue" ||
          section === "directories") && (
          <Section title={SECTIONS.find((s) => s.id === section)?.label ?? ""}>
            {daemonRows(section).map((row) => (
              <ConfigField
                key={row.id}
                row={row}
                value={draft[row.id] ?? ""}
                error={errors[row.id]}
                onChange={(value) =>
                  setDraft((current) => ({ ...current, [row.id]: value }))
                }
              />
            ))}
            {config === null && <p className={styles.meta}>Loading…</p>}
          </Section>
        )}

        {section === "labels" && (
          <Section title="Labels">
            <p className={styles.meta}>
              Labels are ruTorrent-compatible (`d.custom1`). Set one from a
              torrent's row menu or the Set label toolbar action; per-label save
              paths and seed goals are client-side preferences on the desktop
              and are not edited here yet.
            </p>
          </Section>
        )}

        {section === "interface" && (
          <Section title="Interface">
            <div className={styles.row}>
              <span className={styles.label}>Theme</span>
              <div className={styles.control}>
                <select
                  className={styles.select}
                  aria-label="Theme"
                  value={currentChoice()}
                  onChange={(event) => {
                    const choice = event.currentTarget.value as ThemeChoice;
                    applyTheme(choice, ui.accent || null);
                    saveThemeChoice(choice);
                    persistUi({ theme: choice });
                  }}
                >
                  {THEME_CHOICES.map((choice) => (
                    <option key={choice} value={choice}>
                      {choice}
                    </option>
                  ))}
                </select>
              </div>
            </div>
            <div className={styles.row}>
              <span className={styles.label}>Accent</span>
              <div className={styles.control}>
                <input
                  className={styles.input}
                  type="color"
                  aria-label="Accent colour"
                  value={ui.accent || "#f59e0b"}
                  onChange={(event) => {
                    const accent = event.currentTarget.value;
                    applyTheme(currentChoice(), accent);
                    persistUi({ accent });
                  }}
                />
                <button
                  type="button"
                  className={styles.button}
                  onClick={() => {
                    clearAccent();
                    persistUi({ accent: "" });
                  }}
                >
                  Use theme default
                </button>
              </div>
            </div>
            <div className={styles.row}>
              <span className={styles.label}>Density</span>
              <div className={styles.control}>
                <select
                  className={styles.select}
                  aria-label="Row density"
                  value={ui.density ?? "dense"}
                  onChange={(event) => {
                    const density = event.currentTarget.value as Density;
                    applyDensity(density);
                    saveDensity(density);
                    persistUi({ density });
                  }}
                >
                  {DENSITIES.map((density) => (
                    <option key={density} value={density}>
                      {density}
                    </option>
                  ))}
                </select>
              </div>
            </div>
            <p className={styles.meta}>
              These persist on the server, so the console looks the same from
              any browser. A choice made in this browser wins over the server
              default.
            </p>
          </Section>
        )}

        {section === "advanced" && (
          <Section title="Advanced">
            <StaticRow
              label=".rtorrent.rc"
              value={advanced ? advanced.path : "—"}
            />
            {advanced?.block ? (
              <>
                <p className={styles.meta}>
                  The managed block below is read-only here; the desktop tuner
                  writes it.
                </p>
                <pre className={styles.mono}>{advanced.block}</pre>
              </>
            ) : (
              <p className={styles.meta}>
                {advanced?.exists
                  ? "No managed block in .rtorrent.rc."
                  : ".rtorrent.rc was not found on the server."}
              </p>
            )}
          </Section>
        )}

        {section === "interface" && <SecuritySection />}

        {(section === "connection" ||
          section === "bandwidth" ||
          section === "queue" ||
          section === "directories") && (
          <div className={styles.footer}>
            <button
              type="button"
              className={`${styles.button} ${styles.primary}`}
              disabled={!dirty || blocked || saving}
              onClick={() => void onSave()}
            >
              {saving ? "Saving…" : "Save"}
            </button>
            <button
              type="button"
              className={styles.button}
              disabled={!dirty || saving}
              onClick={onRevert}
            >
              Revert
            </button>
            <span className={styles.grow} />
            {report && <ReportLine report={report} />}
          </div>
        )}
      </div>
    </div>
  );
}

function currentChoice(): ThemeChoice {
  return (loadThemeChoice() ?? "dark") as ThemeChoice;
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h2 className={styles.sectionTitle}>{title}</h2>
      {children}
    </section>
  );
}

function StaticRow({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <span className={styles.readonly}>{value}</span>
    </div>
  );
}

function ReportLine({ report }: { report: SaveReport }) {
  const ok = report.failed.length === 0;
  return (
    <span
      className={`${styles.report} ${ok ? styles.reportOk : styles.reportBad}`}
      role="status"
    >
      {ok
        ? `saved ${report.applied.length} setting${report.applied.length === 1 ? "" : "s"}`
        : `${report.applied.length} saved · ${report.failed.length} failed (${report.failed
            .map((failure) => failure.id)
            .join(", ")})`}
    </span>
  );
}

function ConfigField({
  row,
  value,
  error,
  onChange,
}: {
  row: ConfigRow;
  value: string;
  error?: string;
  onChange: (value: string) => void;
}) {
  const kind = row.kind;
  if (!row.available && kind.type === "readonly") {
    return (
      <div className={styles.row}>
        <span className={styles.label}>
          {row.label}
          <span className={styles.key}>{row.key}</span>
        </span>
        <span className={styles.unavailable}>not exposed by this daemon</span>
      </div>
    );
  }

  let control: React.ReactNode;
  if (kind.type === "int") {
    control = (
      <input
        className={styles.input}
        type="number"
        min={kind.min}
        max={kind.max}
        aria-label={row.label}
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
    );
  } else if (kind.type === "choice") {
    control = (
      <select
        className={styles.select}
        aria-label={row.label}
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
      >
        {kind.options.map((option) => (
          <option key={option} value={option}>
            {option}
          </option>
        ))}
      </select>
    );
  } else if (kind.type === "bool") {
    control = (
      <select
        className={styles.select}
        aria-label={row.label}
        value={value === "true" ? "1" : value === "false" ? "0" : value}
        onChange={(event) => onChange(event.currentTarget.value)}
      >
        <option value="1">on</option>
        <option value="0">off</option>
      </select>
    );
  } else if (kind.type === "readonly") {
    control = <span className={styles.readonly}>{value || "—"}</span>;
  } else {
    control = (
      <input
        className={styles.input}
        type="text"
        maxLength={kind.maxLen}
        aria-label={row.label}
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
    );
  }

  return (
    <>
      <div className={styles.row}>
        <span className={styles.label}>
          {row.label}
          <span className={styles.key}>{row.key}</span>
        </span>
        <div className={styles.control}>
          {control}
          {kind.type === "int" && kind.unit && (
            <span className={styles.unit}>{kind.unit}</span>
          )}
          {error && (
            <span className={styles.error} role="alert">
              {error}
            </span>
          )}
        </div>
      </div>
      {row.hint && <p className={styles.hint}>{row.hint}</p>}
    </>
  );
}

/** Password change; only meaningful when the server authenticates. */
function SecuritySection() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setMessage(null);
    if (next !== confirm) {
      setMessage({ ok: false, text: "the new passwords do not match" });
      return;
    }
    if (next.length < 8) {
      setMessage({
        ok: false,
        text: "the new password must be at least 8 characters",
      });
      return;
    }
    setBusy(true);
    try {
      await changePassword(current, next);
      setMessage({ ok: true, text: "password changed" });
      setCurrent("");
      setNext("");
      setConfirm("");
    } catch (error: unknown) {
      setMessage({ ok: false, text: String(error) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section>
      <h2 className={styles.sectionTitle}>Security</h2>
      <div className={styles.row}>
        <span className={styles.label}>Current password</span>
        <div className={styles.control}>
          <input
            className={styles.input}
            type="password"
            autoComplete="current-password"
            aria-label="Current password"
            value={current}
            onChange={(event) => setCurrent(event.currentTarget.value)}
          />
        </div>
      </div>
      <div className={styles.row}>
        <span className={styles.label}>New password</span>
        <div className={styles.control}>
          <input
            className={styles.input}
            type="password"
            autoComplete="new-password"
            aria-label="New password"
            value={next}
            onChange={(event) => setNext(event.currentTarget.value)}
          />
        </div>
      </div>
      <div className={styles.row}>
        <span className={styles.label}>Confirm new password</span>
        <div className={styles.control}>
          <input
            className={styles.input}
            type="password"
            autoComplete="new-password"
            aria-label="Confirm new password"
            value={confirm}
            onChange={(event) => setConfirm(event.currentTarget.value)}
          />
        </div>
      </div>
      <div className={styles.footer}>
        <button
          type="button"
          className={`${styles.button} ${styles.primary}`}
          disabled={busy || !current || !next}
          onClick={() => void submit()}
        >
          {busy ? "Changing…" : "Change password"}
        </button>
        {message && (
          <span
            className={`${styles.report} ${message.ok ? styles.reportOk : styles.reportBad}`}
            role="status"
          >
            {message.text}
          </span>
        )}
      </div>
    </section>
  );
}
