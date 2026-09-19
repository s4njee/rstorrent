//! Virtualised torrent table for the native shell.
//!
//! The table renders DTO references owned by [`TorrentsModel`]: only visible
//! rows are built, through `uniform_list`. Selection is hash-keyed (see
//! [`table_state`]), so sorting, filtering and refresh cannot retarget it.
//!
//! The column set is [`crate::columns::ColumnState`] — widths and visibility
//! are the user's, persisted through settings, and a hidden column costs
//! nothing to render. Header gestures (sort, resize, the visibility menu) and
//! the row context menu emit actions the shell handles, so the table never
//! talks to the daemon itself.

use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, uniform_list, ClickEvent, Context, Div, EventEmitter, FocusHandle, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollStrategy, SharedString,
    Stateful, UniformListScrollHandle, Window,
};

use rtorrent_core::types::{Status, TorrentDto};

use crate::actions;
use crate::columns::{self, ColumnId, ColumnState};
use crate::format;
use crate::icons;
use crate::model::TorrentsModel;
use crate::table_state::{self, Modifiers, Sort, SortColumn};
use crate::theme::{self, Palette};

/// Height of one virtual list row.
pub const ROW_HEIGHT: f32 = theme::geometry::ROW;
const HEADER_HEIGHT: f32 = theme::geometry::COLUMN_HEADER;
/// The width of the grab area on a column's trailing edge.
const GRIP: f32 = 6.;

/// Events the shell subscribes to from a [`TorrentTable`] entity.
#[derive(Clone, Debug, PartialEq)]
pub enum TorrentTableEvent {
    SelectionChanged,
    Activated {
        hash: String,
    },
    SortChanged {
        sort: Sort,
    },
    /// Widths or visibility changed; the shell persists them.
    ColumnsChanged,
    /// The table asked for a dialog to be opened.
    RequestDialog(DialogRequest),
}

/// A dialog the table's context menu asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogRequest {
    Remove,
    SetLabel,
    RateLimit,
    SetLocation,
}

impl EventEmitter<TorrentTableEvent> for TorrentTable {}

/// What the row context menu can offer for the current selection.
#[derive(Clone, Copy, Debug, Default)]
struct MenuState {
    /// The single selection is complete, so super-seeding can be switched.
    can_super_seed: bool,
    super_seeding: bool,
    /// The single selection is not private, so its magnet link can join a swarm.
    can_copy_magnet: bool,
    /// One row is selected, so a single-target action applies.
    single: bool,
    /// The single selection has a live move-on-complete move (V3-14).
    can_cancel_move: bool,
    /// The single selection has a failed/cancelled move to retry (V3-14).
    can_retry_move: bool,
    /// The single selection is force-started (V3-17 / QUE-01).
    force_started: bool,
}

/// An in-progress column resize.
#[derive(Clone, Copy, Debug)]
struct Resizing {
    id: ColumnId,
    start_x: f32,
    start_width: f32,
}

/// A virtualised torrent table bound to [`TorrentsModel`].
pub struct TorrentTable {
    model: gpui_kit::Entity<TorrentsModel>,
    rows: Arc<[TorrentDto]>,
    columns: ColumnState,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    resizing: Option<Resizing>,
    /// Whether the column-visibility panel is showing.
    column_menu: bool,
}

impl TorrentTable {
    pub fn new(
        model: gpui_kit::Entity<TorrentsModel>,
        columns: ColumnState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        // The table takes focus as the window opens: it is the keyboard's
        // target, and an action dispatched from it bubbles to the shell, which
        // is where the toolbar and menu commands are handled.
        focus.focus(window, cx);
        Self {
            model,
            rows: Arc::from(Vec::new()),
            columns,
            focus,
            scroll: UniformListScrollHandle::new(),
            resizing: None,
            column_menu: false,
        }
    }

    /// The current column widths and visibility, for the shell to persist.
    #[must_use]
    pub fn columns(&self) -> ColumnState {
        self.columns.clone()
    }

    /// Close the column-visibility panel, if it is showing.
    pub fn close_column_menu(&mut self, cx: &mut Context<Self>) {
        if self.column_menu {
            self.column_menu = false;
            cx.notify();
        }
    }

