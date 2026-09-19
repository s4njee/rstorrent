/**
 * Right-click context menu for the torrent table (design screen 06).
 *
 * Acts on the current selection. Items mirror the toolbar plus label/location
 * management. "Set label" opens a hover submenu of existing labels, a "none"
 * option, and an inline input for a new label. Copy-magnet and copy-path
 * apply to a single torrent only. Closes on click-away (overlay) or Esc (handled
 * by the global keyboard hook). The menu is clamped to stay on-screen.
 */

import { useMemo, useState } from "react";
import { useUi } from "../../store/ui";
import { useTorrents } from "../../store/torrents";
import * as actions from "../../actions";
import {
  setLabel,
  setSuperSeeding,
  setTorrentLimits,
} from "../../ipc/commands";
import {
  PlayIcon,
  PauseIcon,
  RecheckIcon,
  LabelIcon,
  FolderIcon,
  LinkIcon,
  OpenIcon,
  RemoveIcon,
  ChevronRight,
  RateLimitIcon,
} from "../icons";
import styles from "./ContextMenu.module.css";

export function ContextMenu() {
  const menu = useUi((s) => s.contextMenu);
  const close = useUi((s) => s.closeContextMenu);
  const openDialog = useUi((s) => s.openDialog);
  const selection = useUi((s) => s.selection);
  const torrents = useTorrents((s) => s.torrents);

  const [labelOpen, setLabelOpen] = useState(false);
  const [newLabel, setNewLabel] = useState("");
  const [rateOpen, setRateOpen] = useState(false);

  // Distinct existing labels for the submenu.
  const labels = useMemo(
    () => [...new Set(torrents.map((t) => t.label).filter(Boolean))].sort(),
    [torrents],
  );

  if (!menu) return null;

  const hashes = [...selection];
  const single = hashes.length === 1 ? hashes[0] : null;
  // A private torrent's magnet would be a bare info-hash: useless for joining
  // a private swarm (the tracker requires the .torrent) and a leaky thing to
  // share. Disable Copy magnet rather than hand out a dud link (C7).
  const singlePrivate =
    single !== null &&
    (torrents.find((t) => t.hash === single)?.isPrivate ?? false);
  const singleTorrent =
    single === null ? null : (torrents.find((t) => t.hash === single) ?? null);
  const canSuperSeed = singleTorrent !== null && singleTorrent.percent >= 100;
  const superSeeding = singleTorrent?.connectionType === "initial_seed";
  // Force-start check reads the selection: mixed selections show unchecked
  // and switch on (mirroring the backend toggle).
  const forceStarted =
    hashes.length > 0 &&
    hashes.every(
      (h) => torrents.find((t) => t.hash === h)?.forceStart === true,
    );

  // Clamp so the menu stays within the window.
  const x = Math.min(menu.x, window.innerWidth - 220);
  const y = Math.min(menu.y, window.innerHeight - 340);

  const run = (fn: () => void) => {
    fn();
    close();
  };

  const applyLabel = (value: string) => {
    if (hashes.length) void setLabel(hashes, value);
    close();
  };

  const applyRate = (downKb: number, upKb: number) => {
    if (hashes.length) {
      void setTorrentLimits(hashes, downKb, upKb).catch(() => {
        // The Rust command records the failure in the app log.
      });
    }
    close();
  };

  /** Copy a torrent's on-disk path to the clipboard (the web stand-in for
   *  "open destination", which a browser can't do). */
  const copyPath = async (hash: string) => {
    const path = torrents.find((t) => t.hash === hash)?.savePath ?? "";
    if (path) await navigator.clipboard.writeText(path);
  };

  return (
    <>
      <div
        className={styles.overlay}
        onMouseDown={close}
        onContextMenu={(e) => e.preventDefault()}
      />
      <div className={styles.menu} style={{ left: x, top: y }}>
        <div
          className={styles.item}
          onClick={() => run(() => actions.resume(hashes))}
        >
          <span className={styles.icon}>
            <PlayIcon size={11} />
          </span>
          Resume
        </div>
        <div
          className={styles.item}
          onClick={() => run(() => actions.pause(hashes))}
        >
          <span className={styles.icon}>
            <PauseIcon size={11} />
          </span>
          Pause
        </div>

        <div className={styles.sep} />

        <div
          className={`${styles.item} ${canSuperSeed ? "" : styles.disabled}`}
          title={
            canSuperSeed
              ? "Use rtorrent initial-seed connection mode"
              : "only complete torrents can use super-seeding"
          }
          onClick={() => {
            if (!single || !canSuperSeed) return;
            close();
            void setSuperSeeding(single, !superSeeding).catch(() => {});
          }}
        >
          <span className={styles.icon}>{superSeeding ? "✓" : ""}</span>
          Super-seeding
        </div>

        <div
          className={styles.item}
          title="Exempt from the client queue scheduler (stays running past the caps)"
          onClick={() => run(() => actions.toggleForceStart(hashes))}
        >
          <span className={styles.icon}>{forceStarted ? "✓" : ""}</span>
          Force start
        </div>

        <div
          className={styles.item}
          onClick={() => run(() => actions.recheck(hashes))}
        >
          <span className={styles.icon}>
            <RecheckIcon size={12} />
          </span>
          Force recheck
        </div>
        <div
          className={styles.item}
          onClick={() => run(() => actions.forceReannounce(hashes))}
        >
          <span className={styles.icon}>
            <RecheckIcon size={12} />
          </span>
          Force reannounce
        </div>

        <div
          className={styles.item}
          onMouseEnter={() => setLabelOpen(true)}
          onMouseLeave={() => setLabelOpen(false)}
        >
          <span className={styles.icon}>
            <LabelIcon size={12} />
          </span>
          Set label
          <span className={styles.grow} />
          <span className={styles.arrow}>
            <ChevronRight size={10} />
          </span>
          {labelOpen && (
            <div className={styles.submenu}>
              {labels.map((l) => (
                <div
                  key={l}
                  className={styles.item}
                  onClick={() => applyLabel(l)}
                >
                  {l}
                </div>
              ))}
              <div className={styles.item} onClick={() => applyLabel("")}>
                none
              </div>
              <div className={styles.sep} />
              <div className={styles.newLabel}>
                <input
                  className={styles.newInput}
                  placeholder="new label…"
                  value={newLabel}
                  onClick={(e) => e.stopPropagation()}
                  onChange={(e) => setNewLabel(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && newLabel.trim())
                      applyLabel(newLabel.trim());
                  }}
                />
              </div>
            </div>
          )}
        </div>

        <div
          className={styles.item}
          onClick={() => run(() => openDialog("set-tags"))}
        >
          <span className={styles.icon}>
            <LabelIcon size={12} />
          </span>
          Edit tags…
        </div>

        <div
          className={styles.item}
          onMouseEnter={() => setRateOpen(true)}
          onMouseLeave={() => setRateOpen(false)}
        >
          <span className={styles.icon}>
            <RateLimitIcon size={12} />
          </span>
          Limit rates
          <span className={styles.grow} />
          <span className={styles.arrow}>
            <ChevronRight size={10} />
          </span>
          {rateOpen && (
            <div className={styles.submenu}>
              <div className={styles.item} onClick={() => applyRate(512, 512)}>
                512 KiB/s
              </div>
              <div
                className={styles.item}
                onClick={() => applyRate(1024, 1024)}
              >
                1 MiB/s
              </div>
              <div
                className={styles.item}
                onClick={() => applyRate(5120, 5120)}
              >
                5 MiB/s
              </div>
              <div className={styles.sep} />
              <div
                className={styles.item}
                onClick={() => {
                  close();
                  openDialog("rate-limit");
                }}
              >
                Custom…
              </div>
              <div className={styles.item} onClick={() => applyRate(0, 0)}>
                Unlimited
              </div>
            </div>
          )}
        </div>

        <div
          className={styles.item}
          onClick={() => {
            close();
            openDialog("set-location");
          }}
        >
          <span className={styles.icon}>
            <FolderIcon size={12} />
          </span>
          Set location…
        </div>

        <div className={styles.sep} />

        <div
          className={`${styles.item} ${
            single && !singlePrivate ? "" : styles.disabled
          }`}
          title={
            singlePrivate
              ? "private torrent — a magnet link can't join a private swarm"
              : undefined
          }
          onClick={() =>
            single &&
            !singlePrivate &&
            run(() => void actions.copyMagnet(single))
          }
        >
          <span className={styles.icon}>
            <LinkIcon size={12} />
          </span>
          Copy magnet link
        </div>
        <div
          className={`${styles.item} ${single ? "" : styles.disabled}`}
          onClick={() => single && run(() => void copyPath(single))}
        >
          <span className={styles.icon}>
            <OpenIcon size={12} />
          </span>
          Copy path
        </div>

        <div className={styles.sep} />

        <div
          className={`${styles.item} ${styles.danger}`}
          onClick={() => run(() => openDialog("remove"))}
        >
          <span className={styles.icon}>
            <RemoveIcon size={11} />
          </span>
          Remove
        </div>
      </div>
    </>
  );
}
