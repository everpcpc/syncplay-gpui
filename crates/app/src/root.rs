use crate::chat::ChatPanel;
use crate::connection::ConnectionDialog;
use crate::playlist::PlaylistPanel;
use crate::store::{AppStore, ConnectParams};
use crate::users::UserListPanel;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable, ResizableState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, TitleBar, WindowExt as _};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, relative, AnyElement, AppContext as _, Context, Entity,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, Styled as _, Subscription, Task,
    Window,
};

pub struct RootView {
    store: Entity<AppStore>,
    chat: Entity<ChatPanel>,
    user_list: Entity<UserListPanel>,
    playlist: Entity<PlaylistPanel>,
    connection_dialog: Entity<ConnectionDialog>,
    main_split: Entity<ResizableState>,
    side_split: Entity<ResizableState>,
    layout_save_task: Task<()>,
    _window_size_watch: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl RootView {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let chat = cx.new(|cx| ChatPanel::new(store.clone(), window, cx));
        let user_list = cx.new(|cx| UserListPanel::new(store.clone(), window, cx));
        let playlist = cx.new(|cx| PlaylistPanel::new(store.clone(), window, cx));
        let connection_dialog = cx.new(|cx| ConnectionDialog::new(store.clone(), window, cx));
        let main_split = cx.new(|_| ResizableState::default());
        let side_split = cx.new(|_| ResizableState::default());
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];

