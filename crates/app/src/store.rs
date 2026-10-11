use std::sync::Arc;

use async_channel::{Receiver, Sender};
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::component::WindowExt as _;
use gpui_kit::{Context, EventEmitter, Task, Window};
use syncplay_core::app_state::AppState;
use syncplay_core::commands;
use syncplay_core::commands::playlist::PlaylistItemInfo;
use syncplay_core::config::SyncplayConfig;
use syncplay_core::player::detection::DetectedPlayer;

use crate::events::{
    ChatMessageEvent, ConnectionStatusEvent, MediaIndexRefreshingEvent, MediaIndexUpdatedEvent,
    PingEvent, PlayerStateEvent, PlaylistEvent, ServerRoomFeaturesEvent, SyncOffsetEvent,
    TlsStatusEvent, UserInfoEvent, UserListEvent,
};

const MAX_CHAT_MESSAGES: usize = 1000;

/// Messages flowing into the UI thread: core events plus command outcomes.
pub enum UiEvent {
    Core {
        name: String,
        payload: serde_json::Value,
    },
    ConnectFinished(Result<String, String>),
    CommandFailed(&'static str, String),
    PlayersDetected {
        players: Vec<DetectedPlayer>,
        updated_at: Option<i64>,
    },
    PlaylistItemsChecked {
        /// True when `targets` enumerated every playlist row at schedule time;
        /// the result then replaces the whole availability vector.
        full_refresh: bool,
        /// Row index plus the item that sat there when the check was
        /// scheduled, so stale answers are dropped instead of misapplied.
        targets: Vec<(usize, String)>,
        results: Vec<PlaylistItemInfo>,
    },
    UpdateCheckFinished {
        manual: bool,
        result: Result<Option<String>, String>,
    },
    UpdateInstallFinished(Result<String, String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    /// Newer release version published on GitHub.
    Available(String),
    /// Download/install of this version is running.
    Installing(String),
    /// Installed; restart to finish.
    Ready(String),
}

/// Emitted on the store entity so views with a window handle can react.
#[derive(Clone)]
pub enum StoreEvent {
    ConnectFinished,
    PlayersDetected,
}

pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub room: String,
    pub password: Option<String>,
}

pub struct AppStore {
    core: Arc<AppState>,
    tokio_handle: tokio::runtime::Handle,
    ui_tx: Sender<UiEvent>,

    pub connection: ConnectionStatusEvent,
    pub tls_status: String,
    pub users: Vec<UserInfoEvent>,
    pub rooms: Vec<String>,
    pub server_room_features: ServerRoomFeaturesEvent,
    pub messages: Vec<ChatMessageEvent>,
    pub playlist: PlaylistEvent,
    /// Availability per playlist row, aligned with `playlist.items`; rows
    /// without data render as available.
    pub playlist_availability: Vec<PlaylistItemInfo>,
    pub player: PlayerStateEvent,
    pub rtt_ms: Option<f64>,
    pub sync_offset_seconds: Option<f64>,
    pub config: SyncplayConfig,
    pub media_index_version: i64,
    pub media_index_refreshing: bool,
    pub detected_players: Vec<DetectedPlayer>,
    pub detected_players_at: Option<i64>,
    pub players_refreshing: bool,
    pub connect_pending: bool,
    pub connect_result: Option<Result<String, String>>,
    pub update_state: UpdateState,

