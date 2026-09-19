/**
 * Session dialog (V3-22 / LIB-09): export the library to a portable
 * manifest, and import one back.
 *
 * Export carries hashes, re-addable sources, trackers, labels/tags, paths,
 * priorities, limits and client metadata — never credentials. Import adds
 * torrents stopped, rechecks them, and resumes only verified data through a
 * crash-safe journaled job the dialog polls.
 *
 * Export downloads the manifest as a blob; import uploads one through a file
 * input, and the manifest text flows through the same server commands.
 */

import { useEffect, useRef, useState } from "react";
import { useUi } from "../../store/ui";
import {
  cancelImport,
  exportSessionText,
  importSession,
  importStatus,
  validateSession,
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

  // --- export ---
  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  // --- import source ---
  const [manifestText, setManifestText] = useState<string | null>(null);
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

  const sourceArgs = (): { manifestText: string } | null =>
    manifestText !== null ? { manifestText } : null;

  // --- export actions ---

  const doExport = async () => {
    setExporting(true);
    setExportMsg(null);
    try {
      const text = await exportSessionText();
      const count = JSON.parse(text).torrents?.length ?? 0;
      downloadText("session-manifest.json", text);
      setExportMsg(`exported ${count} torrent(s).`);
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

  const onWebFile = async (file: File) => {
    try {
      const text = await file.text();
      resetPreview();
      setManifestText(text);
    } catch (e) {
      setImportError(`could not read file: ${String(e)}`);
    }
  };

  // --- validate / import ---

  const doValidate = async () => {
    const source = sourceArgs();
    if (!source) {
      setImportError("upload a manifest file first.");
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
        new Set(
          report.items.filter((i) => i.action === "add").map((i) => i.hash),
        ),
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
          <Button
            variant="secondary"
            disabled={exporting}
            onClick={() => void doExport()}
          >
            {exporting ? "Exporting…" : "Download manifest"}
          </Button>
        </div>
        {exportMsg && <span className={forms.meta}>{exportMsg}</span>}

        <div className={forms.section}>Import</div>
        <span className={forms.meta}>
          Manifests import stopped, recheck, then resume only verified data. A
          crash-safe journal resumes interrupted runs instead of re-adding.
        </span>
        <div className={forms.field}>
          <Button
            variant="secondary"
            onClick={() => fileInputRef.current?.click()}
          >
            Upload manifest…
          </Button>
          {manifestText !== null && (
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
              {preview.torrentCount} in manifest · {preview.restorableCount}{" "}
              restorable · {addable.length} to add · {preview.errors.length}{" "}
              error(s) · {preview.warnings.length} warning(s)
            </span>
            {preview.errors.slice(0, 5).map((e, i) => (
              <div key={`e${i}`} className={forms.error}>
                {e.hash ? `${e.hash.slice(0, 8)}…: ` : ""}
                {e.message}
              </div>
            ))}
            <div
              className={forms.col}
              style={{ maxHeight: 220, overflowY: "auto" }}
            >
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
                {importRunning
                  ? "Importing…"
                  : `Import ${selected.size} stopped`}
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
                <Button
                  variant="secondary"
                  onClick={() => void cancelImport().catch(() => {})}
                >
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
              <span key={i} style={{ display: "block" }}>
                {f}
              </span>
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
        <span className={forms.meta} aria-hidden="true">
          ·
        </span>
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
          <div
            className={forms.meta}
            style={{ overflow: "hidden", textOverflow: "ellipsis" }}
          >
            → {item.dstDir}
          </div>
        )}
      </div>
    </div>
  );
}
