/**
 * Pure torrent-table column state.
 *
 * Keeping definitions, width clamping, visibility rules, grid derivation, and
 * persistence normalization here makes the interaction layer deliberately
 * small and keeps malformed localStorage data away from rendering code.
 */

export const COLUMN_IDS = [
  "name",
  "size",
  "done",
  "status",
  "seeds",
  "peers",
  "down",
  "up",
  "eta",
  "ratio",
  "label",
  "tracker",
] as const;

export type ColumnId = (typeof COLUMN_IDS)[number];

export interface ColumnDefinition {
  id: ColumnId;
  label: string;
  defaultWidth: number;
  minWidth: number;
  flexible?: boolean;
}

export const COLUMN_DEFINITIONS: readonly ColumnDefinition[] = [
  {
    id: "name",
    label: "Name",
    defaultWidth: 200,
    minWidth: 120,
    flexible: true,
  },
  { id: "size", label: "Size", defaultWidth: 70, minWidth: 40 },
  { id: "done", label: "Done", defaultWidth: 92, minWidth: 40 },
  { id: "status", label: "Status", defaultWidth: 84, minWidth: 64 },
  { id: "seeds", label: "S", defaultWidth: 52, minWidth: 40 },
  { id: "peers", label: "P", defaultWidth: 52, minWidth: 40 },
  { id: "down", label: "Down", defaultWidth: 76, minWidth: 40 },
  { id: "up", label: "Up", defaultWidth: 76, minWidth: 40 },
  { id: "eta", label: "ETA", defaultWidth: 62, minWidth: 40 },
  { id: "ratio", label: "Ratio", defaultWidth: 46, minWidth: 40 },
  { id: "label", label: "Label", defaultWidth: 72, minWidth: 52 },
  { id: "tracker", label: "Tracker", defaultWidth: 110, minWidth: 70 },
];

export interface ColumnState {
  widths: Record<ColumnId, number>;
  visibility: Record<ColumnId, boolean>;
}

interface SerializedColumnState {
  version: 1;
  widths: Record<ColumnId, number>;
  visibility: Record<ColumnId, boolean>;
}

const DEFINITIONS_BY_ID = Object.fromEntries(
  COLUMN_DEFINITIONS.map((column) => [column.id, column]),
) as Record<ColumnId, ColumnDefinition>;

/** A fresh default state (callers may safely mutate their returned copy). */
export function defaultColumnState(): ColumnState {
  return {
    widths: Object.fromEntries(
      COLUMN_DEFINITIONS.map((column) => [column.id, column.defaultWidth]),
    ) as Record<ColumnId, number>,
    visibility: Object.fromEntries(
      COLUMN_DEFINITIONS.map((column) => [column.id, true]),
    ) as Record<ColumnId, boolean>,
  };
}

/** Clamp a width to the column's usable minimum and round to whole pixels. */
export function clampColumnWidth(id: ColumnId, width: number): number {
  const fallback = DEFINITIONS_BY_ID[id].defaultWidth;
  const finiteWidth = Number.isFinite(width) ? width : fallback;
  return Math.max(DEFINITIONS_BY_ID[id].minWidth, Math.round(finiteWidth));
}

/** Return a new state with one clamped width. */
export function resizeColumn(
  state: ColumnState,
  id: ColumnId,
  width: number,
): ColumnState {
  return {
    ...state,
    widths: { ...state.widths, [id]: clampColumnWidth(id, width) },
  };
}

/** Return a new state with one visibility flag; Name is always visible. */
export function setColumnVisible(
  state: ColumnState,
  id: ColumnId,
  visible: boolean,
): ColumnState {
  return {
    ...state,
    visibility: {
      ...state.visibility,
      [id]: id === "name" ? true : visible,
    },
  };
}

export function toggleColumn(state: ColumnState, id: ColumnId): ColumnState {
  return setColumnVisible(state, id, !state.visibility[id]);
}

/** Ordered definitions for columns that currently participate in the grid. */
export function visibleColumns(state: ColumnState): ColumnDefinition[] {
  return COLUMN_DEFINITIONS.filter((column) => state.visibility[column.id]);
}

/** CSS grid template with hidden columns omitted and Name kept flexible. */
export function gridTemplateColumns(state: ColumnState): string {
  return visibleColumns(state)
    .map((column) => {
      const width = clampColumnWidth(column.id, state.widths[column.id]);
      return column.flexible ? `minmax(${width}px, 1fr)` : `${width}px`;
    })
    .join(" ");
}

/** Serialize a normalized, versioned payload for the persisted view object. */
export function serializeColumnState(state: ColumnState): string {
  const normalized = normalizeColumnState(state);
  const payload: SerializedColumnState = { version: 1, ...normalized };
  return JSON.stringify(payload);
}

/**
 * Restore persisted state, falling back per field and entirely on bad input.
 * This accepts `unknown` because values originate in localStorage JSON.
 */
export function deserializeColumnState(value: unknown): ColumnState {
  if (typeof value !== "string") return defaultColumnState();

  try {
    const parsed = JSON.parse(value) as unknown;
    if (!isRecord(parsed) || parsed.version !== 1) return defaultColumnState();
    return normalizeColumnState(parsed);
  } catch {
    return defaultColumnState();
  }
}

function normalizeColumnState(value: unknown): ColumnState {
  const defaults = defaultColumnState();
  if (!isRecord(value)) return defaults;

  const widths = isRecord(value.widths) ? value.widths : {};
  const visibility = isRecord(value.visibility) ? value.visibility : {};

  for (const id of COLUMN_IDS) {
    const width = widths[id];
    defaults.widths[id] =
      typeof width === "number"
        ? clampColumnWidth(id, width)
        : defaults.widths[id];

    const visible = visibility[id];
    defaults.visibility[id] =
      id === "name" ? true : typeof visible === "boolean" ? visible : true;
  }

  return defaults;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
