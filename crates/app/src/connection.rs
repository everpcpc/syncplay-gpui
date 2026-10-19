use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::{field, v_form};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, App, AppContext as _, Context, Entity, FontWeight,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window,
};
use rust_i18n::t;
use syncplay_core::config::ServerConfig;
use syncplay_core::player::detection::player_path_resolves;

use crate::rooms::{display_room_name, managed_seed_hint};
use crate::settings::{open_settings_dialog_at, SettingsTab};
use crate::store::{AppStore, ConnectParams, StoreEvent};

/// Inline connection form filling the main column while disconnected; the
/// player picker lives in Settings, so connecting needs no modal dialog.
pub struct ConnectPanel {
    store: Entity<AppStore>,
    address: Entity<InputState>,
    username: Entity<InputState>,
    room: Entity<InputState>,
    password: Entity<InputState>,
    error: Option<SharedString>,
    /// Raw `+name:hash` behind the stripped room input, so an untouched field
    /// still joins as operator.
    room_seed_raw: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl ConnectPanel {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = store.read(cx).config.clone();

        let address = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("syncplay.pl:8999");
            input.set_value(
                format!("{}:{}", config.server.host, config.server.port),
                window,
                cx,
            );
            input
        });
        let username = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(t!("conn.username_ph"));
            input.set_value(config.user.username.clone(), window, cx);
            input
        });
        let (room_display, room_managed) = display_room_name(&config.user.default_room);
        let room_seed_raw = room_managed.then_some(config.user.default_room.clone());
        let room = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("default");
            input.set_value(room_display.to_string(), window, cx);
            input
        });
        let password = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .placeholder(t!("conn.password_ph"))
                .masked(true);
            input.set_value(
                config.server.password.clone().unwrap_or_default(),
                window,
                cx,
            );
            input
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));
        subscriptions.push(cx.subscribe_in(
            &store,
            window,
            |this, store, event: &StoreEvent, _, cx| {
                if matches!(event, StoreEvent::ConnectFinished) {
                    this.error = match store.read(cx).connect_result.clone() {
                        Some(Err(error)) => Some(error.into()),
                        _ => None,
                    };
                    cx.notify();
                }
            },
        ));
        for input in [&address, &username, &room, &password] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    match event {
                        InputEvent::PressEnter { .. } => this.connect(cx),
                        // The seed hint under the room input tracks the text live.
                        InputEvent::Change => cx.notify(),
                        _ => {}
                    }
                },
            ));
        }

        Self {
            store,
            address,
            username,
            room,
            password,
            error: None,
            room_seed_raw,
            _subscriptions: subscriptions,
        }
    }

    fn player_path_missing(&self, path: &str) -> bool {
        let path = path.trim();
        if path.is_empty() || path == "custom" {
            return true;
        }
        !player_path_resolves(path)
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        let username = self.username.read(cx).value().trim().to_string();
        if username.is_empty() {
            self.error = Some(t!("conn.username_required").into());
            cx.notify();
            return;
        }
        let config = self.store.read(cx).config.clone();
        if self.player_path_missing(&config.player.player_path) {
            self.error = Some(t!("conn.player_warning").into());
            cx.notify();
            return;
        }
        let address = self.address.read(cx).value().to_string();
        let Some((host, port)) = parse_address(&address) else {
            self.error = Some(t!("conn.address_invalid").into());
            cx.notify();
            return;
        };
        self.error = None;

        let room = self.room_from_input(cx);
        let password = {
            let value = self.password.read(cx).value().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        };

        // One action: connecting always remembers the server, room, and
        // credentials; the dialog's separate "Connect & Save" went away with
        // the dialog.
        let mut save = config.clone();
        save.server.host = host.clone();
        save.server.port = port;
        save.server.password = password.clone();
        save.user.username = username.clone();
        save.user.default_room = room.clone();
        save.recent_servers =
            add_server_to_list(&save.recent_servers, host.clone(), port, password.clone());
        save.user.room_list = add_room_to_list(&save.user.room_list, room.clone());

        self.store.update(cx, |store, cx| {
            store.connect(
                ConnectParams {
                    host,
                    port,
                    username,
                    room,
                    password,
                },
                Some(save),
                cx,
            );
        });
        cx.notify();
    }

    fn render_address_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let config = &self.store.read(cx).config;
        let mut options: Vec<(String, String, Option<String>)> = config
            .public_servers
            .iter()
            .map(|server| (server.name.clone(), server.address.clone(), None))
            .collect();
        options.extend(config.recent_servers.iter().map(|server| {
            (
                format!("{}:{}", server.host, server.port),
                format!("{}:{}", server.host, server.port),
                server.password.clone(),
            )
        }));

        let address = self.address.clone();
        let password = self.password.clone();
        let presets = Button::new("address-presets")
            .ghost()
            .xsmall()
            .icon(IconName::ChevronDown)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(240.));
                for (label, value, password_value) in options.clone() {
                    let address = address.clone();
                    let password = password.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        address.update(cx, |input, cx| {
                            input.set_value(value.clone(), window, cx);
                        });
                        if let Some(password_value) = password_value.clone() {
                            password.update(cx, |input, cx| {
                                input.set_value(password_value, window, cx);
                            });
                        }
                    }));
                }
                menu
            });
        Input::new(&self.address).suffix(presets)
    }

    fn render_player_hint(&self, show_change: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let path = store.config.player.player_path.trim().to_string();
        let name = store
            .detected_players
            .iter()
            .find(|player| player.path == path)
            .map(|player| player.name.clone());
        let player = match name {
            Some(name) => format!("{} — {}", name, path),
            None => path,
        };
        h_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("conn.player_using", player = player).to_string()),
            )
            .when(show_change, |this| {
                this.child(
                    Button::new("change-player")
                        .ghost()
                        .xsmall()
                        .flex_shrink_0()
                        .label(t!("conn.change_player"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            open_settings_dialog_at(
                                window,
                                cx,
                                this.store.clone(),
                                SettingsTab::Player,
                            )
                        })),
                )
            })
    }

    /// Raw seed when the room input still shows its stripped form.
    fn applicable_seed(&self, cx: &App) -> Option<String> {
        let raw = self.room_seed_raw.as_ref()?;
        (self.room.read(cx).value().trim() == display_room_name(raw).0).then(|| raw.clone())
    }

    /// Room the input refers to: the raw seed while the field still shows it
    /// (stripped), otherwise the text as typed.
    fn room_from_input(&self, cx: &App) -> String {
        self.applicable_seed(cx)
            .unwrap_or_else(|| self.room.read(cx).value().trim().to_string())
    }

    fn render_room_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rooms = self.store.read(cx).config.user.room_list.clone();
        let room = self.room.clone();
        let panel = cx.entity();
        let presets = Button::new("room-presets")
            .ghost()
            .xsmall()
            .icon(IconName::ChevronDown)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(160.));
                for value in rooms.clone() {
                    let (display, managed) = display_room_name(&value);
                    let label = if managed {
                        format!("{} ({})", display, t!("rooms.managed"))
                    } else {
                        display.to_string()
                    };
                    let room = room.clone();
                    let panel = panel.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                        let (display, managed) = display_room_name(&value);
                        room.update(cx, |input, cx| {
                            input.set_value(display.to_string(), window, cx);
                        });
                        panel.update(cx, |this, _| {
                            this.room_seed_raw = managed.then_some(value.clone());
                        });
                    }));
                }
                menu
            });
        Input::new(&self.room).suffix(presets)
    }
}

