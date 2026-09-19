/**
 * Pure torrent-table column state.
 *
 * Keeping definitions, order, width clamping, visibility rules, grid derivation
 * and persistence normalization here makes the interaction layer deliberately
 * small and keeps malformed localStorage data away from rendering code.
 *
 * The set is the console's (design frame 1a): a select box, then Name · Size ·
 * Done · Status · Seeds/Peers · Down · Up · ETA · Ratio · Label · Added ·
 * Tracker. Two extras ride along hidden by default — `started` and `finished`,
 * which the desktop shell has always offered and the design has no home for.
 */

export const COLUMN_IDS = [
  "select",
  "name",
  "size",
  "done",
  "status",
  "seedsPeers",
  "down",
  "up",
  "eta",
  "ratio",
  "label",
  "added",
  "tracker",
  "started",
  "finished",
] as const;

export type ColumnId = (typeof COLUMN_IDS)[number];

export interface ColumnDefinition {
  id: ColumnId;
  label: string;
  defaultWidth: number;
  minWidth: number;
  /** Takes the leftover width instead of a fixed one (Name only). */
  flexible?: boolean;
  /** Hidden until the user enables it (optional depth columns). */
  defaultHidden?: boolean;
  /** Right-aligned numeric column. */
  numeric?: boolean;
  /**
   * Fixed to its place: the selection box and the name. Neither can be hidden,
   * resized or moved — a table without a name column is not a table.
   */
  locked?: boolean;
}

/** The design's column set, in its order. */
export const COLUMN_DEFINITIONS: readonly ColumnDefinition[] = [
  { id: "select", label: "", defaultWidth: 26, minWidth: 26, locked: true },
  {
    id: "name",
    label: "Name",
    defaultWidth: 200,
    minWidth: 200,
    flexible: true,
    locked: true,
  },
  { id: "size", label: "Size", defaultWidth: 74, minWidth: 40, numeric: true },
  { id: "done", label: "Done", defaultWidth: 112, minWidth: 64 },
  { id: "status", label: "Status", defaultWidth: 96, minWidth: 64 },
  {
    id: "seedsPeers",
    label: "Seeds/Peers",
    defaultWidth: 66,
    minWidth: 56,
    numeric: true,
  },
  { id: "down", label: "Down", defaultWidth: 78, minWidth: 40, numeric: true },
  { id: "up", label: "Up", defaultWidth: 78, minWidth: 40, numeric: true },
  { id: "eta", label: "ETA", defaultWidth: 64, minWidth: 40, numeric: true },
  {
    id: "ratio",
    label: "Ratio",
    defaultWidth: 54,
    minWidth: 40,
    numeric: true,
  },
  { id: "label", label: "Label", defaultWidth: 88, minWidth: 52 },
  {
    id: "added",
    label: "Added",
    defaultWidth: 86,
    minWidth: 62,
    numeric: true,
  },
  { id: "tracker", label: "Tracker", defaultWidth: 120, minWidth: 70 },
  {
    id: "started",
    label: "Started",
    defaultWidth: 72,
    minWidth: 52,
    defaultHidden: true,
  },
  {
    id: "finished",
    label: "Finished",
    defaultWidth: 72,
    minWidth: 52,
    defaultHidden: true,
  },
];

export interface ColumnState {
  /** Display order; reordering rewrites it. */
  order: ColumnId[];
  widths: Record<ColumnId, number>;
  visibility: Record<ColumnId, boolean>;
}

interface SerializedColumnState {
  version: 1;
  order?: unknown;
  widths: unknown;
  visibility: unknown;
}

const DEFINITIONS_BY_ID = Object.fromEntries(
  COLUMN_DEFINITIONS.map((column) => [column.id, column]),
) as Record<ColumnId, ColumnDefinition>;

export function columnDefinition(id: ColumnId): ColumnDefinition {
  return DEFINITIONS_BY_ID[id];
}

/** A fresh default state (callers may safely mutate their returned copy). */
export function defaultColumnState(): ColumnState {
  return {
    order: [...COLUMN_IDS],
    widths: Object.fromEntries(
      COLUMN_DEFINITIONS.map((column) => [column.id, column.defaultWidth]),
    ) as Record<ColumnId, number>,
    visibility: Object.fromEntries(
      COLUMN_DEFINITIONS.map((column) => [column.id, !column.defaultHidden]),
    ) as Record<ColumnId, boolean>,
  };
}