        let mut view = Self {
            store,
            chat,
            user_list,
            playlist,
            connection_dialog,
            main_split,
            side_split,
            layout_save_task: Task::ready(()),
            _window_size_watch: Task::ready(()),
            _subscriptions: subscriptions,
        };
        view.watch_window_size(window, cx);
        // Defer so the window's Root (dialogs, notifications) exists by the
        // time the startup logic may open the connection dialog.
        cx.defer_in(window, |this, window, cx| this.startup(window, cx));
        view
    }

    fn startup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.store.read(cx).config.clone();
        if config.user.force_gui_prompt {
            self.open_connection_dialog(window, cx);
        } else if config.user.auto_connect
            && !config.user.username.trim().is_empty()
            && !self.store.read(cx).is_connected()
        {
            self.store.update(cx, |store, cx| {
                store.connect(
                    ConnectParams {
                        host: config.server.host.clone(),
                        port: config.server.port,
                        username: config.user.username.clone(),
                        room: config.user.default_room.clone(),
                        password: config.server.password.clone(),
                    },
                    None,
                    cx,
                );
            });
        }
    }

    fn open_connection_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog_view = self.connection_dialog.clone();
        dialog_view.update(cx, |dialog, cx| dialog.prepare(window, cx));
        window.open_dialog(cx, move |dialog, _, cx| {
            let connected = dialog_view.read(cx).store().read(cx).is_connected();
            dialog
                .title(if connected {
                    "Connected"
                } else {
                    "Connect to Server"
                })
                .w(px(560.))
                .child(dialog_view.clone())
        });
    }

    fn toggle_theme(&self, cx: &mut Context<Self>) {
        let mut config = self.store.read(cx).config.clone();
        config.user.theme = if config.user.theme == "light" {
            "dark".to_string()
        } else {
            "light".to_string()
        };
        self.store.read(cx).update_config(config);
    }

    fn toggle_playlist(&self, cx: &mut Context<Self>) {
        let mut config = self.store.read(cx).config.clone();
        config.user.show_playlist = !config.user.show_playlist;
        self.store.read(cx).update_config(config);
    }

    /// gpui offers no window-resize event, so poll the bounds once a second
    /// and run the same debounced persist as a panel drag.
    fn watch_window_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._window_size_watch = cx.spawn_in(window, async move |this, cx| {
            let mut last_size = None;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let Ok(changed) = this.update_in(cx, |_, window, _| {
                    let size = window.bounds().size;
                    if last_size == Some(size) {
                        false
                    } else {
                        last_size = Some(size);
                        true
                    }
                }) else {
                    break;
                };
                if changed {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.queue_layout_save(window, cx);
                    });
                }
            }
        });
    }

    fn queue_layout_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout_save_task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(400))
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.persist_layout(window, cx);
            });
        });
    }

    fn persist_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let mut config = store.config.clone();
        let mut changed = false;

        if let Some(width) = self.main_split.read(cx).sizes().get(1) {
            let width = f32::from(*width).round() as u32;
            if width > 0 && config.user.side_column_width != Some(width) {
                config.user.side_column_width = Some(width);
                changed = true;
            }
        }
        // With the playlist hidden the split collapses to one panel and its
        // sizes no longer describe the primary/secondary division.
        if config.user.show_playlist {
            if let Some(primary) = self.side_split.read(cx).sizes().first() {
                let primary = f32::from(*primary).round() as u32;
                if primary > 0 && config.user.side_panel_primary_size != Some(primary) {
                    config.user.side_panel_primary_size = Some(primary);
                    changed = true;
                }
            }
        }

        let window_size = window.bounds().size;
        let (width, height) = (
            f32::from(window_size.width).round() as u32,
            f32::from(window_size.height).round() as u32,
        );
        if width > 0 && config.user.window_width != Some(width) {
            config.user.window_width = Some(width);
            changed = true;
        }
        if height > 0 && config.user.window_height != Some(height) {
            config.user.window_height = Some(height);
            changed = true;
        }

        if changed {
            store.update_config(config);
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let show_tls = connected && store.tls_status == "enabled";
        let rtt = store.rtt_ms.filter(|_| connected);
        let show_playlist = store.config.user.show_playlist;
        let light_theme = store.config.user.theme == "light";

        let theme_icon = if light_theme {
            IconName::Sun
        } else {
            IconName::Moon
        };
        let playlist_icon = if show_playlist {
            IconName::ListMusic
        } else {
            IconName::ListMinus
        };

        h_flex()
            .w_full()
            .px_4()
            .justify_between()
            .child(self.render_player_status(cx))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("toggle-playlist")
                            .ghost()
                            .small()
                            .icon(playlist_icon)
                            .tooltip(if show_playlist {
                                "Playlist shown"
                            } else {
                                "Playlist hidden"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_playlist(cx))),
                    )
                    .child(
                        Button::new("toggle-theme")
                            .ghost()
                            .small()
                            .icon(theme_icon)
                            .tooltip(if light_theme {
                                "Theme light"
                            } else {
                                "Theme dark"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx))),
                    )
                    .when_some(rtt, |this, rtt| {
                        this.child(
                            h_flex()
                                .gap_1()
                                .px_2()
                                .rounded_full()
                                .bg(cx.theme().muted)
                                .child(
                                    Icon::new(IconName::Zap)
                                        .small()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .child(format!("{}ms", rtt.max(0.).round() as i64)),
                                ),
                        )
                    })
                    .when(show_tls, |this| {
                        this.child(
                            div()
                                .p_1()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().muted)
                                .child(
                                    Icon::new(IconName::Lock)
                                        .small()
                                        .text_color(cx.theme().muted_foreground),
                                ),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(concat!("v", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        Button::new("open-connection")
                            .ghost()
                            .small()
                            .icon(IconName::Link2)
                            .tooltip("Connect")
                            .when(connected, |button| button.text_color(cx.theme().info))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_connection_dialog(window, cx)
                            })),
                    )
                    .child(
                        Button::new("open-settings")
                            .ghost()
                            .small()
                            .icon(IconName::Settings)
                            .tooltip("Settings")
                            .on_click(cx.listener(|this, _, window, cx| {
                                crate::settings::open_settings_dialog(
                                    window,
                                    cx,
                                    this.store.clone(),
                                )
                            })),
                    ),
            )
    }

    fn render_player_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let store = self.store.read(cx);
        if !store.is_connected() {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Not connected")
                .into_any_element();
        }

        let player = &store.player;
        let paused = player.paused.unwrap_or(true);
        let filename = player.filename.clone();
        let position = player.position;
        let duration = player.duration;
        let speed = player.speed;
        let offset = store.sync_offset_seconds.filter(|_| filename.is_some());

        let icon_bg = if paused {
            cx.theme().muted
        } else {
            cx.theme().info.opacity(0.15)
        };
        let icon_color = if paused {
            cx.theme().warning
        } else {
            cx.theme().info
        };

        h_flex()
            .gap_3()
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .flex_shrink_0()
                    .size_7()
                    .rounded(cx.theme().radius)
                    .bg(icon_bg)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(if paused {
                            IconName::Pause
                        } else {
                            IconName::Play
                        })
                        .small()
                        .text_color(icon_color),
                    ),
            )
            .when(position.is_some() && duration.is_some(), |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .child(format_time(position)),
                                )
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("/ {}", format_time(duration))),
                                ),
                        ),
                )
            })
            .child(
                div()
                    .min_w_0()
                    .max_w(px(280.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .child(display_filename(filename.as_deref())),
            )
            .when_some(offset, |this, offset| {
                let absolute = offset.abs();
                let in_sync = absolute < 1.0;
                let (label, color, bg) = if in_sync {
                    (
                        "in sync".to_string(),
                        cx.theme().success,
                        cx.theme().success.opacity(0.15),
                    )
                } else {
                    let label = if absolute >= 10.0 {
                        format!("{absolute:.0}")
                    } else {
                        format!("{absolute:.1}")
                    };
                    let direction = if offset < 0.0 { "behind" } else { "ahead" };
                    (
                        format!("{direction} {label}s"),
                        cx.theme().warning,
                        cx.theme().warning.opacity(0.15),
                    )
                };
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .px_2()
                        .rounded_full()
                        .text_color(color)
                        .bg(bg)
                        .child(label),
                )
            })
            .when_some(speed.filter(|speed| *speed != 1.0), |this, speed| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child(format!("{speed:.2}x")),
                )
            })
            .into_any_element()
    }

    fn render_progress(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let store = self.store.read(cx);
        let fraction = match (store.player.position, store.player.duration) {
            (Some(position), Some(duration)) if duration > 0.0 => {
                (position / duration).clamp(0.0, 1.0) as f32
            }
            _ => return None,
        };
        Some(
            div()
                .h(px(2.))
                .w_full()
                .child(div().h_full().w(relative(fraction)).bg(cx.theme().info)),
        )
    }

    fn render_side_column(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let show_playlist = store.config.user.show_playlist;
        let saved_primary = store.config.user.side_panel_primary_size;

        let root = cx.entity();
        v_resizable("side-split")
            .with_state(&self.side_split)
            .on_resize(move |_, window, cx| {
                root.update(cx, |this, cx| this.queue_layout_save(window, cx));
            })
            .child(
                resizable_panel()
                    .when_some(saved_primary, |panel, primary| {
                        panel.size(px(primary as f32))
                    })
                    .size_range(px(200.)..Pixels::MAX)
                    .child(self.user_list.clone()),
            )
            .when(show_playlist, |this| {
                this.child(
                    resizable_panel()
                        .size_range(px(200.)..Pixels::MAX)
                        .child(self.playlist.clone()),
                )
            })
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sidebar default: min(560, max(320, 36%)) of the window width, per
        // the web client's layout constants; a persisted width wins, clamped
        // so the chat panel keeps its 360px minimum.
        let viewport_width = window.viewport_size().width;
        let saved_width = self
            .store
            .read(cx)
            .config
            .user
            .side_column_width
            .map(|width| px(width as f32));
        let side_default = saved_width
            .unwrap_or_else(|| (viewport_width * 0.36).max(px(320.)).min(px(560.)))
            .max(px(320.))
            .min((viewport_width - px(360.)).max(px(320.)));

        // While the playlist was hidden the users panel owned the whole
        // column; when it comes back the state still holds that full height,
        // so re-pin the primary panel to the persisted size (or an even
        // split). The group only re-creates the second panel when it paints,
        // which can take a frame or two, so the restore retries.
        let saved_primary = self.store.read(cx).config.user.side_panel_primary_size;
        if self.store.read(cx).config.user.show_playlist
            && self.side_split.read(cx).sizes().len() < 2
        {
            restore_primary_later(self.side_split.clone(), saved_primary, 4, window);
        }

        let root = cx.entity();
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(TitleBar::new().child(self.render_header(cx)))
            .children(self.render_progress(cx))
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("main-split")
                        .with_state(&self.main_split)
                        .on_resize(move |_, window, cx| {
                            root.update(cx, |this, cx| this.queue_layout_save(window, cx));
                        })
                        .child(
                            resizable_panel()
                                .size_range(px(360.)..Pixels::MAX)
                                .child(self.chat.clone()),
                        )
                        .child(
                            resizable_panel()
                                .size(side_default)
                                .size_range(px(320.)..Pixels::MAX)
                                .flex_none()
                                .child(
                                    div()
                                        .size_full()
                                        .border_l_1()
                                        .border_color(cx.theme().border)
                                        .child(self.render_side_column(cx)),
                                ),
                        ),
                ),
            )
    }
}

