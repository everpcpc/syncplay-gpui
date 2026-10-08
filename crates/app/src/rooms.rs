use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _};
use gpui_kit::{
    div, prelude::FluentBuilder as _, AppContext as _, Context, ElementId, Entity, FontWeight,
    IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window,
};
use syncplay_core::config::SyncplayConfig;

use crate::store::AppStore;

pub struct RoomManagerDialog {
    store: Entity<AppStore>,
    room_name: Entity<InputState>,
    room_list_entry: Entity<InputState>,
    managed_room_name: Entity<InputState>,
    controller_password: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl RoomManagerDialog {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let room_name = cx.new(|cx| InputState::new(window, cx).placeholder("Room name"));
        let room_list_entry = cx.new(|cx| InputState::new(window, cx).placeholder("Add a room"));
        let managed_room_name = cx.new(|cx| InputState::new(window, cx).placeholder("Room name"));
        let controller_password =
            cx.new(|cx| InputState::new(window, cx).placeholder("AA-000-000"));

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));
        subscriptions.push(cx.subscribe_in(
            &room_name,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.connect_to_room(None, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &room_list_entry,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_saved_room(window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &managed_room_name,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.create_managed_room(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &controller_password,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.identify_as_controller(window, cx);
                }
            },
        ));

        Self {
            store,
            room_name,
            room_list_entry,
            managed_room_name,
            controller_password,
            _subscriptions: subscriptions,
        }
    }

    /// Reload the form from current state; runs on every dialog open.
    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (default_room, live_room) = {
            let store = self.store.read(cx);
            (store.config.user.default_room.clone(), live_room(store))
        };
        let managed_seed = live_room.as_deref().unwrap_or(default_room.as_str());
        let managed_seed = strip_managed_room_name(managed_seed).to_string();

        self.room_name.update(cx, |input, cx| {
            input.set_value(default_room, window, cx);
        });
        self.managed_room_name.update(cx, |input, cx| {
            input.set_value(managed_seed, window, cx);
        });
        self.room_list_entry.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.controller_password.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    fn connect_to_room(&mut self, room: Option<String>, cx: &mut Context<Self>) {
        let room = room.unwrap_or_else(|| self.room_name.read(cx).value().trim().to_string());
        if room.is_empty() {
            return;
        }
        self.store.read(cx).change_room(room);
    }

    fn create_managed_room(&mut self, cx: &mut Context<Self>) {
        let room = self.managed_room_name.read(cx).value().trim().to_string();
        if room.is_empty() {
            return;
        }
        self.store.read(cx).create_managed_room(room);
    }

    fn identify_as_controller(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = self.controller_password.read(cx).value().to_string();
        if password.is_empty() {
            return;
        }
        self.store.read(cx).identify_as_controller(password);
        self.controller_password.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    fn add_saved_room(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let room = self.room_list_entry.read(cx).value().trim().to_string();
        if room.is_empty() {
            return;
        }
        if self.store.read(cx).config.user.room_list.contains(&room) {
            window.push_notification((NotificationType::Warning, "Room already exists"), cx);
            self.room_list_entry.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
            return;
        }
        self.update_config(cx, |config| {
            config.user.room_list.push(room);
        });
        self.room_list_entry.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    fn update_config(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut SyncplayConfig)) {
        let mut config = self.store.read(cx).config.clone();
        edit(&mut config);
        self.store.read(cx).update_config(config);
    }

    fn render_section_label(
        &self,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(cx.theme().foreground)
            .child(label)
    }

    fn render_server_rooms(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let rooms = store.rooms.clone();
        let persistent = store.server_room_features.persistent_rooms;
        let current = live_room(store);

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.render_section_label("Server Rooms", cx))
                    .when(persistent, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .px_2()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(cx.theme().info)
                                .bg(cx.theme().info.opacity(0.15))
                                .child("Persistent"),
                        )
                    }),
            )
            .when(rooms.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No rooms advertised by the server."),
                )
            })
            .children(rooms.iter().map(|room| {
                let is_current = current.as_deref() == Some(room.as_str());
                h_flex()
                    .justify_between()
                    .gap_3()
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
                            .child(room.clone()),
                    )
                    .child(
                        Button::new((ElementId::from("join-room"), room.clone()))
                            .secondary()
                            .xsmall()
                            .label(if is_current { "Current" } else { "Join" })
                            .disabled(is_current)
                            .on_click(cx.listener({
                                let room = room.clone();
                                move |this, _, _, cx| this.connect_to_room(Some(room.clone()), cx)
                            })),
                    )
                    .into_any_element()
            }))
    }

    fn render_managed_rooms(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_2()
                    .child(self.render_section_label("Create Managed Room", cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.managed_room_name)))
                            .child(
                                Button::new("create-managed-room")
                                    .primary()
                                    .label("Create")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.create_managed_room(cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(self.render_section_label("Authenticate as Room Operator", cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.controller_password)))
                            .child(
                                Button::new("identify-controller")
                                    .primary()
                                    .label("Authenticate")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.identify_as_controller(window, cx)
                                    })),
                            ),
                    ),
            )
    }

    fn render_saved_rooms(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let room_list = store.config.user.room_list.clone();
        let default_room = store.config.user.default_room.clone();

        v_flex()
            .gap_2()
            .child(self.render_section_label("Saved Rooms", cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.room_list_entry)))
                    .child(
                        Button::new("add-saved-room")
                            .primary()
                            .label("Add")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_saved_room(window, cx)),
                            ),
                    ),
            )
            .when(room_list.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("No rooms saved."),
                )
            })
            .children(room_list.iter().map(|room| {
                let is_default = *room == default_room;
                h_flex()
                    .justify_between()
                    .px_3()
                    .py_2()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_sm()
                                    .child(room.clone()),
                            )
                            .when(is_default, |this| {
                                this.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .px_2()
                                        .rounded(cx.theme().radius)
                                        .border_1()
                                        .border_color(cx.theme().info)
                                        .bg(cx.theme().info.opacity(0.15))
                                        .child("Default"),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .flex_shrink_0()
                            .when(!is_default, |this| {
                                this.child(
                                    Button::new((ElementId::from("make-default"), room.clone()))
                                        .ghost()
                                        .xsmall()
                                        .label("Make default")
                                        .on_click(cx.listener({
                                            let room = room.clone();
                                            move |this, _, _, cx| {
                                                this.update_config(cx, |config| {
                                                    config.user.default_room = room.clone();
                                                });
                                            }
                                        })),
                                )
                            })
                            .child(
                                Button::new((ElementId::from("remove-room"), room.clone()))
                                    .ghost()
                                    .xsmall()
                                    .label("Remove")
                                    .text_color(cx.theme().danger)
                                    .on_click(cx.listener({
                                        let room = room.clone();
                                        move |this, _, _, cx| {
                                            this.update_config(cx, |config| {
                                                config
                                                    .user
                                                    .room_list
                                                    .retain(|entry| entry != &room);
                                            });
                                        }
                                    })),
                            ),
                    )
                    .into_any_element()
            }))
    }
}

