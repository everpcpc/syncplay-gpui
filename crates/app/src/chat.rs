use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::{
    div, px, AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Window,
};
use gpui_kit::{prelude::FluentBuilder as _, FontWeight, StyleRefinement};

use crate::events::ChatMessageEvent;
use crate::store::AppStore;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChatFilter {
    All,
    Chat,
    Events,
}

impl ChatFilter {
    fn from_index(ix: usize) -> Self {
        match ix {
            1 => Self::Chat,
            2 => Self::Events,
            _ => Self::All,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::All => 0,
            Self::Chat => 1,
            Self::Events => 2,
        }
    }
}

#[derive(Clone, PartialEq)]
struct DisplayMessage {
    collapse_key: Option<String>,
    timestamp: SharedString,
    username: Option<SharedString>,
    message: SharedString,
    message_type: SharedString,
    count: u32,
}

pub struct ChatPanel {
    store: Entity<AppStore>,
    scroller: Entity<MessageScrollerState>,
    input: Entity<InputState>,
    filter: ChatFilter,
    displayed: Rc<Vec<DisplayMessage>>,
    _subscriptions: Vec<Subscription>,
}

impl ChatPanel {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Type a message... (or /help for commands)")
        });

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.send_message(window, cx);
                }
            },
        ));
        subscriptions.push(cx.observe_in(&store, window, |this, _, window, cx| {
            this.sync_input_state(window, cx);
            this.sync_display(cx);
        }));

        let mut panel = Self {
            store,
            scroller,
            input,
            filter: ChatFilter::All,
            displayed: Rc::new(Vec::new()),
            _subscriptions: subscriptions,
        };
        panel.sync_input_state(window, cx);
        panel
    }

    fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().trim().to_string();
        if value.is_empty() {
            return;
        }
        {
            let store = self.store.read(cx);
            if !store.is_connected() || !store.config.user.chat_input_enabled {
                return;
            }
        }
        self.store.read(cx).send_chat(value);
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn set_filter(&mut self, ix: usize, cx: &mut Context<Self>) {
        let filter = ChatFilter::from_index(ix);
        if filter != self.filter {
            self.filter = filter;
            self.sync_display(cx);
        }
    }

    fn sync_input_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (connected, chat_input_enabled) = {
            let store = self.store.read(cx);
            (store.is_connected(), store.config.user.chat_input_enabled)
        };
        let placeholder: SharedString = if !connected {
            "Not connected".into()
        } else if chat_input_enabled {
            "Type a message... (or /help for commands)".into()
        } else {
            "Chat input disabled".into()
        };
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
    }

    fn sync_display(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter;
        let store = self.store.read(cx);
        let visible = store.messages.iter().filter(|message| match filter {
            ChatFilter::All => true,
            ChatFilter::Chat => message.message_type != "system",
            ChatFilter::Events => message.message_type == "system",
        });
        let next = collapse_messages(visible);

        let old_len = self.displayed.len();
        let prefix = self
            .displayed
            .iter()
            .zip(next.iter())
            .take_while(|(old, new)| old == new)
            .count();
        if prefix == old_len && prefix == next.len() {
            return;
        }

        let removed = old_len - prefix;
        let added = next.len() - prefix;
        self.displayed = Rc::new(next);
        self.scroller.update(cx, |scroller, cx| {
            scroller.splice(prefix..prefix + removed, added, cx);
        });
        cx.notify();
    }

    fn empty_hint(&self, cx: &App) -> SharedString {
        let connected = self.store.read(cx).is_connected();
        if !connected {
            return "Welcome to Syncplay! Connect to a server to get started.".into();
        }
        match self.filter {
            ChatFilter::All => "No messages yet. Start chatting!".into(),
            ChatFilter::Chat => "No chat messages yet. Start chatting!".into(),
            ChatFilter::Events => "No sync events yet.".into(),
        }
    }
}

