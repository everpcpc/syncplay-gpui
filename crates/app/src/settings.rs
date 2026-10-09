use std::time::Duration;

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::form::{field, v_form};
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, AppContext as _, Context, Entity,
    FontWeight, Hsla, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window,
};
use syncplay_core::config::settings::PlayerConfig;
use syncplay_core::config::settings::{
    ChatInputPosition, ChatOutputMode, PrivacyMode, UnpauseAction,
};
use syncplay_core::config::UserPreferences;
use syncplay_core::player::detection::DetectedPlayer;

use crate::store::{AppStore, StoreEvent};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

const UNPAUSE_OPTIONS: [(&str, UnpauseAction); 4] = [
    ("If already ready", UnpauseAction::IfAlreadyReady),
    ("If others ready", UnpauseAction::IfOthersReady),
    ("If min users ready", UnpauseAction::IfMinUsersReady),
    ("Always", UnpauseAction::Always),
];

const PRIVACY_OPTIONS: [(&str, PrivacyMode); 3] = [
    ("Send raw", PrivacyMode::SendRaw),
    ("Send hashed", PrivacyMode::SendHashed),
    ("Do not send", PrivacyMode::DoNotSend),
];

const CHAT_INPUT_POSITION_OPTIONS: [(&str, ChatInputPosition); 3] = [
    ("Top", ChatInputPosition::Top),
    ("Middle", ChatInputPosition::Middle),
    ("Bottom", ChatInputPosition::Bottom),
];

const CHAT_OUTPUT_MODE_OPTIONS: [(&str, ChatOutputMode); 2] = [
    ("Chatroom", ChatOutputMode::Chatroom),
    ("Scrolling", ChatOutputMode::Scrolling),
];

const UPDATE_CHECK_OPTIONS: [(&str, Option<bool>); 3] = [
    ("Use default", None),
    ("Enabled", Some(true)),
    ("Disabled", Some(false)),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    Player = 0,
    Sync,
    Readiness,
    Privacy,
    Chat,
    Osd,
    Misc,
}

impl SettingsTab {
    fn from_index(ix: usize) -> Self {
        match ix {
            1 => Self::Sync,
            2 => Self::Readiness,
            3 => Self::Privacy,
            4 => Self::Chat,
            5 => Self::Osd,
            6 => Self::Misc,
            _ => Self::Player,
        }
    }
}

#[derive(Clone, Copy)]
enum SaveStatus {
    Idle,
    Saving,
    Saved,
    Failed,
}

pub struct SettingsDialog {
    store: Entity<AppStore>,
    active_tab: SettingsTab,
    // Edits accumulate in this draft; the store stays untouched until the
    // debounce fires, so a store reflow never rewrites a half-typed value.
    draft_user: UserPreferences,
    dirty: bool,
    save_status: SaveStatus,
    save_error: Option<SharedString>,
    save_generation: u64,
    // Set after dispatching a save. Store notifies between dispatch and the
    // config-updated echo still carry the pre-save config; syncing the form
    // from those would briefly revert the user's just-saved values.
    awaiting_echo: bool,

    // Sync
    seek_threshold_rewind: Entity<InputState>,
    seek_threshold_fastforward: Entity<InputState>,
    slowdown_threshold: Entity<InputState>,
    slowdown_reset_threshold: Entity<InputState>,
    slowdown_rate: Entity<InputState>,

    // Readiness
    unpause_action: Entity<SelectState<Vec<String>>>,
    autoplay_min_users: Entity<InputState>,

    // Privacy
    filename_privacy_mode: Entity<SelectState<Vec<String>>>,
    filesize_privacy_mode: Entity<SelectState<Vec<String>>>,

    // Chat
    chat_input_position: Entity<SelectState<Vec<String>>>,
    chat_input_font_family: Entity<InputState>,
    chat_input_relative_font_size: Entity<InputState>,
    chat_input_font_weight: Entity<InputState>,
    chat_input_font_color: Entity<InputState>,
    chat_output_mode: Entity<SelectState<Vec<String>>>,
    chat_output_font_family: Entity<InputState>,
    chat_output_relative_font_size: Entity<InputState>,
    chat_output_font_weight: Entity<InputState>,
    chat_max_lines: Entity<InputState>,
    chat_top_margin: Entity<InputState>,
    chat_left_margin: Entity<InputState>,
    chat_bottom_margin: Entity<InputState>,
    chat_osd_margin: Entity<InputState>,
    notification_timeout: Entity<InputState>,
    alert_timeout: Entity<InputState>,
    chat_timeout: Entity<InputState>,

    // OSD
    osd_duration: Entity<InputState>,

    // Misc
    check_for_updates: Entity<SelectState<Vec<String>>>,

