use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, ElementExt as _, Icon, Sizable as _, StyledExt as _,
    WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, App, AppContext as _, Bounds, Context, DragMoveEvent,
    Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
};
use syncplay_core::config::SyncplayConfig;

use crate::store::AppStore;

const VIDEO_EXTENSIONS: [&str; 20] = [
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "m2ts", "mts",
    "vob", "rm", "rmvb", "3gp", "ogv", "divx", "asf",
];
const AUDIO_EXTENSIONS: [&str; 11] = [
    "mp3", "flac", "aac", "ogg", "oga", "opus", "wav", "wma", "m4a", "aiff", "ape",
];

#[derive(Clone)]
struct PlaylistItemDrag {
    index: usize,
    label: SharedString,
}

/// The drag preview gpui floats under the cursor while a reorder is active.
struct PlaylistDragGhost {
    label: SharedString,
}

impl Render for PlaylistDragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(320.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().info)
            .bg(cx.theme().popover.opacity(0.94))
            .px_2()
            .py_2()
            .text_sm()
            .child(self.label.clone())
    }
}

pub struct PlaylistPanel {
    store: Entity<AppStore>,
    media_directories: Entity<MediaDirectoriesDialog>,
    trusted_domains: Entity<TrustedDomainsDialog>,
    /// Fallback directory for the next add-file picker; the web client keeps
    /// this in localStorage, in-memory covers the same session habit.
    last_add_directory: Option<String>,
    hovered_row: Option<usize>,
    drag_target: Option<usize>,
    drag_source: Option<usize>,
    /// Row geometry as last prepainted, for drop-target hit testing; only
    /// meaningful for the current frame's item count.
    row_bounds: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    _subscriptions: Vec<Subscription>,
}

