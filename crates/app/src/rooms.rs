use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, StyledExt as _, WindowExt as _};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, App, AppContext as _, ClipboardItem, Context, ElementId,
    Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
};
use rust_i18n::t;
use syncplay_core::config::SyncplayConfig;
use syncplay_core::utils::parse_controlled_room_input;

use crate::store::AppStore;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RoomManagerTab {
    Switch,
    Manage,
}

impl RoomManagerTab {
    fn from_index(ix: usize) -> Self {
        match ix {
            1 => Self::Manage,
            _ => Self::Switch,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Switch => 0,
            Self::Manage => 1,
        }
    }
}

pub struct RoomManagerDialog {
    store: Entity<AppStore>,
    room_name: Entity<InputState>,
    managed_room_name: Entity<InputState>,
    controller_password: Entity<InputState>,
    /// Raw `+name:hash` behind the stripped switch-room input, so an untouched
    /// field still joins as operator.
    room_seed_raw: Option<String>,
    tab: RoomManagerTab,
    /// Row whose management buttons are revealed by hover, like the playlist.
    hovered_room: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl RoomManagerDialog {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let room_name =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("rooms.room_name_ph")));
        let managed_room_name =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("rooms.room_name_ph")));
        let controller_password =
            cx.new(|cx| InputState::new(window, cx).placeholder("AA-000-000"));

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&store, |_, _, cx| cx.notify()));
        subscriptions.push(cx.subscribe_in(
            &room_name,
            window,
            |this, _, event: &InputEvent, _, cx| {
                match event {
                    InputEvent::PressEnter { .. } => this.connect_to_room(None, cx),
                    // The seed hint under the input tracks the text live.
                    InputEvent::Change => cx.notify(),
                    _ => {}
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
            managed_room_name,
            controller_password,
            room_seed_raw: None,
            tab: RoomManagerTab::Switch,
            hovered_room: None,
            _subscriptions: subscriptions,
        }
    }

    /// Reload the form from current state; runs on every dialog open.
    pub fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = RoomManagerTab::Switch;
        self.hovered_room = None;
        let (live_room, room_seed) = {
            let store = self.store.read(cx);
            let default_room = &store.config.user.default_room;
            // Prefer the password-bearing bookmark as seed so the hinted join
            // really re-authenticates as operator.
            let room_seed = saved_operator_entry(&store.config, default_room)
                .unwrap_or_else(|| default_room.clone());
            (live_room(store), room_seed)
        };
        let managed_seed = live_room.as_deref().unwrap_or(room_seed.as_str());
        let managed_seed = strip_managed_room_name(managed_seed).to_string();

        let (room_display, room_managed) = display_room_name(&room_seed);
        let room_display = room_display.to_string();
        self.room_seed_raw = room_managed.then_some(room_seed);
        self.room_name.update(cx, |input, cx| {
            input.set_value(room_display, window, cx);
        });
        self.managed_room_name.update(cx, |input, cx| {
            input.set_value(managed_seed, window, cx);
        });
        self.controller_password.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
    }

    /// Raw seed when the switch-room input still shows its stripped form.
    fn applicable_seed(&self, cx: &App) -> Option<String> {
        let raw = self.room_seed_raw.as_ref()?;
        (self.room_name.read(cx).value().trim() == display_room_name(raw).0).then(|| raw.clone())
    }

    /// Seed whose join really ends up operator: the bookmark embeds the
    /// control password, or one is already known this session. Only then is
    /// the join-as-operator hint shown.
    fn operator_seed(&self, cx: &App) -> Option<String> {
        let raw = self.applicable_seed(cx)?;
        self.store
            .read(cx)
            .has_operator_credentials(&raw)
            .then_some(raw)
    }

    /// Room the switch-room input refers to: the raw seed while the field
    /// still shows it (stripped), otherwise the text as typed.
    fn room_from_input(&self, cx: &App) -> String {
        self.applicable_seed(cx)
            .unwrap_or_else(|| self.room_name.read(cx).value().trim().to_string())
    }

    fn connect_to_room(&mut self, room: Option<String>, cx: &mut Context<Self>) {
        let room = room.unwrap_or_else(|| self.room_from_input(cx));
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

    fn save_room_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let room = self.room_name.read(cx).value().trim().to_string();
        if room.is_empty() {
            return;
        }
        if self.store.read(cx).config.user.room_list.contains(&room) {
            window.push_notification(
                (
                    NotificationType::Warning,
                    t!("rooms.room_exists").to_string(),
                ),
                cx,
            );
            return;
        }
        self.update_config(cx, |config| {
            config.user.room_list.push(room);
        });
        self.room_name.update(cx, |input, cx| {
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
        label: gpui_kit::SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(label)
    }

    /// Saved and server rooms merged into one scrollable card; every row
    /// switches on click, and saved rows reveal their management buttons only
    /// on hover so long lists stay quiet.
    fn render_room_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let saved = store.config.user.room_list.clone();
        let default_room = store.config.user.default_room.clone();
        let persistent = store.server_room_features.persistent_rooms;
        let current = live_room(store);
        let hairline = cx.theme().border.opacity(0.55);

        // Merge by room identity: an operator bookmark (`+name:hash:pw`) and
        // the live room (`+name:hash`) are the same room — one row, keeping
        // the bookmark so clicking it re-authenticates.
        let mut rows: Vec<(String, bool)> = Vec::new();
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for room in &saved {
            let identity = room_identity(room);
            match seen.get(&identity) {
                Some(&ix) => {
                    if parse_controlled_room_input(&rows[ix].0).1.is_none()
                        && parse_controlled_room_input(room).1.is_some()
                    {
                        rows[ix].0 = room.clone();
                    }
                }
                None => {
                    seen.insert(identity, rows.len());
                    rows.push((room.clone(), true));
                }
            }
        }
        for room in store.rooms.iter() {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                seen.entry(room_identity(room))
            {
                entry.insert(rows.len());
                rows.push((room.clone(), false));
            }
        }
        let total = rows.len();
        let current_identity = current.as_deref().map(room_identity);

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.render_section_label(t!("rooms.room_list").into(), cx))
                    .when(persistent, |this| {
                        this.child(soft_badge(t!("rooms.persistent"), cx.theme().info, cx))
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{total} · {}", t!("rooms.click_to_switch"))),
                    ),
            )
            .when(rows.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("rooms.empty_list")),
                )
            })
            .when(!rows.is_empty(), |this| {
                // max_h lives on the card, never on the scrollable content:
                // clamping the content itself would collapse the scroll range
                // to zero. The flex_1 + overflow_hidden layer hands the
                // clamped height down, like gpui-component's own
                // MaxHeightParentTest.
                this.child(
                    v_flex()
                        .max_h(px(280.))
                        .rounded(cx.theme().radius_lg)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .rounded(cx.theme().radius_lg)
                                .child(
                                    div()
                                        .id("room-list")
                                        .v_flex()
                                        .size_full()
                                        .overflow_y_scrollbar()
                                        .children(rows.iter().enumerate().map(
                                            |(ix, (room, is_saved))| {
                                                let is_current = current_identity.as_deref()
                                                    == Some(room_identity(room).as_str());
                                                let is_default = *is_saved
                                                    && room_identity(room)
                                                        == room_identity(&default_room);
                                                let hovered = self.hovered_room.as_deref()
                                                    == Some(room.as_str());
                                                let (display, managed) = display_room_name(room);
                                                h_flex()
                                                    .id((ElementId::from("room-row"), room.clone()))
                                                    .w_full()
                                                    .gap_2()
                                                    .pl_3()
                                                    .pr_2()
                                                    .when(ix > 0, |this| {
                                                        this.border_t_1().border_color(hairline)
                                                    })
                                                    .when(hovered && !is_current, |this| {
                                                        this.bg(cx.theme().accent)
                                                    })
                                                    .on_hover(cx.listener({
                                                        let room = room.clone();
                                                        move |this, hovered: &bool, _, cx| {
                                                            this.hovered_room = if *hovered {
                                                                Some(room.clone())
                                                            } else if this.hovered_room.as_deref()
                                                                == Some(room.as_str())
                                                            {
                                                                None
                                                            } else {
                                                                return;
                                                            };
                                                            cx.notify();
                                                        }
                                                    }))
                                                    .child(
                                                        h_flex()
                                                            .id((
                                                                ElementId::from("room-join"),
                                                                room.clone(),
                                                            ))
                                                            .gap_2()
                                                            .min_w_0()
                                                            .flex_1()
                                                            .py_2()
                                                            .when(!is_current, |this| {
                                                                this.cursor_pointer().on_click(
                                                                    cx.listener({
                                                                        let room = room.clone();
                                                                        move |this, _, window, cx| {
                                                                            this.connect_to_room(
                                                                                Some(room.clone()),
                                                                                cx,
                                                                            );
                                                                            window
                                                                                .close_dialog(cx);
                                                                        }
                                                                    }),
                                                                )
                                                            })
                                                            .child(
                                                                div()
                                                                    .min_w_0()
                                                                    .overflow_hidden()
                                                                    .whitespace_nowrap()
                                                                    .text_ellipsis()
                                                                    .text_sm()
                                                                    .child(display.to_string()),
                                                            )
                                                            .when(managed, |this| {
                                                                this.child(managed_badge(cx))
                                                            })
                                                            .when(is_current, |this| {
                                                                this.child(soft_badge(
                                                                    t!("rooms.current"),
                                                                    cx.theme().success,
                                                                    cx,
                                                                ))
                                                            })
                                                            .when(is_default, |this| {
                                                                this.child(soft_badge(
                                                                    t!("rooms.default"),
                                                                    cx.theme().info,
                                                                    cx,
                                                                ))
                                                            }),
                                                    )
                                                    .when(hovered && *is_saved, |this| {
                                                        this.child(
                                                            h_flex()
                                                                .gap_1()
                                                                .flex_shrink_0()
                                                                .when(!is_default, |this| {
                                                                    this.child(
                                                                        Button::new((
                                                                            ElementId::from(
                                                                                "make-default",
                                                                            ),
                                                                            room.clone(),
                                                                        ))
                                                                        .ghost()
                                                                        .xsmall()
                                                                        .label(t!(
                                                                            "rooms.make_default"
                                                                        ))
                                                                        .on_click(cx.listener({
                                                                            let room = room.clone();
                                                                            move |this, _, _, cx| {
                                                                                this.update_config(
                                                                                    cx,
                                                                                    |config| {
                                                                                        config
                                                                                            .user
                                                                                            .default_room =
                                                                                            room
                                                                                                .clone(
                                                                                                );
                                                                                    },
                                                                                );
                                                                            }
                                                                        })),
                                                                    )
                                                                })
                                                                .child(
                                                                    Button::new((
                                                                        ElementId::from(
                                                                            "remove-room",
                                                                        ),
                                                                        room.clone(),
                                                                    ))
                                                                    .ghost()
                                                                    .xsmall()
                                                                    .label(t!("common.remove"))
                                                                    .text_color(cx.theme().danger)
                                                                    .on_click(cx.listener({
                                                                        let room = room.clone();
                                                                        move |this, _, _, cx| {
                                                                            // Forget the room
                                                                            // entirely, plain and
                                                                            // operator bookmark alike.
                                                                            this.update_config(
                                                                                cx,
                                                                                |config| {
                                                                                    config
                                                                                        .user
                                                                                        .room_list
                                                                                        .retain(
                                                                                            |entry| {
                                                                                                room_identity(entry)
                                                                                                    != room_identity(&room)
                                                                                            },
                                                                                        );
                                                                                },
                                                                            );
                                                                        }
                                                                    })),
                                                                ),
                                                        )
                                                    })
                                                    .into_any_element()
                                            },
                                        )),
                                ),
                        ),
                )
            })
    }

    fn render_managed_rooms(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_2()
                    .child(self.render_section_label(t!("rooms.create_managed").into(), cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("rooms.create_managed_desc")),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.managed_room_name)))
                            .child(
                                Button::new("create-managed-room")
                                    .primary()
                                    .label(t!("rooms.create"))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.create_managed_room(cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(self.render_section_label(t!("rooms.auth_operator").into(), cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("rooms.auth_operator_desc")),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&self.controller_password)))
                            .child(
                                Button::new("identify-controller")
                                    .primary()
                                    .label(t!("rooms.authenticate"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.identify_as_controller(window, cx)
                                    })),
                            ),
                    ),
            )
    }
}