    // Player; saved immediately like the old connection dialog did, not via
    // the debounced user-preferences draft.
    player_select: Entity<SelectState<Vec<String>>>,
    player_path: Entity<InputState>,
    player_args: Entity<InputState>,

    _subscriptions: Vec<Subscription>,
}

impl SettingsDialog {
    fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let user = store.read(cx).config.user.clone();

        let mut subscriptions = vec![cx.observe_in(&store, window, |this, store, window, cx| {
            this.on_store_changed(store, window, cx)
        })];

        let (seek_threshold_rewind, sub) = Self::number_field(
            user.seek_threshold_rewind,
            0.1,
            None,
            window,
            cx,
            |user, value| user.seek_threshold_rewind = value,
        );
        subscriptions.push(sub);
        let (seek_threshold_fastforward, sub) = Self::number_field(
            user.seek_threshold_fastforward,
            0.1,
            None,
            window,
            cx,
            |user, value| user.seek_threshold_fastforward = value,
        );
        subscriptions.push(sub);
        let (slowdown_threshold, sub) = Self::number_field(
            user.slowdown_threshold,
            0.1,
            None,
            window,
            cx,
            |user, value| user.slowdown_threshold = value,
        );
        subscriptions.push(sub);
        let (slowdown_reset_threshold, sub) = Self::number_field(
            user.slowdown_reset_threshold,
            0.1,
            None,
            window,
            cx,
            |user, value| user.slowdown_reset_threshold = value,
        );
        subscriptions.push(sub);
        let (slowdown_rate, sub) = Self::number_field(
            user.slowdown_rate,
            0.01,
            Some((0., 1.)),
            window,
            cx,
            |user, value| user.slowdown_rate = value,
        );
        subscriptions.push(sub);

        let (autoplay_min_users, sub) = Self::number_field(
            user.autoplay_min_users,
            1.,
            None,
            window,
            cx,
            |user, value| user.autoplay_min_users = value,
        );
        subscriptions.push(sub);
        let (unpause_action, sub) = Self::select_field(
            &UNPAUSE_OPTIONS,
            &user.unpause_action,
            |user, value| user.unpause_action = value,
            window,
            cx,
        );
        subscriptions.push(sub);

        let (filename_privacy_mode, sub) = Self::select_field(
            &PRIVACY_OPTIONS,
            &user.filename_privacy_mode,
            |user, value| user.filename_privacy_mode = value,
            window,
            cx,
        );
        subscriptions.push(sub);
        let (filesize_privacy_mode, sub) = Self::select_field(
            &PRIVACY_OPTIONS,
            &user.filesize_privacy_mode,
            |user, value| user.filesize_privacy_mode = value,
            window,
            cx,
        );
        subscriptions.push(sub);

        let (chat_input_position, sub) = Self::select_field(
            &CHAT_INPUT_POSITION_OPTIONS,
            &user.chat_input_position,
            |user, value| user.chat_input_position = value,
            window,
            cx,
        );
        subscriptions.push(sub);
        let (chat_input_font_family, sub) =
            Self::text_field(&user.chat_input_font_family, window, cx, |user, value| {
                user.chat_input_font_family = value
            });
        subscriptions.push(sub);
        let (chat_input_relative_font_size, sub) = Self::number_field(
            user.chat_input_relative_font_size,
            1.,
            None,
            window,
            cx,
            |user, value| user.chat_input_relative_font_size = value,
        );
        subscriptions.push(sub);
        let (chat_input_font_weight, sub) = Self::number_field(
            user.chat_input_font_weight,
            1.,
            None,
            window,
            cx,
            |user, value| user.chat_input_font_weight = value,
        );
        subscriptions.push(sub);
        let (chat_input_font_color, sub) =
            Self::text_field(&user.chat_input_font_color, window, cx, |user, value| {
                user.chat_input_font_color = value
            });
        subscriptions.push(sub);
        let (chat_output_mode, sub) = Self::select_field(
            &CHAT_OUTPUT_MODE_OPTIONS,
            &user.chat_output_mode,
            |user, value| user.chat_output_mode = value,
            window,
            cx,
        );
        subscriptions.push(sub);
        let (chat_output_font_family, sub) =
            Self::text_field(&user.chat_output_font_family, window, cx, |user, value| {
                user.chat_output_font_family = value
            });
        subscriptions.push(sub);
        let (chat_output_relative_font_size, sub) = Self::number_field(
            user.chat_output_relative_font_size,
            1.,
            None,
            window,
            cx,
            |user, value| user.chat_output_relative_font_size = value,
        );
        subscriptions.push(sub);
        let (chat_output_font_weight, sub) = Self::number_field(
            user.chat_output_font_weight,
            1.,
            None,
            window,
            cx,
            |user, value| user.chat_output_font_weight = value,
        );
        subscriptions.push(sub);
        let (chat_max_lines, sub) =
            Self::number_field(user.chat_max_lines, 1., None, window, cx, |user, value| {
                user.chat_max_lines = value
            });
        subscriptions.push(sub);
        let (chat_top_margin, sub) =
            Self::number_field(user.chat_top_margin, 1., None, window, cx, |user, value| {
                user.chat_top_margin = value
            });
        subscriptions.push(sub);
        let (chat_left_margin, sub) = Self::number_field(
            user.chat_left_margin,
            1.,
            None,
            window,
            cx,
            |user, value| user.chat_left_margin = value,
        );
        subscriptions.push(sub);
        let (chat_bottom_margin, sub) = Self::number_field(
            user.chat_bottom_margin,
            1.,
            None,
            window,
            cx,
            |user, value| user.chat_bottom_margin = value,
        );
        subscriptions.push(sub);
        let (chat_osd_margin, sub) =
            Self::number_field(user.chat_osd_margin, 1., None, window, cx, |user, value| {
                user.chat_osd_margin = value
            });
        subscriptions.push(sub);
        let (notification_timeout, sub) = Self::number_field(
            user.notification_timeout,
            1.,
            None,
            window,
            cx,
            |user, value| user.notification_timeout = value,
        );
        subscriptions.push(sub);
        let (alert_timeout, sub) =
            Self::number_field(user.alert_timeout, 1., None, window, cx, |user, value| {
                user.alert_timeout = value
            });
        subscriptions.push(sub);
        let (chat_timeout, sub) =
            Self::number_field(user.chat_timeout, 1., None, window, cx, |user, value| {
                user.chat_timeout = value
            });
        subscriptions.push(sub);