impl PlaylistPanel {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let media_directories = cx.new(|cx| MediaDirectoriesDialog::new(store.clone(), window, cx));
        let trusted_domains = cx.new(|cx| TrustedDomainsDialog::new(store.clone(), window, cx));
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        Self {
            store,
            media_directories,
            trusted_domains,
            last_add_directory: None,
            hovered_row: None,
            drag_target: None,
            drag_source: None,
            row_bounds: Rc::new(RefCell::new(Vec::new())),
            _subscriptions: subscriptions,
        }
    }

    fn open_media_directories(&self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog_view = self.media_directories.clone();
        dialog_view.update(cx, |dialog, cx| dialog.prepare(window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Media Directories")
                .w(px(672.))
                .child(dialog_view.clone())
        });
    }

    fn open_trusted_domains(&self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog_view = self.trusted_domains.clone();
        dialog_view.update(cx, |dialog, cx| dialog.prepare(window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Trusted Domains")
                .w(px(672.))
                .child(dialog_view.clone())
        });
    }

    fn update_config(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut SyncplayConfig)) {
        let mut config = self.store.read(cx).config.clone();
        edit(&mut config);
        self.store.read(cx).update_config(config);
    }

    fn handle_add_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (connected, media_directories) = {
            let store = self.store.read(cx);
            (store.is_connected(), media_directories(&store.config))
        };
        if !connected {
            return;
        }
        if media_directories.is_empty() {
            window.push_notification(
                (
                    NotificationType::Warning,
                    "Set media directories in Settings before adding files",
                ),
                cx,
            );
            return;
        }
        let default_directory = self
            .last_add_directory
            .clone()
            .filter(|dir| media_directories.iter().any(|d| is_path_inside(dir, d)))
            .or_else(|| media_directories.first().cloned());

        let mut picker = rfd::AsyncFileDialog::new()
            .add_filter("Video", &VIDEO_EXTENSIONS)
            .add_filter("Audio", &AUDIO_EXTENSIONS)
            .add_filter("All files", &["*"]);
        if let Some(directory) = default_directory {
            picker = picker.set_directory(directory);
        }
        cx.spawn_in(window, async move |this, cx| {
            let Some(file) = picker.pick_file().await else {
                return;
            };
            let path = file.path().to_string_lossy().to_string();
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_add_file(path, window, cx);
            });
        })
        .detach();
    }

    fn finish_add_file(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let media_directories = media_directories(&self.store.read(cx).config);
        if !media_directories.iter().any(|d| is_path_inside(&path, d)) {
            window.push_notification(
                (
                    NotificationType::Error,
                    "Selected file is outside the media directories",
                ),
                cx,
            );
            return;
        }
        self.store
            .read(cx)
            .update_playlist("add", Some(path.clone()), None);
        self.last_add_directory = parent_directory(&path);
    }

    fn play_item(&self, index: usize, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        if !store.is_connected() {
            return;
        }
        store.update_playlist("select", Some(index.to_string()), None);
    }

    fn commit_reorder(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        if !store.is_connected() {
            return;
        }
        let Some(items) = reorder_items(&store.playlist.items, from, to) else {
            return;
        };
        store.update_playlist("reorder", None, Some(items));
    }

    fn on_drag_move(
        &mut self,
        event: &DragMoveEvent<PlaylistItemDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position;
        if !event.bounds.contains(&position) {
            if self.drag_target.is_some() || self.drag_source.is_some() {
                self.drag_target = None;
                self.drag_source = None;
                cx.notify();
            }
            return;
        }
        let source = event.drag(cx).index;
        let rows = self.row_bounds.borrow();
        let mut target = rows.len();
        for (ix, row) in rows.iter().enumerate() {
            if position.y < row.center().y {
                target = ix;
                break;
            }
        }
        drop(rows);
        if self.drag_target != Some(target) || self.drag_source != Some(source) {
            self.drag_target = Some(target);
            self.drag_source = Some(source);
            cx.notify();
        }
    }

    fn on_drop(&mut self, drag: &PlaylistItemDrag, _: &mut Window, cx: &mut Context<Self>) {
        let from = drag.index;
        let to = self.drag_target.take().unwrap_or(from);
        self.drag_source = None;
        self.commit_reorder(from, to, cx);
        cx.notify();
    }

    /// OS file drops ride the same drag channel as internal reorders, with
    /// `ExternalPaths` as the payload.
    fn on_file_drop(
        &mut self,
        paths: &gpui_kit::ExternalPaths,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.read(cx);
        if !store.is_connected() {
            return;
        }
        for path in paths.paths() {
            let path = path.to_string_lossy().trim().to_string();
            if !path.is_empty() {
                store.update_playlist("add", Some(path), None);
            }
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let item_count = store.playlist.items.len();
        let refreshing = store.media_index_refreshing;
        let scan_label = if refreshing {
            "Scanning media directory"
        } else {
            "Scan media directory"
        };
        let scan_tooltip = format!(
            "{scan_label} (Last scan: {})",
            format_last_scan(store.media_index_version)
        );

        h_flex()
            .w_full()
            .justify_between()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        Icon::new(IconName::ListMusic)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Playlist"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("({item_count})")),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("add-playlist-item")
                            .primary()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("Add")
                            .disabled(!connected)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.handle_add_file(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("clear-playlist")
                            .danger()
                            .small()
                            .icon(IconName::Trash)
                            .tooltip("Clear")
                            .disabled(!connected || item_count == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.read(cx).update_playlist("clear", None, None);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("open-trusted-domains")
                            .secondary()
                            .small()
                            .icon(IconName::Shield)
                            .tooltip("Trusted domains")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_trusted_domains(window, cx)
                            })),
                    )
                    .child(
                        Button::new("refresh-media-index")
                            .secondary()
                            .small()
                            .icon(IconName::RefreshCw)
                            .tooltip(scan_tooltip)
                            .disabled(refreshing)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.read(cx).refresh_media_index();
                            })),
                    )
                    .child(
                        Button::new("open-media-directories")
                            .secondary()
                            .small()
                            .icon(IconName::Folder)
                            .tooltip("Media directories")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_media_directories(window, cx)
                            })),
                    ),
            )
    }

    fn render_item(&self, ix: usize, item: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let item_count = store.playlist.items.len();
        let info = store.playlist_availability.get(ix);
        let available = info.map(|info| info.available).unwrap_or(true);
        let resolved_path = info.and_then(|info| info.path.clone());
        let current = is_current_item(store.player.filename.as_deref(), item);
        let sortable = connected && item_count >= 2;
        let dragging = self.drag_source == Some(ix);
        // Dropping next to the source row is a no-op; the web client hides the
        // indicator there too.
        let show_indicator = self.drag_target == Some(ix)
            && self.drag_target != self.drag_source
            && self.drag_target != self.drag_source.map(|source| source + 1);
        let show_end_indicator = ix + 1 == item_count
            && self.drag_target == Some(item_count)
            && self.drag_source != Some(item_count - 1);
        let hovered = self.hovered_row == Some(ix) && !current;

        let tooltip_text: SharedString = resolved_path
            .clone()
            .unwrap_or_else(|| "Unresolved path".to_string())
            .into();

        let mut content = div()
            .id(("playlist-item", ix))
            .h_flex()
            .w_full()
            .gap_2()
            .p_2()
            .rounded(cx.theme().radius)
            .border_1()
            .text_sm()
            .child(
                Icon::new(IconName::Film)
                    .small()
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .when(current, |this| this.text_color(cx.theme().info))
                    .when(!available, |this| this.opacity(0.5)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when(current, |this| {
                        this.font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().info)
                    })
                    .when(!available, |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .child(item.to_string()),
            )
            .when(current, |this| this.child(accent_tag("Playing", cx)))
            .when(!available, |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .px_2()
                        .rounded_full()
                        .bg(cx.theme().muted)
                        .text_color(cx.theme().danger)
                        .child("Unavailable"),
                )
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
            .on_click(
                cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                    let gpui_kit::ClickEvent::Mouse(mouse) = event else {
                        return;
                    };
                    if mouse.down.click_count >= 2 && available && !current {
                        this.play_item(ix, cx);
                    }
                }),
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_row = Some(ix);
                } else if this.hovered_row == Some(ix) {
                    this.hovered_row = None;
                }
                cx.notify();
            }));
        if sortable {
            let drag = PlaylistItemDrag {
                index: ix,
                label: item.to_string().into(),
            };
            content = content.on_drag(drag, move |drag, _, _, cx| {
                cx.new(|_| PlaylistDragGhost {
                    label: drag.label.clone(),
                })
            });
        }

        let content = content
            .map(|this| {
                if current {
                    this.bg(cx.theme().muted)
                        .border_color(cx.theme().info.opacity(0.5))
                } else {
                    this.bg(cx.theme().muted.opacity(0.6))
                        .border_color(cx.theme().transparent)
                }
            })
            .when(dragging, |this| this.opacity(0.35));

        let row_bounds = self.row_bounds.clone();
        div()
            .relative()
            .w_full()
            .on_prepaint(move |bounds, _, _| {
                let mut rows = row_bounds.borrow_mut();
                if ix < rows.len() {
                    rows[ix] = bounds;
                }
            })
            .child(content.into_any_element())
            .when(hovered, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .h_flex()
                        .justify_between()
                        .child(
                            Button::new(("play-item", ix))
                                .secondary()
                                .small()
                                .icon(IconName::Play)
                                .tooltip("Play")
                                .disabled(!connected || !available)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.play_item(ix, cx);
                                })),
                        )
                        .child(
                            Button::new(("remove-item", ix))
                                .secondary()
                                .small()
                                .icon(IconName::Trash)
                                .tooltip("Remove")
                                .text_color(cx.theme().danger)
                                .disabled(!connected)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.store.read(cx).update_playlist(
                                        "remove",
                                        Some(ix.to_string()),
                                        None,
                                    );
                                })),
                        ),
                )
            })
            .when(show_indicator, |this| {
                this.child(drop_indicator(cx).top(px(-3.)).into_any_element())
            })
            .when(show_end_indicator, |this| {
                this.child(drop_indicator(cx).bottom(px(-3.)).into_any_element())
            })
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let item_count = store.playlist.items.len();
        let current_index = store.playlist.current_index;
        let shared_enabled = store.config.user.shared_playlist_enabled;
        let loop_playlist = store.config.user.loop_at_end_of_playlist;
        let loop_single = store.config.user.loop_single_files;

        let previous_disabled =
            !connected || item_count == 0 || current_index.is_none() || current_index == Some(0);
        let next_disabled =
            !connected || item_count == 0 || current_index.is_none_or(|ix| ix + 1 >= item_count);

        h_flex()
            .w_full()
            .justify_between()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("playlist-previous")
                            .secondary()
                            .small()
                            .icon(IconName::ChevronLeft)
                            .tooltip("Previous")
                            .disabled(previous_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.read(cx).update_playlist("previous", None, None);
                            })),
                    )
                    .child(
                        Button::new("playlist-next")
                            .secondary()
                            .small()
                            .icon(IconName::ChevronRight)
                            .tooltip("Next")
                            .disabled(next_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.store.read(cx).update_playlist("next", None, None);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("toggle-shared-playlist")
                            .ghost()
                            .small()
                            .icon(IconName::Users)
                            .tooltip(if shared_enabled {
                                "Shared playlists on"
                            } else {
                                "Shared playlists off"
                            })
                            .when(shared_enabled, |this| this.text_color(cx.theme().info))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_config(cx, |config| {
                                    config.user.shared_playlist_enabled = !shared_enabled;
                                });
                            })),
                    )
                    .child(
                        Button::new("toggle-loop-playlist")
                            .ghost()
                            .small()
                            .icon(IconName::Repeat)
                            .tooltip(if loop_playlist {
                                "Loop playlist on"
                            } else {
                                "Loop playlist off"
                            })
                            .when(loop_playlist, |this| this.text_color(cx.theme().info))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_config(cx, |config| {
                                    config.user.loop_at_end_of_playlist = !loop_playlist;
                                });
                            })),
                    )
                    .child(
                        Button::new("toggle-loop-single")
                            .ghost()
                            .small()
                            .icon(IconName::Repeat1)
                            .tooltip(if loop_single {
                                "Loop file on"
                            } else {
                                "Loop file off"
                            })
                            .when(loop_single, |this| this.text_color(cx.theme().info))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_config(cx, |config| {
                                    config.user.loop_single_files = !loop_single;
                                });
                            })),
                    ),
            )
    }
}