    /// Refresh the row snapshot from the model. Called by the shell's model
    /// observer after every poll tick.
    pub fn sync(&mut self, cx: &mut Context<Self>) {
        let rows: Vec<TorrentDto> = {
            let model = self.model.read(cx);
            let index = model.file_index();
            // A short lock: the query is read-only and the map is small.
            let guard = index.lock().ok();
            table_state::visible_with_files(
                model.torrents(),
                model.filter(),
                model.search(),
                model.sort(),
                guard.as_deref(),
            )
            .into_iter()
            .cloned()
            .collect()
        };
        self.rows = Arc::from(rows);
        cx.notify();
    }

    fn order(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.hash.clone()).collect()
    }

    fn select_at(&mut self, index: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let hash = row.hash.clone();
        self.model.update(cx, |model, cx| {
            model.click(&hash, modifiers, cx);
        });
        cx.emit(TorrentTableEvent::SelectionChanged);
        cx.notify();
    }

    /// Select a row for a right-click, without disturbing a multi-selection it
    /// is already part of.
    fn select_for_menu(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else {
            return;
        };
        let hash = row.hash.clone();
        let already = self.model.read(cx).selection().hashes.contains(&hash);
        if !already {
            self.model.update(cx, |model, cx| {
                model.click(&hash, Modifiers::default(), cx);
            });
            cx.emit(TorrentTableEvent::SelectionChanged);
            cx.notify();
        }
    }

    fn activate(&self, index: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(index) {
            cx.emit(TorrentTableEvent::Activated {
                hash: row.hash.clone(),
            });
        }
    }

    fn header_clicked(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        let sort = self.model.read(cx).sort().clicked(column);
        self.model.update(cx, |model, cx| {
            model.set_sort(sort, cx);
        });
        cx.emit(TorrentTableEvent::SortChanged { sort });
        cx.notify();
    }

    fn begin_resize(&mut self, id: ColumnId, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let definition = columns::definition(id);
        if definition.flexible {
            // Name absorbs the leftover width, so it has no width to drag.
            return;
        }
        #[allow(clippy::cast_possible_truncation)]
        let start_x = f32::from(event.position.x);
        self.resizing = Some(Resizing {
            id,
            start_x,
            start_width: self.columns.width(id),
        });
        cx.notify();
    }

    fn drag_resize(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some(resizing) = self.resizing else {
            return;
        };
        #[allow(clippy::cast_possible_truncation)]
        let x = f32::from(event.position.x);
        let width = (resizing.start_width + (x - resizing.start_x)).max(0.);
        self.columns.resize(resizing.id, width);
        cx.notify();
    }

    fn end_resize(&mut self, cx: &mut Context<Self>) {
        if self.resizing.take().is_some() {
            cx.emit(TorrentTableEvent::ColumnsChanged);
            cx.notify();
        }
    }

    fn toggle_column(&mut self, id: ColumnId, cx: &mut Context<Self>) {
        self.columns.toggle(id);
        cx.emit(TorrentTableEvent::ColumnsChanged);
        cx.notify();
    }

    fn reset_columns(&mut self, cx: &mut Context<Self>) {
        self.columns.reset();
        cx.emit(TorrentTableEvent::ColumnsChanged);
        cx.notify();
    }

    fn menu_state(&self, cx: &Context<Self>) -> MenuState {
        let model = self.model.read(cx);
        let single = model.single_selection();
        let single_hash = single.map(|torrent| torrent.hash.clone());
        let active = model.active_move_hashes();
        let retryable = model.retryable_move_hashes();
        MenuState {
            can_super_seed: single.is_some_and(|torrent| torrent.percent >= 100.0),
            super_seeding: single.is_some_and(|torrent| torrent.connection_type == "initial_seed"),
            can_copy_magnet: single.is_some_and(|torrent| !torrent.is_private),
            single: single.is_some(),
            can_cancel_move: single_hash
                .as_ref()
                .is_some_and(|hash| active.contains(hash)),
            can_retry_move: single_hash
                .as_ref()
                .is_some_and(|hash| retryable.contains(hash)),
            force_started: single.is_some_and(|torrent| torrent.force_start),
        }
    }

    fn page_rows(&self) -> usize {
        let state = self.scroll.0.borrow();
        let height = f32::from(state.base_handle.bounds().size.height);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "GPUI layout dimensions are f32 and are known non-negative here"
        )]
        let rows = (height / ROW_HEIGHT).ceil() as usize;
        rows.max(1)
    }

    fn keyboard(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if self.rows.is_empty() {
            return;
        }
        if key == "enter" {
            let order = self.order();
            let selection = self.model.read(cx).selection().clone();
            if let Some(index) = order
                .iter()
                .position(|hash| Some(hash) == selection.anchor.as_ref())
                .or_else(|| {
                    selection
                        .hashes
                        .iter()
                        .filter_map(|hash| order.iter().position(|h| h == hash))
                        .max()
                })
            {
                cx.stop_propagation();
                self.activate(index, cx);
            }
            return;
        }
        let moved = match key {
            "up" | "down" | "pageup" | "pagedown" | "home" | "end" => key,
            _ => return,
        };
        let page = self.page_rows() as isize;
        let delta = match moved {
            "up" => -1,
            "down" => 1,
            "pageup" => -page.max(1),
            "pagedown" => page.max(1),
            _ => 0,
        };
        let order = self.order();
        let selection = self.model.read(cx).selection().clone();
        let moved = if moved == "home" {
            table_state::step(&selection, &order, isize::MIN / 2)
        } else if moved == "end" {
            table_state::step(&selection, &order, isize::MAX / 2)
        } else if event.keystroke.modifiers.shift {
            table_state::extend_by(&selection, &order, delta)
        } else {
            table_state::step(&selection, &order, delta)
        };
        let reveal = moved.reveal;
        self.model.update(cx, |model, cx| {
            model.set_selection(moved.selection, cx);
        });
        if let Some(index) = reveal {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
        cx.emit(TorrentTableEvent::SelectionChanged);
        self.focus.focus(window, cx);
        cx.stop_propagation();
        cx.notify();
    }

    fn row_mouse_down(
        &mut self,
        index: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        if event.button == MouseButton::Right {
            // Deliberately does not stop propagation: the context-menu wrapper
            // around this row listens for the same press.
            self.select_for_menu(index, cx);
            return;
        }
        let modifiers = Modifiers {
            shift: event.modifiers.shift,
            toggle: event.modifiers.secondary(),
        };
        self.select_at(index, modifiers, cx);
        if event.click_count >= 2 {
            self.activate(index, cx);
        }
        cx.stop_propagation();
    }

    // --- Header ------------------------------------------------------------

    fn render_header(&self, palette: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let sort = self.model.read(cx).sort();
        let mut header = div()
            .id("torrent-table-header")
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .bg(theme::color(palette.panel))
            .border_b_1()
            .border_color(theme::color(palette.border_mid))
            // Resize drags and their release are watched for the whole strip,
            // so a drag that strays between columns still tracks.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.drag_resize(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.end_resize(cx);
                }),
            );
        for id in self.columns.visible() {
            let definition = columns::definition(id);
            header = header.child(self.render_header_cell(id, definition.sort, sort, palette, cx));
        }
        header.child(
            div()
                .id("column-menu-button")
                .w(px(24.))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(self.column_menu, |button| {
                    button.bg(theme::color(palette.selected))
                })
                .hover(|style| style.bg(theme::color(palette.selected)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.column_menu = !this.column_menu;
                    cx.notify();
                }))
                .child(icons::glyph(
                    icons::COLUMNS,
                    11.,
                    theme::color(palette.text_dim),
                )),
        )
    }

    fn render_header_cell(
        &self,
        id: ColumnId,
        sortable: Option<SortColumn>,
        sort: Sort,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let definition = columns::definition(id);
        let sorted = sortable == Some(sort.column);
        let mut cell = div()
            .id(SharedString::from(format!(
                "header-{}",
                definition.id.as_str()
            )))
            .relative()
            .h_full()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(8.));
        cell = match definition.flexible {
            true => cell.flex_1().min_w(px(self.columns.width(id))),
            false => cell.flex_none().w(px(self.columns.width(id))),
        };
        if let Some(column) = sortable {
            cell = cell
                .cursor_pointer()
                .hover(|style| style.bg(theme::color(palette.selected)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.header_clicked(column, cx);
                }));
        }
        cell = cell.child(theme::mono(
            definition.label,
            9.5,
            theme::color(palette.text_dim),
        ));
        if sorted {
            let arrow = if sort.ascending { "▲" } else { "▼" };
            cell = cell.child(theme::mono(arrow, 8., theme::color(palette.accent_cyan)));
        }
        // The trailing grip sits over the column edge, outside the label's flow.
        if !definition.flexible {
            cell = cell.child(
                div()
                    .id(SharedString::from(format!(
                        "header-grip-{}",
                        definition.id.as_str()
                    )))
                    .absolute()
                    .top_0()
                    .right_0()
                    .h_full()
                    .w(px(GRIP))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.begin_resize(id, event, cx);
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseUpEvent, _, cx| {
                            cx.stop_propagation();
                            this.end_resize(cx);
                        }),
                    ),
            );
        }
        cell
    }

    // --- Column visibility menu -------------------------------------------

    fn render_column_menu(&self, palette: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut panel = div()
            .id("column-menu-panel")
            .absolute()
            .top(px(HEADER_HEIGHT))
            .right(px(8.))
            .w(px(190.))
            .max_h(px(320.))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .rounded(px(6.))
            .border_1()
            .border_color(theme::color(palette.border_strong))
            .bg(theme::color(palette.panel))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for id in ColumnId::ALL {
            let definition = columns::definition(id);
            let visible = self.columns.is_visible(id);
            let locked = id == ColumnId::Name;
            panel = panel.child(
                div()
                    .id(SharedString::from(format!(
                        "column-menu-{}",
                        definition.id.as_str()
                    )))
                    .h(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .px(px(9.))
                    .when(!locked, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(theme::color(palette.selected)))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_column(id, cx)))
                    })
                    .child(div().w(px(12.)).flex_none().child(if visible {
                        icons::glyph(icons::CHECK, 10., theme::color(palette.accent_cyan))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }))
                    .child(theme::text(
                        definition.label.to_owned(),
                        11.,
                        theme::color(if visible {
                            palette.text_body
                        } else {
                            palette.text_muted
                        }),
                    )),
            );
        }
        panel = panel.child(
            div()
                .id("column-menu-reset")
                .h(px(22.))
                .flex_none()
                .flex()
                .items_center()
                .px(px(9.))
                .mt(px(4.))
                .border_t_1()
                .border_color(theme::color(palette.border_mid))
                .cursor_pointer()
                .hover(|style| style.bg(theme::color(palette.selected)))
                .on_click(cx.listener(|this, _, _, cx| this.reset_columns(cx)))
                .child(theme::text(
                    "Reset columns".to_owned(),
                    11.,
                    theme::color(palette.text_muted),
                )),
        );
        div()
            .id("column-menu-overlay")
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.column_menu = false;
                    cx.notify();
                }),
            )
            .child(panel)
    }

    // --- Rows --------------------------------------------------------------

    fn render_rows(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui_kit::AnyElement> {
        let palette = Palette::dark();
        let selection = self.model.read(cx).selection().clone();
        let focused = self.focus.is_focused(window);
        let columns = self.columns.clone();
        let menu_state = self.menu_state(cx);
        range
            .filter_map(|index| self.rows.get(index).cloned().map(|row| (index, row)))
            .map(|(index, row)| {
                let selected = selection.hashes.contains(&row.hash);
                let focus_ring = focused && selection.anchor.as_deref() == Some(row.hash.as_str());
                let mut element = div()
                    .id(SharedString::from(format!("torrent-row-{}", row.hash)))
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(theme::color(palette.row_line))
                    .when(index % 2 == 1, |row| row.bg(theme::color(palette.row_alt)))
                    .when(selected, |row| row.bg(theme::color(palette.selected)))
                    .when(!selected, |row| {
                        row.hover(|style| style.bg(theme::color(palette.selected)))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.row_mouse_down(index, event, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.row_mouse_down(index, event, window, cx);
                        }),
                    );
                if focus_ring {
                    element = element.child(
                        div()
                            .absolute()
                            .inset_0()
                            .border_1()
                            .border_color(theme::color(palette.accent_cyan)),
                    );
                }
                for id in columns.visible() {
                    element = element.child(render_cell(&row, id, &columns, palette));
                }
                // The header's trailing column-menu button has no cell here;
                // keep the row's columns aligned with the header's.
                element = element.child(div().w(px(24.)).flex_none().h_full());
                element
                    .context_menu(move |menu, _, _| build_row_menu(menu, menu_state))
                    .into_any_element()
            })
            .collect()
    }
}

