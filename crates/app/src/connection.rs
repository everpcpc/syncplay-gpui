use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::form::{field, v_form};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, AppContext as _, Context, Entity,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Window,
};
use syncplay_core::config::settings::PlayerConfig;
use syncplay_core::config::{ServerConfig, SyncplayConfig, UserPreferences};
use syncplay_core::player::detection::DetectedPlayer;

use crate::store::{AppStore, ConnectParams, StoreEvent};

const CUSTOM_PATH_LABEL: &str = "Custom path...";

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogTab {
    Connection,
    Player,
}

pub struct ConnectionDialog {
    store: Entity<AppStore>,
    active_tab: DialogTab,
    address: Entity<InputState>,
    username: Entity<InputState>,
    room: Entity<InputState>,
    password: Entity<InputState>,
    player_path: Entity<InputState>,
    player_args: Entity<InputState>,
    player_select: Entity<SelectState<Vec<String>>>,
    show_options: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl ConnectionDialog {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| InputState::new(window, cx).placeholder("syncplay.pl:8999"));
        let username = cx.new(|cx| InputState::new(window, cx).placeholder("Your username"));
        let room = cx.new(|cx| InputState::new(window, cx).placeholder("default"));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Server password")
                .masked(true)
        });
        let player_path =
            cx.new(|cx| InputState::new(window, cx).placeholder("/usr/local/bin/mpv"));
        let player_args =
            cx.new(|cx| InputState::new(window, cx).placeholder("--fullscreen --no-border"));
        let player_select = cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx));

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&store, |_, _, cx| {
            cx.notify();
        }));
        subscriptions.push(cx.subscribe_in(
            &store,
            window,
            |this, store, event: &StoreEvent, window, cx| match event {
                StoreEvent::ConnectFinished => {
                    if !window.has_active_dialog(cx) {
                        return;
                    }
                    match store.read(cx).connect_result.clone() {
                        Some(Ok(_)) => window.close_dialog(cx),
                        Some(Err(error)) => {
                            this.error = Some(error.into());
                            cx.notify();
                        }
                        None => {}
                    }
                }
                StoreEvent::PlayersDetected => this.sync_player_select(window, cx),
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &player_select,
            window,
            |this, _, event: &SelectEvent<Vec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.on_player_selected(value.clone(), window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &player_path,
            window,
            |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let path = input.read(cx).value().to_string();
                    this.update_player_config(cx, |player| player.player_path = path);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &player_args,
            window,
            |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let args = input
                        .read(cx)
                        .value()
                        .split_whitespace()
                        .map(str::to_string)
                        .collect();
                    this.update_player_config(cx, |player| player.player_arguments = args);
                }
            },
        ));

        Self {
            store,
            active_tab: DialogTab::Connection,
            address,
            username,
            room,
            password,
            player_path,
            player_args,
            player_select,
            show_options: false,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn store(&self) -> &Entity<AppStore> {
        &self.store
    }

    /// Reload the form from the current config; runs on every dialog open.
    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.store.read(cx).config.clone();

        self.address.update(cx, |input, cx| {
            input.set_value(
                format!("{}:{}", config.server.host, config.server.port),
                window,
                cx,
            );
        });
        self.username.update(cx, |input, cx| {
            input.set_value(config.user.username.clone(), window, cx);
        });
        self.room.update(cx, |input, cx| {
            input.set_value(config.user.default_room.clone(), window, cx);
        });
        self.password.update(cx, |input, cx| {
            input.set_value(
                config.server.password.clone().unwrap_or_default(),
                window,
                cx,
            );
        });
        self.player_path.update(cx, |input, cx| {
            let path = &config.player.player_path;
            input.set_value(if path == "custom" { "" } else { path }, window, cx);
        });
        self.player_args.update(cx, |input, cx| {
            input.set_value(config.player.player_arguments.join(" "), window, cx);
        });

        self.error = None;
        self.show_options = false;
        let initial_tab = if player_selection_missing(&config) {
            DialogTab::Player
        } else {
            DialogTab::Connection
        };
        self.activate_tab(initial_tab, window, cx);
    }

    fn activate_tab(&mut self, tab: DialogTab, window: &mut Window, cx: &mut Context<Self>) {
        self.active_tab = tab;
        if tab == DialogTab::Player {
            self.store.update(cx, |store, cx| {
                store.load_cached_players(cx);
                store.refresh_players(cx);
            });
            self.sync_player_select(window, cx);
        }
        cx.notify();
    }

    fn sync_player_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (player_path, players) = {
            let store = self.store.read(cx);
            (
                store.config.player.player_path.clone(),
                store.detected_players.clone(),
            )
        };

        let mut items: Vec<String> = players.iter().map(player_label).collect();
        items.push(CUSTOM_PATH_LABEL.to_string());

        let selected = if player_path == "custom" {
            Some(IndexPath::new(items.len() - 1))
        } else {
            players
                .iter()
                .position(|player| player.path == player_path)
                .map(IndexPath::new)
        };
        self.player_select.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            select.set_selected_index(selected, window, cx);
        });
    }

    fn on_player_selected(&mut self, label: String, window: &mut Window, cx: &mut Context<Self>) {
        if label == CUSTOM_PATH_LABEL {
            self.player_path.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
            self.update_player_config(cx, |player| player.player_path = "custom".to_string());
        } else if let Some(path) = self
            .store
            .read(cx)
            .detected_players
            .iter()
            .find(|player| player_label(player) == label)
            .map(|player| player.path.clone())
        {
            self.player_path.update(cx, |input, cx| {
                input.set_value(path.clone(), window, cx);
            });
            self.update_player_config(cx, |player| player.player_path = path);
        }
    }

    fn update_user_config(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut UserPreferences)) {
        let mut config = self.store.read(cx).config.clone();
        edit(&mut config.user);
        self.store.read(cx).update_config(config);
    }

    fn update_player_config(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut PlayerConfig)) {
        let mut config = self.store.read(cx).config.clone();
        edit(&mut config.player);
        self.store.read(cx).update_config(config);
    }

    fn connect(&mut self, save: bool, cx: &mut Context<Self>) {
        let username = self.username.read(cx).value().trim().to_string();
        if username.is_empty() {
            self.error = Some("Username is required".into());
            cx.notify();
            return;
        }
        let config = self.store.read(cx).config.clone();
        if player_selection_missing(&config) {
            self.error = Some("Select a media player before connecting.".into());
            self.active_tab = DialogTab::Player;
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

        let save_config = save.then(|| {
            let mut next = config.clone();
            next.server.host = host.clone();
            next.server.port = port;
            next.server.password = password.clone();
            next.user.username = username.clone();
            next.user.default_room = room.clone();
            next.recent_servers =
                add_server_to_list(&next.recent_servers, host.clone(), port, password.clone());
            next.user.room_list = add_room_to_list(&next.user.room_list, room.clone());
            next
        });

        self.store.update(cx, |store, cx| {
            store.connect(
                ConnectParams {
                    host,
                    port,
                    username,
                    room,
                    password,
                },
                save_config,
                cx,
            );
        });
        cx.notify();
    }

    fn disconnect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.store.read(cx).disconnect();
        window.push_notification((NotificationType::Info, "Disconnected from server"), cx);
        window.close_dialog(cx);
    }

    fn render_connected(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let server = self
            .store
            .read(cx)
            .connection
            .server
            .clone()
            .unwrap_or_default();
        v_flex()
            .gap_4()
            .child(
                div()
                    .p_4()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .text_sm()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Connected to:"),
                            )
                            .child(
                                div()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(server),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("disconnect")
                            .danger()
                            .label("Disconnect")
                            .flex_1()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.disconnect(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("close")
                            .outline()
                            .label("Close")
                            .flex_1()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_player_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (players, players_refreshing, players_checked_at, player_path) = {
            let store = self.store.read(cx);
            (
                store.detected_players.clone(),
                store.players_refreshing,
                store.detected_players_at,
                store.config.player.player_path.clone(),
            )
        };

        let checked_label = players_checked_at.and_then(|at| {
            chrono::DateTime::from_timestamp_millis(at)
                .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
        });
        let picker: AnyElement = if players.is_empty() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("No players detected. Enter path manually.")
                .into_any_element()
        } else {
            Select::new(&self.player_select)
                .w_full()
                .placeholder("Select a player...")
                .into_any_element()
        };

        let mut form = v_form().child(
            field().label_indent(false).child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child("Media Player"),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .when_some(checked_label, |this, label| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("Last checked {label}")),
                                        )
                                    })
                                    .child(
                                        Button::new("refresh-players")
                                            .outline()
                                            .xsmall()
                                            .label(if players_refreshing {
                                                "Refreshing..."
                                            } else {
                                                "Refresh Players"
                                            })
                                            .disabled(players_refreshing)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.store.update(cx, |store, cx| {
                                                    store.refresh_players(cx);
                                                });
                                            })),
                                    ),
                            ),
                    )
                    .child(picker),
            ),
        );
        if player_path == "custom"
            || players.is_empty()
            || !players.iter().any(|player| player.path == player_path)
        {
            form = form.child(
                field()
                    .label("Player Path (Manual)")
                    .child(Input::new(&self.player_path))
                    .description("Full path to media player executable"),
            );
        }
        form.child(
            field()
                .label("Player Arguments")
                .child(Input::new(&self.player_args))
                .description("Arguments applied when launching the player"),
        )
    }

    fn render_connection_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let config = self.store.read(cx).config.clone();

        let mut form = v_form()
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
            )
            .child(
                field().label_indent(false).child(
                    h_flex()
                        .justify_between()
                        .child(div().text_sm().child("Connection Options"))
                        .child(
                            Button::new("toggle-options")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Settings)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_options = !this.show_options;
                                    cx.notify();
                                })),
                        ),
                ),
            );
        if self.show_options {
            form = form.child(
                field().label_indent(false).child(
                    v_flex()
                        .gap_2()
                        .child(
                            Checkbox::new("autosave-joins")
                                .label("Auto-save joined rooms")
                                .checked(config.user.autosave_joins_to_list)
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    let checked = *checked;
                                    this.update_user_config(cx, |user| {
                                        user.autosave_joins_to_list = checked;
                                    });
                                })),
                        )
                        .child(
                            Checkbox::new("auto-connect")
                                .label("Auto-connect on startup")
                                .checked(config.user.auto_connect)
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    let checked = *checked;
                                    this.update_user_config(cx, |user| {
                                        user.auto_connect = checked;
                                    });
                                })),
                        )
                        .child(
                            Checkbox::new("force-gui-prompt")
                                .label("Always show connect dialog on startup")
                                .checked(config.user.force_gui_prompt)
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    let checked = *checked;
                                    this.update_user_config(cx, |user| {
                                        user.force_gui_prompt = checked;
                                    });
                                })),
                        ),
                ),
            );
        }
        form
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (connecting, player_missing) = {
            let store = self.store.read(cx);
            (
                store.connect_pending,
                player_selection_missing(&store.config),
            )
        };

        let tab_bar = TabBar::new("connection-tabs")
            .underline()
            .selected_index(match self.active_tab {
                DialogTab::Connection => 0,
                DialogTab::Player => 1,
            })
            .children([Tab::new().label("Connection"), Tab::new().label("Player")])
            .on_click(cx.listener(|this, ix, window, cx| {
                this.activate_tab(
                    if *ix == 1 {
                        DialogTab::Player
                    } else {
                        DialogTab::Connection
                    },
                    window,
                    cx,
                );
            }));

        let tab_content: AnyElement = match self.active_tab {
            DialogTab::Connection => self.render_connection_tab(cx).into_any_element(),
            DialogTab::Player => self.render_player_tab(cx).into_any_element(),
        };

        v_flex()
            .gap_4()
            .child(
                div()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(tab_bar),
            )
            .child(tab_content)
            .when(player_missing, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child("Select a media player in the Player tab before connecting."),
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
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("connect")
                            .primary()
                            .label(if connecting {
                                "Connecting..."
                            } else {
                                "Connect"
                            })
                            .disabled(connecting || player_missing)
                            .flex_1()
                            .on_click(cx.listener(|this, _, _, cx| this.connect(false, cx))),
                    )
                    .child(
                        Button::new("connect-save")
                            .secondary()
                            .label(if connecting {
                                "Connecting..."
                            } else {
                                "Connect & Save"
                            })
                            .disabled(connecting || player_missing)
                            .flex_1()
                            .on_click(cx.listener(|this, _, _, cx| this.connect(true, cx))),
                    )
                    .child(
                        Button::new("cancel")
                            .outline()
                            .label("Cancel")
                            .flex_1()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .into_any_element()
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

impl Render for ConnectionDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let connected = self.store.read(cx).is_connected();
        div().w_full().child(if connected {
            self.render_connected(cx)
        } else {
            self.render_form(cx)
        })
    }
}

fn player_selection_missing(config: &SyncplayConfig) -> bool {
    let path = config.player.player_path.trim();
    path.is_empty() || path == "custom"
}

fn player_label(player: &DetectedPlayer) -> String {
    match &player.version {
        Some(version) => format!("{} ({}) - {}", player.name, version, player.path),
        None => format!("{} - {}", player.name, player.path),
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
