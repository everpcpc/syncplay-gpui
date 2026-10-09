use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _};
use gpui_kit::{
    div, hsla, prelude::FluentBuilder as _, px, white, App, AppContext as _, Context, Entity,
    FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window,
};

use crate::events::UserInfoEvent;
use crate::rooms::RoomManagerDialog;
use crate::store::AppStore;

pub struct UserListPanel {
    store: Entity<AppStore>,
    room_manager: Entity<RoomManagerDialog>,
    _subscriptions: Vec<Subscription>,
}

impl UserListPanel {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let room_manager = cx.new(|cx| RoomManagerDialog::new(store.clone(), window, cx));
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        Self {
            store,
            room_manager,
            _subscriptions: subscriptions,
        }
    }

    fn open_room_manager(&self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog_view = self.room_manager.clone();
        dialog_view.update(cx, |dialog, cx| dialog.prepare(window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("Rooms").w(px(672.)).child(dialog_view.clone())
        });
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let current_username = store.config.user.username.clone();
        let current_user = store
            .users
            .iter()
            .find(|user| user.username == current_username);
        let is_ready = current_user.map(|user| user.is_ready).unwrap_or(false);
        let room = current_room(store);
        let user_count = store.users.len();

        let ready_button = Button::new("toggle-ready")
            .small()
            .icon(if is_ready {
                IconName::Check
            } else {
                IconName::Circle
            })
            .tooltip(if is_ready { "Ready" } else { "Not ready" })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.store.read(cx).set_ready(!is_ready);
            }));
        let ready_button = if is_ready {
            ready_button.primary()
        } else {
            ready_button.secondary()
        };

        h_flex()
            .justify_between()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .min_w_0()
                    .child(
                        Icon::new(IconName::Users)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(room),
                    )
                    .when(connected, |this| {
                        this.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("({user_count})")),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .when(connected, |this| this.child(ready_button))
                    .child(
                        Button::new("open-rooms")
                            .secondary()
                            .small()
                            .icon(IconName::PencilLine)
                            .tooltip("Rooms")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_room_manager(window, cx)
                            })),
                    ),
            )
    }

    fn render_user_card(
        &self,
        user: &UserInfoEvent,
        self_user: Option<&UserInfoEvent>,
        room: &str,
        current_username: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let same_room = user.room == room;
        let is_self = user.username == current_username;
        let self_file = self_user.and_then(|user| user.file.as_deref());
        let self_size = self_user.and_then(|user| user.file_size.clone());
        let self_duration = self_user.and_then(|user| user.file_duration);

        let name_color = if same_room && !has_same_file_name(user.file.as_deref(), self_file) {
            cx.theme().warning
        } else {
            cx.theme().muted_foreground
        };
        let size_color = if same_room && !has_same_file_size(&user.file_size, &self_size) {
            cx.theme().warning
        } else {
            cx.theme().muted_foreground
        };
        let duration_color = if same_room && !has_same_duration(user.file_duration, self_duration) {
            cx.theme().warning
        } else {
            cx.theme().muted_foreground
        };

        v_flex()
            .p_3()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .min_w_0()
                            .child(render_avatar(&user.username))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_sm()
                                    .child(user.username.clone()),
                            )
                            .child(if user.is_ready {
                                tag("Ready", cx.theme().success)
                            } else {
                                tag_muted("Not Ready", cx)
                            })
                            .when(is_self, |this| this.child(tag_accent("You", true, cx))),
                    )
                    .when(user.is_controller, |this| {
                        this.child(tag_accent("Controller", false, cx))
                    }),
            )
            .when_some(user.file.clone(), |this, file| {
                this.child(
                    v_flex()
                        .mt_1()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(name_color)
                                .child(format!("File: {file}")),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .child(
                                    div().text_color(size_color).child(format!(
                                        "Size: {}",
                                        format_file_size(&user.file_size)
                                    )),
                                )
                                .child(div().text_color(cx.theme().muted_foreground).child("/"))
                                .child(div().text_color(duration_color).child(format!(
                                    "Duration: {}",
                                    format_duration(user.file_duration)
                                ))),
                        ),
                )
            })
    }
}