/// The row context menu: the selection verbs, then the single-row actions.
fn build_row_menu(menu: PopupMenu, state: MenuState) -> PopupMenu {
    let menu = menu
        .menu("Resume", Box::new(actions::StartSelection))
        .menu("Pause", Box::new(actions::StopSelection))
        .separator()
        .menu_with_check_and_disabled(
            "Super-seeding",
            state.super_seeding,
            Box::new(actions::SuperSeeding),
            !state.can_super_seed,
        )
        .menu("Force recheck", Box::new(actions::RecheckSelection))
        .menu("Force reannounce", Box::new(actions::ForceReannounce))
        .menu_with_check_and_disabled(
            "Force start",
            state.force_started,
            Box::new(actions::ToggleForceStart),
            !state.single,
        )
        .separator()
        .menu("Set label…", Box::new(actions::OpenLabelDialog))
        .menu("Edit tags…", Box::new(actions::OpenTagDialog))
        .menu("Limit rates…", Box::new(actions::OpenRateLimitDialog))
        .menu("Set location…", Box::new(actions::SetLocation))
        .menu_with_disabled(
            "Cancel move",
            Box::new(actions::CancelMove),
            !state.can_cancel_move,
        )
        .menu_with_disabled(
            "Retry move",
            Box::new(actions::RetryMove),
            !state.can_retry_move,
        )
        .separator()
        .menu_with_disabled(
            "Copy magnet link",
            Box::new(actions::CopyMagnet),
            !state.can_copy_magnet,
        )
        .menu_with_disabled(
            "Open destination",
            Box::new(actions::OpenDestination),
            !state.single,
        )
        .separator();
    menu.menu("Remove…", Box::new(actions::RemoveSelection))
        .menu("Remove + data…", Box::new(actions::RemoveSelectionWithData))
}