impl Render for RoomManagerDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let managed_rooms = store.server_room_features.managed_rooms;

        v_flex()
            .gap_4()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Manage room connections and defaults."),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(self.render_section_label("Connect to Room", cx))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.room_name)))
                            .child(
                                Button::new("connect-to-room")
                                    .primary()
                                    .label("Connect")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.connect_to_room(None, cx)
                                        }),
                                    ),
                            ),
                    ),
            )
            .when(connected, |this| this.child(self.render_server_rooms(cx)))
            .when(connected && managed_rooms, |this| {
                this.child(self.render_managed_rooms(cx))
            })
            .child(self.render_saved_rooms(cx))
    }
}

/// The room the local user is in right now, if the server has told us.
fn live_room(store: &AppStore) -> Option<String> {
    let username = &store.config.user.username;
    store
        .users
        .iter()
        .find(|user| &user.username == username)
        .map(|user| user.room.clone())
        .filter(|room| !room.is_empty())
}

/// `+name:xxxxxxxxxxxx` controlled-room form back to `name` for the create
/// input, mirroring the web client's prefill.
fn strip_managed_room_name(room: &str) -> &str {
    let Some(rest) = room.strip_prefix('+') else {
        return room;
    };
    let Some((name, suffix)) = rest.rsplit_once(':') else {
        return room;
    };
    if suffix.len() == 12
        && suffix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        name
    } else {
        room
    }
}