    _event_task: Task<()>,
}

impl EventEmitter<StoreEvent> for AppStore {}

impl AppStore {
    pub fn new(
        core: Arc<AppState>,
        tokio_handle: tokio::runtime::Handle,
        ui_tx: Sender<UiEvent>,
        ui_rx: Receiver<UiEvent>,
        config: SyncplayConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let event_task = cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = ui_rx.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.handle_ui_event(event, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        let media_index_refreshing = core.media_index.is_refreshing();
        Self {
            core,
            tokio_handle,
            ui_tx,
            connection: ConnectionStatusEvent {
                connected: false,
                server: None,
            },
            tls_status: "unknown".to_string(),
            users: Vec::new(),
            rooms: Vec::new(),
            server_room_features: ServerRoomFeaturesEvent::default(),
            messages: Vec::new(),
            playlist: PlaylistEvent::default(),
            playlist_availability: Vec::new(),
            player: PlayerStateEvent::default(),
            rtt_ms: None,
            sync_offset_seconds: None,
            config,
            media_index_version: 0,
            media_index_refreshing,
            detected_players: Vec::new(),
            detected_players_at: None,
            players_refreshing: false,
            connect_pending: false,
            connect_result: None,
            update_state: UpdateState::Idle,
            _event_task: event_task,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connection.connected
    }

    fn handle_ui_event(&mut self, event: UiEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            UiEvent::Core { name, payload } => self.handle_core_event(&name, payload, window, cx),
            UiEvent::ConnectFinished(result) => {
                self.connect_pending = false;
                if let Err(error) = &result {
                    window.push_notification(
                        (
                            NotificationType::Error,
                            format!("Connection failed: {error}"),
                        ),
                        cx,
                    );
                }
                self.connect_result = Some(result);
                cx.emit(StoreEvent::ConnectFinished);
            }
            UiEvent::CommandFailed(action, error) => {
                tracing::error!(action, error, "command failed");
                window.push_notification((NotificationType::Error, error), cx);
            }
            UiEvent::PlayersDetected {
                players,
                updated_at,
            } => {
                self.detected_players = players;
                self.detected_players_at = updated_at;
                self.players_refreshing = false;
                cx.emit(StoreEvent::PlayersDetected);
            }
            UiEvent::PlaylistItemsChecked {
                full_refresh,
                targets,
                results,
            } => {
                self.apply_availability(full_refresh, targets, results);
            }
            UiEvent::UpdateCheckFinished { manual, result } => match result {
                Ok(Some(version)) => {
                    self.update_state = UpdateState::Available(version.clone());
                    window.push_notification(
                        (
                            NotificationType::Info,
                            format!("Update available: v{version}"),
                        ),
                        cx,
                    );
                }
                Ok(None) => {
                    self.update_state = UpdateState::Idle;
                    if manual {
                        window.push_notification(
                            (NotificationType::Success, "Already up to date"),
                            cx,
                        );
                    }
                }
                Err(error) => {
                    self.update_state = UpdateState::Idle;
                    if manual {
                        window.push_notification(
                            (
                                NotificationType::Error,
                                format!("Update check failed: {error}"),
                            ),
                            cx,
                        );
                    } else {
                        tracing::warn!("update check failed: {error}");
                    }
                }
            },
            UiEvent::UpdateInstallFinished(result) => match result {
                Ok(version) => {
                    self.update_state = UpdateState::Ready(version.clone());
                    window.push_notification(
                        (
                            NotificationType::Success,
                            format!("v{version} installed — restart to finish"),
                        ),
                        cx,
                    );
                }
                Err(error) => {
                    if let UpdateState::Installing(version) = &self.update_state {
                        self.update_state = UpdateState::Available(version.clone());
                    }
                    window.push_notification(
                        (NotificationType::Error, format!("Update failed: {error}")),
                        cx,
                    );
                }
            },
        }
        cx.notify();
    }

    fn handle_core_event(
        &mut self,
        name: &str,
        payload: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match name {
            "connection-status-changed" => {
                if let Ok(event) = serde_json::from_value::<ConnectionStatusEvent>(payload) {
                    if event.connected {
                        if let Some(server) = event.server.as_deref() {
                            window.push_notification(
                                (NotificationType::Success, format!("Connected to {server}")),
                                cx,
                            );
                        }
                    }
                    self.connection = event;
                    self.rtt_ms = None;
                    self.sync_offset_seconds = None;
                }
            }
            "tls-status-changed" => {
                if let Ok(event) = serde_json::from_value::<TlsStatusEvent>(payload) {
                    self.tls_status = event.status;
                }
            }
            "user-list-updated" => {
                if let Ok(event) = serde_json::from_value::<UserListEvent>(payload) {
                    self.users = event.users;
                    if let Some(rooms) = event.rooms {
                        self.rooms = rooms;
                    }
                }
            }
            "server-features-updated" => {
                if let Ok(event) = serde_json::from_value::<ServerRoomFeaturesEvent>(payload) {
                    self.server_room_features = event;
                }
            }
            "chat-message-received" => {
                if let Ok(message) = serde_json::from_value::<ChatMessageEvent>(payload) {
                    self.messages.push(message);
                    if self.messages.len() > MAX_CHAT_MESSAGES {
                        let overflow = self.messages.len() - MAX_CHAT_MESSAGES;
                        self.messages.drain(..overflow);
                    }
                }
            }
            "playlist-updated" => {
                if let Ok(event) = serde_json::from_value::<PlaylistEvent>(payload) {
                    // Availability is positional: any item change invalidates
                    // the whole vector before the re-check lands.
                    if event.items != self.playlist.items {
                        self.playlist_availability = Vec::new();
                        self.schedule_availability_check(true, &event.items);
                    }
                    self.playlist = event;
                }
            }
            "player-state-changed" => {
                if let Ok(event) = serde_json::from_value::<PlayerStateEvent>(payload) {
                    // Without a loaded file the sync offset is meaningless.
                    if event.filename.is_none() {
                        self.sync_offset_seconds = None;
                    }
                    self.player = event;
                }
            }
            "sync-offset-updated" => {
                if let Ok(event) = serde_json::from_value::<SyncOffsetEvent>(payload) {
                    self.sync_offset_seconds = Some(event.offset_seconds);
                }
            }
            "ping-updated" => {
                if let Ok(event) = serde_json::from_value::<PingEvent>(payload) {
                    self.rtt_ms = Some(event.rtt_ms);
                }
            }
            "config-updated" => {
                if let Ok(config) = serde_json::from_value::<SyncplayConfig>(payload) {
                    apply_theme(&config.user.theme, Some(window), cx);
                    if config.player.media_directories != self.config.player.media_directories {
                        let items = self.playlist.items.clone();
                        self.schedule_availability_check(true, &items);
                    }
                    self.config = config;
                }
            }
            "media-index-updated" => {
                if let Ok(event) = serde_json::from_value::<MediaIndexUpdatedEvent>(payload) {
                    self.media_index_version = parse_timestamp_ms(&event.timestamp)
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
                    let items = self.playlist.items.clone();
                    self.schedule_availability_check(false, &items);
                }
            }
            "media-index-refreshing" => {
                if let Ok(event) = serde_json::from_value::<MediaIndexRefreshingEvent>(payload) {
                    self.media_index_refreshing = event.refreshing;
                }
            }
            _ => tracing::debug!(name, "unhandled core event"),
        }
    }
    // Command exits: spawn core work on the tokio runtime and report failures
    // back through the UI channel. Successful outcomes return to the UI as
    // core events (`connection-status-changed`, `config-updated`, ...), so no
    // optimistic local updates happen here.

    pub fn connect(
        &mut self,
        params: ConnectParams,
        save_config: Option<SyncplayConfig>,
        cx: &mut Context<Self>,
    ) {
        self.connect_pending = true;
        self.connect_result = None;
        cx.notify();

        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            // "Connect & Save" persists first so the connection sees the new
            // config, mirroring the web client's sequential invokes.
            if let Some(config) = save_config {
                if let Err(error) = commands::config::update_config(config, &core).await {
                    let _ = ui_tx.send(UiEvent::ConnectFinished(Err(error))).await;
                    return;
                }
            }
            let label = format!("{}:{}", params.host, params.port);
            let result = commands::connection::connect_to_server_state(
                params.host,
                params.port,
                params.username,
                params.room,
                params.password,
                &core,
            )
            .await
            .map(|_| label);
            let _ = ui_tx.send(UiEvent::ConnectFinished(result)).await;
        });
    }