impl Render for PlaylistPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.store.read(cx).playlist.items.clone();
        self.row_bounds
            .borrow_mut()
            .resize(items.len(), Bounds::default());

        let list = div()
            .id("playlist-items")
            .v_flex()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_2()
            .on_drag_move(cx.listener(Self::on_drag_move))
            .on_drop(cx.listener(Self::on_drop))
            .on_drop(cx.listener(Self::on_file_drop))
            .overflow_y_scrollbar();

        v_flex()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .p_4()
                    .child(self.render_header(cx)),
            )
            .child(
                div().flex_1().min_h_0().child(
                    list.when(items.is_empty(), |this| {
                        this.child(
                            v_flex()
                                .size_full()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .p_4()
                                .child(
                                    Icon::new(IconName::ListMusic)
                                        .small()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("No items in playlist"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .text_center()
                                        .child(
                                            "Drag files here or click + to add. Double-click an item to play it.",
                                        ),
                                ),
                        )
                    })
                    .children(
                        items
                            .iter()
                            .enumerate()
                            .map(|(ix, item)| self.render_item(ix, item, cx).into_any_element()),
                    ),
                ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .p_4()
                    .child(self.render_footer(cx)),
            )
    }
}

fn drop_indicator(cx: &App) -> gpui_kit::Div {
    div()
        .absolute()
        .left_0()
        .w_full()
        .h(px(2.))
        .bg(cx.theme().info)
}

