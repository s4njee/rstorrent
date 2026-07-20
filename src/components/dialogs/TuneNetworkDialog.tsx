/**
 * "Tune for 1 Gbps" dialog (menu action).
 *
 * Previews the exact block the tuner will write to `.rtorrent.rc` and, on
 * Apply, writes it and pushes the same values to the running daemon over
 * XML-RPC. rtorrent only reads its rc file at startup, so the live-apply is what
 * makes most of the profile take effect without a restart; the file write is
 * what makes it survive one. A remote daemon's rc file isn't reachable from
 * here, so for those the tuner applies live-only (surfaced in the copy).
 */

import { useEffect, useState } from "react";
import { useUi } from "../../store/ui";
import { tuningPreview, applyTuning } from "../../ipc/commands";
import type { TuningPreview, TuningResult } from "../../ipc/types";
import { ModalBase, Button } from "./ModalBase";
import forms from "./forms.module.css";

const codeBlock: React.CSSProperties = {
  margin: 0,
  padding: 10,
  maxHeight: 260,
  overflow: "auto",
  background: "var(--bg-inset, rgba(0,0,0,0.2))",
  border: "1px solid var(--border-row)",
  borderRadius: 4,
  fontFamily: "var(--font-mono, monospace)",
  fontSize: 12,
  lineHeight: 1.5,
  whiteSpace: "pre",
};

export function TuneNetworkDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const [preview, setPreview] = useState<TuningPreview | null>(null);
  const [applying, setApplying] = useState(false);
  const [result, setResult] = useState<TuningResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void tuningPreview().then(setPreview);
  }, []);

  const apply = () => {
    if (applying) return;
    setApplying(true);
    setError(null);
    applyTuning()
      .then(setResult)
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setApplying(false));
  };

  const done = result !== null;

  return (
    <ModalBase
      title="Tune for 1 Gbps"
      width={520}
      onCancel={closeDialog}
      onPrimary={done ? closeDialog : apply}
      footer={
        done ? (
          <Button variant="primary" onClick={closeDialog}>
            Close
          </Button>
        ) : (
          <>
            <Button
              variant="secondary"
              onClick={closeDialog}
              disabled={applying}
            >
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={apply}
              disabled={applying || !preview}
            >
              {applying ? "Applying…" : "Apply tuning"}
            </Button>
          </>
        )
      }
    >
      {!preview ? (
        <div className={forms.meta}>loading…</div>
      ) : done ? (
        <ResultView result={result} />
      ) : (
        <div className={forms.col}>
          <div className={forms.meta}>
            Writes network settings tuned for a 1 Gbps connection and applies
            them to the running daemon. Most take effect immediately; a few need
            a daemon restart.
          </div>

          {error && (
            <div
              className={forms.meta}
              style={{ color: "var(--accent-red-soft, #ea6962)" }}
            >
              {error}
            </div>
          )}

          {preview.canWriteFile ? (
            <div className={forms.meta}>
              Target file: <code>{preview.rcPath}</code>
            </div>
          ) : (
            <div
              className={forms.meta}
              style={{ color: "var(--accent-amber-soft, #d8a657)" }}
            >
              This is a remote daemon, so its <code>.rtorrent.rc</code> can’t be
              edited from here — the settings will be applied live over XML-RPC
              only, and won’t survive a daemon restart.
            </div>
          )}

          <pre style={codeBlock}>{preview.block}</pre>
        </div>
      )}
    </ModalBase>
  );
}

function ResultView({ result }: { result: TuningResult }) {
  return (
    <div className={forms.col}>
      {result.fileWritten ? (
        <div className={forms.meta}>
          Wrote <code>{result.rcPath}</code>.
        </div>
      ) : result.fileError ? (
        <div
          className={forms.meta}
          style={{ color: "var(--accent-red-soft, #ea6962)" }}
        >
          Could not write the rc file: {result.fileError}
        </div>
      ) : (
        <div className={forms.meta}>
          No local rc file to write (remote daemon) — applied live only.
        </div>
      )}

      {result.liveError ? (
        <div
          className={forms.meta}
          style={{ color: "var(--accent-red-soft, #ea6962)" }}
        >
          Could not reach the daemon to apply live: {result.liveError}
        </div>
      ) : (
        <div className={forms.meta}>
          Applied {result.liveApplied} of {result.liveTotal} settings to the
          running daemon.
          {result.liveApplied < result.liveTotal &&
            " The rest need a daemon restart (older builds reject some at runtime)."}
        </div>
      )}
    </div>
  );
}
