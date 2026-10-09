use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::{field, v_form};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AppContext as _, Context, Entity, FontWeight,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window,
};
use syncplay_core::config::{ServerConfig, SyncplayConfig};

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
            let mut input = InputState::new(window, cx).placeholder("Your username");
            input.set_value(config.user.username.clone(), window, cx);
            input
        });
        let room = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("default");
            input.set_value(config.user.default_room.clone(), window, cx);
            input
        });
        let password = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .placeholder("Server password")
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
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.connect(cx);
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
            _subscriptions: subscriptions,
        }
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        let username = self.username.read(cx).value().trim().to_string();
        if username.is_empty() {
            self.error = Some("Username is required".into());
            cx.notify();
            return;
        }
        let config = self.store.read(cx).config.clone();
        if player_selection_missing(&config) {
            self.error = Some("Select a media player in Settings before connecting.".into());
            cx.notify();
            return;
        }
        let address = self.address.read(cx).value().to_string();
        let Some((host, port)) = parse_address(&address) else {
            self.error = Some("Address must be in host:port format".into());
            cx.notify();
            return;
        };
        self.error = None;

        let room = self.room.read(cx).value().trim().to_string();
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

    fn render_room_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rooms = self.store.read(cx).config.user.room_list.clone();
        let room = self.room.clone();
        let presets = Button::new("room-presets")
            .ghost()
            .xsmall()
            .icon(IconName::ChevronDown)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.min_w(px(160.));
                for value in rooms.clone() {
                    let room = room.clone();
                    menu = menu.item(PopupMenuItem::new(value.clone()).on_click(
                        move |_, window, cx| {
                            room.update(cx, |input, cx| {
                                input.set_value(value.clone(), window, cx);
                            });
                        },
                    ));
                }
                menu
            });
        Input::new(&self.room).suffix(presets)
    }
}

impl Render for ConnectPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (connecting, player_missing) = {
            let store = self.store.read(cx);
            (
                store.connect_pending,
                player_selection_missing(&store.config),
            )
        };

        v_flex().size_full().items_center().justify_center().child(
            v_flex()
                .w(px(380.))
                .gap_4()
                .child(
                    v_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .size(px(56.))
                                .rounded(cx.theme().radius_lg)
                                .bg(cx.theme().info.opacity(0.12))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::MonitorPlay)
                                        .with_size(px(28.))
                                        .text_color(cx.theme().info),
                                ),
                        )
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Welcome to Syncplay"),
                        )
                        .child(
                            div()
                                .pb_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Connect to a server to watch together."),
                        ),
                )
                .child(
                    v_form()
                        .child(
                            field()
                                .label("Address (host:port)")
                                .child(self.render_address_input(cx)),
                        )
                        .child(
                            field()
                                .label("Username *")
                                .child(Input::new(&self.username)),
                        )
                        .child(field().label("Room").child(self.render_room_input(cx)))
                        .child(
                            field()
                                .label("Password (optional)")
                                .child(Input::new(&self.password)),
                        ),
                )
                .when(player_missing, |this| {
                    this.child(
                        h_flex()
                            .gap_2()
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .small()
                                    .text_color(cx.theme().warning),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(cx.theme().warning)
                                    .child("Select a media player before connecting."),
                            )
                            .child(
                                Button::new("open-player-settings")
                                    .ghost()
                                    .xsmall()
                                    .label("Open Settings")
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
                        div()
                            .p_2()
                            .rounded(cx.theme().radius)
                            .text_sm()
                            .text_color(cx.theme().danger_foreground)
                            .bg(cx.theme().danger)
                            .child(error),
                    )
                })
                .child(
                    Button::new("connect")
                        .primary()
                        .w_full()
                        .label(if connecting {
                            "Connecting..."
                        } else {
                            "Connect"
                        })
                        .disabled(connecting || player_missing)
                        .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                ),
        )
    }
}

fn player_selection_missing(config: &SyncplayConfig) -> bool {
    let path = config.player.player_path.trim();
    path.is_empty() || path == "custom"
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