impl Render for ConnectPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (connecting, player_path) = {
            let store = self.store.read(cx);
            (
                store.connect_pending,
                store.config.player.player_path.clone(),
            )
        };
        let player_missing = self.player_path_missing(&player_path);
        let player_configured = {
            let path = player_path.trim();
            !path.is_empty() && path != "custom"
        };

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .p_6()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .w(px(440.))
                    .max_w_full()
                    .p_6()
                    .gap_5()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("conn.title")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("conn.subtitle")),
                            ),
                    )
                    .child(
                        v_form()
                            .child(
                                field()
                                    .label(t!("conn.address_label").to_string())
                                    .child(self.render_address_input(cx)),
                            )
                            .child(
                                field()
                                    .label(t!("conn.username_label").to_string())
                                    .child(Input::new(&self.username)),
                            )
                            .child(
                                field().label(t!("conn.room_label").to_string()).child(
                                    v_flex()
                                        .gap_2()
                                        .child(self.render_room_input(cx))
                                        .when_some(self.applicable_seed(cx), |this, raw| {
                                            this.child(managed_seed_hint(&raw, cx))
                                        }),
                                ),
                            )
                            .child(
                                field()
                                    .label(t!("conn.password_label").to_string())
                                    .child(Input::new(&self.password)),
                            ),
                    )
                    .when(player_configured, |this| {
                        this.child(self.render_player_hint(!player_missing, cx))
                    })
                    .when(player_missing, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().warning.opacity(0.12))
                                .child(
                                    Icon::new(IconName::TriangleAlert)
                                        .small()
                                        .text_color(cx.theme().warning),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .text_color(cx.theme().foreground)
                                        .child(t!("conn.player_warning")),
                                )
                                .child(
                                    Button::new("open-player-settings")
                                        .secondary()
                                        .xsmall()
                                        .flex_shrink_0()
                                        .label(t!("conn.choose_player"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            open_settings_dialog_at(
                                                window,
                                                cx,
                                                this.store.clone(),
                                                SettingsTab::Player,
                                            )
                                        })),
                                ),
                        )
                    })
                    .when_some(self.error.clone(), |this, error| {
                        this.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().danger.opacity(0.10))
                                .child(
                                    Icon::new(IconName::TriangleAlert)
                                        .small()
                                        .text_color(cx.theme().danger),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .text_color(cx.theme().foreground)
                                        .child(error),
                                ),
                        )
                    })
                    .child(
                        Button::new("connect")
                            .primary()
                            .large()
                            .w_full()
                            .label(if connecting {
                                t!("conn.connecting")
                            } else {
                                t!("common.connect")
                            })
                            .disabled(connecting || player_missing)
                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                    ),
            )
    }
}

fn parse_address(address: &str) -> Option<(String, u16)> {
    let trimmed = address.trim();
    let (host, port) = trimmed.rsplit_once(':')?;
    let host = host.trim();
    let port: u16 = port.trim().parse().ok()?;
    if host.is_empty() || port == 0 {
        return None;
    }
    Some((host.to_string(), port))
}

fn add_room_to_list(rooms: &[String], room: String) -> Vec<String> {
    let trimmed = room.trim();
    if trimmed.is_empty() {
        return rooms.to_vec();
    }
    let mut next: Vec<String> = rooms
        .iter()
        .filter(|entry| entry.as_str() != trimmed)
        .cloned()
        .collect();
    next.insert(0, trimmed.to_string());
    next
}

fn add_server_to_list(
    servers: &[ServerConfig],
    host: String,
    port: u16,
    password: Option<String>,
) -> Vec<ServerConfig> {
    let mut next: Vec<_> = servers
        .iter()
        .filter(|entry| entry.host != host || entry.port != port)
        .cloned()
        .collect();
    next.insert(
        0,
        ServerConfig {
            host,
            port,
            password,
        },
    );
    next.truncate(10);
    next
}
