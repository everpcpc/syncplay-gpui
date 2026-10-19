use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::{field, v_form};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Icon, IndexPath, Sizable as _,
    WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, AppContext as _, Context, Entity,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window,
};
use rust_i18n::t;
use syncplay_core::player::detection::player_path_resolves;

use crate::i18n;
use crate::player_picker;
use crate::store::{AppStore, StoreEvent};

/// First-run setup: language plus a working player. Everything else stays on
/// the connect panel, so the wizard never grows into a second settings page.
pub struct SetupWizard {
    store: Entity<AppStore>,
    language: Entity<SelectState<Vec<String>>>,
    player_select: Entity<SelectState<Vec<String>>>,
    player_path: Entity<InputState>,
    // Locale the player select items were last built with; the custom-path
    // entry is translated, so a language switch rebuilds the items.
    applied_language: String,
    _subscriptions: Vec<Subscription>,
}

impl SetupWizard {
    fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let user_language = store.read(cx).config.user.language.clone();
        let player_path_value = store.read(cx).config.player.player_path.clone();

        let language_labels: Vec<String> = i18n::LANGUAGES
            .iter()
            .map(|(_, name)| (*name).to_string())
            .collect();
        let language_selected = i18n::LANGUAGES
            .iter()
            .position(|(code, _)| *code == user_language)
            .map(IndexPath::new);
        let language =
            cx.new(|cx| SelectState::new(language_labels, language_selected, window, cx));
        let mut subscriptions = vec![cx.subscribe_in(
            &language,
            window,
            |this, select, event: &SelectEvent<Vec<String>>, _, cx| {
                if !matches!(event, SelectEvent::Confirm(Some(_))) {
                    return;
                }
                if let Some(ix) = select.read(cx).selected_index(cx) {
                    let code = i18n::LANGUAGES[ix.row].0.to_string();
                    let mut config = this.store.read(cx).config.clone();
                    config.user.language = code;
                    this.store.read(cx).update_config(config);
                }
            },
        )];

        let player_select = cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx));
        subscriptions.push(cx.subscribe_in(
            &player_select,
            window,
            |this, _, event: &SelectEvent<Vec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    player_picker::on_player_selected(
                        &this.store,
                        &this.player_path,
                        value.clone(),
                        window,
                        cx,
                    );
                }
            },
        ));

        let player_path = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("/usr/local/bin/mpv");
            input.set_value(
                if player_path_value == "custom" {
                    String::new()
                } else {
                    player_path_value
                },
                window,
                cx,
            );
            input
        });
        subscriptions.push(cx.subscribe_in(
            &player_path,
            window,
            |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let path = input.read(cx).value().to_string();
                    player_picker::update_player_config(&this.store, cx, |player| {
                        player.player_path = path
                    });
                }
            },
        ));

        subscriptions.push(cx.subscribe_in(
            &store,
            window,
            |this, _, event: &StoreEvent, window, cx| {
                if matches!(
                    event,
                    StoreEvent::PlayersDetected | StoreEvent::ConfigUpdated
                ) {
                    player_picker::sync_player_select(&this.store, &this.player_select, window, cx);
                }
                cx.notify();
            },
        ));

        let this = Self {
            store,
            language,
            player_select,
            player_path,
            applied_language: rust_i18n::locale().to_string(),
            _subscriptions: subscriptions,
        };
        player_picker::sync_player_select(&this.store, &this.player_select, window, cx);
        this
    }
}

impl Render for SetupWizard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = rust_i18n::locale().to_string();
        if locale != self.applied_language {
            self.applied_language = locale;
            player_picker::sync_player_select(&self.store, &self.player_select, window, cx);
        }

        let (players, players_refreshing, players_checked_at, player_path) = {
            let store = self.store.read(cx);
            (
                store.detected_players.clone(),
                store.players_refreshing,
                store.detected_players_at,
                store.config.player.player_path.clone(),
            )
        };
        let trimmed = player_path.trim();
        let player_missing =
            trimmed.is_empty() || trimmed == "custom" || !player_path_resolves(trimmed);
        let show_path_input = player_picker::player_path_unmatched(&player_path, &players);

        let checked_label = player_picker::last_checked_label(players_checked_at);
        let picker: AnyElement = if players.is_empty() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t!("settings.no_players"))
                .into_any_element()
        } else {
            Select::new(&self.player_select)
                .w_full()
                .placeholder(t!("settings.select_player"))
                .into_any_element()
        };
        let status: AnyElement = if player_missing {
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .xsmall()
                                .text_color(cx.theme().warning),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().warning)
                                .child(t!("setup.player_missing")),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("setup.install_hint")),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Icon::new(IconName::CircleCheck)
                        .xsmall()
                        .text_color(cx.theme().success),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().success)
                        .child(t!("setup.player_ready")),
                )
                .into_any_element()
        };

        div()
            .id("setup-wizard-drop")
            .on_drop(cx.listener(|this, paths, window, cx| {
                player_picker::on_player_executable_drop(
                    &this.store,
                    &this.player_path,
                    paths,
                    window,
                    cx,
                );
            }))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("setup.subtitle")),
                    )
                    .child(
                        v_form().child(
                            field()
                                .label(t!("settings.language").to_string())
                                .child(Select::new(&self.language).w_full()),
                        ),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(t!("settings.media_player")),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .when_some(checked_label, |this, label| {
                                                this.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(t!(
                                                            "settings.last_checked",
                                                            time = label
                                                        )),
                                                )
                                            })
                                            .child(
                                                Button::new("refresh-players")
                                                    .outline()
                                                    .xsmall()
                                                    .label(if players_refreshing {
                                                        t!("settings.refreshing")
                                                    } else {
                                                        t!("settings.refresh_players")
                                                    })
                                                    .disabled(players_refreshing)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        player_picker::refresh_players(
                                                            &this.store,
                                                            cx,
                                                        );
                                                    })),
                                            ),
                                    ),
                            )
                            .child(picker)
                            .when(show_path_input, |this| {
                                this.child(
                                    v_form().child(
                                        field()
                                            .label(t!("settings.player_path_label").to_string())
                                            .child(Input::new(&self.player_path))
                                            .description(
                                                t!("settings.player_path_desc").to_string(),
                                            ),
                                    ),
                                )
                            })
                            .child(status),
                    ),
            )
    }
}

pub fn open_setup_wizard(window: &mut Window, cx: &mut App, store: Entity<AppStore>) {
    let view = cx.new(|cx| SetupWizard::new(store, window, cx));
    view.update(cx, |this, cx| {
        player_picker::refresh_players(&this.store, cx)
    });
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(t!("setup.title"))
            .w(px(560.))
            .child(view.clone())
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("setup-skip")
                            .ghost()
                            .label(t!("setup.skip"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("setup-done")
                            .primary()
                            .label(t!("setup.get_started"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}
