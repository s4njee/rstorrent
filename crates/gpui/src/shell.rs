//! The top-level browser window.
//!
//! The shell owns no daemon policy: it lays out the title bar, toolbar, filter
//! sidebar, torrent table, selection bar and status bar over [`TorrentsModel`],
//! observes the model for poll ticks, and routes actions — from the native
//! menus, the toolbar, the context menu and the keyboard — to the model.
//!
//! It is also the single writer of `settings.json`: view state (filter, search,
//! sort, columns, sidebar width) is persisted here, and daemon-affecting
//! settings go through [`TorrentsModel::update_settings`], so a save can never
//! race another save.

use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Root, Sizable, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, App, Context, Div, Entity, EventEmitter, FocusHandle, Stateful, Subscription, Window,
};

use crate::actions::{
    self, CancelMove, ClearSelection, CopyMagnet, DismissOverlay, FocusFilter, ForceReannounce,
    OpenDestination, OpenLabelDialog, OpenPreferences, OpenRateLimitDialog, OpenSession,
    OpenStatistics, OpenTagDialog, PeerBan, PeerDisconnect, PeerSnub, QueueBottom, QueueDown,
    QueueTop, QueueUp, RecheckSelection, RemoveSelection, RemoveSelectionWithData, RetryMove,
    SaveSession, SelectAll, SetFilePriority, SetLabel, SetLocation, SetPieceLength, SetRateLimit,
    ShutdownDaemon, StartDaemon, StartSelection, StopSelection, SuperSeeding, ToggleForceStart,
    ToggleSelection, ToggleTurtle, ToggleWebUi, TrackerRemove, TrackerToggle,
};
use crate::add_dialogs::{AddEvent, AddMagnetDialog, AddSource, AddTorrentDialog};
use crate::columns::ColumnState;
use crate::create_torrent::{CreateEvent, CreateTorrentDialog};
use crate::detail_panel::DetailPanel;
use crate::dialogs::{
    LabelDialog, LabelEvent, RateLimitDialog, RateLimitEvent, RemoveDialog, RemoveEvent,
    SetLocationDialog, SetLocationEvent, StatisticsDialog, StatisticsEvent, TagDialog, TagEvent,
};
use crate::icons;
use crate::model::TorrentsModel;
use crate::prefs::{PreferencesDialog, PreferencesEvent};
use crate::services::{PeerVerb, QueueDirection, Services};
use crate::session_dialog::{SessionDialog, SessionEvent};
use crate::settings::SettingsStore;
use crate::sidebar::{FilterSidebar, FilterSidebarEvent};
use crate::table_state::{self, Filter};
use crate::theme::{self, Palette};
use crate::torrent_table::{DialogRequest, TorrentTable, TorrentTableEvent};