impl Render for ChatPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (connected, chat_input_enabled) = {
            let store = self.store.read(cx);
            (store.is_connected(), store.config.user.chat_input_enabled)
        };
        let input_enabled = connected && chat_input_enabled;

        // Tighter than the scroller's default message rhythm: chat lines
        // stack at 2px like the web client's space-y-0.5.
        let mut row_style = StyleRefinement::default();
        row_style.padding.bottom = Some(px(2.).into());

        let displayed = self.displayed.clone();
        let messages_view: AnyElement = if displayed.is_empty() {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    Icon::new(IconName::MessageSquare)
                        .small()
                        .text_color(cx.theme().muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.empty_hint(cx)),
                )
                .into_any_element()
        } else {
            MessageScroller::new("chat-messages", self.scroller.clone(), move |ix, _, cx| {
                render_message_row(&displayed[ix], cx)
            })
            .with_jump_button_label("New messages")
            .with_jump_button_renderer(|button| button.label("New messages"))
            .with_row_style(row_style)
            .into_any_element()
        };

        v_flex()
            .size_full()
            .child(
                div()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .px_5()
                    .pt_2()
                    .child(
                        TabBar::new("chat-filter")
                            .underline()
                            .selected_index(self.filter.index())
                            .children([
                                Tab::new().label("All"),
                                Tab::new().label("Chat"),
                                Tab::new().label("Events"),
                            ])
                            .on_click(cx.listener(|this, ix, _, cx| this.set_filter(*ix, cx))),
                    ),
            )
            .child(div().flex_1().min_h_0().px_5().pt_4().child(messages_view))
            .child(
                div()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .p_4()
                    .child(Input::new(&self.input).disabled(!input_enabled)),
            )
    }
}

/// Sync events look like "haruru paused at 17:38" / "haruru unpaused" /
/// "haruru jumped from 17:47 to 17:38". Runs of the same actor + action
/// collapse into one line with a xN badge.
fn sync_event_key(message: &str) -> Option<String> {
    let mut parts = message.split_whitespace();
    let actor = parts.next()?;
    let action = parts.next()?.to_lowercase();
    let matched = ["paused", "unpaused", "jumped"].into_iter().find(|word| {
        action.starts_with(word)
            && action[word.len()..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    })?;
    Some(format!("{}:{matched}", actor.to_lowercase()))
}

fn collapse_key_for(message: &ChatMessageEvent) -> Option<String> {
    if message.message_type != "system" {
        return None;
    }
    sync_event_key(&message.message)
}

fn collapse_messages<'a>(
    messages: impl Iterator<Item = &'a ChatMessageEvent>,
) -> Vec<DisplayMessage> {
    let mut result: Vec<DisplayMessage> = Vec::new();
    for message in messages {
        let collapse_key = collapse_key_for(message);
        if let Some(last) = result.last_mut() {
            if collapse_key.is_some() && last.collapse_key == collapse_key {
                last.message = message.message.trim().into();
                last.timestamp = message.timestamp.clone().into();
                last.count += 1;
                continue;
            }
        }
        result.push(DisplayMessage {
            collapse_key,
            timestamp: message.timestamp.clone().into(),
            username: message.username.clone().map(Into::into),
            message: message.message.trim().into(),
            message_type: message.message_type.clone().into(),
            count: 1,
        });
    }
    result
}

fn format_timestamp(timestamp: &str) -> SharedString {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|parsed| {
            parsed
                .with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
                .into()
        })
        .unwrap_or_else(|_| timestamp.into())
}

fn render_message_row(message: &DisplayMessage, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let is_jumped = message
        .collapse_key
        .as_deref()
        .is_some_and(|key| key.ends_with(":jumped"));
    let is_system = message.message_type == "system";
    let is_error = message.message_type == "error";

    let mut line = h_flex()
        .w_full()
        .gap_2()
        .when(is_jumped, |this| this.opacity(0.55))
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .text_color(theme.muted_foreground)
                .child(format_timestamp(&message.timestamp)),
        )
        .when_some(message.username.clone(), |this, username| {
            this.child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.info)
                    .child(format!("{username}:")),
            )
        });

    let mut body = div().min_w_0();
    if is_system {
        body = body.text_xs().text_color(theme.muted_foreground);
    } else if is_error {
        body = body.text_sm().text_color(theme.danger);
    } else {
        body = body.text_sm();
    }
    line = line.child(body.child(message.message.clone()));

    if message.count > 1 {
        line = line.child(
            div()
                .flex_shrink_0()
                .text_xs()
                .px_1()
                .rounded_full()
                .text_color(theme.muted_foreground)
                .bg(theme.muted)
                .child(format!("×{}", message.count)),
        );
    }
    line.into_any_element()
}
