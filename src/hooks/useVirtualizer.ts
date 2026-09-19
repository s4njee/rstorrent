/**
 * Fixed-height windowing hook for the torrent table (FND-01).
 *
 * The table row height is constant (CSS --row-height: 30px, 34px when the
 * density setting is comfortable),
 * so we can compute the visible range from scrollTop + viewport height without
 * measuring DOM nodes. Overscan keeps fast scroll from flashing blank rows.
 *
 * Callers supply count + rowHeight; this hook owns scrollTop + viewportHeight
 * and returns the clamped window plus a scrollToIndex helper.
 */

import { useCallback, useEffect, useRef, useState } from "react";

export interface VirtualizerOptions {
  count: number;
  rowHeight: number;
  overscan?: number;
}

export interface VirtualRange {
  start: number;
  end: number;
  offsetY: number;
  totalHeight: number;
}

export function useVirtualizer(
  scrollRef: React.RefObject<HTMLElement | null>,
  options: VirtualizerOptions,
) {
  const { count, rowHeight, overscan = 10 } = options;
  const [range, setRange] = useState<VirtualRange>(() => ({
    start: 0,
    end: Math.min(count - 1, 30),
    offsetY: 0,
    totalHeight: count * rowHeight,
  }));

  const rafRef = useRef<number | null>(null);
  const scrollTopRef = useRef(0);
  const viewportRef = useRef(300);

  const recompute = useCallback(() => {
    const totalHeight = count * rowHeight;
    const scrollTop = scrollTopRef.current;
    const viewport = viewportRef.current;
    const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
    const end = Math.min(
      count - 1,
      Math.ceil((scrollTop + viewport) / rowHeight) + overscan,
    );
    const offsetY = start * rowHeight;
    setRange({ start, end: Math.max(start, end), offsetY, totalHeight });
  }, [count, rowHeight, overscan]);

  // Keep totalHeight in sync when count/rowHeight changes, preserve window.
  useEffect(() => {
    recompute();
  }, [recompute]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const measureViewport = () => {
      viewportRef.current = el.clientHeight || 300;
    };
    measureViewport();

    const onScroll = () => {
      scrollTopRef.current = el.scrollTop;
      if (rafRef.current != null) return;
      rafRef.current = requestAnimationFrame(() => {
        rafRef.current = null;
        recompute();
      });
    };

    let ro: ResizeObserver | null = null;
    if (typeof ResizeObserver !== "undefined") {
      ro = new ResizeObserver(() => {
        measureViewport();
        recompute();
      });
      ro.observe(el);
    } else {
      window.addEventListener("resize", recompute);
    }
    el.addEventListener("scroll", onScroll, { passive: true });

    // initial
    scrollTopRef.current = el.scrollTop;
    recompute();

    return () => {
      el.removeEventListener("scroll", onScroll);
      ro?.disconnect();
      if (typeof ResizeObserver === "undefined") {
        window.removeEventListener("resize", recompute);
      }
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, [scrollRef, recompute]);

  const scrollToIndex = useCallback(
    (index: number, align: "auto" | "center" | "start" = "auto") => {
      const el = scrollRef.current;
      if (!el || count === 0) return;
      const clamped = Math.max(0, Math.min(index, count - 1));
      const viewport = el.clientHeight || viewportRef.current;
      const currentTop = el.scrollTop;
      const currentBottom = currentTop + viewport;
      const rowTop = clamped * rowHeight;
      const rowBottom = rowTop + rowHeight;

      let target = currentTop;
      if (align === "center") {
        target = rowTop - viewport / 2 + rowHeight / 2;
      } else if (align === "start") {
        target = rowTop;
      } else {
        // auto: only scroll if not already visible
        if (rowTop < currentTop) target = rowTop;
        else if (rowBottom > currentBottom) target = rowBottom - viewport;
        else return;
      }
      target = Math.max(0, Math.min(target, count * rowHeight - viewport));
      el.scrollTop = target;
      // sync refs and recompute synchronously for immediate paint
      scrollTopRef.current = target;
      recompute();
    },
    [count, rowHeight, scrollRef, recompute],
  );

  return { range, scrollToIndex, recompute };
}

/** Resolve the CSS --row-height variable to a number (px). */
export function resolveRowHeight(fallback = 30): number {
  try {
    const raw = getComputedStyle(document.documentElement)
      .getPropertyValue("--row-height")
      .trim();
    const n = parseFloat(raw);
    if (Number.isFinite(n) && n > 0) return n;
  } catch {
    // jsdom or no document
  }
  return fallback;
}