impl Render for RoomManagerDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let managed_rooms = store.server_room_features.managed_rooms;
        let on_switch_tab = self.tab == RoomManagerTab::Switch;

        v_flex()
            .gap_4()
            .child(
                TabBar::new("room-manager-tabs")
                    .segmented()
                    .small()
                    .selected_index(self.tab.index())
                    .children([
                        Tab::new().label(t!("rooms.tab_switch")),
                        Tab::new().label(t!("rooms.tab_manage")),
                    ])
                    .on_click(cx.listener(|this, ix, _, cx| {
                        this.tab = RoomManagerTab::from_index(*ix);
                        cx.notify();
                    })),
            )
            .when(on_switch_tab, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(self.render_section_label(t!("rooms.connect_title").into(), cx))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().flex_1().child(Input::new(&self.room_name)))
                                .child(
                                    Button::new("connect-to-room")
                                        .primary()
                                        .label(t!("rooms.join"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.connect_to_room(None, cx)
                                        })),
                                )
                                .child(
                                    Button::new("save-room")
                                        .secondary()
                                        .label(t!("rooms.save"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.save_room_from_input(window, cx)
                                        })),
                                ),
                        )
                        .when_some(self.operator_seed(cx), |this, raw| {
                            this.child(managed_seed_hint(&raw, cx))
                        }),
                )
                .child(self.render_room_list(cx))
            })
            .when(!on_switch_tab, |this| {
                if connected && managed_rooms {
                    this.child(self.render_managed_rooms(cx))
                } else {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("rooms.manage_unavailable")),
                    )
                }
            })
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

