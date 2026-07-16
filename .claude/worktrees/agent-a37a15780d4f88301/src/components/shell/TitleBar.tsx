/**
 * The overlay title bar. Shows `rtorrent {version} · {n} torrents` when
 * connected, or a connecting/disconnected message otherwise. The whole strip is
 * a Tauri drag region (`data-tauri-drag-region`) so the frameless window can be
 * moved by it; the native traffic lights float over its left gutter.
 */

import { useTorrents } from "../../store/torrents";
import styles from "./TitleBar.module.css";

export function TitleBar() {
  const connection = useTorrents((s) => s.connection);
  const count = useTorrents((s) => s.torrents.length);

  let title: string;
  if (connection.phase === "connected") {
    const version = connection.daemonVersion ?? "";
    title =
      `rtorrent ${version} · ${count} torrent${count === 1 ? "" : "s"}`.replace(
        "  ",
        " ",
      );
  } else if (connection.phase === "connecting") {
    title = "rtorrent — connecting…";
  } else {
    title = "rtorrent — disconnected";
  }

  return (
    <div className={styles.bar} data-tauri-drag-region>
      <span className={styles.title} data-tauri-drag-region>
        {title}
      </span>
      <span className={styles.spacer} />
    </div>
  );
}