/// One data cell, sized by the column's definition.
fn render_cell(row: &TorrentDto, id: ColumnId, columns: &ColumnState, palette: Palette) -> Div {
    let definition = columns::definition(id);
    let mut cell = div()
        .h_full()
        .flex()
        .items_center()
        .px(px(8.))
        .overflow_hidden();
    cell = if definition.flexible {
        cell.flex_1().min_w(px(columns.width(id)))
    } else {
        cell.flex_none().w(px(columns.width(id)))
    };
    match id {
        ColumnId::Name => cell.child(
            theme::text(row.name.clone(), 11.5, theme::color(palette.text_primary))
                .truncate()
                .min_w_0(),
        ),
        ColumnId::Size => cell.child(right(format::bytes(row.size), palette)),
        ColumnId::Done => cell.child(done(row, columns.width(id), palette)),
        ColumnId::Status => cell.child(
            theme::mono(
                status_label(row.status, row.is_open),
                10.,
                theme::color(palette.status_text(row.status)),
            )
            .truncate(),
        ),
        ColumnId::Seeds => cell.child(right(
            format!("{}/{}", row.seeds_connected, row.seeds_swarm),
            palette,
        )),
        ColumnId::Peers => cell.child(right(
            format!("{}/{}", row.peers_connected, row.peers_swarm),
            palette,
        )),
        ColumnId::Down => cell.child(right(format::down_cell(row.down_rate, row.status), palette)),
        ColumnId::Up => cell.child(right(format::up_cell(row.up_rate), palette)),
        ColumnId::Eta => cell.child(right(format::eta(row.eta_seconds, row.status), palette)),
        ColumnId::Ratio => cell.child(right(format::ratio(row.ratio), palette)),
        ColumnId::Label => cell.child(label_cell(row, palette)),
        ColumnId::Tracker => cell.child(text_or_dash(&row.tracker_host, palette)),
        ColumnId::Started => cell.child(text_or_dash(&format::date(row.started_at), palette)),
        ColumnId::Finished => cell.child(text_or_dash(&format::date(row.finished_at), palette)),
    }
}

