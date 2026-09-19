/**
 * Create Torrent Dialog (B13).
 *
 * Allows users to create a .torrent file from a single file or directory,
 * configure trackers, piece length (auto or manual), private flags, comments,
 * and optionally start seeding the torrent immediately.
 */

import { useState } from "react";
import { useUi } from "../../store/ui";
import { useSettings } from "../../store/settings";
import { createTorrent } from "../../ipc/commands";
import type { CreateTorrentResult } from "../../ipc/types";
import { ModalBase, Button } from "./ModalBase";
import { Checkbox } from "./Checkbox";
import forms from "./forms.module.css";
import styles from "./CreateTorrentDialog.module.css";

const PIECE_SIZE_OPTIONS = [
  { label: "Auto (optimal)", value: 0 },
  { label: "16 KiB", value: 16384 },
  { label: "32 KiB", value: 32768 },
  { label: "64 KiB", value: 65536 },
  { label: "128 KiB", value: 131072 },
  { label: "256 KiB", value: 262144 },
  { label: "512 KiB", value: 524288 },
  { label: "1 MiB", value: 1048576 },
  { label: "2 MiB", value: 2097152 },
  { label: "4 MiB", value: 4194304 },
  { label: "8 MiB", value: 8388608 },
  { label: "16 MiB", value: 16777216 },
  { label: "32 MiB", value: 33554432 },
];

export function CreateTorrentDialog() {
  const closeDialog = useUi((s) => s.closeDialog);
  const settings = useSettings((s) => s.settings);

  const [sourcePath, setSourcePath] = useState("");
  const [outputPath, setOutputPath] = useState("");
  const [trackersText, setTrackersText] = useState("");
  const [pieceLength, setPieceLength] = useState(0);
  const [isPrivate, setIsPrivate] = useState(false);
  const [startSeeding, setStartSeeding] = useState(false);
  const [comment, setComment] = useState("");
  const [source, setSource] = useState("");

  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<CreateTorrentResult | null>(null);

  const suggestOutputPath = (src: string) => {
    if (!outputPath) {
      // Suggest saving in defaultSavePath or alongside source
      const trimmed = src.replace(/[/\\]+$/, "");
      const name = trimmed.split(/[/\\]/).pop() || "new";
      const defaultDir = settings?.defaultSavePath || "";
      if (defaultDir) {
        const sep = defaultDir.includes("\\") ? "\\" : "/";
        setOutputPath(`${defaultDir}${sep}${name}.torrent`);
      } else {
        setOutputPath(`${trimmed}.torrent`);
      }
    }
  };

  const handleSubmit = async () => {
    if (!sourcePath.trim()) {
      setError("Enter the path of a file or folder on the host");
      return;
    }

    setSubmitting(true);
    setError(null);

    const trackers = trackersText
      .split("\n")
      .map((t) => t.trim())
      .filter((t) => t.length > 0);

    try {
      const res = await createTorrent({
        sourcePath: sourcePath.trim(),
        outputPath: outputPath.trim() || null,
        pieceLength: pieceLength > 0 ? pieceLength : null,
        trackers,
        isPrivate,
        comment: comment.trim() || null,
        source: source.trim() || null,
        startSeeding,
      });

      setResult(res);
      setTimeout(() => {
        closeDialog();
      }, 1200);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setSubmitting(false);
    }
  };

  return (
    <ModalBase
      title="Create New Torrent"
      onCancel={closeDialog}
      width={560}
      footer={
        <>
          <Button
            variant="secondary"
            onClick={closeDialog}
            disabled={submitting}
          >
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => void handleSubmit()}
            disabled={submitting || !sourcePath.trim()}
          >
            {submitting ? "Creating…" : "Create Torrent"}
          </Button>
        </>
      }
    >
      <div className={styles.dialog}>
        {error && <div className={styles.error}>{error}</div>}
        {result && (
          <div className={styles.success}>
            <div>
              Created torrent <strong>{result.name}</strong> (
              {result.pieceCount} pieces)
            </div>
            <div className={styles.successHash}>
              Info-hash: {result.infoHash}
            </div>
          </div>
        )}

        {/* Source selection */}
        <div className={styles.section}>
          <div className={styles.sectionTitle}>Source</div>
          <div className={styles.row}>
            <input
              className={forms.input}
              placeholder="Path to file or folder on the host"
              value={sourcePath}
              onChange={(e) => {
                setSourcePath(e.currentTarget.value);
                suggestOutputPath(e.currentTarget.value);
              }}
              disabled={submitting}
              spellCheck={false}
            />
          </div>
        </div>

        {/* Output file destination */}
        <div className={styles.section}>
          <div className={styles.sectionTitle}>Save .torrent File</div>
          <div className={styles.row}>
            <input
              className={forms.input}
              placeholder="Output .torrent path (optional)"
              value={outputPath}
              onChange={(e) => setOutputPath(e.currentTarget.value)}
              disabled={submitting}
              spellCheck={false}
            />
          </div>
        </div>

        {/* Trackers */}
        <div className={styles.section}>
          <div className={styles.sectionTitle}>Trackers (Announce URLs)</div>
          <textarea
            className={styles.trackersTextarea}
            placeholder={`http://tracker.example.com:80/announce\nudp://tracker.opentrackr.org:1337/announce\n\n(Separate tiers with blank lines)`}
            value={trackersText}
            onChange={(e) => setTrackersText(e.currentTarget.value)}
            disabled={submitting}
            spellCheck={false}
          />
          <div className={styles.hint}>
            Enter announce URLs one per line. Blank lines separate tracker
            tiers.
          </div>
        </div>

        {/* Settings grid: Piece size & Metadata */}
        <div className={styles.grid}>
          <div className={styles.section}>
            <label className={styles.sectionTitle} htmlFor="piece-size-select">
              Piece Size
            </label>
            <select
              id="piece-size-select"
              className={forms.input}
              value={pieceLength}
              onChange={(e) => setPieceLength(Number(e.currentTarget.value))}
              disabled={submitting}
            >
              {PIECE_SIZE_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </div>

          <div className={styles.section}>
            <label
              className={styles.sectionTitle}
              htmlFor="torrent-source-input"
            >
              Source (optional)
            </label>
            <input
              id="torrent-source-input"
              className={forms.input}
              placeholder="e.g. Private Tracker Name"
              value={source}
              onChange={(e) => setSource(e.currentTarget.value)}
              disabled={submitting}
              spellCheck={false}
            />
          </div>
        </div>

        {/* Comment */}
        <div className={styles.section}>
          <label
            className={styles.sectionTitle}
            htmlFor="torrent-comment-input"
          >
            Comment (optional)
          </label>
          <input
            id="torrent-comment-input"
            className={forms.input}
            placeholder="Description or torrent notes"
            value={comment}
            onChange={(e) => setComment(e.currentTarget.value)}
            disabled={submitting}
          />
        </div>

        {/* Checkboxes */}
        <div className={styles.optionsList}>
          <Checkbox
            checked={isPrivate}
            onChange={setIsPrivate}
            label="Private torrent (disable DHT and Peer Exchange / BEP 27)"
            disabled={submitting}
          />
          <Checkbox
            checked={startSeeding}
            onChange={setStartSeeding}
            label="Start seeding immediately in daemon"
            disabled={submitting}
          />
        </div>
      </div>
    </ModalBase>
  );
}