    pub fn disconnect(&self) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::connection::disconnect_from_server_state(&core).await {
                let _ = ui_tx
                    .send(UiEvent::CommandFailed("disconnect", error))
                    .await;
            }
        });
    }

    pub fn send_chat(&self, message: String) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::chat::send_chat_message_inner(&core, &message).await {
                let _ = ui_tx.send(UiEvent::CommandFailed("send_chat", error)).await;
            }
        });
    }

    pub fn set_ready(&self, is_ready: bool) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::room::set_ready(is_ready, &core).await {
                let _ = ui_tx.send(UiEvent::CommandFailed("set_ready", error)).await;
            }
        });
    }

    pub fn change_room(&self, room: String) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::room::change_room(room, &core).await {
                let _ = ui_tx
                    .send(UiEvent::CommandFailed("change_room", error))
                    .await;
            }
        });
    }

    pub fn create_managed_room(&self, room: String) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::room::create_managed_room(room, &core).await {
                let _ = ui_tx
                    .send(UiEvent::CommandFailed("create_managed_room", error))
                    .await;
            }
        });
    }

    pub fn identify_as_controller(&self, password: String) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::room::identify_as_controller(password, &core).await {
                let _ = ui_tx
                    .send(UiEvent::CommandFailed("identify_as_controller", error))
                    .await;
            }
        });
    }

    pub fn update_playlist(
        &self,
        action: &'static str,
        filename: Option<String>,
        items: Option<Vec<String>>,
    ) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) =
                commands::playlist::update_playlist(action.to_string(), filename, items, &core)
                    .await
            {
                let _ = ui_tx
                    .send(UiEvent::CommandFailed("update_playlist", error))
                    .await;
            }
        });
    }

    pub fn refresh_media_index(&self) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::config::refresh_media_index(&core).await {
                let _ = ui_tx
                    .send(UiEvent::CommandFailed("refresh_media_index", error))
                    .await;
            }
        });
    }

    /// Re-resolve playlist items against the media index. `full_refresh`
    /// checks every row; otherwise only rows still lacking a resolved path.
    /// Answers return as `UiEvent::PlaylistItemsChecked`.
    fn schedule_availability_check(&self, full_refresh: bool, items: &[String]) {
        if items.is_empty() {
            return;
        }
        let full_refresh = full_refresh || self.playlist_availability.len() != items.len();
        let targets: Vec<(usize, String)> = if full_refresh {
            items.iter().cloned().enumerate().collect()
        } else {
            self.playlist_availability
                .iter()
                .enumerate()
                .filter(|(ix, info)| info.path.is_none() && items.get(*ix) == Some(&info.filename))
                .map(|(ix, _)| (ix, items[ix].clone()))
                .collect()
        };
        if targets.is_empty() {
            return;
        }

        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            let request: Vec<String> = targets.iter().map(|(_, item)| item.clone()).collect();
            let results = match commands::playlist::check_playlist_items(request, &core).await {
                Ok(results) => results,
                // A failed check marks the targets unavailable, matching the
                // web client's fallback.
                Err(_) => targets
                    .iter()
                    .map(|(_, item)| PlaylistItemInfo {
                        filename: item.clone(),
                        path: None,
                        available: false,
                    })
                    .collect(),
            };
            let _ = ui_tx
                .send(UiEvent::PlaylistItemsChecked {
                    full_refresh,
                    targets,
                    results,
                })
                .await;
        });
    }

    fn apply_availability(
        &mut self,
        full_refresh: bool,
        targets: Vec<(usize, String)>,
        results: Vec<PlaylistItemInfo>,
    ) {
        if full_refresh {
            // A playlist edit that landed after scheduling makes this answer
            // stale; the edit already scheduled its own check.
            let current = targets.len() == self.playlist.items.len()
                && targets.iter().enumerate().all(|(ix, (target_ix, item))| {
                    *target_ix == ix && *item == self.playlist.items[ix]
                });
            if current {
                self.playlist_availability = results;
            }
            return;
        }
        if self.playlist_availability.len() != self.playlist.items.len() {
            return;
        }
        for ((ix, item), info) in targets.into_iter().zip(results) {
            if self.playlist.items.get(ix) == Some(&item) {
                self.playlist_availability[ix] = info;
            }
        }
    }

    pub fn update_config(&self, config: SyncplayConfig) {
        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            if let Err(error) = commands::config::update_config(config, &core).await {
                let _ = ui_tx
                    .send(UiEvent::CommandFailed("update_config", error))
                    .await;
            }
        });
    }

    /// Read the cached player list from core (cheap mutex read).
    pub fn load_cached_players(&mut self, cx: &mut Context<Self>) {
        let cache = commands::player::get_cached_players(&self.core);
        self.detected_players = cache.players;
        self.detected_players_at = cache.updated_at;
        cx.notify();
    }

    /// Re-run player detection off the UI thread; results arrive as
    /// `UiEvent::PlayersDetected`.
    pub fn refresh_players(&mut self, cx: &mut Context<Self>) {
        if self.players_refreshing {
            return;
        }
        self.players_refreshing = true;
        cx.notify();

        let core = self.core.clone();
        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            let cache = tokio::task::spawn_blocking(move || {
                commands::player::refresh_player_detection(&core)
            })
            .await;
            if let Ok(cache) = cache {
                let _ = ui_tx
                    .send(UiEvent::PlayersDetected {
                        players: cache.players,
                        updated_at: cache.updated_at,
                    })
                    .await;
            }
        });
    }

    pub fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(
            self.update_state,
            UpdateState::Checking | UpdateState::Installing(_)
        ) {
            return;
        }
        self.update_state = UpdateState::Checking;
        cx.notify();

        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            let result = crate::updater::check_for_update().await;
            let _ = ui_tx
                .send(UiEvent::UpdateCheckFinished { manual, result })
                .await;
        });
    }

    pub fn install_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Available(version) = &self.update_state else {
            return;
        };
        let version = version.clone();
        self.update_state = UpdateState::Installing(version);
        cx.notify();

        let ui_tx = self.ui_tx.clone();
        self.tokio_handle.spawn(async move {
            let result = crate::updater::install_update().await;
            let _ = ui_tx.send(UiEvent::UpdateInstallFinished(result)).await;
        });
    }

    pub fn restart_for_update(&self) {
        crate::updater::restart_app();
    }
}