fn right(value: String, palette: Palette) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .justify_end()
        .child(theme::mono(value, 10.5, theme::color(palette.text_body)).truncate())
}

fn text_or_dash(value: &str, palette: Palette) -> Div {
    let (value, color) = if value.is_empty() || value == "—" {
        ("—".to_owned(), palette.text_dim)
    } else {
        (value.to_owned(), palette.text_muted)
    };
    theme::mono(value, 10.5, theme::color(color))
        .truncate()
        .min_w_0()
}

/// The label plus its tags (V3-10): the label reads as before, each tag a small
/// chip in its name-derived colour.
fn label_cell(row: &TorrentDto, palette: Palette) -> Div {
    if row.label.is_empty() && row.tags.is_empty() {
        return text_or_dash("", palette);
    }
    let mut cell = div().flex().items_center().gap(px(4.)).min_w_0();
    if !row.label.is_empty() {
        cell = cell.child(
            theme::mono(row.label.clone(), 10.5, theme::color(palette.text_muted))
                .truncate()
                .min_w_0(),
        );
    }
    for tag in &row.tags {
        let colour = theme::tag_colour(tag);
        cell = cell.child(
            div()
                .flex_none()
                .px(px(4.))
                .py(px(1.))
                .rounded(px(3.))
                .bg(theme::color(theme::blend(palette.app, colour, 0.22)))
                .child(theme::mono(tag.clone(), 9.5, theme::color(colour)).truncate()),
        );
    }
    cell
}

