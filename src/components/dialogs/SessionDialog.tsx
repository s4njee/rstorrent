/**
 * Session dialog (V3-22 / LIB-09, LIB-10): export the library to a portable
 * manifest, and import one back — including manifests discovered from
 * qBittorrent (`BT_backup`) or Transmission (config dir) resume data.
 *
 * Export carries hashes, re-addable sources, trackers, labels/tags, paths,
 * priorities, limits and client metadata — never credentials. Import adds
 * torrents stopped, rechecks them, and resumes only verified data through a
 * crash-safe journaled job the dialog polls.
 *
 * Desktop uses native pickers (manifest path in, save path out, client folder
 * in for discovery); the browser uses a file input and blob downloads, with
 * manifest text flowing through the same commands.
 */

import { useEffect, useRef, useState } from "react";
import { useUi } from "../../store/ui";
import { capabilities } from "../../ipc/backend";
import {
  cancelImport,
  exportSession,
  exportSessionText,
  importSession,
  importStatus,
  scanForeign,
  validateSession,
  type ForeignScanReport,
  type SessionImportStatus,
  type SessionPlanItem,
  type SessionValidation,
} from "../../ipc/commands";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";

function downloadText(filename: string, text: string) {
  const blob = new Blob([text], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

const ACTION_LABEL: Record<string, string> = {
  add: "Will add",
  have: "Already here",
  skip: "Skipped",
  invalid: "Cannot restore",
};

export function SessionDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const canNative = capabilities().nativeDialogs;

  // --- export ---
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  // --- import source ---
  const [manifestPath, setManifestPath] = useState("");
  const [manifestText, setManifestText] = useState<string | null>(null);
  const [scanMsg, setScanMsg] = useState<string | null>(null);
  const [scanReport, setScanReport] = useState<ForeignScanReport | null>(null);
  const [scanning, setScanning] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // --- preview / import ---
  const [remapFrom, setRemapFrom] = useState("");
  const [remapTo, setRemapTo] = useState("");
  const [preview, setPreview] = useState<SessionValidation | null>(null);
  const [validating, setValidating] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [importError, setImportError] = useState<string | null>(null);
  const [status, setStatus] = useState<SessionImportStatus | null>(null);
  const pollRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (pollRef.current !== null) window.clearInterval(pollRef.current);
    },
    [],
  );

  const sourceArgs = (): { path?: string; manifestText?: string } | null => {
    if (manifestText !== null) return { manifestText };
    if (manifestPath.trim()) return { path: manifestPath.trim() };
    return null;
  };

  // --- export actions ---

  const doExport = async () => {
    setExporting(true);
    setExportMsg(null);
    try {
      if (canNative) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const picked = await save({
          defaultPath: "session-manifest.json",
          filters: [{ name: "Session manifest", extensions: ["json"] }],
        });
        if (typeof picked !== "string") {
          setExportMsg("export cancelled.");
          return;
        }
        const report = await exportSession(picked);
        const sourceless = report.torrents - report.withSources;
        setExportMsg(
          `exported ${report.torrents} torrent(s)${sourceless > 0 ? ` — ${sourceless} without a re-addable source (re-add those by hand first for a complete backup)` : " — every entry has a re-addable source"}.`,
        );
      } else {
        const text = await exportSessionText();
        const count = JSON.parse(text).torrents?.length ?? 0;
        downloadText("session-manifest.json", text);
        setExportMsg(`exported ${count} torrent(s).`);
      }
    } catch (e) {
      setExportMsg(`export failed: ${String(e)}`);
    } finally {
      setExporting(false);
    }
  };

  // --- import source actions ---

  const resetPreview = () => {
    setPreview(null);
    setSelected(new Set());
    setImportError(null);
    setStatus(null);
    if (pollRef.current !== null) {
      window.clearInterval(pollRef.current);
      pollRef.current = null;
    }
  };

  const chooseManifestFile = async () => {
    if (!canNative) {
      fileInputRef.current?.click();
      return;
    }
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Session manifest", extensions: ["json"] }],
      });
      if (typeof picked === "string") {
        resetPreview();
        setManifestText(null);
        setScanReport(null);
        setScanMsg(null);
        setManifestPath(picked);
      }
    } catch (e) {
      setImportError(String(e));
    }
  };

  const onWebFile = async (file: File) => {
    try {
      const text = await file.text();
      resetPreview();
      setManifestPath("");
      setScanReport(null);
      setScanMsg(null);
      setManifestText(text);
    } catch (e) {
      setImportError(`could not read file: ${String(e)}`);
    }
  };

  const doScan = async (client: "qbittorrent" | "transmission") => {
    if (!canNative) {
      setScanMsg(
        "discovery reads the other client's files on the daemon host — upload a manifest instead, or run this from the desktop app.",
      );
      return;
    }
    setScanning(true);
    setScanMsg(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, directory: true });
      if (typeof picked !== "string") return;
      const report = await scanForeign(client, picked);
      resetPreview();
      setManifestPath("");
      setManifestText(report.manifestText);
      setScanReport(report);
      const warnBits = report.problems
        .slice(0, 3)
        .map((p) => p.message)
        .join(" · ");
      setScanMsg(
        `found ${report.entryCount} torrent(s), ${report.restorableCount} with sources` +
          (report.problems.length > 0
            ? ` — ${report.problems.length} note(s), e.g. ${warnBits}`
            : ""),
      );
    } catch (e) {
      setScanMsg(`scan failed: ${String(e)}`);
    } finally {
      setScanning(false);
    }
  };

  // --- validate / import ---

  const doValidate = async () => {
    const source = sourceArgs();
    if (!source) {
      setImportError("pick a manifest file, scan another client, or paste manifest text first.");
      return;
    }
    setValidating(true);
    setImportError(null);
    try {
      const report = await validateSession({
        ...source,
        remapFrom: remapFrom.trim() || undefined,
        remapTo: remapTo.trim() || undefined,
      });
      setPreview(report);
      setSelected(
        new Set(report.items.filter((i) => i.action === "add").map((i) => i.hash)),
      );
    } catch (e) {
      setImportError(String(e));
    } finally {
      setValidating(false);
    }
  };

  const toggleSelected = (hash: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(hash)) next.delete(hash);
      else next.add(hash);
      return next;
    });
  };

  const doImport = async (resume: boolean) => {
    const source = sourceArgs();
    if (!source || !preview) return;
    setImportError(null);
    try {
      await importSession({
        ...source,
        selected: [...selected],
        remapFrom: remapFrom.trim() || undefined,
        remapTo: remapTo.trim() || undefined,
        resume,
      });
      if (pollRef.current !== null) window.clearInterval(pollRef.current);
      const poll = async () => {
        try {
          const s = await importStatus();
          setStatus(s);
          if (!s.running) {
            if (pollRef.current !== null) {
              window.clearInterval(pollRef.current);
              pollRef.current = null;
            }
          }
        } catch {
          // A failed poll is a network wobble, not a failed import; the
          // journal keeps the run safe and the next poll recovers.
        }
      };
      await poll();
      pollRef.current = window.setInterval(() => void poll(), 1000);
    } catch (e) {
      setImportError(String(e));
    }
  };

  const addable = preview?.items.filter((i) => i.action === "add") ?? [];
  const importRunning = status?.running ?? false;

  return (
    <ModalBase
      title="Session export / import"
      width={640}
      onCancel={closeDialog}
      footer={
        <Button variant="primary" onClick={closeDialog}>
          Close
        </Button>
      }
    >
      <div className={forms.col}>
        <div className={forms.section}>Export</div>
        <span className={forms.meta}>
          A portable manifest of hashes, re-addable sources, trackers,
          labels/tags, paths, priorities, limits and client metadata — never
          credentials. Torrents whose `.torrent` cannot be found export without
          a source and are flagged, not dropped.
        </span>
        <div className={forms.field}>
          <Button variant="secondary" disabled={exporting} onClick={() => void doExport()}>
            {exporting ? "Exporting…" : canNative ? "Export to file…" : "Download manifest"}
          </Button>
        </div>
        {exportMsg && <span className={forms.meta}>{exportMsg}</span>}

        <div className={forms.section}>Import</div>
        <span className={forms.meta}>
          Manifests import stopped, recheck, then resume only verified data. A
          crash-safe journal resumes interrupted runs instead of re-adding.
        </span>
        <div className={forms.field}>
          <Button variant="secondary" onClick={() => void chooseManifestFile()}>
            {canNative ? "Choose manifest…" : "Upload manifest…"}
          </Button>
          {manifestPath && <span className={forms.meta}>{manifestPath}</span>}
          {manifestText !== null && !scanReport && (
            <span className={forms.meta}>manifest text loaded.</span>
          )}
        </div>
        <input
          ref={fileInputRef}
          type="file"
          accept=".json,application/json"
          className={forms.hidden}
          aria-label="Upload manifest file"
          onChange={(e) => {
            const file = e.target.files?.[0];
            if (file) void onWebFile(file);
            e.target.value = "";
          }}
        />
        <div className={forms.field}>
          <span className={forms.meta}>…or discover from another client:</span>
          <Button variant="secondary" disabled={scanning} onClick={() => void doScan("qbittorrent")}>
            Scan qBittorrent…
          </Button>
          <Button variant="secondary" disabled={scanning} onClick={() => void doScan("transmission")}>
            Scan Transmission…
          </Button>
        </div>
        {scanMsg && <span className={forms.meta}>{scanMsg}</span>}
        {scanReport && scanReport.problems.length > 0 && (
          <div className={forms.col}>
            {scanReport.problems.slice(0, 8).map((p, i) => (
              <span key={i} className={forms.meta}>
                {p.file.split(/[/\\]/).pop()}: {p.message}
              </span>
            ))}
            {scanReport.problems.length > 8 && (
              <span className={forms.meta}>
                …and {scanReport.problems.length - 8} more (see the exported manifest).
              </span>
            )}
          </div>
        )}

        <div className={forms.field}>
          <label className={forms.fieldLabel} htmlFor="session-remap-from">
            Move paths from
          </label>
          <input
            id="session-remap-from"
            className={forms.input}
            value={remapFrom}
            onChange={(e) => setRemapFrom(e.target.value)}
            placeholder="/old/media"
            spellCheck={false}
          />
          <label className={forms.fieldLabel} htmlFor="session-remap-to">
            to
          </label>
          <input
            id="session-remap-to"
            className={forms.input}
            value={remapTo}
            onChange={(e) => setRemapTo(e.target.value)}
            placeholder="/srv/media"
            spellCheck={false}
          />
          <Button
            variant="secondary"
            disabled={validating || sourceArgs() === null}
            onClick={() => void doValidate()}
          >
            {validating ? "Checking…" : "Preview"}
          </Button>
        </div>

        {importError && <div className={forms.error}>{importError}</div>}

        {preview && (
          <>
            <span className={forms.meta}>
              {preview.torrentCount} in manifest · {preview.restorableCount} restorable ·{" "}
              {addable.length} to add · {preview.errors.length} error(s) ·{" "}
              {preview.warnings.length} warning(s)
            </span>
            {preview.errors.slice(0, 5).map((e, i) => (
              <div key={`e${i}`} className={forms.error}>
                {e.hash ? `${e.hash.slice(0, 8)}…: ` : ""}{e.message}
              </div>
            ))}
            <div className={forms.col} style={{ maxHeight: 220, overflowY: "auto" }}>
              {preview.items.map((item) => (
                <PlanRow
                  key={item.hash}
                  item={item}
                  checked={selected.has(item.hash)}
                  onToggle={() => toggleSelected(item.hash)}
                />
              ))}
            </div>
            <div className={forms.field}>
              <Button
                variant="primary"
                disabled={importRunning || selected.size === 0}
                onClick={() => void doImport(false)}
              >
                {importRunning ? "Importing…" : `Import ${selected.size} stopped`}
              </Button>
              <Button
                variant="secondary"
                disabled={importRunning || selected.size === 0}
                title="Continue an interrupted journal for this manifest instead of starting over"
                onClick={() => void doImport(true)}
              >
                Resume previous
              </Button>
              {importRunning && (
                <Button variant="secondary" onClick={() => void cancelImport().catch(() => {})}>
                  Cancel
                </Button>
              )}
            </div>
          </>
        )}

        {status && (
          <span className={forms.meta}>
            {status.running
              ? `importing… ${status.added} added, ${status.skipped} skipped, ${status.failed.length} failed`
              : `finished: ${status.added} added, ${status.resumed} resumed, ${status.skipped} skipped, ${status.failed.length} failed`}
            {status.failed.slice(0, 3).map((f, i) => (
              <span key={i} style={{ display: "block" }}>{f}</span>
            ))}
          </span>
        )}
      </div>
    </ModalBase>
  );
}

function PlanRow({
  item,
  checked,
  onToggle,
}: {
  item: SessionPlanItem;
  checked: boolean;
  onToggle: () => void;
}) {
  const checkable = item.action === "add";
  return (
    <div className={forms.field} style={{ alignItems: "flex-start" }}>
      {checkable ? (
        <input
          type="checkbox"
          checked={checked}
          onChange={onToggle}
          aria-label={`Import ${item.name}`}
        />
      ) : (
        <span className={forms.meta} aria-hidden="true">·</span>
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
          {item.name}{" "}
          <span className={forms.meta}>
            {ACTION_LABEL[item.action] ?? item.action}
            {item.reason ? ` — ${item.reason}` : ""}
          </span>
        </div>
        {item.dstDir && (
          <div className={forms.meta} style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
            → {item.dstDir}
          </div>
        )}
      </div>
    </div>
  );
}
