/**
 * Rich scheduler (V3-18 / QUE-05) — TS mirror of the evaluation half of
 * `crates/rtorrent/src/schedule.rs`.
 *
 * Pure: weekly-window matching, override precedence, and the next-change
 * walk. The pollers evaluate host-side; this exists so Preferences can show
 * the "next change at" preview without an IPC round-trip per keystroke. Keep
 * in lockstep with the Rust module; the tests on both sides pin the shared
 * behaviour.
 */

import type { Schedule, SchedWindow, TempOverride } from "../ipc/types";

export type SchedState =
  | { kind: "open" }
  | { kind: "limited"; downKb: number; upKb: number }
  | { kind: "paused" };

/** A legacy turtle window expressed as a one-element grid, or []. */
export function importLegacy(
  enabled: boolean,
  startMin: number,
  endMin: number,
  days: readonly number[],
  downKb: number,
  upKb: number,
): SchedWindow[] {
  if (!enabled || startMin === endMin) return [];
  return [{ days: [...days], startMin, endMin, pause: false, downKb, upKb }];
}

function covers(w: SchedWindow, weekday: number, minute: number): boolean {
  if (w.days.length > 0 && !w.days.includes(weekday)) return false;
  if (w.startMin === w.endMin) return false;
  if (w.startMin < w.endMin) {
    return minute >= w.startMin && minute < w.endMin;
  }
  return minute >= w.startMin || minute < w.endMin;
}

export function evaluate(
  windows: readonly SchedWindow[],
  over: TempOverride | null | undefined,
  manual: { downKb: number; upKb: number } | null,
  weekday: number,
  minute: number,
  nowMs: number,
): SchedState {
  if (over && over.untilMs > nowMs) {
    if (over.pause) return { kind: "paused" };
    return { kind: "limited", downKb: over.downKb, upKb: over.upKb };
  }
  if (windows.some((w) => w.pause && covers(w, weekday, minute))) {
    return { kind: "paused" };
  }
  const limited = windows.find((w) => !w.pause && covers(w, weekday, minute));
  if (limited) {
    return { kind: "limited", downKb: limited.downKb, upKb: limited.upKb };
  }
  if (manual) return { kind: "limited", downKb: manual.downKb, upKb: manual.upKb };
  return { kind: "open" };
}

export type SchedKind = "open" | "limited" | "paused";

export interface Transition {
  inMinutes: number;
  weekday: number;
  minute: number;
  to: SchedKind;
}

function kindOf(state: SchedState): SchedKind {
  return state.kind === "open" ? "open" : state.kind === "paused" ? "paused" : "limited";
}

/** Next scheduled change by walking boundaries over the coming eight days. */
export function nextChange(
  windows: readonly SchedWindow[],
  over: TempOverride | null | undefined,
  manual: { downKb: number; upKb: number } | null,
  weekday: number,
  minute: number,
  nowMs: number,
): Transition | null {
  const current = kindOf(evaluate(windows, over, manual, weekday, minute, nowMs));
  const events: Array<[number, number, number]> = [];
  for (let offset = 0; offset < 8; offset++) {
    const day = (weekday + offset) % 7;
    for (const w of windows) {
      if (w.startMin === w.endMin) continue;
      if (w.days.length > 0 && !w.days.includes(day)) continue;
      events.push([offset * 1440 + w.startMin, day, w.startMin]);
      const endOffset =
        w.endMin <= w.startMin ? offset * 1440 + 1440 + w.endMin : offset * 1440 + w.endMin;
      const endDay = w.endMin <= w.startMin ? (day + 1) % 7 : day;
      events.push([endOffset, endDay, w.endMin]);
    }
  }
  if (over && over.untilMs > nowMs) {
    const inMinutes = Math.floor((over.untilMs - nowMs) / 60000);
    const total = minute + inMinutes;
    events.push([
      total,
      (weekday + Math.floor(total / 1440)) % 7,
      ((total % 1440) + 1440) % 1440,
    ]);
  }
  events.sort((a, b) => a[0] - b[0]);
  for (const [at, day, min] of events) {
    if (at <= minute) continue;
    const pday = min + 1 >= 1440 ? (day + 1) % 7 : day;
    const pmin = min + 1 >= 1440 ? 0 : min + 1;
    const after = kindOf(
      evaluate(windows, over, manual, pday, pmin, nowMs + (at - minute) * 60000),
    );
    if (after !== current) {
      return { inMinutes: at - minute, weekday: day, minute: min, to: after };
    }
  }
  return null;
}

const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function two(n: number): string {
  return String(n).padStart(2, "0");
}

/** "Mon 23:00 → paused (in 6h 12m)". */
export function formatTransition(t: Transition): string {
  const h = Math.floor(t.inMinutes / 60);
  const m = t.inMinutes % 60;
  const away = h > 0 ? `${h}h ${m}m` : `${m}m`;
  return `${DAY_NAMES[t.weekday]} ${two(Math.floor(t.minute / 60))}:${two(t.minute % 60)} → ${t.to} (in ${away})`;
}

/** Effective grid: the grid itself, or the imported legacy window. */
export function effectiveWindows(
  schedule: Schedule,
  legacy: {
    enabled: boolean;
    startMin: number;
    endMin: number;
    days: readonly number[];
    downKb: number;
    upKb: number;
  },
): SchedWindow[] {
  if (schedule.windows.length > 0) return [...schedule.windows];
  return importLegacy(
    legacy.enabled,
    legacy.startMin,
    legacy.endMin,
    legacy.days,
    legacy.downKb,
    legacy.upKb,
  );
}