/// The design's status text: lowercase and coloured, never a pill. A paused
/// torrent that is still loaded (the queue holding it back, or the user
/// pausing without closing) reads "queued", matching the console.
fn status_label(status: Status, is_open: bool) -> &'static str {
    match status {
        Status::Downloading => "downloading",
        Status::Seeding => "seeding",
        Status::Completed => "completed",
        Status::Paused if is_open => "queued",
        Status::Paused => "paused",
        Status::Stalled => "stalled",
        Status::Checking => "checking",
        Status::Error => "error",
    }
}

/// The Done cell: an 8 px flat bar in the status colour, with the percentage.
fn done(row: &TorrentDto, width: f32, palette: Palette) -> Div {
    let fill = palette.status_fill(row.status);
    let bar_width = (width - 46.).max(20.);
    #[allow(clippy::cast_possible_truncation)]
    let filled = (row.percent.clamp(0.0, 100.0) / 100.0 * f64::from(bar_width)) as f32;
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .w(px(bar_width))
                .flex_none()
                .h(px(8.))
                .rounded(px(1.))
                .bg(theme::color(palette.track))
                .child(div().h_full().w(px(filled.max(0.))).bg(theme::color(fill))),
        )
        .child(theme::mono(
            format!("{:>3.0}%", row.percent),
            10.,
            theme::color(palette.text_muted),
        ))
}

fn empty_notice(text: &'static str, palette: Palette) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::color(palette.app))
        .child(theme::text(
            text.to_owned(),
            11.5,
            theme::color(palette.text_muted),
        ))
}

impl gpui_kit::Render for TorrentTable {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let palette = Palette::dark();
        let notice = if self.rows.is_empty() {
            let model = self.model.read(cx);
            let filtered =
                !model.search().is_empty() || *model.filter() != table_state::Filter::All;
            Some(if filtered {
                "no torrents match — clear the filter"
            } else {
                "no torrents"
            })
        } else {
            None
        };
        let mut body = div()
            .id("torrent-table")
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(theme::color(palette.app))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::keyboard))
            .child(self.render_header(palette, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        uniform_list(
                            "torrent-table-rows",
                            self.rows.len(),
                            cx.processor(|this, range: Range<usize>, window, cx| {
                                this.render_rows(range, window, cx)
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .size_full(),
                    )
                    .children(notice.map(|text| empty_notice(text, palette))),
            );
        if self.column_menu {
            body = body.child(self.render_column_menu(palette, cx));
        }
        body
    }
}
