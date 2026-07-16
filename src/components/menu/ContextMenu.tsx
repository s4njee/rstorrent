/**
 * Right-click context menu for the torrent table (design screen 06).
 *
 * Acts on the current selection. Items mirror the toolbar plus label/location
 * management. "Set label" opens a hover submenu of existing labels, a "none"
 * option, and an inline input for a new label. Copy-magnet and open-destination
 * apply to a single torrent only. Closes on click-away (overlay) or Esc (handled
 * by the global keyboard hook). The menu is clamped to stay on-screen.
 */

import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useUi } from "../../store/ui";
import { useTorrents } from "../../store/torrents";
import * as actions from "../../actions";
import { setLabel, setLocation } from "../../ipc/commands";
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

  // Distinct existing labels for the submenu.
  const labels = useMemo(
    () => [...new Set(torrents.map((t) => t.label).filter(Boolean))].sort(),
    [torrents],
  );

  if (!menu) return null;

  const hashes = [...selection];
  const single = hashes.length === 1 ? hashes[0] : null;

  // Clamp so the menu stays within the window.
  const x = Math.min(menu.x, window.innerWidth - 220);
  const y = Math.min(menu.y, window.innerHeight - 320);

  const run = (fn: () => void) => {
    fn();
    close();
  };

  const applyLabel = (value: string) => {
    if (hashes.length) void setLabel(hashes, value);
    close();
  };

  const chooseLocation = async () => {
    const dir = await open({ directory: true });
    if (typeof dir === "string") {
      for (const h of hashes) void setLocation(h, dir);
    }
    close();
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

        <div className={styles.item} onClick={() => void chooseLocation()}>
          <span className={styles.icon}>
            <FolderIcon size={12} />
          </span>
          Set location…
        </div>

        <div className={styles.sep} />

        <div
          className={`${styles.item} ${single ? "" : styles.disabled}`}
          onClick={() => single && run(() => void actions.copyMagnet(single))}
        >
          <span className={styles.icon}>
            <LinkIcon size={12} />
          </span>
          Copy magnet link
        </div>
        <div
          className={`${styles.item} ${single ? "" : styles.disabled}`}
          onClick={() => single && run(() => actions.openDestination(single))}
        >
          <span className={styles.icon}>
            <OpenIcon size={12} />
          </span>
          Open destination
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
