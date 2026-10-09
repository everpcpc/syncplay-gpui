use crate::app_state::AppState;
use crate::player::detection::{detect_players, DetectedPlayer};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
pub struct PlayerDetectionCache {
    pub players: Vec<DetectedPlayer>,
    pub updated_at: Option<i64>,
}

pub fn detect_available_players(state: &Arc<AppState>) -> PlayerDetectionCache {
    refresh_player_detection_inner(state)
}

pub fn get_cached_players(state: &Arc<AppState>) -> PlayerDetectionCache {
    let players = state.detected_players.lock().clone();
    let updated_at = *state.detected_players_updated_at.lock();
    PlayerDetectionCache {
        players,
        updated_at,
    }
}

pub fn refresh_player_detection(state: &Arc<AppState>) -> PlayerDetectionCache {
    refresh_player_detection_inner(state)
}

fn refresh_player_detection_inner(state: &Arc<AppState>) -> PlayerDetectionCache {
    let players = detect_players();
    let updated_at = Some(chrono::Utc::now().timestamp_millis());
    *state.detected_players.lock() = players.clone();
    *state.detected_players_updated_at.lock() = updated_at;
    PlayerDetectionCache {
        players,
        updated_at,
    }
}