/// Display form of a room name: `+name:xxxxxxxxxxxx` controlled rooms show as
/// `name` plus a managed badge (the bool), plain rooms show as-is. Operator
/// bookmarks carry the control password (`+name:hash:password`); it stays out
/// of the UI.
pub(crate) fn display_room_name(room: &str) -> (&str, bool) {
    let Some(rest) = room.strip_prefix('+') else {
        return (room, false);
    };
    let Some((name, suffix)) = rest.rsplit_once(':') else {
        return (room, false);
    };
    if is_room_hash(suffix) {
        return (name, true);
    }
    let Some((name, hash)) = name.rsplit_once(':') else {
        return (room, false);
    };
    if is_room_hash(hash) {
        (name, true)
    } else {
        (room, false)
    }
}

fn is_room_hash(value: &str) -> bool {
    value.len() == 12 && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Room identity ignoring any embedded control password, so an operator
/// bookmark and the live room compare equal.
pub(crate) fn room_identity(room: &str) -> String {
    parse_controlled_room_input(room).0
}

/// Saved `+name:hash:password` bookmark covering `room`, if any — joining
/// through it re-authenticates as operator.
pub(crate) fn saved_operator_entry(config: &SyncplayConfig, room: &str) -> Option<String> {
    let identity = room_identity(room);
    config
        .user
        .room_list
        .iter()
        .find(|entry| {
            entry.as_str() != room
                && parse_controlled_room_input(entry).1.is_some()
                && room_identity(entry) == identity
        })
        .cloned()
}

/// `+name:xxxxxxxxxxxx` controlled-room form back to `name` for the create
/// input, mirroring the web client's prefill.
fn strip_managed_room_name(room: &str) -> &str {
    display_room_name(room).0
}

/// Soft pill badge in a semantic color — no border, tinted background.
pub(crate) fn soft_badge(
    label: impl Into<gpui_kit::SharedString>,
    color: gpui_kit::Hsla,
    cx: &gpui_kit::App,
) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .text_xs()
        .px_2()
        .rounded(cx.theme().radius)
        .text_color(color)
        .bg(color.opacity(0.12))
        .child(label.into())
}

