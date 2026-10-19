// Shared state logic for the player picker used by both the settings dialog
// and the first-run wizard: the echo-sensitive select/input sync must behave
// identically in both, so it lives here exactly once.

use gpui_kit::component::input::InputState;
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::{IndexPath, WindowExt as _};
use gpui_kit::{App, Entity, Window};
use rust_i18n::t;
use syncplay_core::config::settings::PlayerConfig;
use syncplay_core::player::detection::{resolve_player_executable, DetectedPlayer};

use crate::i18n;
use crate::store::AppStore;

const CUSTOM_PATH_KEY: &str = "settings.custom_path";

pub(crate) fn custom_path_label() -> String {
    i18n::tr(CUSTOM_PATH_KEY).to_string()
}

pub(crate) fn player_label(player: &DetectedPlayer) -> String {
    match &player.version {
        Some(version) => format!("{} ({}) - {}", player.name, version, player.path),
        None => format!("{} - {}", player.name, player.path),
    }
}

pub(crate) fn update_player_config(
    store: &Entity<AppStore>,
    cx: &mut App,
    edit: impl FnOnce(&mut PlayerConfig),
) {
    let mut config = store.read(cx).config.clone();
    edit(&mut config.player);
    store.read(cx).update_config(config);
}

pub(crate) fn refresh_players(store: &Entity<AppStore>, cx: &mut App) {
    store.update(cx, |store, cx| {
        store.load_cached_players(cx);
        store.refresh_players(cx);
    });
}

pub(crate) fn sync_player_select(
    store: &Entity<AppStore>,
    select: &Entity<SelectState<Vec<String>>>,
    window: &mut Window,
    cx: &mut App,
) {
    let (player_path, players) = {
        let store = store.read(cx);
        (
            store.config.player.player_path.clone(),
            store.detected_players.clone(),
        )
    };

    let mut items: Vec<String> = players.iter().map(player_label).collect();
    items.push(custom_path_label());

    let selected = if player_path == "custom" {
        Some(IndexPath::new(items.len() - 1))
    } else {
        players
            .iter()
            .position(|player| player.path == player_path)
            .map(IndexPath::new)
    };
    select.update(cx, |select, cx| {
        select.set_items(items, window, cx);
        select.set_selected_index(selected, window, cx);
    });
}

pub(crate) fn on_player_selected(
    store: &Entity<AppStore>,
    path_input: &Entity<InputState>,
    label: String,
    window: &mut Window,
    cx: &mut App,
) {
    if label == custom_path_label() {
        path_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        update_player_config(store, cx, |player| {
            player.player_path = "custom".to_string()
        });
    } else if let Some(path) = store
        .read(cx)
        .detected_players
        .iter()
        .find(|player| player_label(player) == label)
        .map(|player| player.path.clone())
    {
        path_input.update(cx, |input, cx| {
            input.set_value(path.clone(), window, cx);
        });
        update_player_config(store, cx, |player| player.player_path = path);
    }
}

pub(crate) fn on_player_executable_drop(
    store: &Entity<AppStore>,
    path_input: &Entity<InputState>,
    paths: &gpui_kit::ExternalPaths,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(path) = paths.paths().first() else {
        return;
    };
    let Some(executable) = resolve_player_executable(path) else {
        window.push_notification(
            (
                NotificationType::Warning,
                t!("settings.player_drop_invalid").to_string(),
            ),
            cx,
        );
        return;
    };
    let value = executable.to_string_lossy().to_string();
    path_input.update(cx, |input, cx| {
        input.set_value(value.clone(), window, cx);
    });
    update_player_config(store, cx, |player| player.player_path = value);
}

/// The manual path field is shown whenever the configured path is not one of
/// the detected players (or none were detected at all).
pub(crate) fn player_path_unmatched(player_path: &str, players: &[DetectedPlayer]) -> bool {
    player_path == "custom" || players.is_empty() || !players.iter().any(|p| p.path == player_path)
}

pub(crate) fn last_checked_label(at: Option<i64>) -> Option<String> {
    at.and_then(|at| {
        chrono::DateTime::from_timestamp_millis(at)
            .map(|at| at.with_timezone(&chrono::Local).format("%H:%M").to_string())
    })
}