/// The top-level browser window.
pub struct Shell {
    palette: Palette,
    model: Entity<TorrentsModel>,
    table: Entity<TorrentTable>,
    detail: Entity<DetailPanel>,
    sidebar: Entity<FilterSidebar>,
    search: Entity<InputState>,
    remove_dialog: Option<Entity<RemoveDialog>>,
    label_dialog: Option<Entity<LabelDialog>>,
    tag_dialog: Option<Entity<TagDialog>>,
    rate_dialog: Option<Entity<RateLimitDialog>>,
    location_dialog: Option<Entity<SetLocationDialog>>,
    add_torrent_dialog: Option<Entity<AddTorrentDialog>>,
    add_magnet_dialog: Option<Entity<AddMagnetDialog>>,
    create_dialog: Option<Entity<CreateTorrentDialog>>,
    statistics_dialog: Option<Entity<StatisticsDialog>>,
    preferences_dialog: Option<Entity<PreferencesDialog>>,
    session_dialog: Option<Entity<SessionDialog>>,
    selection_summary: String,
    status_line: String,
    focus: FocusHandle,
    subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(
        services: Arc<Services>,
        settings_store: SettingsStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let columns = ColumnState::from_prefs(&settings_store.load().columns);
        let model = cx.new(|cx| {
            let mut model = TorrentsModel::new(Arc::clone(&services), settings_store.clone());
            model.start_polling(cx);
            model
        });
        let table = cx.new(|cx| TorrentTable::new(model.clone(), columns, window, cx));
        let detail = cx.new(|cx| DetailPanel::new(model.clone(), window, cx));
        let sidebar = cx.new(|_| FilterSidebar::new(model.clone()));
        let search = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("/ filter");
            let value = services.settings().search.clone();
            if !value.is_empty() {
                state.set_value(value, window, cx);
            }
            state
        });
        let mut shell = Self {
            palette: Palette::dark(),
            model,
            table,
            detail,
            sidebar,
            search,
            remove_dialog: None,
            label_dialog: None,
            tag_dialog: None,
            rate_dialog: None,
            location_dialog: None,
            add_torrent_dialog: None,
            add_magnet_dialog: None,
            create_dialog: None,
            statistics_dialog: None,
            preferences_dialog: None,
            session_dialog: None,
            selection_summary: String::new(),
            status_line: "connecting…".to_owned(),
            focus: cx.focus_handle(),
            subscriptions: Vec::new(),
        };
        shell.observe_model(window, cx);
        shell.subscribe_table(window, cx);
        shell.subscribe_sidebar(cx);
        shell.subscribe_search(window, cx);
        shell
    }

    fn observe_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        self.subscriptions
            .push(cx.observe_in(&model, window, |shell, _, window, cx| {
                shell.table.update(cx, |table, cx| table.sync(cx));
                shell.refresh_status(cx);
                let _ = window;
                cx.notify();
            }));
    }

    fn subscribe_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let table = self.table.clone();
        self.subscriptions.push(cx.subscribe_in(
            &table,
            window,
            |shell, _, event: &TorrentTableEvent, window, cx| match event {
                TorrentTableEvent::SelectionChanged => {
                    shell.refresh_status(cx);
                    cx.notify();
                }
                TorrentTableEvent::SortChanged { .. } => {
                    shell.persist_view(cx);
                    cx.notify();
                }
                TorrentTableEvent::ColumnsChanged => {
                    shell.persist_view(cx);
                    cx.notify();
                }
                TorrentTableEvent::Activated { .. } => {
                    // Double-click toggles start/stop on the selection; the
                    // model re-polls immediately so the row updates.
                    shell.toggle_selection(cx);
                }
                TorrentTableEvent::RequestDialog(request) => {
                    shell.open_dialog(*request, window, cx);
                }
            },
        ));
    }

    fn subscribe_sidebar(&mut self, cx: &mut Context<Self>) {
        let sidebar = self.sidebar.clone();
        self.subscriptions.push(cx.subscribe(
            &sidebar,
            |shell, _, event: &FilterSidebarEvent, cx| {
                if matches!(event, FilterSidebarEvent::FilterChanged) {
                    shell.persist_view(cx);
                    cx.notify();
                }
            },
        ));
    }

    fn subscribe_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = self.search.clone();
        self.subscriptions.push(cx.subscribe_in(
            &search,
            window,
            |shell, input: &Entity<InputState>, event: &InputEvent, _, cx| match event {
                InputEvent::Change => {
                    let value = input.read(cx).value().to_string();
                    shell
                        .model
                        .update(cx, |model, cx| model.set_search(value, cx));
                    cx.notify();
                }
                // Persist when the field is done, not on every keystroke.
                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                    shell.persist_view(cx);
                }
                InputEvent::Focus => {}
            },
        ));
    }

    fn toggle_selection(&mut self, cx: &mut Context<Self>) {
        let stopped = self.model.read(cx).selection_is_stopped();
        self.model.update(cx, |model, cx| {
            if stopped {
                model.start_selection(cx);
            } else {
                model.stop_selection(cx);
            }
        });
    }

    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let summary = table_state::selection_summary(model.torrents(), &model.selection().hashes);
        self.selection_summary = if summary.count == 0 {
            String::new()
        } else {
            format!(
                "{} selected · {} · ↓ {} · ↑ {}",
                summary.count,
                crate::format::bytes(summary.size),
                crate::format::rate(summary.down_rate),
                crate::format::rate(summary.up_rate),
            )
        };
        let globals = model.globals();
        let free = crate::format::free(globals.free_space);
        self.status_line = format!(
            "dht: {} nodes · ↓ {} · ↑ {}{}",
            globals.dht_nodes,
            crate::format::rate(globals.down_rate),
            crate::format::rate(globals.up_rate),
            if free.is_empty() {
                String::new()
            } else {
                format!(" · {free}")
            },
        );
    }

    /// Write the view state — filter, search, sort and columns — to settings.
    fn persist_view(&mut self, cx: &mut Context<Self>) {
        let filter = {
            let model = self.model.read(cx);
            match model.filter() {
                Filter::All => String::new(),
                Filter::Status(status) => format!("status:{}", status.as_str()),
                Filter::ErrorKind(kind) => format!("error:{kind}"),
                Filter::Label(label) => format!("label:{label}"),
                Filter::Tag(tag) => format!("tag:{tag}"),
                Filter::Tracker(host) => format!("tracker:{host}"),
                Filter::View(view) => format!("view:{view}"),
            }
        };
        let search = self.model.read(cx).search().to_owned();
        let sort = self.model.read(cx).sort();
        let columns = self.table.read(cx).columns().prefs();
        let model = self.model.clone();
        model.update(cx, move |model, cx| {
            model.update_settings(
                move |settings| {
                    settings.filter = filter;
                    settings.search = search;
                    settings.sort_column = sort_column_name(sort.column).to_owned();
                    settings.sort_descending = !sort.ascending;
                    settings.columns = columns;
                },
                cx,
            );
        });
    }

    fn open_dialog(&mut self, request: DialogRequest, window: &mut Window, cx: &mut Context<Self>) {
        match request {
            DialogRequest::Remove => self.open_remove(false, window, cx),
            DialogRequest::SetLabel => self.open_label(window, cx),
            DialogRequest::RateLimit => self.open_rate_limit(window, cx),
            DialogRequest::SetLocation => self.open_location(window, cx),
        }
    }

    /// Where a new torrent lands by default: the daemon's configured save path.
    fn default_save_path(&self, cx: &App) -> String {
        self.model.read(cx).services().settings().default_save_path
    }

    fn open_add_torrent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_torrent_dialog.is_some() {
            return;
        }
        let save_path = self.default_save_path(cx);
        let model = self.model.clone();
        let dialog = cx.new(|cx| AddTorrentDialog::new(&save_path, model, window, cx));
        self.close_overlays(cx);
        self.subscribe_add(&dialog, window, cx);
        self.add_torrent_dialog = Some(dialog);
    }

    fn open_add_magnet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_magnet_dialog.is_some() {
            return;
        }
        let save_path = self.default_save_path(cx);
        let model = self.model.clone();
        let dialog = cx.new(|cx| AddMagnetDialog::new(&save_path, model, window, cx));
        self.close_overlays(cx);
        self.subscribe_add(&dialog, window, cx);
        self.add_magnet_dialog = Some(dialog);
    }

    /// The create dialog hashes a local file or folder into a `.torrent`.
    fn open_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.create_dialog.is_some() {
            return;
        }
        let model = self.model.clone();
        let dialog = cx.new(|cx| CreateTorrentDialog::new(model, window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &CreateEvent, _, cx| {
                match event {
                    // The dialog keeps the info-hash on screen; the shell only
                    // records what happened.
                    CreateEvent::Cancel => shell.close_overlays(cx),
                    CreateEvent::Done { message, hash } => {
                        let message = message.clone();
                        let hash = hash.clone();
                        shell.model.update(cx, |model, cx| {
                            model.note_creation(&message, hash, cx);
                        });
                    }
                }
                cx.notify();
            },
        ));
        self.create_dialog = Some(dialog);
    }

    /// Both add dialogs report the same way: applied through the model, so the
    /// add is logged, a failure is shown, and the list re-polls immediately.
    fn subscribe_add<T: EventEmitter<AddEvent>>(
        &mut self,
        dialog: &Entity<T>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.subscriptions.push(cx.subscribe_in(
            dialog,
            window,
            |shell, _, event: &AddEvent, _, cx| {
                let (source, options) = match event {
                    AddEvent::Cancel => {
                        shell.close_overlays(cx);
                        cx.notify();
                        return;
                    }
                    // An exact duplicate (V3-13): act on the existing torrent,
                    // then close the dialog.
                    AddEvent::Reveal { hash } => {
                        let hash = hash.clone();
                        shell.model.update(cx, |model, cx| {
                            model.focus_torrent(hash, cx);
                        });
                        shell.close_overlays(cx);
                        cx.notify();
                        return;
                    }
                    AddEvent::MergeTrackers { hash, urls } => {
                        let (hash, urls) = (hash.clone(), urls.clone());
                        shell.model.update(cx, |model, cx| {
                            model.merge_trackers(hash, urls, cx);
                        });
                        shell.close_overlays(cx);
                        cx.notify();
                        return;
                    }
                    AddEvent::Confirm { source, options } => (source, options.clone()),
                };
                shell.model.update(cx, |model, cx| match source {
                    AddSource::Bytes(bytes) => model.add_raw(bytes.clone(), options, cx),
                    AddSource::Magnet(uri) => model.add_magnet(uri.clone(), options, cx),
                });
                shell.close_overlays(cx);
                cx.notify();
            },
        ));
    }

    /// Confirm removing the selection. `delete_data` opens the dialog with the
    /// "also delete the files" box already ticked, which is what ⇧Del means.
    fn open_remove(&mut self, delete_data: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.remove_dialog.is_some() {
            return;
        }
        let (count, name, size) = {
            let model = self.model.read(cx);
            let selected = model.selected();
            let size = selected.iter().map(|torrent| torrent.size).sum();
            let name = model
                .single_selection()
                .map_or(String::new(), |torrent| torrent.name.clone());
            (selected.len(), name, size)
        };
        if count == 0 {
            return;
        }
        let can_delete_data = self.model.read(cx).services().is_local();
        let dialog = cx.new(|cx| {
            RemoveDialog::new(count, name, size, can_delete_data, delete_data, window, cx)
        });
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &RemoveEvent, _, cx| {
                match event {
                    RemoveEvent::Cancel => shell.remove_dialog = None,
                    RemoveEvent::Confirm { delete_data } => {
                        let delete_data = *delete_data;
                        shell.model.update(cx, |model, cx| {
                            model.remove_selection(delete_data, cx);
                        });
                        shell.remove_dialog = None;
                    }
                }
                cx.notify();
            },
        ));
        self.remove_dialog = Some(dialog);
    }

    fn open_label(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.label_dialog.is_some() {
            return;
        }
        let (count, existing) = {
            let model = self.model.read(cx);
            (model.selected().len(), model.labels())
        };
        if count == 0 {
            return;
        }
        let dialog = cx.new(|cx| LabelDialog::new(count, existing, window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &LabelEvent, _, cx| {
                match event {
                    LabelEvent::Cancel => shell.label_dialog = None,
                    LabelEvent::Confirm { label } => {
                        let label = label.clone();
                        shell
                            .model
                            .update(cx, |model, cx| model.set_label(label, cx));
                        shell.label_dialog = None;
                    }
                }
                cx.notify();
            },
        ));
        self.label_dialog = Some(dialog);
    }

    /// Open the bulk tag editor on the selection (V3-10).
    fn open_tags(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tag_dialog.is_some() {
            return;
        }
        let (count, union) = {
            let model = self.model.read(cx);
            (model.selected().len(), model.selected_tags())
        };
        if count == 0 {
            return;
        }
        let dialog = cx.new(|cx| TagDialog::new(count, union, window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &TagEvent, _, cx| {
                match event {
                    TagEvent::Cancel => shell.tag_dialog = None,
                    TagEvent::Confirm { add, remove } => {
                        let (add, remove) = (add.clone(), remove.clone());
                        shell.model.update(cx, |model, cx| {
                            model.edit_tags(add, remove, cx);
                        });
                        shell.tag_dialog = None;
                    }
                }
                cx.notify();
            },
        ));
        self.tag_dialog = Some(dialog);
    }

    fn open_rate_limit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rate_dialog.is_some() {
            return;
        }
        let torrent = self.model.read(cx).single_selection().cloned();
        if self.model.read(cx).selected().is_empty() {
            return;
        }
        let dialog = cx.new(|cx| RateLimitDialog::new(torrent.as_ref(), window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &RateLimitEvent, _, cx| {
                if let RateLimitEvent::Confirm {
                    down_kb,
                    up_kb,
                    peers_max,
                    peers_min,
                    uploads_max,
                } = *event
                {
                    shell.model.update(cx, |model, cx| {
                        model.set_torrent_limits(down_kb, up_kb, cx);
                        model.set_connection_limits(peers_max, peers_min, uploads_max, cx);
                    });
                }
                shell.rate_dialog = None;
                cx.notify();
            },
        ));
        self.rate_dialog = Some(dialog);
    }

    fn open_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.location_dialog.is_some() {
            return;
        }
        let (current, can_move) = {
            let model = self.model.read(cx);
            let current = model
                .single_selection()
                .map_or(String::new(), |torrent| torrent.save_path.clone());
            if model.single_selection().is_none() {
                return;
            }
            (current, model.services().is_local())
        };
        let dialog = cx.new(|cx| SetLocationDialog::new(current, can_move, window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &SetLocationEvent, _, cx| {
                if let SetLocationEvent::Confirm { path, move_data } = event {
                    let (path, move_data) = (path.clone(), *move_data);
                    shell.model.update(cx, |model, cx| {
                        model.set_location(path, move_data, cx);
                    });
                }
                shell.location_dialog = None;
                cx.notify();
            },
        ));
        self.location_dialog = Some(dialog);
    }

    /// Open Preferences on a working copy of the live settings (G6-S1).
    /// Apply persists through the model's single settings writer and pushes
    /// daemon-affecting keys; Cancel drops the draft untouched.
    fn open_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preferences_dialog.is_some() {
            return;
        }
        let (services, model, settings, labels) = {
            let model = self.model.read(cx);
            (
                Arc::clone(model.services()),
                self.model.clone(),
                model.services().settings(),
                model.labels(),
            )
        };
        let dialog =
            cx.new(|cx| PreferencesDialog::new(services, model, &settings, labels, window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &PreferencesEvent, _, cx| {
                match event {
                    PreferencesEvent::Cancel => {}
                    PreferencesEvent::Apply(next) => {
                        shell.model.update(cx, |model, cx| {
                            model.apply_preferences((**next).clone(), cx);
                        });
                    }
                }
                shell.preferences_dialog = None;
                cx.notify();
            },
        ));
        self.preferences_dialog = Some(dialog);
    }
    /// Open the session export/import dialog (V3-22). A finished import is
    /// logged like every other mutation; closing mid-run is safe — the
    /// journaled job continues and a reopened dialog rejoins it.
    fn open_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session_dialog.is_some() {
            return;
        }
        let (services, model) = {
            let model = self.model.read(cx);
            (Arc::clone(model.services()), self.model.clone())
        };
        let dialog = cx.new(|cx| SessionDialog::new(services, model, window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &SessionEvent, _, cx| match event {
                SessionEvent::Close => {
                    shell.session_dialog = None;
                    cx.notify();
                }
                SessionEvent::Imported {
                    added,
                    skipped,
                    failed,
                } => {
                    let (added, skipped, failed) = (*added, *skipped, failed.clone());
                    shell.model.update(cx, |model, cx| {
                        model.note_session_import(added, skipped, &failed, cx);
                    });
                }
            },
        ));
        self.session_dialog = Some(dialog);
    }

    fn open_statistics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.statistics_dialog.is_some() {
            return;
        }
        let (services, stats_path) = {
            let model = self.model.read(cx);
            (Arc::clone(model.services()), model.stats_path())
        };
        let dialog = cx.new(|cx| StatisticsDialog::new(services, stats_path, window, cx));
        self.close_overlays(cx);
        self.subscriptions.push(cx.subscribe_in(
            &dialog,
            window,
            |shell, _, event: &StatisticsEvent, _, cx| {
                if matches!(event, StatisticsEvent::Close) {
                    shell.statistics_dialog = None;
                }
                cx.notify();
            },
        ));
        self.statistics_dialog = Some(dialog);
    }

    /// Give focus back to the table after an overlay closes.
    fn close_overlays(&mut self, cx: &mut Context<Self>) {
        self.remove_dialog = None;
        self.label_dialog = None;
        self.tag_dialog = None;
        self.rate_dialog = None;
        self.location_dialog = None;
        self.add_torrent_dialog = None;
        self.add_magnet_dialog = None;
        self.create_dialog = None;
        self.statistics_dialog = None;
        self.preferences_dialog = None;
        cx.notify();
    }

    fn dismiss_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remove_dialog.is_some()
            || self.label_dialog.is_some()
            || self.tag_dialog.is_some()
            || self.rate_dialog.is_some()
            || self.location_dialog.is_some()
            || self.add_torrent_dialog.is_some()
            || self.add_magnet_dialog.is_some()
            || self.create_dialog.is_some()
            || self.statistics_dialog.is_some()
            || self.preferences_dialog.is_some()
        {
            self.close_overlays(cx);
            return;
        }
        self.table
            .update(cx, |table, cx| table.close_column_menu(cx));
        self.model.update(cx, |model, cx| model.clear_selection(cx));
        self.focus.focus(window, cx);
    }

    // --- Action handlers ----------------------------------------------------

    fn title_bar(&self, cx: &App) -> impl IntoElement {
        let palette = self.palette;
        let model = self.model.read(cx);
        let version = model
            .connection()
            .daemon_version
            .clone()
            .unwrap_or_default();
        let count = model.torrents().len();
        let title = if version.is_empty() {
            "rtorrent — connecting…".to_owned()
        } else {
            format!("rtorrent {version} · {count} torrents")
        };
        TitleBar::new()
            .h(px(theme::geometry::TITLE_BAR))
            .flex_none()
            .bg(theme::color(palette.panel))
            .border_b_1()
            .border_color(theme::color(palette.border_mid))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .left_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(theme::text(title, 11., theme::color(palette.text_muted))),
            )
    }

    fn toolbar(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let palette = self.palette;
        let has_selection = self.model.read(cx).has_selection();

        let add_file = icons::toolbar_button("toolbar-add-file", icons::ADD, true, palette)
            .on_click(cx.listener(|shell, _, window, cx| {
                shell.open_add_torrent(window, cx);
            }));
        let add_magnet = icons::toolbar_button("toolbar-add-magnet", icons::MAGNET, true, palette)
            .on_click(cx.listener(|shell, _, window, cx| {
                shell.open_add_magnet(window, cx);
            }));
        let create = icons::toolbar_button(
            "toolbar-create-torrent",
            icons::CREATE_TORRENT,
            true,
            palette,
        )
        .on_click(cx.listener(|shell, _, window, cx| {
            shell.open_create(window, cx);
        }));
        let remove = icons::toolbar_button("toolbar-remove", icons::REMOVE, has_selection, palette)
            .on_click(cx.listener(|shell, _, window, cx| {
                shell.open_remove(false, window, cx);
            }));
        let resume = icons::toolbar_button("toolbar-resume", icons::PLAY, has_selection, palette)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.start_selection(cx));
            }));
        let pause = icons::toolbar_button("toolbar-pause", icons::PAUSE, has_selection, palette)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell.model.update(cx, |model, cx| model.stop_selection(cx));
            }));
        let up = icons::toolbar_button("toolbar-queue-up", icons::UP, has_selection, palette)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.queue_move(QueueDirection::Up, cx));
            }));
        let down = icons::toolbar_button("toolbar-queue-down", icons::DOWN, has_selection, palette)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.queue_move(QueueDirection::Down, cx));
            }));
        let top = icons::toolbar_button("toolbar-queue-top", icons::TOP, has_selection, palette)
            .on_click(cx.listener(|shell, _, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.queue_move(QueueDirection::Top, cx));
            }));
        let bottom = icons::toolbar_button(
            "toolbar-queue-bottom",
            icons::BOTTOM,
            has_selection,
            palette,
        )
        .on_click(cx.listener(|shell, _, _, cx| {
            shell
                .model
                .update(cx, |model, cx| model.queue_move(QueueDirection::Bottom, cx));
        }));

        div()
            .id("toolbar")
            .h(px(theme::geometry::TOOLBAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .bg(theme::color(palette.panel))
            .border_b_1()
            .border_color(theme::color(palette.border_mid))
            .child(add_file)
            .child(add_magnet)
            .child(create)
            .child(remove)
            .child(icons::separator(palette))
            .child(resume)
            .child(pause)
            .child(icons::separator(palette))
            .child(up)
            .child(down)
            .child(top)
            .child(bottom)
            .child(div().flex_1())
            .child(
                div()
                    .id("search-field")
                    .w(px(190.))
                    .h(px(24.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(9.))
                    .bg(theme::color(palette.field))
                    .border_1()
                    .border_color(theme::color(palette.border_strong))
                    .rounded(px(4.))
                    .child(Input::new(&self.search).appearance(false).small()),
            )
    }

    /// The bulk-action bar the design shows once more than one row is selected.
    fn selection_bar(&self, cx: &App) -> Option<Stateful<Div>> {
        let palette = self.palette;
        let selected = self.model.read(cx).selection().hashes.len();
        if selected < 2 {
            return None;
        }
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(18.))
                .flex()
                .items_center()
                .px(px(8.))
                .rounded(px(3.))
                .border_1()
                .border_color(theme::color(palette.border_strong))
                .bg(theme::color(palette.track))
                .cursor_pointer()
                .hover(|style| style.bg(theme::color(palette.selected)))
                .child(theme::text(
                    label.to_owned(),
                    10.5,
                    theme::color(palette.text_body),
                ))
        };
        Some(
            div()
                .id("selection-bar")
                .h(px(theme::geometry::SELECTION_BAR))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .bg(theme::color(palette.panel))
                .border_t_1()
                .border_color(theme::color(palette.border_mid))
                .child(theme::mono(
                    self.selection_summary.clone(),
                    10.5,
                    theme::color(palette.text_muted),
                ))
                .child(div().flex_1())
                .child(self.model_button("selection-resume", "Resume").on_click({
                    let model = self.model.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
                        model.update(cx, |model, cx| model.start_selection(cx));
                    }
                }))
                .child(self.model_button("selection-pause", "Pause").on_click({
                    let model = self.model.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
                        model.update(cx, |model, cx| model.stop_selection(cx));
                    }
                }))
                .child(button("selection-remove", "Remove")),
        )
    }

    fn model_button(&self, id: &'static str, label: &'static str) -> Stateful<Div> {
        let palette = self.palette;
        div()
            .id(id)
            .h(px(18.))
            .flex()
            .items_center()
            .px(px(8.))
            .rounded(px(3.))
            .border_1()
            .border_color(theme::color(palette.border_strong))
            .bg(theme::color(palette.track))
            .cursor_pointer()
            .hover(|style| style.bg(theme::color(palette.selected)))
            .child(theme::text(
                label.to_owned(),
                10.5,
                theme::color(palette.text_body),
            ))
    }

    fn status_bar(&self, cx: &App) -> Stateful<Div> {
        let palette = self.palette;
        let notice = self.model.read(cx).notice().map(str::to_owned);
        div()
            .id("status-bar")
            .h(px(theme::geometry::STATUS_BAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .bg(theme::color(palette.panel))
            .border_t_1()
            .border_color(theme::color(palette.border_mid))
            .child(theme::text(
                self.status_line.clone(),
                11.,
                theme::color(palette.text_muted),
            ))
            .child(div().flex_1())
            .when_some(notice, |bar, notice| {
                bar.child(theme::text(notice, 11., theme::color(palette.accent_amber)))
            })
            .child(theme::text(
                self.selection_summary.clone(),
                11.,
                theme::color(palette.text_muted),
            ))
    }

    fn body(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        use rtorrent_core::types::ConnPhase;
        let palette = self.palette;
        // Copied out of the model before building anything that needs `cx` mutably.
        let (connected, connecting, endpoint, error, retry) = {
            let connection = self.model.read(cx).connection();
            (
                connection.phase == ConnPhase::Connected,
                connection.phase == ConnPhase::Connecting,
                connection.endpoint.clone(),
                connection.error.clone().unwrap_or_default(),
                connection.retry_in_seconds.unwrap_or(0),
            )
        };
        if !connected {
            return div()
                .id("disconnected-card")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .bg(theme::color(palette.app))
                .child(theme::text(
                    if connecting {
                        "connecting to rtorrent…"
                    } else {
                        "can't reach rtorrent"
                    }
                    .to_owned(),
                    13.,
                    theme::color(palette.text_primary),
                ))
                .child(theme::mono(
                    endpoint,
                    10.5,
                    theme::color(palette.text_muted),
                ))
                .when(!error.is_empty(), |card| {
                    card.child(theme::mono(error, 10.5, theme::color(palette.accent_red)))
                })
                .when(retry > 0, |card| {
                    card.child(theme::text(
                        format!("retrying in {retry}s…"),
                        11.,
                        theme::color(palette.text_muted),
                    ))
                })
                // Offered only once a connection attempt has failed: there is
                // nothing to start while we are still waiting on one.
                .when(!connecting, |card| {
                    card.child(
                        self.model_button("disconnected-start", "Start rtorrent")
                            .on_click({
                                let model = self.model.clone();
                                move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut App| {
                                    model.update(cx, |model, cx| model.start_daemon(cx));
                                }
                            }),
                    )
                });
        }
        div()
            .id("browser-body")
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .id("sidebar")
                    .w(px(theme::geometry::SIDEBAR))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(theme::color(palette.border_mid))
                    .child(self.sidebar.clone()),
            )
            .child(
                div()
                    .id("workspace-pane")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(div().flex_1().min_h_0().child(self.table.clone()))
                    // The detail panel sits under the table, inside the
                    // workspace: the sidebar runs the full height beside both.
                    .child(self.detail.clone()),
            )
    }
}