        let (osd_duration, sub) =
            Self::number_field(user.osd_duration, 1., None, window, cx, |user, value| {
                user.osd_duration = value
            });
        subscriptions.push(sub);

        let (check_for_updates, sub) = Self::select_field(
            &UPDATE_CHECK_OPTIONS,
            &user.check_for_updates_automatically,
            |user, value| user.check_for_updates_automatically = value,
            window,
            cx,
        );
        subscriptions.push(sub);

        let player_path_value = store.read(cx).config.player.player_path.clone();
        let player_args_value = store.read(cx).config.player.player_arguments.join(" ");
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
                    this.update_player_config(cx, |player| player.player_path = path);
                }
            },
        ));
        let player_args = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("--fullscreen --no-border");
            input.set_value(player_args_value, window, cx);
            input
        });
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
        let player_select = cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx));
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
            &store,
            window,
            |this, _, event: &StoreEvent, window, cx| {
                if matches!(event, StoreEvent::PlayersDetected) {
                    this.sync_player_select(window, cx);
                }
            },
        ));

        let mut this = Self {
            store,
            active_tab: SettingsTab::Sync,
            draft_user: user,
            dirty: false,
            save_status: SaveStatus::Idle,
            save_error: None,
            save_generation: 0,
            awaiting_echo: false,
            seek_threshold_rewind,
            seek_threshold_fastforward,
            slowdown_threshold,
            slowdown_reset_threshold,
            slowdown_rate,
            unpause_action,
            autoplay_min_users,
            filename_privacy_mode,
            filesize_privacy_mode,
            chat_input_position,
            chat_input_font_family,
            chat_input_relative_font_size,
            chat_input_font_weight,
            chat_input_font_color,
            chat_output_mode,
            chat_output_font_family,
            chat_output_relative_font_size,
            chat_output_font_weight,
            chat_max_lines,
            chat_top_margin,
            chat_left_margin,
            chat_bottom_margin,
            chat_osd_margin,
            notification_timeout,
            alert_timeout,
            chat_timeout,
            osd_duration,
            check_for_updates,
            player_select,
            player_path,
            player_args,
            _subscriptions: subscriptions,
        };
        this.sync_player_select(window, cx);
        this
    }

    fn number_field<T>(
        initial: T,
        step: f64,
        bounds: Option<(f64, f64)>,
        window: &mut Window,
        cx: &mut Context<Self>,
        apply: fn(&mut UserPreferences, T),
    ) -> (Entity<InputState>, Subscription)
    where
        T: std::str::FromStr + ToString + PartialEq + 'static,
    {
        let state = cx.new(|cx| {
            let mut input = InputState::new(window, cx).step(step);
            if let Some((min, max)) = bounds {
                input = input.min(min).max(max);
            }
            input.set_value(initial.to_string(), window, cx);
            input
        });
        let subscription = cx.subscribe_in(
            &state,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = input.read(cx).value();
                if let Ok(value) = text.trim().parse::<T>() {
                    this.edit_user_draft(window, cx, |user| apply(user, value));
                }
            },
        );
        (state, subscription)
    }

    fn text_field(
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        apply: fn(&mut UserPreferences, String),
    ) -> (Entity<InputState>, Subscription) {
        let state = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(initial.to_string(), window, cx);
            input
        });
        let subscription = cx.subscribe_in(
            &state,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let value = input.read(cx).value().to_string();
                    this.edit_user_draft(window, cx, |user| apply(user, value));
                }
            },
        );
        (state, subscription)
    }

    fn select_field<T>(
        options: &'static [(&'static str, T)],
        selected: &T,
        apply: fn(&mut UserPreferences, T),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<SelectState<Vec<String>>>, Subscription)
    where
        T: PartialEq + Clone + 'static,
    {
        let labels: Vec<String> = options
            .iter()
            .map(|(label, _)| (*label).to_string())
            .collect();
        let selected_ix = options
            .iter()
            .position(|(_, value)| value == selected)
            .map(IndexPath::new);
        let state = cx.new(|cx| SelectState::new(labels, selected_ix, window, cx));
        let subscription = cx.subscribe_in(
            &state,
            window,
            move |this, _, event: &SelectEvent<Vec<String>>, window, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                if let Some((_, value)) = options.iter().find(|(text, _)| *text == label.as_str()) {
                    let value = value.clone();
                    this.edit_user_draft(window, cx, |user| apply(user, value));
                }
            },
        );
        (state, subscription)
    }

    fn checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        apply: fn(&mut UserPreferences, bool),
        cx: &mut Context<Self>,
    ) -> Checkbox {
        Checkbox::new(id)
            .label(label)
            .checked(checked)
            .on_click(cx.listener(move |this, checked, window, cx| {
                this.edit_user_draft(window, cx, |user| apply(user, *checked));
            }))
    }

    fn update_player_config(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut PlayerConfig)) {
        let mut config = self.store.read(cx).config.clone();
        edit(&mut config.player);
        self.store.read(cx).update_config(config);
    }

    fn refresh_players(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            store.load_cached_players(cx);
            store.refresh_players(cx);
        });
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

    fn edit_user_draft(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut UserPreferences),
    ) {
        edit(&mut self.draft_user);
        self.dirty = true;
        self.save_status = SaveStatus::Saving;
        self.save_error = None;
        self.save_generation += 1;
        // The generation discards timers superseded by a newer edit.
        let generation = self.save_generation;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            this.update_in(cx, |this, _, cx| this.flush_save(generation, cx))
                .ok();
        })
        .detach();
    }

    fn flush_save(&mut self, generation: u64, cx: &mut Context<Self>) {
        if generation != self.save_generation {
            return;
        }
        let mut config = self.store.read(cx).config.clone();
        let mut user = self.draft_user.clone();
        // Owned by the header, not this dialog: keep the live values so a
        // save does not revert a toggle made while edits were pending.
        user.theme = config.user.theme.clone();
        user.transparency_mode = config.user.transparency_mode.clone();
        user.show_playlist = config.user.show_playlist;
        config.user = user;
        // The store's update_config reports failure only as a notification,
        // so validation is replayed locally to give the dialog a real
        // failure state.
        if let Err(error) = config.validate() {
            self.save_status = SaveStatus::Failed;
            self.save_error = Some(error.into());
            cx.notify();
            return;
        }
        self.dirty = false;
        self.draft_user = config.user.clone();
        self.awaiting_echo = true;
        self.store.read(cx).update_config(config);
        self.save_status = SaveStatus::Saved;
        cx.notify();
    }

    fn on_store_changed(
        &mut self,
        store: Entity<AppStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let user_json = serde_json::to_value(&store.read(cx).config.user).ok();
        let draft_json = serde_json::to_value(&self.draft_user).ok();
        if self.awaiting_echo {
            if user_json == draft_json {
                self.awaiting_echo = false;
            }
            return;
        }
        if self.dirty || user_json == draft_json {
            return;
        }
        self.draft_user = store.read(cx).config.user.clone();
        self.sync_form(window, cx);
        cx.notify();
    }

    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let user = &self.draft_user;
        sync_number_input(
            &self.seek_threshold_rewind,
            &user.seek_threshold_rewind,
            window,
            cx,
        );
        sync_number_input(
            &self.seek_threshold_fastforward,
            &user.seek_threshold_fastforward,
            window,
            cx,
        );
        sync_number_input(
            &self.slowdown_threshold,
            &user.slowdown_threshold,
            window,
            cx,
        );
        sync_number_input(
            &self.slowdown_reset_threshold,
            &user.slowdown_reset_threshold,
            window,
            cx,
        );
        sync_number_input(&self.slowdown_rate, &user.slowdown_rate, window, cx);
        sync_number_input(
            &self.autoplay_min_users,
            &user.autoplay_min_users,
            window,
            cx,
        );
        sync_select(
            &self.unpause_action,
            &UNPAUSE_OPTIONS,
            &user.unpause_action,
            window,
            cx,
        );
        sync_select(
            &self.filename_privacy_mode,
            &PRIVACY_OPTIONS,
            &user.filename_privacy_mode,
            window,
            cx,
        );
        sync_select(
            &self.filesize_privacy_mode,
            &PRIVACY_OPTIONS,
            &user.filesize_privacy_mode,
            window,
            cx,
        );
        sync_select(
            &self.chat_input_position,
            &CHAT_INPUT_POSITION_OPTIONS,
            &user.chat_input_position,
            window,
            cx,
        );
        sync_text_input(
            &self.chat_input_font_family,
            &user.chat_input_font_family,
            window,
            cx,
        );
        sync_number_input(
            &self.chat_input_relative_font_size,
            &user.chat_input_relative_font_size,
            window,
            cx,
        );
        sync_number_input(
            &self.chat_input_font_weight,
            &user.chat_input_font_weight,
            window,
            cx,
        );
        sync_text_input(
            &self.chat_input_font_color,
            &user.chat_input_font_color,
            window,
            cx,
        );
        sync_select(
            &self.chat_output_mode,
            &CHAT_OUTPUT_MODE_OPTIONS,
            &user.chat_output_mode,
            window,
            cx,
        );
        sync_text_input(
            &self.chat_output_font_family,
            &user.chat_output_font_family,
            window,
            cx,
        );
        sync_number_input(
            &self.chat_output_relative_font_size,
            &user.chat_output_relative_font_size,
            window,
            cx,
        );
        sync_number_input(
            &self.chat_output_font_weight,
            &user.chat_output_font_weight,
            window,
            cx,
        );
        sync_number_input(&self.chat_max_lines, &user.chat_max_lines, window, cx);
        sync_number_input(&self.chat_top_margin, &user.chat_top_margin, window, cx);
        sync_number_input(&self.chat_left_margin, &user.chat_left_margin, window, cx);
        sync_number_input(
            &self.chat_bottom_margin,
            &user.chat_bottom_margin,
            window,
            cx,
        );
        sync_number_input(&self.chat_osd_margin, &user.chat_osd_margin, window, cx);
        sync_number_input(
            &self.notification_timeout,
            &user.notification_timeout,
            window,
            cx,
        );
        sync_number_input(&self.alert_timeout, &user.alert_timeout, window, cx);
        sync_number_input(&self.chat_timeout, &user.chat_timeout, window, cx);
        sync_number_input(&self.osd_duration, &user.osd_duration, window, cx);
        sync_select(
            &self.check_for_updates,
            &UPDATE_CHECK_OPTIONS,
            &user.check_for_updates_automatically,
            window,
            cx,
        );
    }

    fn render_save_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let status: Option<(&'static str, Hsla)> = match self.save_status {
            SaveStatus::Idle => None,
            SaveStatus::Saving => Some(("Saving...", muted)),
            SaveStatus::Saved => Some(("Saved.", cx.theme().success)),
            SaveStatus::Failed => Some(("Save failed.", cx.theme().danger)),
        };
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Changes are saved automatically."),
                    )
                    .when_some(status, |this, (text, color)| {
                        this.child(div().text_xs().text_color(color).child(text))
                    }),
            )
            .when_some(self.save_error.clone(), |this, error| {
                this.child(div().text_xs().text_color(cx.theme().danger).child(error))
            })
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
                                    .font_weight(FontWeight::MEDIUM)
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
                                                this.refresh_players(cx);
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

    fn render_sync_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                v_form()
                    .child(
                        field()
                            .label("Seek Threshold Rewind (seconds)")
                            .child(NumberInput::new(&self.seek_threshold_rewind)),
                    )
                    .child(
                        field()
                            .label("Seek Threshold Fastforward (seconds)")
                            .child(NumberInput::new(&self.seek_threshold_fastforward)),
                    )
                    .child(
                        field()
                            .label("Slowdown Threshold (seconds)")
                            .child(NumberInput::new(&self.slowdown_threshold)),
                    )
                    .child(
                        field()
                            .label("Slowdown Reset Threshold (seconds)")
                            .child(NumberInput::new(&self.slowdown_reset_threshold)),
                    )
                    .child(
                        field()
                            .label("Slowdown Rate (0-1)")
                            .child(NumberInput::new(&self.slowdown_rate)),
                    ),
            )
            .child(
                v_form().child(
                    field().label_indent(false).child(
                        v_flex()
                            .gap_2()
                            .child(self.checkbox(
                                "slow_on_desync",
                                "Slow down on desync",
                                self.draft_user.slow_on_desync,
                                |user, value| user.slow_on_desync = value,
                                cx,
                            ))
                            .child(self.checkbox(
                                "rewind_on_desync",
                                "Rewind on desync",
                                self.draft_user.rewind_on_desync,
                                |user, value| user.rewind_on_desync = value,
                                cx,
                            ))
                            .child(self.checkbox(
                                "fastforward_on_desync",
                                "Fast-forward on desync",
                                self.draft_user.fastforward_on_desync,
                                |user, value| user.fastforward_on_desync = value,
                                cx,
                            ))
                            .child(self.checkbox(
                                "dont_slow_down_with_me",
                                "Do not slow down with me",
                                self.draft_user.dont_slow_down_with_me,
                                |user, value| user.dont_slow_down_with_me = value,
                                cx,
                            )),
                    ),
                ),
            )
    }

    fn render_readiness_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                v_form().child(
                    field().label_indent(false).child(
                        v_flex()
                            .gap_2()
                            .child(self.checkbox(
                                "ready_at_start",
                                "Ready at startup",
                                self.draft_user.ready_at_start,
                                |user, value| user.ready_at_start = value,
                                cx,
                            ))
                            .child(self.checkbox(
                                "pause_on_leave",
                                "Pause when someone leaves the room",
                                self.draft_user.pause_on_leave,
                                |user, value| user.pause_on_leave = value,
                                cx,
                            )),
                    ),
                ),
            )
            .child(
                v_form().child(
                    field()
                        .label("Unpause behavior")
                        .child(Select::new(&self.unpause_action).w_full()),
                ),
            )
            .child(
                v_form().child(
                    field().label_indent(false).child(
                        v_flex()
                            .gap_2()
                            .child(self.checkbox(
                                "autoplay_enabled",
                                "Enable auto-play when all ready",
                                self.draft_user.autoplay_enabled,
                                |user, value| user.autoplay_enabled = value,
                                cx,
                            ))
                            .child(self.checkbox(
                                "autoplay_require_same_filenames",
                                "Require same filenames for auto-play",
                                self.draft_user.autoplay_require_same_filenames,
                                |user, value| user.autoplay_require_same_filenames = value,
                                cx,
                            )),
                    ),
                ),
            )
            .child(
                v_form().child(
                    field()
                        .label("Auto-play minimum users")
                        .child(NumberInput::new(&self.autoplay_min_users))
                        .description("Use -1 to disable minimum."),
                ),
            )
    }

    fn render_privacy_tab(&mut self, _cx: &mut Context<Self>) -> impl IntoElement {
        v_form()
            .child(
                field()
                    .label("Filename privacy")
                    .child(Select::new(&self.filename_privacy_mode).w_full()),
            )
            .child(
                field()
                    .label("Filesize privacy")
                    .child(Select::new(&self.filesize_privacy_mode).w_full()),
            )
    }

    fn render_chat_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                v_form().child(
                    field().label_indent(false).child(
                        v_flex()
                            .gap_2()
                            .child(self.checkbox(
                                "chat_input_enabled",
                                "Enable chat input",
                                self.draft_user.chat_input_enabled,
                                |user, value| user.chat_input_enabled = value,
                                cx,
                            ))
                            .child(self.checkbox(
                                "chat_direct_input",
                                "Direct input mode",
                                self.draft_user.chat_direct_input,
                                |user, value| user.chat_direct_input = value,
                                cx,
                            )),
                    ),
                ),
            )
            .child(
                v_form().child(
                    field()
                        .label("Chat input position")
                        .child(Select::new(&self.chat_input_position).w_full()),
                ),
            )
            .child(
                v_form()
                    .columns(2)
                    .child(
                        field()
                            .label("Input font family")
                            .child(Input::new(&self.chat_input_font_family)),
                    )
                    .child(
                        field()
                            .label("Input font size")
                            .child(NumberInput::new(&self.chat_input_relative_font_size)),
                    )
                    .child(
                        field()
                            .label("Input font weight")
                            .child(NumberInput::new(&self.chat_input_font_weight)),
                    )
                    .child(
                        field()
                            .label("Input font color")
                            .child(Input::new(&self.chat_input_font_color)),
                    ),
            )
            .child(
                v_form().child(field().label_indent(false).child(self.checkbox(
                    "chat_input_font_underline",
                    "Underline chat input",
                    self.draft_user.chat_input_font_underline,
                    |user, value| user.chat_input_font_underline = value,
                    cx,
                ))),
            )
            .child(
                v_form().child(field().label_indent(false).child(self.checkbox(
                    "chat_output_enabled",
                    "Enable chat output",
                    self.draft_user.chat_output_enabled,
                    |user, value| user.chat_output_enabled = value,
                    cx,
                ))),
            )
            .child(
                v_form().child(
                    field()
                        .label("Chat output mode")
                        .child(Select::new(&self.chat_output_mode).w_full()),
                ),
            )
            .child(
                v_form()
                    .columns(2)
                    .child(
                        field()
                            .label("Output font family")
                            .child(Input::new(&self.chat_output_font_family)),
                    )
                    .child(
                        field()
                            .label("Output font size")
                            .child(NumberInput::new(&self.chat_output_relative_font_size)),
                    )
                    .child(
                        field()
                            .label("Output font weight")
                            .child(NumberInput::new(&self.chat_output_font_weight)),
                    )
                    .child(
                        field()
                            .label("Chat max lines")
                            .child(NumberInput::new(&self.chat_max_lines)),
                    ),
            )
            .child(
                v_form().child(field().label_indent(false).child(self.checkbox(
                    "chat_output_font_underline",
                    "Underline chat output",
                    self.draft_user.chat_output_font_underline,
                    |user, value| user.chat_output_font_underline = value,
                    cx,
                ))),
            )
            .child(
                v_form()
                    .columns(3)
                    .child(
                        field()
                            .label("Top margin")
                            .child(NumberInput::new(&self.chat_top_margin)),
                    )
                    .child(
                        field()
                            .label("Left margin")
                            .child(NumberInput::new(&self.chat_left_margin)),
                    )
                    .child(
                        field()
                            .label("Bottom margin")
                            .child(NumberInput::new(&self.chat_bottom_margin)),
                    ),
            )
            .child(
                v_form().child(field().label_indent(false).child(self.checkbox(
                    "chat_move_osd",
                    "Move OSD for chat",
                    self.draft_user.chat_move_osd,
                    |user, value| user.chat_move_osd = value,
                    cx,
                ))),
            )
            .child(
                v_form().child(
                    field()
                        .label("OSD margin")
                        .child(NumberInput::new(&self.chat_osd_margin)),
                ),
            )
            .child(
                v_form()
                    .columns(3)
                    .child(
                        field()
                            .label("Notification timeout")
                            .child(NumberInput::new(&self.notification_timeout)),
                    )
                    .child(
                        field()
                            .label("Alert timeout")
                            .child(NumberInput::new(&self.alert_timeout)),
                    )
                    .child(
                        field()
                            .label("Chat timeout")
                            .child(NumberInput::new(&self.chat_timeout)),
                    ),
            )
    }

    fn render_osd_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                v_form().child(
                    field()
                        .label("OSD Duration (ms)")
                        .child(NumberInput::new(&self.osd_duration)),
                ),
            )
            .child(
                v_form()
                    .columns(2)
                    .child(field().label_indent(false).child(self.checkbox(
                        "show_osd",
                        "Show OSD",
                        self.draft_user.show_osd,
                        |user, value| user.show_osd = value,
                        cx,
                    )))
                    .child(field().label_indent(false).child(self.checkbox(
                        "show_osd_warnings",
                        "Show OSD warnings",
                        self.draft_user.show_osd_warnings,
                        |user, value| user.show_osd_warnings = value,
                        cx,
                    )))
                    .child(field().label_indent(false).child(self.checkbox(
                        "show_slowdown_osd",
                        "Show slowdown OSD",
                        self.draft_user.show_slowdown_osd,
                        |user, value| user.show_slowdown_osd = value,
                        cx,
                    )))
                    .child(field().label_indent(false).child(self.checkbox(
                        "show_same_room_osd",
                        "Show same room OSD",
                        self.draft_user.show_same_room_osd,
                        |user, value| user.show_same_room_osd = value,
                        cx,
                    )))
                    .child(field().label_indent(false).child(self.checkbox(
                        "show_different_room_osd",
                        "Show different room OSD",
                        self.draft_user.show_different_room_osd,
                        |user, value| user.show_different_room_osd = value,
                        cx,
                    )))
                    .child(field().label_indent(false).child(self.checkbox(
                        "show_non_controller_osd",
                        "Show non-controller OSD",
                        self.draft_user.show_non_controller_osd,
                        |user, value| user.show_non_controller_osd = value,
                        cx,
                    )))
                    .child(field().label_indent(false).child(self.checkbox(
                        "show_duration_notification",
                        "Show duration notification",
                        self.draft_user.show_duration_notification,
                        |user, value| user.show_duration_notification = value,
                        cx,
                    ))),
            )
    }

    fn render_misc_tab(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                v_form().child(field().label_indent(false).child(self.checkbox(
                    "auto-connect",
                    "Auto-connect on startup",
                    self.draft_user.auto_connect,
                    |user, value| user.auto_connect = value,
                    cx,
                ))),
            )
            .child(
                v_form().child(field().label_indent(false).child(self.checkbox(
                    "autosave-joins",
                    "Auto-save joined rooms",
                    self.draft_user.autosave_joins_to_list,
                    |user, value| user.autosave_joins_to_list = value,
                    cx,
                ))),
            )
            .child(
                v_form().child(
                    field()
                        .label("Check for updates automatically")
                        .child(Select::new(&self.check_for_updates).w_full()),
                ),
            )
            .child(
                v_form().child(field().label_indent(false).child(self.checkbox(
                    "debug",
                    "Enable debug logging",
                    self.draft_user.debug,
                    |user, value| user.debug = value,
                    cx,
                ))),
            )
    }
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Roughly the React max-h-[85vh] envelope once the dialog chrome
        // (title, paddings, margins) is added on top.
        let content_height = window.viewport_size().height * 0.75;
        let tab_index = self.active_tab as usize;

        let tabs = TabBar::new("settings-tabs")
            .underline()
            .selected_index(tab_index)
            .children([
                Tab::new().label("Player"),
                Tab::new().label("Sync"),
                Tab::new().label("Readiness"),
                Tab::new().label("Privacy"),
                Tab::new().label("Chat"),
                Tab::new().label("OSD"),
                Tab::new().label("Misc"),
            ])
            .on_click(cx.listener(|this, ix, _, cx| {
                this.active_tab = SettingsTab::from_index(*ix);
                if this.active_tab == SettingsTab::Player {
                    this.refresh_players(cx);
                }
                cx.notify();
            }));

        let content: AnyElement = match self.active_tab {
            SettingsTab::Player => self.render_player_tab(cx).into_any_element(),
            SettingsTab::Sync => self.render_sync_tab(cx).into_any_element(),
            SettingsTab::Readiness => self.render_readiness_tab(cx).into_any_element(),
            SettingsTab::Privacy => self.render_privacy_tab(cx).into_any_element(),
            SettingsTab::Chat => self.render_chat_tab(cx).into_any_element(),
            SettingsTab::Osd => self.render_osd_tab(cx).into_any_element(),
            SettingsTab::Misc => self.render_misc_tab(cx).into_any_element(),
        };

        v_flex()
            .w_full()
            .h(content_height)
            .gap_2()
            .child(self.render_save_status(cx))
            .child(
                div()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(tabs),
            )
            .child(
                v_flex()
                    .pt_2()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .id(("settings-tab-scroll", tab_index))
                    .child(content),
            )
    }
}