fn accent_tag(label: &'static str, cx: &App) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .text_xs()
        .px_2()
        .rounded_full()
        .border_1()
        .border_color(cx.theme().info)
        .bg(cx.theme().info.opacity(0.15))
        .child(label)
}

fn reorder_items(items: &[String], from: usize, to: usize) -> Option<Vec<String>> {
    if from >= items.len() || to > items.len() || from == to {
        return None;
    }
    let mut next = items.to_vec();
    let moved = next.remove(from);
    let insert = if from < to { to - 1 } else { to }.min(next.len());
    next.insert(insert, moved);
    if next == items {
        None
    } else {
        Some(next)
    }
}

/// Current-row matching is basename-only and case-insensitive, the same as
/// the web client.
fn is_current_item(player_filename: Option<&str>, item: &str) -> bool {
    let current = normalize_filename(player_filename.unwrap_or(""));
    !current.is_empty() && current == normalize_filename(item)
}

fn normalize_filename(value: &str) -> String {
    value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(value)
        .trim()
        .to_lowercase()
}

fn media_directories(config: &SyncplayConfig) -> Vec<String> {
    config
        .player
        .media_directories
        .iter()
        .map(|dir| dir.trim().to_string())
        .filter(|dir| !dir.is_empty())
        .collect()
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn is_path_inside(path: &str, directory: &str) -> bool {
    let path = normalize_path(path);
    let directory = normalize_path(directory);
    path == directory || path.starts_with(&format!("{directory}/"))
}

fn parent_directory(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let ix = normalized.rfind('/')?;
    (ix > 0).then(|| path[..ix].to_string())
}

fn format_last_scan(timestamp_ms: i64) -> String {
    if timestamp_ms <= 0 {
        return "Never".to_string();
    }
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "Never".to_string())
}