/** Clamp a width to the column's usable minimum and round to whole pixels. */
export function clampColumnWidth(id: ColumnId, width: number): number {
  const definition = DEFINITIONS_BY_ID[id];
  // A locked column's width is not the user's to change (the selection box).
  if (definition.locked) return definition.defaultWidth;
  const finite = Number.isFinite(width) ? width : definition.defaultWidth;
  return Math.max(definition.minWidth, Math.round(finite));
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

/** Return a new state with one visibility flag; locked columns stay visible. */
export function setColumnVisible(
  state: ColumnState,
  id: ColumnId,
  visible: boolean,
): ColumnState {
  return {
    ...state,
    visibility: {
      ...state.visibility,
      [id]: DEFINITIONS_BY_ID[id].locked ? true : visible,
    },
  };
}

export function toggleColumn(state: ColumnState, id: ColumnId): ColumnState {
  return setColumnVisible(state, id, !state.visibility[id]);
}

/**
 * Move a column to the slot the dropped-on column occupies.
 *
 * A locked column is never moved, and a column is never moved *past* one: the
 * selection box stays first and the name stays second, so a drop settles at the
 * nearest free slot instead of silently reordering the fixed pair.
 */
export function moveColumn(
  state: ColumnState,
  id: ColumnId,
  before: ColumnId,
): ColumnState {
  if (id === before || DEFINITIONS_BY_ID[id].locked) return state;
  const order = state.order.filter((column) => column !== id);
  const target = order.indexOf(before);
  if (target === -1) return state;

  const lockedBefore = order
    .slice(0, target + 1)
    .filter((column) => DEFINITIONS_BY_ID[column].locked).length;
  order.splice(Math.max(lockedBefore, target), 0, id);
  return { ...state, order };
}

/** Ordered definitions for columns that currently participate in the grid. */
export function visibleColumns(state: ColumnState): ColumnDefinition[] {
  return state.order
    .map((id) => DEFINITIONS_BY_ID[id])
    .filter((definition) => state.visibility[definition.id]);
}

/**
 * CSS grid template. Hidden columns are omitted and Name stays flexible.
 *
 * `only` narrows the grid further for the current viewport (see
 * `useResponsiveColumns`) without changing the saved state.
 */
export function gridTemplateColumns(
  state: ColumnState,
  only?: ColumnId[],
): string {
  return visibleColumns(state)
    .filter((column) => !only || only.includes(column.id))
    .map((column) => {
      const width = clampColumnWidth(column.id, state.widths[column.id]);
      return column.flexible ? `minmax(${width}px, 1fr)` : `${width}px`;
    })
    .join(" ");
}

/** Serialize a normalized, versioned payload for the persisted view object. */
export function serializeColumnState(state: ColumnState): string {
  const payload: SerializedColumnState = {
    version: 1,
    order: state.order,
    widths: state.widths,
    visibility: state.visibility,
  };
  return JSON.stringify(payload);
}

/**
 * Restore persisted state, falling back per field and entirely on bad input.
 * This accepts `unknown` because values originate in localStorage JSON.
 *
 * State saved before the console's column set (a merge of S+P, and Added) still
 * loads: unknown ids are dropped, missing ones appended, and the locked pair
 * pinned to the front.
 */
export function deserializeColumnState(value: unknown): ColumnState {
  if (typeof value !== "string") return defaultColumnState();

  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return defaultColumnState();
  }
  if (!isRecord(parsed) || parsed.version !== 1) return defaultColumnState();

  const state = defaultColumnState();
  state.order = normalizeOrder(parsed.order);

  const widths = isRecord(parsed.widths) ? parsed.widths : {};
  const visibility = isRecord(parsed.visibility) ? parsed.visibility : {};
  for (const id of COLUMN_IDS) {
    const width = widths[id];
    if (typeof width === "number")
      state.widths[id] = clampColumnWidth(id, width);

    const visible = visibility[id];
    if (DEFINITIONS_BY_ID[id].locked) state.visibility[id] = true;
    else if (typeof visible === "boolean") state.visibility[id] = visible;
  }

  return state;
}

/**
 * A complete column order: known ids in the given sequence, unknown ones dropped
 * and anything missing appended, so a set that gains or loses a column still
 * loads.
 */
function normalizeOrder(order: unknown): ColumnId[] {
  const requested = Array.isArray(order)
    ? order.filter((id): id is ColumnId =>
        (COLUMN_IDS as readonly string[]).includes(id as string),
      )
    : [];
  const ordered = [...new Set(requested)];
  const missing = COLUMN_IDS.filter((id) => !ordered.includes(id));
  const locked = COLUMN_IDS.filter((id) => DEFINITIONS_BY_ID[id].locked);
  const rest = [...ordered, ...missing].filter((id) => !locked.includes(id));
  return [...locked, ...rest];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