fn sync_number_input<T>(input: &Entity<InputState>, value: &T, window: &mut Window, cx: &mut App)
where
    T: std::str::FromStr + ToString + PartialEq,
{
    let current = input.read(cx).value();
    // A parse-equal text (e.g. "4." vs stored 4.0) is left untouched so the
    // caret is not yanked while the field is focused.
    let up_to_date = current
        .trim()
        .parse::<T>()
        .is_ok_and(|parsed| &parsed == value);
    if !up_to_date {
        input.update(cx, |input, cx| {
            input.set_value(value.to_string(), window, cx);
        });
    }
}

fn sync_text_input(input: &Entity<InputState>, value: &str, window: &mut Window, cx: &mut App) {
    if &*input.read(cx).value() != value {
        input.update(cx, |input, cx| {
            input.set_value(value.to_string(), window, cx);
        });
    }
}

fn sync_select<T>(
    select: &Entity<SelectState<Vec<String>>>,
    options: &[(&str, T)],
    value: &T,
    window: &mut Window,
    cx: &mut App,
) where
    T: PartialEq + Clone,
{
    let target = options
        .iter()
        .position(|(_, option)| option == value)
        .map(IndexPath::new);
    if select.read(cx).selected_index(cx) != target {
        select.update(cx, |select, cx| {
            select.set_selected_index(target, window, cx);
        });
    }
}

/// Open the settings dialog. A fresh snapshot of the store config is taken on
/// every open, matching the web client's reload-on-open behavior.
pub fn open_settings_dialog(window: &mut Window, cx: &mut App, store: Entity<AppStore>) {
    open_settings_dialog_at(window, cx, store, SettingsTab::Sync);
}

/// Open the settings dialog pre-selected to a tab; the player variant also
/// kicks off player detection so the picker is warm.
pub(crate) fn open_settings_dialog_at(
    window: &mut Window,
    cx: &mut App,
    store: Entity<AppStore>,
    tab: SettingsTab,
) {
    let view = cx.new(|cx| {
        let mut this = SettingsDialog::new(store, window, cx);
        this.active_tab = tab;
        this
    });
    if tab == SettingsTab::Player {
        view.update(cx, |this, cx| this.refresh_players(cx));
    }
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title("Settings").w(px(896.)).child(view.clone())
    });
}

const CUSTOM_PATH_LABEL: &str = "Custom path...";

fn player_label(player: &DetectedPlayer) -> String {
    match &player.version {
        Some(version) => format!("{} ({}) - {}", player.name, version, player.path),
        None => format!("{} - {}", player.name, player.path),
    }
}
