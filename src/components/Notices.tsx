/**
 * The notice stack (design: toasts) and the lost-connection banner.
 *
 * A notice is a transient interruption; the log is the record. Toasts sit over
 * the bottom-right of the window, newest last, and a persistent banner above the
 * toolbar explains why the list is empty when the daemon is unreachable.
 */

import { useNotices, type Notice, type NoticeLevel } from "../store/notices";
import { useTorrents } from "../store/torrents";
import { CloseIcon } from "./icons";
import styles from "./Notices.module.css";

const LEVEL_CLASS: Record<NoticeLevel, string> = {
  info: styles.info,
  warn: styles.warn,
  error: styles.error,
};

function Toast({ notice }: { notice: Notice }) {
  const dismiss = useNotices((s) => s.dismiss);
  return (
    <div
      className={`${styles.toast} ${LEVEL_CLASS[notice.level]}`}
      role="status"
    >
      <div className={styles.toastBody}>
        <span className={styles.message}>{notice.message}</span>
        {notice.detail && (
          <span className={styles.detail}>{notice.detail}</span>
        )}
      </div>
      <button
        type="button"
        className={styles.close}
        onClick={() => dismiss(notice.id)}
        aria-label="Dismiss"
      >
        <CloseIcon size={10} />
      </button>
    </div>
  );
}

/** The toast stack plus a dismiss-all affordance once they pile up. */
export function Notices() {
  const notices = useNotices((s) => s.notices);
  const dismissAll = useNotices((s) => s.dismissAll);
  if (notices.length === 0) return null;
  return (
    <div className={styles.stack}>
      {notices.map((notice) => (
        <Toast key={notice.id} notice={notice} />
      ))}
      {notices.length > 1 && (
        <button
          type="button"
          className={styles.dismissAll}
          onClick={dismissAll}
        >
          Dismiss all
        </button>
      )}
    </div>
  );
}

/**
 * A persistent bar above the toolbar while the daemon is unreachable. It states
 * the endpoint and the retry countdown so the empty table is explained without
 * opening anything.
 */
export function LostConnectionBanner() {
  const connection = useTorrents((s) => s.connection);
  if (connection.phase === "connected") return null;

  const connecting = connection.phase === "connecting";
  const retry = connection.retryInSeconds;

  return (
    <div className={styles.banner} role="alert">
      <span className={styles.bannerDot} aria-hidden="true" />
      <span className={styles.bannerText}>
        {connecting
          ? "Connecting to rTorrent…"
          : "Lost connection to rTorrent — retrying…"}
      </span>
      <span className={styles.bannerEndpoint}>
        {connection.endpoint}
        {!connecting && retry != null ? ` · next try in ${retry}s` : ""}
      </span>
      {connection.error && (
        <span className={styles.bannerError}>{connection.error}</span>
      )}
    </div>
  );
}