fn format_time(seconds: Option<f64>) -> SharedString {
    let Some(seconds) = seconds.filter(|seconds| seconds.is_finite() && *seconds >= 0.0) else {
        return "--:--".into();
    };
    let total = seconds.floor() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}").into()
    } else {
        format!("{minutes:02}:{secs:02}").into()
    }
}

fn restore_primary_later(
    split: Entity<ResizableState>,
    saved_primary: Option<u32>,
    attempts_left: u32,
    window: &mut Window,
) {
    window.on_next_frame(move |window, cx| {
        let resized = split.update(cx, |state, cx| {
            if state.sizes().len() < 2 || state.container_size() <= px(1.) {
                return false;
            }
            // Drive the secondary panel to what remains after the primary:
            // while the playlist was hidden the primary's size overshot the
            // container, and resizing the primary directly would keep that
            // excess and let the overflow clamp eat the target.
            let target = saved_primary
                .map(|primary| px(primary as f32))
                .unwrap_or_else(|| state.container_size() * 0.5);
            let secondary = (state.container_size() - target).max(px(200.));
            let last = state.sizes().len() - 1;
            state.resize_panel(last, secondary, window, cx);
            true
        });
        if !resized && attempts_left > 1 {
            restore_primary_later(split, saved_primary, attempts_left - 1, window);
        }
    });
}

fn display_filename(filename: Option<&str>) -> SharedString {
    let Some(filename) = filename else {
        return "No file loaded".into();
    };
    filename
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(filename)
        .into()
}