/// Small tag marking a stripped controlled-room name, colored like the
/// controller crown in the user list.
pub(crate) fn managed_badge(cx: &gpui_kit::App) -> impl IntoElement {
    soft_badge(t!("rooms.managed"), cx.theme().warning, cx)
}

/// Ghost icon button copying the raw controlled-room code for sharing with
/// operators; the code itself is never shown in the UI.
pub(crate) fn copy_room_code_button(id: impl Into<ElementId>, raw: String) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(IconName::Copy)
        .tooltip(t!("rooms.copy_code"))
        .on_click(move |_, window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(raw.clone()));
            window.push_notification((NotificationType::Info, t!("rooms.copied").to_string()), cx);
        })
}

/// Shown under a room input whose stripped display maps back to a managed
/// seed: makes the operator join visible and offers the code for copying.
pub(crate) fn managed_seed_hint(raw: &str, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_2()
        .child(managed_badge(cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!("rooms.join_as_operator")),
        )
        .child(copy_room_code_button(
            (ElementId::from("copy-seed-code"), raw.to_string()),
            raw.to_string(),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_room_name_strips_managed_hash() {
        assert_eq!(display_room_name("+test:95D00F6E447B"), ("test", true));
    }

    #[test]
    fn display_room_name_strips_operator_bookmark_password() {
        assert_eq!(
            display_room_name("+test:95D00F6E447B:AJ-107-331"),
            ("test", true)
        );
    }

    #[test]
    fn display_room_name_keeps_plain_and_malformed_rooms() {
        assert_eq!(display_room_name("test"), ("test", false));
        assert_eq!(
            display_room_name("+test:notahash"),
            ("+test:notahash", false)
        );
        // A password-looking suffix without the hash is not the managed form.
        assert_eq!(
            display_room_name("+test:AJ-107-331"),
            ("+test:AJ-107-331", false)
        );
    }

    #[test]
    fn room_identity_drops_only_the_password() {
        assert_eq!(
            room_identity("+test:95D00F6E447B:AJ-107-331"),
            "+test:95D00F6E447B"
        );
        assert_eq!(room_identity("+test:95D00F6E447B"), "+test:95D00F6E447B");
        assert_eq!(room_identity("test"), "test");
    }

    #[test]
    fn saved_operator_entry_finds_the_password_bookmark() {
        let mut config = SyncplayConfig::default();
        config.user.room_list = vec![
            "+test:95D00F6E447B".to_string(),
            "+test:95D00F6E447B:AJ-107-331".to_string(),
            "plain".to_string(),
        ];
        assert_eq!(
            saved_operator_entry(&config, "+test:95D00F6E447B"),
            Some("+test:95D00F6E447B:AJ-107-331".to_string())
        );
        assert_eq!(saved_operator_entry(&config, "plain"), None);
        assert_eq!(saved_operator_entry(&config, "+other:95D00F6E447C"), None);
    }
}