// ── Media directories dialog ────────────────────────────────────────────────

pub struct MediaDirectoriesDialog {
    store: Entity<AppStore>,
    timeout_input: Entity<InputState>,
    directory_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl MediaDirectoriesDialog {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let timeout_input = cx.new(|cx| InputState::new(window, cx));
        let directory_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/media"));

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));
        subscriptions.push(cx.subscribe_in(
            &timeout_input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.save_timeout(window, cx),
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &directory_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_directory(window, cx);
                }
            },
        ));

        Self {
            store,
            timeout_input,
            directory_input,
            _subscriptions: subscriptions,
        }
    }

    /// Reload the form from current config; runs on every dialog open.
    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let timeout = self
            .store
            .read(cx)
            .config
            .player
            .media_index_timeout_seconds;
        self.timeout_input.update(cx, |input, cx| {
            input.set_value(timeout.to_string(), window, cx);
        });
        self.directory_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    fn update_config(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut SyncplayConfig)) {
        let mut config = self.store.read(cx).config.clone();
        edit(&mut config);
        self.store.read(cx).update_config(config);
    }

    fn save_timeout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .store
            .read(cx)
            .config
            .player
            .media_index_timeout_seconds;
        let value = self.timeout_input.read(cx).value().to_string();
        let parsed = value.parse::<u64>().ok().filter(|v| *v >= 1);
        let Some(timeout) = parsed else {
            window.push_notification(
                (
                    NotificationType::Warning,
                    "Media scan timeout must be a positive whole number",
                ),
                cx,
            );
            self.timeout_input.update(cx, |input, cx| {
                input.set_value(current.to_string(), window, cx);
            });
            return;
        };
        if timeout != current {
            self.update_config(cx, |config| {
                config.player.media_index_timeout_seconds = timeout;
            });
        }
    }

    fn add_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let directory = self.directory_input.read(cx).value().trim().to_string();
        if directory.is_empty() {
            return;
        }
        if self
            .store
            .read(cx)
            .config
            .player
            .media_directories
            .contains(&directory)
        {
            window.push_notification(
                (NotificationType::Warning, "Media directory already exists"),
                cx,
            );
            return;
        }
        self.update_config(cx, |config| {
            config.player.media_directories.push(directory);
        });
        self.directory_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    fn browse_directory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let picker = rfd::AsyncFileDialog::new();
            let Some(folder) = picker.pick_folder().await else {
                return;
            };
            let path = folder.path().to_string_lossy().to_string();
            let _ = this.update_in(cx, |this, window, cx| {
                this.add_picked_directory(path, window, cx);
            });
        })
        .detach();
    }

    fn add_picked_directory(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .store
            .read(cx)
            .config
            .player
            .media_directories
            .contains(&path)
        {
            window.push_notification(
                (NotificationType::Warning, "Media directory already exists"),
                cx,
            );
            return;
        }
        self.update_config(cx, |config| {
            config.player.media_directories.push(path);
        });
    }

    fn move_directory(&self, ix: usize, up: bool, cx: &mut Context<Self>) {
        self.update_config(cx, |config| {
            let directories = &mut config.player.media_directories;
            let target = if up {
                ix.checked_sub(1)
            } else {
                (ix + 1 < directories.len()).then_some(ix + 1)
            };
            if let Some(target) = target {
                directories.swap(ix, target);
            }
        });
    }
}

