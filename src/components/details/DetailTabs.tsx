/**
 * Detail panel for the selected torrent.
 *
 * The tab strip switches the active tab (persisted in the UI store and used to
 * steer the Rust detail poll). The General tab is derived from the snapshot; the
 * Trackers/Peers/Content tabs render data pushed via `state://detail` (wired in
 * App via `onDetail`); Speed/Log are placeholders until E10-S6/S7.
 */

import {
  Fragment,
  useState,
  useEffect,
  useMemo,
  useRef,
  type FormEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { useTorrents } from "../../store/torrents";
import { useSettings } from "../../store/settings";
import { useUi } from "../../store/ui";
import { useDetail } from "../../store/detail";
import { useLog } from "../../store/log";
import {
  addTracker,
  banPeer,
  disconnectPeer,
  removeTracker,
  setFilePriority,
  setTrackerEnabled,
  snubPeer,
} from "../../ipc/commands";
import type {
  FileNode,
  PeerRow,
  PieceInfo,
  Status,
  TorrentDto,
  TrackerRow,
} from "../../ipc/types";
import {
  formatBytes,
  formatRate,
  formatAgo,
  formatCountdown,
} from "../../utils/format";
import { SpeedChart } from "./SpeedChart";
import { PieceBar } from "./PieceBar";
import { AvailabilityBar } from "./AvailabilityBar";
import { ProgressBar } from "../table/ProgressBar";
import { PauseIcon, PlayIcon, RemoveIcon } from "../icons";
import menuStyles from "../menu/ContextMenu.module.css";
import styles from "./DetailTabs.module.css";
import { buildTree, leafIndexes, type TreeNode } from "../../utils/filetree";
import { availabilityToBytes, distributedCopies } from "../../utils/bitfield";
import { trackerTone } from "../../utils/status";
import { PANES, paneHasRail, type DetailPane } from "../../utils/panes";
import {
  extraFacts,
  primaryFacts,
  type Fact,
  type FactTone,
} from "../../utils/facts";
import { forceReannounce } from "../../ipc/commands";

/**
 * The torrent the panel describes.
 *
 * Multi-selection keeps its subject: the anchor is the row that was clicked
 * last, so a shift-range or a cmd-click set still shows something instead of
 * blanking the panel (WC5-S8). If the anchor is no longer selected (a toggle
 * removed it), the first selected row stands in.
 */
export function focusedHashOf(
  selection: Set<string>,
  anchor: string | null,
): string | null {
  if (selection.size === 0) return null;
  if (anchor && selection.has(anchor)) return anchor;
  return [...selection][0] ?? null;
}

export function DetailTabs() {
  const activeTab = useUi((s) => s.activeTab);
  const setActiveTab = useUi((s) => s.setActiveTab);
  const selection = useUi((s) => s.selection);
  const anchor = useUi((s) => s.anchor);
  const torrents = useTorrents((s) => s.torrents);
  const detail = useDetail((s) => s.data);

  const focusedHash = focusedHashOf(selection, anchor);
  const torrent = focusedHash
    ? (torrents.find((t) => t.hash === focusedHash) ?? null)
    : null;
  const forThis = torrent && detail?.hash === torrent.hash ? detail : null;

  return (
    <div className={styles.panel}>
      <div className={styles.tabs} role="tablist">
        {PANES.map((pane) => (
          <button
            key={pane.id}
            type="button"
            id={`detail-tab-${pane.id}`}
            role="tab"
            aria-selected={activeTab === pane.id}
            // The panel below names itself from the selected tab.
            aria-controls="detail-pane"
            className={`${styles.tab} ${activeTab === pane.id ? styles.active : ""}`}
            onClick={() => setActiveTab(pane.id)}
          >
            {pane.label}
          </button>
        ))}
        {/* The focused torrent, right-aligned and truncated — the panel's answer
            to "which one am I looking at?" while the table scrolls. */}
        <span className={styles.focused} title={torrent?.name}>
          {torrent?.name ?? ""}
        </span>
      </div>
      <div className={styles.body}>
        <div
          id="detail-pane"
          className={styles.pane}
          role="tabpanel"
          aria-labelledby={`detail-tab-${activeTab}`}
        >
          {!torrent ? (
            <div className={styles.placeholder}>
              {selection.size > 1
                ? "select a torrent to see its details"
                : "select a torrent to see its files, peers and trackers"}
            </div>
          ) : (
            <>
              <ErrorBanner torrent={torrent} />
              <PaneContent
                pane={activeTab}
                torrent={torrent}
                detail={forThis}
              />
            </>
          )}
        </div>
        {torrent && paneHasRail(activeTab) && (
          <FactsRail torrent={torrent} pieces={forThis?.pieces} />
        )}
      </div>
    </div>
  );
}

/**
 * Why a torrent is failing, wherever the panel happens to be — the tracker
 * error, the storage error, or just the error state with no explanation.
 */
function ErrorBanner({ torrent: t }: { torrent: TorrentDto }) {
  if (t.status !== "error" || !t.statusMsg) return null;
  const hint = errorHint(t.errorKind ?? "");
  return (
    <div className={styles.errorBanner} role="status">
      <div className={styles.errorKind}>
        {t.errorKind ? t.errorKind.replace(/_/g, " ") : "error"}: {t.statusMsg}
      </div>
      {hint && <div className={styles.errorHint}>{hint}</div>}
    </div>
  );
}

function PaneContent({
  pane,
  torrent,
  detail,
}: {
  pane: DetailPane;
  torrent: TorrentDto;
  detail: ReturnType<typeof useDetail.getState>["data"];
}) {
  switch (pane) {
    case "files":
      // Keyed by hash so the optimistic-priority state resets when the selected
      // torrent changes (otherwise an override would bleed onto another torrent).
      return (
        <FilesTable
          key={torrent.hash}
          hash={torrent.hash}
          files={detail?.files ?? []}
        />
      );
    case "peers":
      return (
        <PeersTable
          key={torrent.hash}
          hash={torrent.hash}
          peers={detail?.peers ?? []}
        />
      );
    case "trackers":
      return (
        <TrackersTable
          key={torrent.hash}
          hash={torrent.hash}
          trackers={detail?.trackers ?? []}
          isPrivate={torrent.isPrivate}
        />
      );
    case "transfer":
      return <TransferPane hash={torrent.hash} />;
    case "pieces":
      return <PiecesPane torrent={torrent} pieces={detail?.pieces} />;
    case "log":
      return <LogView hash={torrent.hash} />;
  }
}

/**
 * Transfer: the per-torrent throughput chart. The panel's own facts live in the
 * rail beside it, so this pane is the chart and nothing else.
 */
function TransferPane({ hash }: { hash: string }) {
  return (
    <div className={styles.transfer}>
      <SpeedChart hash={hash} />
    </div>
  );
}

/**
 * Pieces: the piece stripe and the swarm's availability for those pieces, with
 * the figures the tables above them cannot show.
 */
function PiecesPane({
  torrent: t,
  pieces,
}: {
  torrent: TorrentDto;
  pieces?: PieceInfo;
}) {
  if (!pieces || pieces.sizeChunks <= 0) {
    return (
      <div className={styles.placeholder}>
        waiting for the piece map — it arrives with the detail poll
      </div>
    );
  }
  const counts = availabilityToBytes(pieces.availability ?? "");
  const copies = distributedCopies(counts, pieces.sizeChunks);
  const rows: Fact[] = [
    [
      "Chunks",
      `${pieces.completedChunks.toLocaleString()} / ${pieces.sizeChunks.toLocaleString()}`,
    ],
    ["Chunk size", pieces.chunkSize > 0 ? formatBytes(pieces.chunkSize) : "—"],
    ["Piece map", formatBytes(pieces.sizeChunks * pieces.chunkSize)],
    ["Distributed copies", copies.toFixed(2)],
  ];
  return (
    <div className={styles.pieces}>
      <PieceBar pieces={pieces} status={t.status} />
      <AvailabilityBar pieces={pieces} />
      <div className={styles.factGrid}>
        {rows.map(([key, value]) => (
          <div key={key} className={styles.fact}>
            <span className={styles.factKey}>{key}</span>
            <span className={styles.factValue}>{value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * The facts rail: key/value rows, the handoff's eleven first, then ours.
 * Values truncate rather than wrap, and every one carries its full text as a
 * title — a hash or a path is worth reading in full when it matters.
 */
function FactsRail({
  torrent,
  pieces,
}: {
  torrent: TorrentDto;
  pieces?: PieceInfo;
}) {
  const g = useTorrents((s) => s.globals);
  const rules = useSettings((s) => s.settings?.bandwidthRules ?? []);
  const primary = primaryFacts(torrent, pieces);
  const extra = extraFacts(
    torrent,
    g.downRateLimit,
    g.upRateLimit,
    rules,
    g.turtleActive,
  );
  return (
    <div className={styles.rail}>
      {primary.map(([key, value, tone]) => (
        <FactRow key={key} name={key} value={value} tone={tone} />
      ))}
      <div className={styles.railDivider} />
      {extra.map(([key, value, tone]) => (
        <FactRow key={key} name={key} value={value} tone={tone} />
      ))}
    </div>
  );
}

function FactRow({
  name,
  value,
  tone,
}: {
  name: string;
  value: string;
  tone?: FactTone;
}) {
  return (
    <div className={styles.fact}>
      <span className={styles.factKey}>{name}</span>
      <span
        className={`${styles.factValue} ${tone ? styles[tone] : ""}`}
        title={value}
      >
        {value}
      </span>
    </div>
  );
}

interface TrackerMenuState {
  x: number;
  y: number;
  tracker: TrackerRow;
}

/** Tracker detail rows with inline add and row-level management. */
function TrackersTable({
  hash,
  trackers,
  isPrivate,
}: {
  hash: string;
  trackers: TrackerRow[];
  /** A private torrent runs with DHT and peer exchange off. */
  isPrivate: boolean;
}) {
  const [url, setUrl] = useState("");
  const [adding, setAdding] = useState(false);
  const [menu, setMenu] = useState<TrackerMenuState | null>(null);
  // The footer's Remove acts on this row; the row menu acts on its own.
  const [active, setActive] = useState<number | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const announceUrl = url.trim();
    if (!announceUrl || adding) return;
    setAdding(true);
    try {
      await addTracker(hash, announceUrl);
      setUrl("");
    } catch {
      // The Rust command writes the failure to the app log.
    } finally {
      setAdding(false);
    }
  };

  const runMenuAction = async (action: () => Promise<void>) => {
    setMenu(null);
    try {
      await action();
    } catch {
      // The Rust command writes the failure to the app log.
    }
  };

  return (
    <div className={styles.trackerPane}>
      <div className={styles.trackerList}>
        {trackers.map((tracker) => {
          const tone = trackerTone(tracker.status, tracker.enabled);
          return (
            <div
              key={`${tracker.index}:${tracker.url}`}
              className={`${styles.trackerRow} ${active === tracker.index ? styles.trackerRowActive : ""} ${tracker.enabled ? "" : styles.trackerDisabled}`}
              onClick={() => setActive(tracker.index)}
              // The design's row shows status rather than type and last-announce;
              // both stay reachable here rather than being dropped.
              title={`${tracker.url}\n${tracker.kind || "public"} · last announce ${formatAgo(tracker.lastAnnounce)}`}
              onContextMenu={(event) => {
                event.preventDefault();
                setMenu({ x: event.clientX, y: event.clientY, tracker });
              }}
            >
              <span
                className={`${styles.dot} ${styles[`dot${tone}`]}`}
                aria-hidden="true"
              />
              <span className={styles.trackerUrl}>{tracker.url}</span>
              <span
                className={`${styles.trackerStatus} ${styles[`text${tone}`]}`}
              >
                {trackerStatusWord(tracker)}
              </span>
              <span className={styles.trackerSeeds}>
                {tracker.enabled
                  ? `${tracker.seeds} / ${tracker.leeches}`
                  : "—"}
              </span>
              <span className={styles.trackerNext}>
                {tracker.enabled ? formatCountdown(tracker.nextAnnounce) : "—"}
              </span>
            </div>
          );
        })}

        {/* DHT and peer exchange are not trackers, but they are peer sources the
            design lists with them: two more *enabled* rows, in the handoff's cyan
            — the tone it gives a source that is on without announcing. A private
            torrent turns both off, so there they read inert instead. */}
        {PEER_SOURCES.map(({ name, hint }) => (
          <div key={name} className={styles.trackerRow} title={hint}>
            <span
              className={`${styles.dot} ${isPrivate ? styles.dotidle : styles.dotup}`}
              aria-hidden="true"
            />
            <span className={styles.trackerUrl}>{name}</span>
            <span
              className={`${styles.trackerStatus} ${isPrivate ? styles.textidle : styles.textup}`}
            >
              {isPrivate ? "off — private" : "enabled"}
            </span>
            <span className={styles.trackerSeeds}>—</span>
            <span className={styles.trackerNext}>—</span>
          </div>
        ))}

        {trackers.length === 0 && (
          <div className={styles.placeholder}>no tracker data</div>
        )}
      </div>

      <div className={styles.paneFooter}>
        <form
          className={styles.trackerAdd}
          onSubmit={(event) => void submit(event)}
        >
          <input
            aria-label="Tracker announce URL"
            placeholder="announce URL…"
            value={url}
            onChange={(event) => setUrl(event.currentTarget.value)}
          />
          <button type="submit" disabled={!url.trim() || adding}>
            {adding ? "adding…" : "Add tracker"}
          </button>
        </form>
        <span className={styles.footerSpacer} />
        <button
          type="button"
          className={styles.footerButton}
          onClick={() => void runMenuAction(() => forceReannounce([hash]))}
        >
          Force reannounce
        </button>
        <button
          type="button"
          className={`${styles.footerButton} ${styles.footerDanger}`}
          disabled={active === null}
          onClick={() => {
            if (active === null) return;
            void runMenuAction(() => removeTracker(hash, active));
            setActive(null);
          }}
        >
          Remove
        </button>
      </div>

      {menu && (
        <>
          <div
            className={menuStyles.overlay}
            onMouseDown={() => setMenu(null)}
            onContextMenu={(event) => event.preventDefault()}
          />
          <div
            className={menuStyles.menu}
            style={{
              left: Math.min(menu.x, window.innerWidth - 220),
              top: Math.min(menu.y, window.innerHeight - 110),
            }}
          >
            <div
              className={menuStyles.item}
              onClick={() =>
                void runMenuAction(() =>
                  setTrackerEnabled(
                    hash,
                    menu.tracker.index,
                    !menu.tracker.enabled,
                  ),
                )
              }
            >
              <span className={menuStyles.icon}>
                {menu.tracker.enabled ? (
                  <PauseIcon size={11} />
                ) : (
                  <PlayIcon size={11} />
                )}
              </span>
              {menu.tracker.enabled ? "Disable" : "Enable"}
            </div>
            <div className={menuStyles.sep} />
            <div
              className={`${menuStyles.item} ${menuStyles.danger}`}
              onClick={() =>
                void runMenuAction(() =>
                  removeTracker(hash, menu.tracker.index),
                )
              }
            >
              <span className={menuStyles.icon}>
                <RemoveIcon size={11} />
              </span>
              Remove
            </div>
          </div>
        </>
      )}
    </div>
  );
}

/** The Status column's word: the daemon's status, in the design's register. */
export function trackerStatusWord(tracker: TrackerRow): string {
  if (!tracker.enabled) return "disabled";
  switch (tracker.status) {
    case "working":
      return "working";
    case "updating":
      return "updating";
    case "error":
    case "timeout":
    case "timed out":
      return "timed out";
    default:
      return tracker.status || "—";
  }
}

/** The two peer sources that are not trackers, listed with them per the design. */
const DHT_HINT =
  "The distributed hash table: peers found without a tracker. Off for private torrents.";
const PEX_HINT =
  "Peer exchange: peers learned from other peers. Off for private torrents.";

/** Listed after the real trackers, in the order the design draws them. */
const PEER_SOURCES = [
  { name: "[DHT]", hint: DHT_HINT },
  { name: "[Peer exchange]", hint: PEX_HINT },
] as const;

/** Legend for the peer flag letters (C16), shown as the Flags column tooltip. */
const PEER_FLAGS_LEGEND =
  "E encrypted · I incoming · O obfuscated · P preferred · U unwanted";

interface PeerMenuState {
  x: number;
  y: number;
  peer: PeerRow;
}

/** Peers tab: connected peers with richer flags (C16) and a right-click menu
 *  to ban / snub / disconnect a peer (B16). */
function PeersTable({ hash, peers }: { hash: string; peers: PeerRow[] }) {
  const [menu, setMenu] = useState<PeerMenuState | null>(null);

  const runMenuAction = async (action: () => Promise<void>) => {
    setMenu(null);
    try {
      await action();
    } catch {
      // The Rust command writes the failure to the app log.
    }
  };

  return (
    <div className={styles.peerPane}>
      <div className={styles.paneHeader}>Peers — {peers.length} connected</div>
      {peers.length === 0 ? (
        <div className={styles.placeholder}>no peers</div>
      ) : (
        <div className={styles.peerList}>
          <div className={`${styles.peerRow} ${styles.peerHead}`}>
            <span className={styles.peerIp}>IP</span>
            <span className={styles.peerPort}>Port</span>
            <span className={styles.peerClient}>Client</span>
            <span className={styles.peerHave}>Have</span>
            <span className={styles.peerRate}>Down</span>
            <span className={styles.peerRate}>Up</span>
            <span className={styles.peerFlags} title={PEER_FLAGS_LEGEND}>
              Flags
            </span>
          </div>
          {peers.map((p, i) => {
            const { ip, port } = splitAddress(p.address);
            return (
              <div
                key={p.id || `${p.address}:${i}`}
                className={styles.peerRow}
                onContextMenu={(event) => {
                  // Without a peer id there's nothing to target, so no menu.
                  if (!p.id) return;
                  event.preventDefault();
                  setMenu({ x: event.clientX, y: event.clientY, peer: p });
                }}
              >
                <span className={styles.peerIp}>{ip}</span>
                <span className={styles.peerPort}>{port ?? "—"}</span>
                <span className={styles.peerClient} title={p.client}>
                  {p.client || "—"}
                </span>
                <span className={styles.peerHave}>
                  <span className={styles.haveTrack}>
                    <span
                      className={styles.haveFill}
                      style={{
                        width: `${Math.max(0, Math.min(100, p.progress))}%`,
                      }}
                    />
                  </span>
                  <span className={styles.haveText}>
                    {p.progress.toFixed(0)}%
                  </span>
                </span>
                <span
                  className={`${styles.peerRate} ${p.downRate > 0 ? styles.rate : styles.none}`}
                >
                  {p.downRate > 0 ? formatRate(p.downRate) : "—"}
                </span>
                <span
                  className={`${styles.peerRate} ${p.upRate > 0 ? styles.up : styles.none}`}
                >
                  {p.upRate > 0 ? formatRate(p.upRate) : "—"}
                </span>
                <span className={styles.peerFlags} title={PEER_FLAGS_LEGEND}>
                  {p.flags || "—"}
                </span>
              </div>
            );
          })}
        </div>
      )}

      {menu && (
        <>
          <div
            className={menuStyles.overlay}
            onMouseDown={() => setMenu(null)}
            onContextMenu={(event) => event.preventDefault()}
          />
          <div
            className={menuStyles.menu}
            style={{
              left: Math.min(menu.x, window.innerWidth - 220),
              top: Math.min(menu.y, window.innerHeight - 130),
            }}
          >
            <div
              className={menuStyles.item}
              onClick={() =>
                void runMenuAction(() => snubPeer(hash, menu.peer.id))
              }
            >
              <span className={menuStyles.icon}>
                <PauseIcon size={11} />
              </span>
              Snub
            </div>
            <div
              className={menuStyles.item}
              onClick={() =>
                void runMenuAction(() => disconnectPeer(hash, menu.peer.id))
              }
            >
              <span className={menuStyles.icon}>
                <RemoveIcon size={11} />
              </span>
              Disconnect
            </div>
            <div className={menuStyles.sep} />
            <div
              className={`${menuStyles.item} ${menuStyles.danger}`}
              onClick={() =>
                void runMenuAction(() => banPeer(hash, menu.peer.id))
              }
            >
              <span className={menuStyles.icon}>
                <RemoveIcon size={11} />
              </span>
              Ban
            </div>
          </div>
        </>
      )}
    </div>
  );
}

/**
 * A peer's address as the host and the port.
 *
 * rtorrent reports `p.address` as `ip:port`, but not always — a bare IP is
 * common enough in fixtures and on older daemons that the port column shows a
 * dash rather than mangling the address. IPv6 is bracketed before the split, so
 * the last colon is the port only when there is one.
 */
export function splitAddress(address: string): {
  ip: string;
  port: string | null;
} {
  const value = address.trim();
  if (!value) return { ip: "—", port: null };
  const bracketed = /^\[(.+)\]:(\d+)$/.exec(value);
  if (bracketed) return { ip: bracketed[1], port: bracketed[2] };
  const colon = value.lastIndexOf(":");
  if (colon === -1) return { ip: value, port: null };
  const port = value.slice(colon + 1);
  // A colon inside the host means IPv6 without brackets: not a port.
  if (!/^\d+$/.test(port) || value.slice(0, colon).includes(":"))
    return { ip: value, port: null };
  return { ip: value.slice(0, colon), port };
}

/** Priority cycle for the Files pane (0 off → 1 normal → 2 high). */
const PRIORITY_LABELS = ["Skip", "Normal", "High"];

const PRIORITY_HINT =
  "click to cycle priority; right-click for multi-file actions";

/** The chip's word; a folder's mixed children have no single value. */
function priorityLabel(priority: number): string {
  if (priority < 0) return "Mixed";
  return PRIORITY_LABELS[priority] ?? String(priority);
}

/** The chip's colour: High is the accent, Normal is the neutral chip, Skip is
 * dimmer still — the design's three states. */
function priorityChipClass(priority: number): string {
  if (priority === 2) return styles.prioHigh!;
  if (priority === 0) return styles.prioSkip!;
  return styles.prioNormal!;
}

/** Per-file progress-bar status: skipped files read dim, complete ones green. */
function fileStatus(priority: number, progress: number): Status {
  if (priority === 0) return "paused";
  return progress >= 100 ? "completed" : "downloading";
}

/** Files pane: nested file tree with progress bars and multi-file priorities. */
function FilesTable({ hash, files }: { hash: string; files: FileNode[] }) {
  // Optimistic priority overrides keyed by file index, so a click updates the
  // cell instantly instead of waiting for the ~2s detail poll. An entry is
  // dropped once the incoming file data agrees, keeping this from masking a
  // rejected change (the Rust command logs failures).
  const [pending, setPending] = useState<Record<number, number>>({});
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set<string>(),
  );
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    indexes: number[];
  } | null>(null);
  const tree = useMemo(() => buildTree(files), [files]);

  // Prune overrides the polled data has caught up with. In an effect, not
  // during render, so we never setState mid-render.
  useEffect(() => {
    setPending((p) => {
      let changed = false;
      const next: Record<number, number> = {};
      for (const [k, v] of Object.entries(p)) {
        if (files[Number(k)]?.priority === v) changed = true;
        else next[Number(k)] = v;
      }
      return changed ? next : p;
    });
  }, [files]);

  useEffect(() => {
    setSelected((current) => {
      const next = new Set(
        [...current].filter((index) => index < files.length),
      );
      return next.size === current.size ? current : next;
    });
  }, [files.length]);

  if (files.length === 0)
    return <div className={styles.placeholder}>no file data</div>;

  const priorityOf = (index: number, actual: number) =>
    pending[index] ?? actual;

  const setPriority = (indexes: number[], priority: number) => {
    if (indexes.length === 0) return;
    setPending((p) => {
      const next = { ...p };
      for (const index of indexes) next[index] = priority;
      return next;
    });
    setSelected(new Set());
    setMenu(null);
    void Promise.all(
      indexes.map((index) => setFilePriority(hash, index, priority)),
    ).catch(() => {
      // The host logs the failure; the next detail poll removes stale overrides.
    });
  };

  const cyclePriority = (indexes: number[], current: number) => {
    const next = (current + 1) % 3;
    setPriority(indexes, next);
  };

  const selectFile = (index: number, event: MouseEvent) => {
    setAnchor(index);
    setSelected((current) => {
      if (event.shiftKey && anchor != null) {
        const [start, end] = [anchor, index].sort((a, b) => a - b);
        return new Set([
          ...current,
          ...Array.from({ length: end - start + 1 }, (_, i) => start + i),
        ]);
      }
      if (event.metaKey || event.ctrlKey) {
        const next = new Set(current);
        if (next.has(index)) next.delete(index);
        else next.add(index);
        return next;
      }
      return new Set([index]);
    });
  };

  const openMenu = (node: TreeNode, event: MouseEvent) => {
    event.preventDefault();
    const indexes = leafIndexes(node);
    if (indexes.length === 0) return;
    const chosen = selected.has(indexes[0])
      ? [...selected].filter((index) => indexes.includes(index))
      : indexes;
    if (!selected.has(indexes[0])) setSelected(new Set(indexes));
    setMenu({ x: event.clientX, y: event.clientY, indexes: chosen });
  };

  const renderNode = (
    node: TreeNode,
    depth: number,
    path: string,
  ): ReactNode => {
    const indexes = leafIndexes(node);
    const fileIndex = node.fileIndex;
    const actual =
      fileIndex == null ? node.priority : (files[fileIndex]?.priority ?? 1);
    const priority = fileIndex == null ? actual : priorityOf(fileIndex, actual);
    const progress =
      fileIndex == null
        ? node.progress
        : (files[fileIndex]?.progress ?? node.progress);
    const isOpen = expanded.has(path);
    const isSelected = fileIndex != null && selected.has(fileIndex);
    return (
      <Fragment key={path}>
        <div
          className={`${styles.fileRow} ${isSelected ? styles.fileSelected : ""} ${priority === 0 ? styles.fileSkipped : ""}`}
          role="treeitem"
          aria-level={depth + 1}
          aria-selected={isSelected}
          aria-expanded={node.isDir ? isOpen : undefined}
          onClick={(event) => fileIndex != null && selectFile(fileIndex, event)}
          onContextMenu={(event) => openMenu(node, event)}
        >
          <span
            className={styles.fileName}
            title={node.isDir ? `${node.name} folder` : files[fileIndex!]?.path}
            // 0 / 14 / 28px per the design: the glyph plus its gap is one step.
            style={{ paddingLeft: `${depth * 14}px` }}
          >
            {node.isDir ? (
              <button
                className={styles.folderToggle}
                onClick={(event) => {
                  event.stopPropagation();
                  setExpanded((current) => {
                    const next = new Set(current);
                    if (next.has(path)) next.delete(path);
                    else next.add(path);
                    return next;
                  });
                }}
                aria-label={`${isOpen ? "Collapse" : "Expand"} ${node.name}`}
              >
                {isOpen ? "▾" : "▸"}
              </button>
            ) : (
              <span className={styles.fileGlyph}>·</span>
            )}
            {node.name}
          </span>
          <span className={styles.fileSize}>{formatBytes(node.size)}</span>
          <span className={styles.fileProgressCol}>
            <ProgressBar
              percent={progress}
              status={fileStatus(priority === 0 ? 0 : 1, progress)}
              compact
            />
          </span>
          <span className={styles.fileDone}>
            {formatBytes(Math.round((node.size * progress) / 100))}
          </span>
          <span className={styles.filePriorityCell}>
            <button
              type="button"
              className={`${styles.prio} ${priorityChipClass(priority)}`}
              title={PRIORITY_HINT}
              aria-label={`${node.name}: priority ${priorityLabel(priority)}, change it`}
              onClick={(event) => {
                event.stopPropagation();
                cyclePriority(indexes, priority < 0 ? 1 : priority);
              }}
            >
              {priorityLabel(priority)}
            </button>
          </span>
        </div>
        {node.isDir &&
          isOpen &&
          node.children.map((child) =>
            renderNode(child, depth + 1, `${path}/${child.name}`),
          )}
      </Fragment>
    );
  };

  return (
    <>
      <div className={styles.fileList} role="tree" aria-label="Files">
        {/* The column legend, not structure: a tree's children are its items. */}
        <div
          className={`${styles.fileRow} ${styles.fileHead}`}
          role="presentation"
        >
          <span className={styles.fileName}>File</span>
          <span className={styles.fileSize}>Size</span>
          <span className={styles.fileProgressCol}>Progress</span>
          <span className={styles.fileDone}>Done</span>
          <span className={styles.filePriorityCell}>Priority</span>
        </div>
        {tree.map((node) => renderNode(node, 0, node.name))}
      </div>
      {menu && (
        <>
          <div
            className={menuStyles.overlay}
            onMouseDown={() => setMenu(null)}
            onContextMenu={(event) => event.preventDefault()}
          />
          <div
            className={menuStyles.menu}
            style={{
              left: Math.min(menu.x, window.innerWidth - 180),
              top: Math.min(menu.y, window.innerHeight - 130),
            }}
          >
            <div
              className={menuStyles.item}
              onClick={() => setPriority(menu.indexes, 0)}
            >
              Skip
            </div>
            <div
              className={menuStyles.item}
              onClick={() => setPriority(menu.indexes, 1)}
            >
              Normal
            </div>
            <div
              className={menuStyles.item}
              onClick={() => setPriority(menu.indexes, 2)}
            >
              High
            </div>
          </div>
        </>
      )}
    </>
  );
}

/**
 * Log pane: the app's event log, newest last, entries for the torrent in hand
 * picked out. Kept as a sixth tab in the console's language (WC5-S7) — the design
 * has no log, and losing it would cost the only view of what the app has done.
 */
function LogView({ hash }: { hash: string }) {
  const entries = useLog((s) => s.entries);
  const marker = useRef<HTMLDivElement>(null);
  const mine = entries.filter((entry) => entry.hash === hash).length;

  // Follow the tail, as a log should, without stealing the scroll position of
  // someone reading further up.
  useEffect(() => {
    const node = marker.current;
    const scroller = node?.parentElement;
    if (!node || !scroller) return;
    const atBottom =
      scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;
    if (atBottom) node.scrollIntoView({ block: "end" });
  }, [entries.length]);

  if (entries.length === 0)
    return <div className={styles.placeholder}>no log entries yet</div>;

  return (
    <div className={styles.logPane}>
      <div className={styles.paneHeader} style={{ marginBottom: 0 }}>
        Log — {entries.length} entries, {mine} for this torrent
      </div>
      <div className={styles.logList}>
        {entries.map((entry, index) => (
          <div
            key={index}
            className={`${styles.logRow} ${logLevelClass(entry.level)} ${
              entry.hash === hash ? styles.logMine : ""
            }`}
          >
            <span className={styles.logTime}>
              {new Date(entry.time).toLocaleTimeString()}
            </span>
            <span className={styles.logMessage}>{entry.message}</span>
          </div>
        ))}
        <div ref={marker} />
      </div>
    </div>
  );
}

/** The log level's colour class; info is the default body colour. */
function logLevelClass(level: string): string {
  if (level === "error") return styles.logerror ?? "";
  if (level === "warn") return styles.logwarn ?? "";
  return "";
}

function errorHint(kind: string): string | null {
  switch (kind) {
    case "missing_files":
      return "Files missing — try Force Recheck or Set location to the correct path.";
    case "no_space":
      return "No space left on device — free disk space or move the torrent.";
    case "permission":
      return "Permission denied — check the save path's permissions.";
    case "disk_error":
      return "Storage error — check disk health and permissions.";
    case "unregistered":
      return "Tracker says unregistered — the torrent may have been removed or the passkey is wrong. Try updating the tracker or pausing.";
    case "tracker_timeout":
      return "Tracker timed out — transient. Try Reannounce.";
    case "tracker_error":
      return "Tracker error — try Reannounce or check the Trackers tab.";
    default:
      return null;
  }
}