pub fn apply_theme(theme: &str, window: Option<&mut Window>, cx: &mut gpui_kit::App) {
    let dark = theme != "light";
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, window, cx);

    let color = |hex: u32| gpui_kit::Hsla::from(gpui_kit::rgb(hex));
    let (
        background,
        card,
        card_alt,
        pill,
        pill_hover,
        border,
        foreground,
        muted,
        faint,
        accent,
        accent_soft,
        selection,
        success,
        danger,
        warning,
    ) = if dark {
        (
            color(0x1a1a1e),
            color(0x232327),
            color(0x1f1f23),
            color(0x2d2d33),
            color(0x36363d),
            color(0x333339),
            color(0xededf1),
            color(0xa1a1aa),
            color(0x56565f),
            color(0xa5b4fc),
            color(0x2b2c47),
            color(0x33345a),
            color(0x6fd99b),
            color(0xf28b82),
            color(0xf2b544),
        )
    } else {
        (
            color(0xf4f4f6),
            color(0xffffff),
            color(0xf9f9fb),
            color(0xf1f1f4),
            color(0xe9e9ee),
            color(0xe3e3e8),
            color(0x1c1c21),
            color(0x63636b),
            color(0xb9b9c2),
            color(0x4f46e5),
            color(0xeeedfd),
            color(0xecebfb),
            color(0x189a58),
            color(0xd64545),
            color(0xb45309),
        )
    };
    let primary_foreground = if dark {
        color(0x1c1c21)
    } else {
        color(0xffffff)
    };
    let primary_hover = if dark {
        color(0xd7d7dc)
    } else {
        color(0x34343a)
    };
    let primary_active = if dark {
        color(0xffffff)
    } else {
        color(0x000000)
    };

    Theme::update(cx, |theme| {
        theme.background = background;
        theme.foreground = foreground;
        theme.popover = card;
        theme.popover_foreground = foreground;
        theme.group_box = card_alt;
        theme.group_box_foreground = foreground;
        theme.muted = pill;
        theme.muted_foreground = muted;
        theme.skeleton = faint;
        theme.border = border;
        theme.input = border;
        theme.accent = pill_hover;
        theme.accent_foreground = foreground;
        theme.secondary = pill;
        theme.secondary_hover = pill_hover;
        theme.secondary_active = pill_hover;
        theme.secondary_foreground = foreground;
        theme.primary = foreground;
        theme.primary_hover = primary_hover;
        theme.primary_active = primary_active;
        theme.primary_foreground = primary_foreground;
        theme.button = pill;
        theme.button_hover = pill_hover;
        theme.button_active = pill_hover;
        theme.button_foreground = foreground;
        theme.button_primary = foreground;
        theme.button_primary_hover = primary_hover;
        theme.button_primary_active = primary_active;
        theme.button_primary_foreground = primary_foreground;
        theme.button_secondary = pill;
        theme.button_secondary_hover = pill_hover;
        theme.button_secondary_active = pill_hover;
        theme.button_secondary_foreground = foreground;
        theme.info = accent;
        theme.info_hover = accent_soft;
        theme.info_active = accent;
        theme.info_foreground = primary_foreground;
        theme.success = success;
        theme.success_hover = success;
        theme.success_active = success;
        theme.success_foreground = if dark {
            color(0x14301f)
        } else {
            color(0xffffff)
        };
        theme.danger = danger;
        theme.danger_hover = danger;
        theme.danger_active = danger;
        theme.danger_foreground = if dark {
            color(0x3a1b1b)
        } else {
            color(0xffffff)
        };
        theme.warning = warning;
        theme.warning_hover = warning;
        theme.warning_active = warning;
        theme.warning_foreground = if dark {
            color(0x342712)
        } else {
            color(0xffffff)
        };
        theme.colors.list = card;
        theme.list_head = card_alt;
        theme.list_even = card_alt;
        theme.list_hover = pill;
        theme.list_active = selection;
        theme.list_active_border = accent;
        theme.sidebar = background;
        theme.sidebar_accent = pill;
        theme.sidebar_accent_foreground = foreground;
        theme.sidebar_border = border;
        theme.sidebar_foreground = foreground;
        theme.sidebar_primary = foreground;
        theme.sidebar_primary_foreground = primary_foreground;
        theme.selection = selection;
        theme.progress_bar = accent;
        theme.ring = accent;
        theme.caret = accent;
        theme.link = accent;
        theme.link_hover = accent;
        theme.link_active = accent;
        theme.scrollbar = background;
        theme.scrollbar_thumb = faint;
        theme.scrollbar_thumb_hover = muted;
        theme.font_size = gpui_kit::px(17.);
        theme.mono_font_size = gpui_kit::px(14.);
        theme.radius = gpui_kit::px(7.);
        theme.radius_lg = gpui_kit::px(12.);
    });
}

fn parse_timestamp_ms(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}
