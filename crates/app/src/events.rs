//! Mirror structs for the JSON payloads emitted by `syncplay-core`.
//!
//! Field names must match what core serializes: ad-hoc `serde_json::json!`
//! payloads use camelCase keys (`messageType`, `rttMs`, `offsetSeconds`,
//! `managedRooms`, `isReady`, `isController`), while the typed event structs
//! in `app_state.rs` derive `Serialize` without renames, so `PlaylistEvent`
//! keeps its snake_case `current_index`.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionStatusEvent {
    pub connected: bool,
    pub server: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TlsStatusEvent {
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChatMessageEvent {
    pub timestamp: String,
    pub username: Option<String>,
    pub message: String,
    #[serde(rename = "messageType")]
    pub message_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserInfoEvent {
    pub username: String,
    pub room: String,
    pub file: Option<String>,
    /// Number of bytes normally, but a hash string under size privacy mode —
    /// hence an untyped value rather than `u64`.
    #[serde(rename = "fileSize", default)]
    pub file_size: Option<serde_json::Value>,
    #[serde(rename = "fileDuration", default)]
    pub file_duration: Option<f64>,
    #[serde(rename = "isReady", default)]
    pub is_ready: bool,
    #[serde(rename = "isController", default)]
    pub is_controller: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserListEvent {
    pub users: Vec<UserInfoEvent>,
    pub rooms: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ServerRoomFeaturesEvent {
    #[serde(rename = "managedRooms", default)]
    pub managed_rooms: bool,
    #[serde(rename = "persistentRooms", default)]
    pub persistent_rooms: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlaylistEvent {
    pub items: Vec<String>,
    pub current_index: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerStateEvent {
    pub filename: Option<String>,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub paused: Option<bool>,
    pub speed: Option<f64>,
}

impl Default for PlayerStateEvent {
    fn default() -> Self {
        Self {
            filename: None,
            position: None,
            duration: None,
            paused: Some(true),
            speed: Some(1.0),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PingEvent {
    #[serde(rename = "rttMs")]
    pub rtt_ms: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncOffsetEvent {
    #[serde(rename = "offsetSeconds")]
    pub offset_seconds: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MediaIndexRefreshingEvent {
    pub refreshing: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MediaIndexUpdatedEvent {
    pub timestamp: String,
}