impl Render for MediaDirectoriesDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let directories = store.config.player.media_directories.clone();
        let directory_count = directories.len();

        v_flex()
            .gap_4()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Manage directories used for playlist matching."),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Scan timeout"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w_32().child(Input::new(&self.timeout_input)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Seconds allowed for media directory scanning."),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Add directory"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.directory_input)))
                            .child(
                                Button::new("add-media-directory")
                                    .primary()
                                    .label("Add")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_directory(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("browse-media-directory")
                                    .secondary()
                                    .label("Browse")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse_directory(window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Current directories"),
                    )
                    .when(directories.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No media directories added."),
                        )
                    })
                    .children(directories.iter().enumerate().map(|(ix, directory)| {
                        h_flex()
                            .justify_between()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_sm()
                                    .child(directory.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .flex_shrink_0()
                                    .child(
                                        Button::new(("move-directory-up", ix))
                                            .secondary()
                                            .xsmall()
                                            .icon(IconName::ChevronUp)
                                            .tooltip("Move up")
                                            .disabled(ix == 0)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.move_directory(ix, true, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(("move-directory-down", ix))
                                            .secondary()
                                            .xsmall()
                                            .icon(IconName::ChevronDown)
                                            .tooltip("Move down")
                                            .disabled(ix + 1 >= directory_count)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.move_directory(ix, false, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(("remove-directory", ix))
                                            .ghost()
                                            .xsmall()
                                            .label("Remove")
                                            .text_color(cx.theme().danger)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.update_config(cx, |config| {
                                                    config.player.media_directories.remove(ix);
                                                });
                                            })),
                                    ),
                            )
                            .into_any_element()
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Files are matched locally against these directories."),
                    ),
            )
    }
}

// ── Trusted domains dialog ──────────────────────────────────────────────────

pub struct TrustedDomainsDialog {
    store: Entity<AppStore>,
    domain_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl TrustedDomainsDialog {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let domain_input = cx.new(|cx| InputState::new(window, cx).placeholder("youtube.com"));
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));
        subscriptions.push(cx.subscribe_in(
            &domain_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_domain(window, cx);
                }
            },
        ));
        Self {
            store,
            domain_input,
            _subscriptions: subscriptions,
        }
    }

    /// Reset the entry field; runs on every dialog open.
    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.domain_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    fn update_config(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut SyncplayConfig)) {
        let mut config = self.store.read(cx).config.clone();
        edit(&mut config);
        self.store.read(cx).update_config(config);
    }

    fn add_domain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let domain = self.domain_input.read(cx).value().trim().to_string();
        if domain.is_empty() {
            return;
        }
        if self
            .store
            .read(cx)
            .config
            .user
            .trusted_domains
            .contains(&domain)
        {
            window.push_notification(
                (NotificationType::Warning, "Trusted domain already exists"),
                cx,
            );
            self.domain_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
            return;
        }
        self.update_config(cx, |config| {
            config.user.trusted_domains.push(domain);
        });
        self.domain_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }
}

impl Render for TrustedDomainsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let only_trusted = store.config.user.only_switch_to_trusted_domains;
        let domains = store.config.user.trusted_domains.clone();

        v_flex()
            .gap_4()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Control which URLs can be opened."),
            )
            .child(
                Checkbox::new("only-switch-trusted")
                    .label("Only switch to trusted domains")
                    .checked(only_trusted)
                    .on_change(cx.listener(move |this, checked, _, cx| {
                        let checked = *checked;
                        this.update_config(cx, |config| {
                            config.user.only_switch_to_trusted_domains = checked;
                        });
                    })),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Trusted domains"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.domain_input)))
                            .child(
                                Button::new("add-trusted-domain")
                                    .primary()
                                    .label("Add")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_domain(window, cx)
                                    })),
                            ),
                    )
                    .when(domains.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("No trusted domains added."),
                        )
                    })
                    .children(domains.iter().enumerate().map(|(ix, domain)| {
                        h_flex()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_sm()
                                    .child(domain.clone()),
                            )
                            .child(
                                Button::new(("remove-domain", ix))
                                    .ghost()
                                    .xsmall()
                                    .label("Remove")
                                    .text_color(cx.theme().danger)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.update_config(cx, |config| {
                                            config.user.trusted_domains.remove(ix);
                                        });
                                    })),
                            )
                            .into_any_element()
                    })),
            )
    }
}