impl Render for UserListPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let connected = store.is_connected();
        let current_username = store.config.user.username.clone();
        let room = current_room(store);
        let mut sorted = store.users.clone();
        let self_user = sorted
            .iter()
            .find(|user| user.username == current_username)
            .cloned();
        if !current_username.is_empty() {
            sorted.sort_by_key(|user| usize::from(user.username != current_username));
        }

        let empty_hint: Option<SharedString> = if !connected {
            Some("Not connected".into())
        } else if sorted.is_empty() {
            Some("No users in room".into())
        } else {
            None
        };

        v_flex()
            .size_full()
            .min_h_0()
            .p_5()
            .gap_2()
            .child(self.render_header(cx))
            .when_some(empty_hint.clone(), |this, hint| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(hint),
                )
            })
            .when(empty_hint.is_none(), |this| {
                this.child(
                    div()
                        .id("user-list")
                        .v_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_2()
                        .pr_1()
                        .overflow_y_scrollbar()
                        .children(sorted.iter().map(|user| {
                            self.render_user_card(
                                user,
                                self_user.as_ref(),
                                &room,
                                &current_username,
                                cx,
                            )
                            .into_any_element()
                        })),
                )
            })
    }
}

/// Room shown in the panel header: the live room when connected, otherwise
/// the configured default, matching the web client's fallback chain.
fn current_room(store: &AppStore) -> SharedString {
    let username = &store.config.user.username;
    store
        .users
        .iter()
        .find(|user| &user.username == username)
        .map(|user| user.room.clone())
        .filter(|room| !room.is_empty())
        .or_else(|| {
            let default = store.config.user.default_room.clone();
            (!default.is_empty()).then_some(default)
        })
        .unwrap_or_else(|| "Room".to_string())
        .into()
}

fn render_avatar(username: &str) -> impl IntoElement {
    // The chip hue is derived from the username, so it is a data color, not a
    // theme decision.
    let hue = (avatar_hash(username) % 360) as f32 / 360.0;
    let initial: String = username
        .trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_string());
    div()
        .flex_shrink_0()
        .size_6()
        .rounded_full()
        .bg(hsla(hue, 0.62, 0.46, 1.0))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(white())
        .child(initial)
}

fn avatar_hash(username: &str) -> u32 {
    let mut hash: u32 = 0;
    for c in username.chars() {
        hash = hash.wrapping_mul(31).wrapping_add(c as u32);
    }
    hash
}

fn tag(label: &'static str, color: Hsla) -> gpui_kit::Div {
    div()
        .flex_shrink_0()
        .text_xs()
        .px_2()
        .rounded_full()
        .border_1()
        .border_color(color)
        .bg(color.opacity(0.15))
        .child(label)
}

fn tag_muted(label: &'static str, cx: &App) -> gpui_kit::Div {
    div()
        .flex_shrink_0()
        .text_xs()
        .px_2()
        .rounded_full()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

fn tag_accent(label: &'static str, pill: bool, cx: &App) -> gpui_kit::Div {
    let this = div()
        .flex_shrink_0()
        .text_xs()
        .px_2()
        .border_1()
        .border_color(cx.theme().info)
        .bg(cx.theme().info.opacity(0.15));
    if pill {
        this.rounded_full().child(label)
    } else {
        this.rounded(cx.theme().radius).child(label)
    }
}

fn has_same_file_name(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

/// `fileSize` arrives as a byte count, or as a hash string when the peer
/// hides sizes; the two shapes compare by different rules.
enum SizeRepr {
    Number(f64),
    Text(String),
}

fn normalize_file_size(value: &Option<serde_json::Value>) -> Option<SizeRepr> {
    match value {
        Some(serde_json::Value::String(text)) => Some(SizeRepr::Text(text.clone())),
        Some(serde_json::Value::Number(number)) => {
            number.as_f64().filter(|v| *v > 0.0).map(SizeRepr::Number)
        }
        _ => None,
    }
}

fn has_same_file_size(a: &Option<serde_json::Value>, b: &Option<serde_json::Value>) -> bool {
    let (Some(left), Some(right)) = (normalize_file_size(a), normalize_file_size(b)) else {
        return false;
    };
    match (&left, &right) {
        (SizeRepr::Number(left), SizeRepr::Number(right)) => (left - right).abs() < 1.0,
        (SizeRepr::Text(left), SizeRepr::Text(right)) => left == right,
        _ => false,
    }
}

fn has_same_duration(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < 1.0,
        _ => false,
    }
}

fn format_file_size(value: &Option<serde_json::Value>) -> String {
    match normalize_file_size(value) {
        Some(SizeRepr::Text(text)) => text,
        Some(SizeRepr::Number(size)) => {
            const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
            let mut size = size;
            let mut unit = 0;
            while size >= 1024.0 && unit < UNITS.len() - 1 {
                size /= 1024.0;
                unit += 1;
            }
            let precision = if size >= 100.0 {
                0
            } else if size >= 10.0 {
                1
            } else {
                2
            };
            format!("{size:.precision$} {}", UNITS[unit])
        }
        None => "--".to_string(),
    }
}

fn format_duration(duration: Option<f64>) -> String {
    let Some(duration) = duration.filter(|d| *d > 0.0) else {
        return "--".to_string();
    };
    let total = duration.floor() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}
