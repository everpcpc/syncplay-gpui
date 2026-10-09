use crate::chat::ChatPanel;
use crate::connection::ConnectionDialog;
use crate::playlist::PlaylistPanel;
use crate::store::{AppStore, ConnectParams, UpdateState};
use crate::users::UserListPanel;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable, ResizableState};
use gpui_kit::component::status_bar::StatusBar;
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
        if config.user.check_for_updates_automatically != Some(false) {
            self.store
                .update(cx, |store, cx| store.check_for_updates(false, cx));
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

        if let Some(width) = self.main_split.read(cx).sizes().first() {
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

    fn render_titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let show_playlist = store.config.user.show_playlist;
        let light_theme = store.config.user.theme == "light";

        let theme_icon = if light_theme {
            IconName::Sun
        } else {
            IconName::Moon
        };
        let playlist_icon = if show_playlist {
            IconName::PanelLeftClose
        } else {
            IconName::PanelLeftOpen
        };

        let connect_button = if connected {
            Button::new("open-connection")
                .ghost()
                .small()
                .icon(IconName::Link2)
                .tooltip("Connected")
                .text_color(cx.theme().info)
        } else {
            Button::new("open-connection")
                .primary()
                .small()
                .icon(IconName::Link2)
                .label("Connect")
        };

        h_flex()
            .w_full()
            .px_4()
            .justify_between()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .size_6()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().info.opacity(0.15))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::MonitorPlay)
                                    .small()
                                    .text_color(cx.theme().info),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Syncplay"),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("toggle-playlist")
                            .ghost()
                            .small()
                            .icon(playlist_icon)
                            .tooltip(if show_playlist {
                                "Hide playlist"
                            } else {
                                "Show playlist"
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
                    .child(div().w(px(1.)).h_4().bg(cx.theme().border))
                    .child(connect_button.on_click(
                        cx.listener(|this, _, window, cx| this.open_connection_dialog(window, cx)),
                    ))
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

    fn render_playback_header(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let store = self.store.read(cx);
        if !store.is_connected() {
            return None;
        }

        let player = &store.player;
        let paused = player.paused.unwrap_or(true);
        let filename = player.filename.clone();
        let has_media = filename.is_some();
        let position = player.position;
        let duration = player.duration;
        let speed = player.speed;
        let offset = store.sync_offset_seconds.filter(|_| has_media);
        let progress = match (position, duration) {
            (Some(position), Some(duration)) if duration > 0.0 => {
                (position / duration).clamp(0.0, 1.0) as f32
            }
            _ => 0.0,
        };

        // Without a local file the room's global playstate can still leak a
        // position/paused projection into the store; the header treats that
        // as idle instead of showing phantom playback.
        let (icon_bg, icon_color, state_icon) = if !has_media {
            (
                cx.theme().muted,
                cx.theme().muted_foreground,
                IconName::Play,
            )
        } else if paused {
            (cx.theme().muted, cx.theme().warning, IconName::Pause)
        } else {
            (
                cx.theme().info.opacity(0.15),
                cx.theme().info,
                IconName::Play,
            )
        };

        Some(
            v_flex()
                .w_full()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .w_full()
                        .px_6()
                        .pt_4()
                        .gap_4()
                        .child(
                            div()
                                .flex_shrink_0()
                                .size_11()
                                .rounded(cx.theme().radius_lg)
                                .bg(icon_bg)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(state_icon).large().text_color(icon_color)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_base()
                                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                .when(!has_media, |this| {
                                    this.text_color(cx.theme().muted_foreground)
                                })
                                .child(display_filename(filename.as_deref())),
                        )
                        .child(
                            h_flex()
                                .flex_shrink_0()
                                .gap_2()
                                .items_center()
                                .when_some(offset, |this, offset| {
                                    let absolute = offset.abs();
                                    let in_sync = absolute < 1.0;
                                    let (label, color) = if in_sync {
                                        ("in sync".to_string(), cx.theme().success)
                                    } else {
                                        let label = if absolute >= 10.0 {
                                            format!("{absolute:.0}")
                                        } else {
                                            format!("{absolute:.1}")
                                        };
                                        let direction =
                                            if offset < 0.0 { "behind" } else { "ahead" };
                                        (format!("{direction} {label}s"), cx.theme().warning)
                                    };
                                    this.child(
                                        div()
                                            .text_xs()
                                            .px_2()
                                            .rounded_full()
                                            .text_color(color)
                                            .bg(color.opacity(0.15))
                                            .child(label),
                                    )
                                })
                                .when_some(speed.filter(|speed| *speed != 1.0), |this, speed| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().warning)
                                            .child(format!("{speed:.2}x")),
                                    )
                                })
                                .child(
                                    div()
                                        .text_sm()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().muted_foreground)
                                        .child(if has_media {
                                            format!(
                                                "{} / {}",
                                                format_time(position),
                                                format_time(duration)
                                            )
                                        } else {
                                            "--:-- / --:--".to_string()
                                        }),
                                ),
                        ),
                )
                .child(
                    div().px_6().pt_3().pb_4().child(
                        div()
                            .h(px(6.))
                            .w_full()
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .when(has_media, |this| {
                                this.child(
                                    div()
                                        .h_full()
                                        .rounded_full()
                                        .w(relative(progress))
                                        .bg(cx.theme().progress_bar),
                                )
                            }),
                    ),
                ),
        )
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .size_16()
                    .rounded(cx.theme().radius_lg)
                    .bg(cx.theme().info.opacity(0.12))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::MonitorPlay)
                            .with_size(px(32.))
                            .text_color(cx.theme().info),
                    ),
            )
            .child(
                div()
                    .pt_2()
                    .text_lg()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Welcome to Syncplay"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Connect to a server to watch together."),
            )
            .child(
                div().pt_3().child(
                    Button::new("welcome-connect")
                        .primary()
                        .icon(IconName::Link2)
                        .label("Connect to Server")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_connection_dialog(window, cx)
                        })),
                ),
            )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let server = store.connection.server.clone();
        let rtt = store.rtt_ms.filter(|_| connected);
        let show_tls = connected && store.tls_status == "enabled";
        let indexing = store.media_index_refreshing;

        let status_text: SharedString = match (connected, server) {
            (true, Some(server)) => format!("Connected to {server}").into(),
            (true, None) => "Connected".into(),
            (false, _) => "Not connected".into(),
        };

        StatusBar::new()
            .left(
                h_flex()
                    .gap_2()
                    .child(div().size_2().rounded_full().bg(if connected {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(status_text),
                    )
                    .when(indexing, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("· Indexing media…"),
                        )
                    }),
            )
            .right(
                h_flex()
                    .gap_3()
                    .when_some(rtt, |this, rtt| {
                        this.child(
                            h_flex()
                                .gap_1()
                                .child(
                                    Icon::new(IconName::Wifi)
                                        .xsmall()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{}ms", rtt.max(0.).round() as i64)),
                                ),
                        )
                    })
                    .when(show_tls, |this| {
                        this.child(
                            h_flex()
                                .gap_1()
                                .child(
                                    Icon::new(IconName::Lock)
                                        .xsmall()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("TLS"),
                                ),
                        )
                    })
                    .child(self.render_update_status(cx)),
            )
    }

    fn render_update_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.store.read(cx).update_state.clone();
        let muted = cx.theme().muted_foreground;
        match state {
            UpdateState::Idle => Button::new("check-updates")
                .ghost()
                .xsmall()
                .label(concat!("v", env!("CARGO_PKG_VERSION")))
                .tooltip("Check for updates")
                .text_color(muted)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.store
                        .update(cx, |store, cx| store.check_for_updates(true, cx));
                }))
                .into_any_element(),
            UpdateState::Checking => div()
                .text_xs()
                .text_color(muted)
                .child("Checking for updates…")
                .into_any_element(),
            UpdateState::Available(version) => Button::new("install-update")
                .ghost()
                .xsmall()
                .icon(IconName::Download)
                .label(format!("v{version}"))
                .tooltip("Download and install")
                .text_color(cx.theme().info)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.store.update(cx, |store, cx| store.install_update(cx));
                }))
                .into_any_element(),
            UpdateState::Installing(_) => div()
                .text_xs()
                .text_color(muted)
                .child("Updating…")
                .into_any_element(),
            UpdateState::Ready(version) => Button::new("restart-for-update")
                .primary()
                .xsmall()
                .icon(IconName::RotateCw)
                .label("Restart")
                .tooltip(format!("Restart to finish v{version}"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.store.read(cx).restart_for_update();
                }))
                .into_any_element(),
        }
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
        // Sidebar default 280px; a persisted width wins, clamped so the main
        // column keeps its 420px minimum.
        let viewport_width = window.viewport_size().width;
        let saved_width = self
            .store
            .read(cx)
            .config
            .user
            .side_column_width
            .map(|width| px(width as f32));
        let side_default = saved_width
            .unwrap_or(px(280.))
            .max(px(240.))
            .min(px(480.))
            .min((viewport_width - px(420.)).max(px(240.)));

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

        let connected = self.store.read(cx).is_connected();

        let root = cx.entity();
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(TitleBar::new().child(self.render_titlebar(cx)))
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("main-split")
                        .with_state(&self.main_split)
                        .on_resize(move |_, window, cx| {
                            root.update(cx, |this, cx| this.queue_layout_save(window, cx));
                        })
                        .child(
                            resizable_panel()
                                .size(side_default)
                                .size_range(px(240.)..px(480.))
                                .flex_none()
                                .child(
                                    div()
                                        .size_full()
                                        .bg(cx.theme().sidebar)
                                        .text_color(cx.theme().sidebar_foreground)
                                        .border_r_1()
                                        .border_color(cx.theme().sidebar_border)
                                        .child(self.render_side_column(cx)),
                                ),
                        )
                        .child(
                            resizable_panel().size_range(px(420.)..Pixels::MAX).child(
                                v_flex()
                                    .size_full()
                                    .min_w_0()
                                    .children(self.render_playback_header(cx))
                                    .child(div().flex_1().min_h_0().child(if connected {
                                        self.chat.clone().into_any_element()
                                    } else {
                                        self.render_welcome(cx).into_any_element()
                                    })),
                            ),
                        ),
                ),
            )
            .child(self.render_status_bar(cx))
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
