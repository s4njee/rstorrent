/**
 * ModalBase — the shared shell every dialog is built on.
 *
 * Renders a dimming backdrop over the window and a centered, window-styled
 * panel with a header (title + ✕) and a footer slot. Handles the modal keyboard
 * contract: Esc cancels, Enter triggers the primary action (unless focus is in a
 * textarea, where Enter should insert a newline). Focus is trapped inside the
 * panel and returned to the previously-focused element on close.
 *
 * Exposes `Button` with the design's secondary/primary/danger variants so
 * dialogs share one button style.
 */

import { useEffect, useRef, type ReactNode } from "react";
import { CloseIcon } from "../icons";
import styles from "./ModalBase.module.css";

interface ModalBaseProps {
  title: string;
  onCancel: () => void;
  /** Invoked on Enter; dialogs pass their primary-button handler. */
  onPrimary?: () => void;
  width?: number;
  /** Drop the body's default padding (for full-bleed layouts like Preferences). */
  noPad?: boolean;
  children: ReactNode;
  footer: ReactNode;
}

export function ModalBase({
  title,
  onCancel,
  onPrimary,
  width = 460,
  noPad = false,
  children,
  footer,
}: ModalBaseProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  // Focus the panel on open; restore focus to the prior element on close.
  useEffect(() => {
    const previouslyFocused = document.activeElement as HTMLElement | null;
    // Focus the first focusable control, or the panel itself.
    const focusable = panelRef.current?.querySelector<HTMLElement>(
      "input, textarea, select, button, [tabindex]",
    );
    (focusable ?? panelRef.current)?.focus();
    return () => previouslyFocused?.focus();
  }, []);

  // Keyboard contract + focus trap, scoped to the panel.
  const onKeyDown = (e: React.KeyboardEvent) => {
    // Stop these keys reaching the global shortcut handler.
    if (e.key === "Escape") {
      e.stopPropagation();
      onCancel();
      return;
    }
    if (e.key === "Enter" && onPrimary) {
      const el = document.activeElement;
      // Let textareas keep Enter for newlines.
      if (el?.tagName !== "TEXTAREA") {
        e.preventDefault();
        e.stopPropagation();
        onPrimary();
      }
      return;
    }
    if (e.key === "Tab") {
      trapFocus(e, panelRef.current);
    }
  };

  return (
    <div className={styles.backdrop}>
      <div
        ref={panelRef}
        className={styles.win}
        style={{ width }}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        <div className={styles.header}>
          <span className={styles.title}>{title}</span>
          <span className={styles.grow} />
          <span className={styles.close} onClick={onCancel} aria-label="Close">
            <CloseIcon size={11} />
          </span>
        </div>
        <div className={styles.body} style={noPad ? { padding: 0 } : undefined}>
          {children}
        </div>
        <div className={styles.footer}>{footer}</div>
      </div>
    </div>
  );
}

/** Keep Tab focus cycling within the modal's focusable elements. */
function trapFocus(e: React.KeyboardEvent, container: HTMLElement | null) {
  if (!container) return;
  const items = container.querySelectorAll<HTMLElement>(
    'input, textarea, select, button, [tabindex]:not([tabindex="-1"])',
  );
  if (items.length === 0) return;
  const first = items[0];
  const last = items[items.length - 1];
  const active = document.activeElement;
  if (e.shiftKey && active === first) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && active === last) {
    e.preventDefault();
    first.focus();
  }
}

type Variant = "secondary" | "primary" | "danger";

/** Shared dialog button. */
export function Button({
  variant,
  children,
  ...rest
}: { variant: Variant } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button className={`${styles.btn} ${styles[variant]}`} {...rest}>
      {children}
    </button>
  );
}
