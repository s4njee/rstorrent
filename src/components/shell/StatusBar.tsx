/**
 * Status bar: `dht: N nodes` on the left; download rate (cyan), upload rate
 * (green), and free disk space on the right. Free space is hidden when unknown
 * (e.g. a remote daemon where we can't stat the volume).
 */

import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { useSettings } from "../../store/settings";
import { setTurtle } from "../../ipc/commands";
import { formatRate, formatFree } from "../../utils/format";
import styles from "./StatusBar.module.css";

export function StatusBar() {
  const g = useTorrents((s) => s.globals);
  const openDialog = useUi((s) => s.openDialog);
  const turtleEnabled = useSettings((s) => s.settings?.turtleEnabled ?? false);
  const setSettings = useSettings((s) => s.set);
  const free = formatFree(g.freeSpace);

  // Clicking flips the manual switch. A running schedule can still keep turtle
  // mode active even with the manual switch off (turtleActive reflects both).
  const toggleTurtle = () => void setTurtle(!turtleEnabled).then(setSettings);

  return (
    <div className={styles.bar}>
      {/* DHT segment doubles as the entry point to the Statistics dialog. */}
      <span
        style={{ cursor: "default" }}
        title="Statistics"
        onClick={() => openDialog("stats")}
      >
        dht: {g.dhtNodes} nodes
      </span>
      <span className={styles.grow} />
      <span
        title={`Turtle mode ${g.turtleActive ? "on" : "off"} — click to toggle`}
        aria-label="Toggle turtle mode"
        onClick={toggleTurtle}
        style={{ cursor: "pointer", opacity: g.turtleActive ? 1 : 0.35 }}
      >
        🐢
      </span>
      <span className={styles.down}>↓ {formatRate(g.downRate)}</span>
      <span className={styles.up}>↑ {formatRate(g.upRate)}</span>
      {free && <span>{free}</span>}
    </div>
  );
}
