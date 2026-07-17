/** Small DOM predicates shared by the global keyboard and paste handlers. */

/**
 * True when focus is in a text-entry element.
 *
 * Global shortcuts and paste-to-add both need this: a keystroke aimed at the
 * filter box or a dialog field must reach that field, not the window handler.
 */
export function typingInField(
  el: Element | null = document.activeElement,
): boolean {
  if (!el) return false;
  const tag = el.tagName;
  // `isContentEditable` is typed as boolean but is absent on non-HTML elements
  // (and in jsdom), so compare explicitly rather than leaking `undefined` out
  // of a function that promises a boolean.
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    (el as HTMLElement).isContentEditable === true
  );
}