fn sort_column_name(column: table_state::SortColumn) -> &'static str {
    match column {
        table_state::SortColumn::Name => "name",
        table_state::SortColumn::Size => "size",
        table_state::SortColumn::Percent => "percent",
        table_state::SortColumn::Status => "status",
        table_state::SortColumn::DownRate => "downRate",
        table_state::SortColumn::UpRate => "upRate",
        table_state::SortColumn::EtaSeconds => "etaSeconds",
        table_state::SortColumn::Ratio => "ratio",
        table_state::SortColumn::StartedAt => "startedAt",
        table_state::SortColumn::FinishedAt => "finishedAt",
    }
}

impl gpui_kit::Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.palette;
        let selection_bar = self.selection_bar(cx);
        div()
            .id("shell")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .font_family(theme::SANS)
            .text_color(theme::color(palette.text_primary))
            .on_action(cx.listener(|shell, _: &DismissOverlay, window, cx| {
                shell.dismiss_overlay(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &StartSelection, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.start_selection(cx));
            }))
            .on_action(cx.listener(|shell, _: &StopSelection, _, cx| {
                shell.model.update(cx, |model, cx| model.stop_selection(cx));
            }))
            .on_action(cx.listener(|shell, _: &ToggleSelection, _, cx| {
                shell.toggle_selection(cx);
            }))
            .on_action(cx.listener(|shell, _: &RecheckSelection, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.recheck_selection(cx));
            }))
            .on_action(cx.listener(|shell, _: &ForceReannounce, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.reannounce_selection(cx));
            }))
            .on_action(cx.listener(|shell, _: &QueueUp, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.queue_move(QueueDirection::Up, cx));
            }))
            .on_action(cx.listener(|shell, _: &QueueDown, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.queue_move(QueueDirection::Down, cx));
            }))
            .on_action(cx.listener(|shell, _: &QueueTop, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.queue_move(QueueDirection::Top, cx));
            }))
            .on_action(cx.listener(|shell, _: &QueueBottom, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.queue_move(QueueDirection::Bottom, cx));
            }))
            .on_action(cx.listener(|shell, _: &ToggleForceStart, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.toggle_force_start(cx));
            }))
            .on_action(cx.listener(|shell, _: &SelectAll, _, cx| {
                shell.model.update(cx, |model, cx| model.select_all(cx));
            }))
            .on_action(cx.listener(|shell, _: &RemoveSelection, window, cx| {
                shell.open_remove(false, window, cx);
            }))
            .on_action(
                cx.listener(|shell, _: &RemoveSelectionWithData, window, cx| {
                    shell.open_remove(true, window, cx);
                }),
            )
            .on_action(cx.listener(|shell, _: &SuperSeeding, _, cx| {
                let next = !shell
                    .model
                    .read(cx)
                    .single_selection()
                    .is_some_and(|torrent| torrent.connection_type == "initial_seed");
                shell
                    .model
                    .update(cx, |model, cx| model.set_super_seeding(next, cx));
            }))
            .on_action(cx.listener(|shell, _: &SetLocation, window, cx| {
                shell.open_location(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &OpenLabelDialog, window, cx| {
                shell.open_label(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &OpenTagDialog, window, cx| {
                shell.open_tags(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &OpenRateLimitDialog, window, cx| {
                shell.open_rate_limit(window, cx);
            }))
            .on_action(cx.listener(|shell, action: &SetLabel, _, cx| {
                let label = action.0.clone();
                shell
                    .model
                    .update(cx, |model, cx| model.set_label(label, cx));
            }))
            .on_action(cx.listener(|shell, action: &SetRateLimit, _, cx| {
                let (down_kb, up_kb) = (action.down_kb, action.up_kb);
                shell
                    .model
                    .update(cx, |model, cx| model.set_torrent_limits(down_kb, up_kb, cx));
            }))
            .on_action(cx.listener(|shell, _: &CopyMagnet, _, cx| {
                shell.model.update(cx, |model, cx| model.copy_magnet(cx));
            }))
            .on_action(cx.listener(|shell, _: &OpenDestination, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.open_destination(cx));
            }))
            .on_action(cx.listener(|shell, _: &CancelMove, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.cancel_move_for_selection(cx));
            }))
            .on_action(cx.listener(|shell, _: &RetryMove, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.retry_move_for_selection(cx));
            }))
            .on_action(cx.listener(|shell, _: &StartDaemon, _, cx| {
                shell.model.update(cx, |model, cx| model.start_daemon(cx));
            }))
            .on_action(cx.listener(|shell, _: &ToggleWebUi, _, cx| {
                shell.model.update(cx, |model, cx| model.toggle_web_ui(cx));
            }))
            // The detail panel's row menus. Handled here rather than in the
            // panel because a menu dispatches through the focus path, which is
            // not guaranteed to run through the panel's own element.
            .on_action(cx.listener(|shell, action: &SetFilePriority, _, cx| {
                let priority = action.0;
                shell
                    .detail
                    .update(cx, |panel, cx| panel.set_file_menu_priority(priority, cx));
            }))
            .on_action(cx.listener(|shell, _: &PeerSnub, _, cx| {
                shell
                    .detail
                    .update(cx, |panel, cx| panel.act_on_peer(PeerVerb::Snub, cx));
            }))
            .on_action(cx.listener(|shell, _: &PeerDisconnect, _, cx| {
                shell
                    .detail
                    .update(cx, |panel, cx| panel.act_on_peer(PeerVerb::Disconnect, cx));
            }))
            .on_action(cx.listener(|shell, _: &PeerBan, _, cx| {
                shell
                    .detail
                    .update(cx, |panel, cx| panel.act_on_peer(PeerVerb::Ban, cx));
            }))
            .on_action(cx.listener(|shell, _: &TrackerToggle, _, cx| {
                shell
                    .detail
                    .update(cx, |panel, cx| panel.toggle_tracker(cx));
            }))
            .on_action(cx.listener(|shell, _: &TrackerRemove, _, cx| {
                shell
                    .detail
                    .update(cx, |panel, cx| panel.remove_tracker(cx));
            }))
            .on_action(cx.listener(|shell, _: &SaveSession, _, cx| {
                shell.model.update(cx, |model, cx| model.save_session(cx));
            }))
            .on_action(cx.listener(|shell, _: &ShutdownDaemon, _, cx| {
                shell.model.update(cx, |model, cx| model.shutdown(cx));
            }))
            .on_action(cx.listener(|shell, _: &FocusFilter, window, cx| {
                let search = shell.search.clone();
                search.update(cx, |state, cx| state.focus(window, cx));
            }))
            .on_action(cx.listener(|shell, _: &ClearSelection, _, cx| {
                shell
                    .model
                    .update(cx, |model, cx| model.clear_selection(cx));
            }))
            .on_action(cx.listener(|shell, _: &OpenPreferences, window, cx| {
                shell.open_preferences(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &OpenStatistics, window, cx| {
                shell.open_statistics(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &OpenSession, window, cx| {
                shell.open_session(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &ToggleTurtle, _, cx| {
                shell.model.update(cx, |model, cx| model.toggle_turtle(cx));
            }))
            .on_action(cx.listener(|shell, _: &actions::AddFile, window, cx| {
                shell.open_add_torrent(window, cx);
            }))
            .on_action(cx.listener(|shell, _: &actions::AddMagnet, window, cx| {
                shell.open_add_magnet(window, cx);
            }))
            .on_action(
                cx.listener(|shell, _: &actions::CreateTorrent, window, cx| {
                    shell.open_create(window, cx);
                }),
            )
            .on_action(cx.listener(|shell, action: &SetPieceLength, _, cx| {
                let bytes = action.0;
                shell.create_dialog.clone().into_iter().for_each(|dialog| {
                    dialog.update(cx, |dialog, cx| dialog.set_piece_length(bytes, cx))
                });
            }))
            .child(self.title_bar(cx))
            .child(self.toolbar(cx))
            .child(self.body(cx))
            .children(selection_bar)
            .child(self.status_bar(cx))
            // Every overlay is the last child, so it paints over the window.
            .children(self.remove_dialog.clone())
            .children(self.label_dialog.clone())
            .children(self.tag_dialog.clone())
            .children(self.rate_dialog.clone())
            .children(self.location_dialog.clone())
            .children(self.add_torrent_dialog.clone())
            .children(self.add_magnet_dialog.clone())
            .children(self.create_dialog.clone())
            .children(self.statistics_dialog.clone())
            .children(self.preferences_dialog.clone())
            .children(self.session_dialog.clone())
    }
}

/// Open the single native window.
pub fn open_main_window(services: Arc<Services>, settings_store: SettingsStore, cx: &mut App) {
    let bounds = gpui_kit::Bounds::centered(None, gpui_kit::size(px(1060.), px(680.)), cx);
    cx.open_window(
        gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(bounds)),
            window_min_size: Some(gpui_kit::size(px(840.), px(520.))),
            window_decorations: Some(gpui_kit::WindowDecorations::Client),
            ..TitleBar::window_options()
        },
        |window, cx| {
            let shell = cx.new(|cx| Shell::new(services, settings_store, window, cx));
            cx.new(|cx| Root::new(shell, window, cx))
        },
    )
    .expect("the rstorrent GPUI window opens");
}
