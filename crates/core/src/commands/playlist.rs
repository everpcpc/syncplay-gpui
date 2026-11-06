// Playlist command handlers

use crate::app_state::{AppState, PendingInitialMedia};
use crate::client::playback_runtime;
use crate::config::SyncplayConfig;
use crate::network::messages::{PlayState, StateMessage};
use crate::network::messages::{PlaylistChange, PlaylistIndexUpdate, ProtocolMessage, SetMessage};
use crate::player::controller::resolve_media_path;
use crate::utils::{is_music_file, is_url};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;

pub fn queue_initial_media(state: &Arc<AppState>, target: String) {
    *state.pending_initial_media.lock() = Some(PendingInitialMedia {
        target,
        connection_generation: state
            .connection_session_generation
            .load(std::sync::atomic::Ordering::Acquire),
    });
}

pub(crate) async fn open_initial_media_if_ready(state: &Arc<AppState>) {
    let pending = state.pending_initial_media.lock().clone();
    let Some(pending) = pending else {
        return;
    };
    let shared_playlist = {
        let config = state.config.lock().clone();
        shared_playlists_enabled(state, &config)
    };
    let ready = {
        let playback = state.playback.state.lock();
        !shared_playlist || playback.received_server_index || playback.room_playlist_received
    };
    if !state.is_connected()
        || state
            .connection_session_generation
            .load(std::sync::atomic::Ordering::Acquire)
            != pending.connection_generation
        || !ready
    {
        return;
    }
    state.pending_initial_media.lock().take();
    let snapshot = state.playback.snapshot();
    if shared_playlist {
        let current = snapshot
            .displayed_index()
            .and_then(|index| snapshot.playlist_items.get(index));
        if current.is_some() && current != Some(&pending.target) {
            // Match the official delayed-load behavior: make the stream
            // available without overriding a room's established selection.
            if let Err(error) = add_media_to_shared_playlist(state, pending.target).await {
                emit_media_load_error(state, error);
            }
            return;
        }
    }
    if let Err(error) = open_media_now(state, pending.target).await {
        emit_media_load_error(state, error);
    }
}

fn emit_media_load_error(state: &Arc<AppState>, error: String) {
    tracing::warn!("Failed to open initial media: {error}");
    crate::commands::connection::emit_error_message(state, &error);
}

async fn open_media_now(state: &Arc<AppState>, target: String) -> Result<(), String> {
    let (target, override_path) = normalize_playlist_entry(&target);
    if is_url(&target) {
        *state.user_initiated_url.lock() = Some(target.clone());
    }
    if let Some(path) = override_path {
        state.media_index.add_override_path(&target, path);
    }
    let config = state.config.lock().clone();
    let shared_playlist = shared_playlists_enabled(state, &config) && state.is_connected();
    let selection_target = target.clone();
    let previous_items = state.playback.snapshot().playlist_items;
    let adding = !previous_items.contains(&selection_target);
    let edit = playback_runtime::edit_playlist_and_publish(
        state,
        false,
        move |items, _| {
            if !items.contains(&selection_target) {
                items.push(selection_target.clone());
            }
            Ok(())
        },
        |result| {
            if shared_playlist && adding {
                send_to_server(
                    state,
                    ProtocolMessage::Set {
                        Set: Box::new(SetMessage {
                            room: None,
                            file: None,
                            user: None,
                            ready: None,
                            playlist_index: None,
                            playlist_change: Some(PlaylistChange {
                                user: None,
                                files: result.items.clone(),
                            }),
                            controller_auth: None,
                            new_controlled_room: None,
                            features: None,
                        }),
                    },
                )?;
            }
            Ok(())
        },
    )
    .await?;
    let index = edit
        .items
        .iter()
        .position(|item| item == &target)
        .ok_or_else(|| "Requested media was not added to the playlist".to_string())?;
    playback_runtime::local_open(state, index, true).await
}

async fn add_media_to_shared_playlist(state: &Arc<AppState>, target: String) -> Result<(), String> {
    if state.playback.snapshot().playlist_items.contains(&target) {
        return Ok(());
    }
    playback_runtime::edit_playlist_and_publish(
        state,
        false,
        move |items, _| {
            items.push(target);
            Ok(())
        },
        |result| {
            send_to_server(
                state,
                ProtocolMessage::Set {
                    Set: Box::new(SetMessage {
                        room: None,
                        file: None,
                        user: None,
                        ready: None,
                        playlist_index: None,
                        playlist_change: Some(PlaylistChange {
                            user: None,
                            files: result.items.clone(),
                        }),
                        controller_auth: None,
                        new_controlled_room: None,
                        features: None,
                    }),
                },
            )
        },
    )
    .await?;
    Ok(())
}

pub async fn update_playlist(
    action: String,
    filename: Option<String>,
    items: Option<Vec<String>>,
    state: &Arc<AppState>,
) -> Result<(), String> {
    tracing::info!("Playlist action: {} for file: {:?}", action, filename);
    let config = state.config.lock().clone();
    if !shared_playlists_enabled(state, &config) {
        return Err("Shared playlists are disabled".to_string());
    }
    match action.as_str() {
        "add" => {
            let file = filename.ok_or_else(|| "Filename required for add action".to_string())?;
            let (normalized, override_path) = normalize_playlist_entry(&file);
            if let Some(path) = override_path {
                state.media_index.add_override_path(&normalized, path);
            }
            apply_playlist_edit_local(state, false, move |items, _| {
                items.push(normalized);
                Ok(())
            })
            .await?;
        }
        "remove" => {
            let index_str =
                filename.ok_or_else(|| "Index required for remove action".to_string())?;
            let index = index_str
                .parse::<usize>()
                .map_err(|_| "Invalid index for remove action".to_string())?;
            apply_playlist_edit_local(state, false, move |items, _| {
                if index >= items.len() {
                    return Err("Invalid index for remove action".to_string());
                }
                items.remove(index);
                Ok(())
            })
            .await?;
        }
        "clear" => {
            apply_playlist_edit_local(state, false, |items, _| {
                items.clear();
                Ok(())
            })
            .await?;
        }
        "select" => {
            let index_str =
                filename.ok_or_else(|| "Index required for select action".to_string())?;
            let index = index_str
                .parse::<usize>()
                .map_err(|_| "Invalid index for select action".to_string())?;
            playback_runtime::local_select(state, index, true).await?;
        }
        "next" => {
            let loop_at_end = config.user.loop_at_end_of_playlist || is_playing_music(state);
            playback_runtime::local_step(
                state,
                playback_runtime::PlaylistStep::Next { loop_at_end },
                true,
            )
            .await?;
        }
        "previous" => {
            playback_runtime::local_step(state, playback_runtime::PlaylistStep::Previous, true)
                .await?;
        }
        "undo" => {
            if state.playlist.previous_playlist().is_some() {
                let playlist = state.playlist.clone();
                apply_playlist_edit_local(state, false, move |items, _| {
                    if let Some(previous) = playlist.previous_playlist() {
                        *items = previous;
                    }
                    Ok(())
                })
                .await?;
            }
        }
        "shuffle" => {
            apply_playlist_edit_local(state, true, |items, _| {
                items.shuffle(&mut thread_rng());
                Ok(())
            })
            .await?;
        }
        "shuffle_remaining" => {
            apply_playlist_edit_local(state, false, |items, current_index| {
                let Some(current_index) = current_index else {
                    return Ok(());
                };
                let split_point = current_index + 1;
                if split_point < items.len() {
                    let mut tail = items.split_off(split_point);
                    tail.shuffle(&mut thread_rng());
                    items.extend(tail);
                }
                Ok(())
            })
            .await?;
        }
        "load" => {
            let path = filename.ok_or_else(|| "Path required for load action".to_string())?;
            let contents = std::fs::read_to_string(&path)
                .map_err(|_| "Failed to read playlist file".to_string())?;
            let items: Vec<String> = contents
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
            if items.is_empty() {
                return Err("Playlist file is empty".to_string());
            }
            apply_playlist_edit_local(state, true, move |current, _| {
                *current = items;
                Ok(())
            })
            .await?;
        }
        "reorder" => {
            let items = items.ok_or_else(|| "Items required for reorder action".to_string())?;
            apply_playlist_edit_local(state, false, move |current, _| {
                *current = items;
                Ok(())
            })
            .await?;
        }
        "save" => {
            let path = filename.ok_or_else(|| "Path required for save action".to_string())?;
            let contents = state.playback.snapshot().playlist_items.join("\n");
            std::fs::write(&path, contents)
                .map_err(|_| "Failed to save playlist file".to_string())?;
        }
        _ => {
            return Err(format!("Unknown playlist action: {}", action));
        }
    }

    Ok(())
}

pub async fn check_playlist_items(
    items: Vec<String>,
    state: &Arc<AppState>,
) -> Result<Vec<PlaylistItemInfo>, String> {
    let config = state.config.lock().clone();
    let mut results = Vec::with_capacity(items.len());
    for item in items {
        let path = if is_url(&item) {
            Some(item.clone())
        } else {
            state
                .media_index
                .resolve_path(&item)
                .or_else(|| resolve_media_path(&config.player.media_directories, &item))
                .map(|path| path.to_string_lossy().to_string())
        };
        let available = path.is_some();
        results.push(PlaylistItemInfo {
            filename: item,
            path,
            available,
        });
    }
    Ok(results)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlaylistItemInfo {
    pub filename: String,
    pub path: Option<String>,
    pub available: bool,
}

pub(crate) fn shared_playlists_enabled(state: &Arc<AppState>, config: &SyncplayConfig) -> bool {
    config.user.shared_playlist_enabled && state.server_features.lock().shared_playlists
}

pub(crate) fn send_playlist_index(
    state: &Arc<AppState>,
    index: usize,
    reset_position: bool,
) -> Result<(), String> {
    let message = ProtocolMessage::Set {
        Set: Box::new(SetMessage {
            room: None,
            file: None,
            user: None,
            ready: None,
            playlist_index: Some(PlaylistIndexUpdate {
                user: None,
                index: Some(index),
            }),
            playlist_change: None,
            controller_auth: None,
            new_controlled_room: None,
            features: None,
        }),
    };
    send_to_server(state, message)?;

    if reset_position {
        *state.last_advance_time.lock() = Some(std::time::Instant::now());
        let state_message = ProtocolMessage::State {
            State: StateMessage {
                playstate: Some(PlayState {
                    position: 0.0,
                    paused: true,
                    do_seek: None,
                    set_by: None,
                }),
                ping: None,
                ignoring_on_the_fly: None,
            },
        };
        let _ = send_to_server(state, state_message);
    }

    Ok(())
}

async fn apply_playlist_edit_local(
    state: &Arc<AppState>,
    reset_index: bool,
    edit: impl FnOnce(&mut Vec<String>, Option<usize>) -> Result<(), String>,
) -> Result<playback_runtime::PlaylistEditResult, String> {
    playback_runtime::edit_playlist_and_publish(state, reset_index, edit, |result| {
        let message = ProtocolMessage::Set {
            Set: Box::new(SetMessage {
                room: None,
                file: None,
                user: None,
                ready: None,
                playlist_index: None,
                playlist_change: Some(PlaylistChange {
                    user: None,
                    files: result.items.clone(),
                }),
                controller_auth: None,
                new_controlled_room: None,
                features: None,
            }),
        };
        send_to_server(state, message)?;
        if let Some(index) = result.index {
            send_playlist_index(state, index, false)?;
        }
        Ok(())
    })
    .await
}

fn normalize_playlist_entry(entry: &str) -> (String, Option<PathBuf>) {
    if is_url(entry) {
        return (entry.to_string(), None);
    }
    let path = Path::new(entry);
    if path.is_absolute() && path.is_file() {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            return (name.to_string(), Some(path.to_path_buf()));
        }
    }
    (entry.to_string(), None)
}

fn is_playing_music(state: &Arc<AppState>) -> bool {
    state
        .client_state
        .get_file()
        .as_deref()
        .map(is_music_file)
        .unwrap_or(false)
}

fn send_to_server(state: &Arc<AppState>, message: ProtocolMessage) -> Result<(), String> {
    let connection = state.connection.lock().clone();
    let Some(connection) = connection else {
        return Err("Not connected to server".to_string());
    };
    connection
        .send(message)
        .map_err(|e| format!("Failed to send message: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::connection::Connection;
    use crate::network::fake_server::FakeSyncplayServer;
    use crate::player::backend::{FakePlayerCommand, FakePlayerFactory};

    async fn fixture() -> (Arc<AppState>, FakeSyncplayServer) {
        let state = AppState::new();
        let server = FakeSyncplayServer::start().await.unwrap();
        let connection = Arc::new(Connection::new());
        let (_receiver, _peer) = connection
            .connect(server.host(), server.port())
            .await
            .unwrap();
        *state.connection.lock() = Some(connection);
        playback_runtime::replace_playlist(
            &state,
            vec!["a.mkv".into(), "b.mkv".into(), "c.mkv".into()],
            Some(1),
        )
        .await
        .unwrap();
        (state, server)
    }

    #[tokio::test]
    async fn explicitly_opened_stream_loads_despite_domain_switch_restrictions() {
        let state = AppState::new();
        let factory = Arc::new(FakePlayerFactory::default());
        *state.fake_player_factory.lock() = Some(factory.clone());
        state.server_features.lock().shared_playlists = false;
        let server = FakeSyncplayServer::start().await.unwrap();
        let connection = Arc::new(Connection::new());
        connection
            .connect(server.host(), server.port())
            .await
            .unwrap();
        connection.set_authenticated();
        *state.connection.lock() = Some(connection);
        let target = "https://media.example.test/episode.m3u8".to_string();

        queue_initial_media(&state, target.clone());
        open_initial_media_if_ready(&state).await;

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let players = factory.players();
                if players.iter().any(|player| {
                    player.commands().iter().any(|command| {
                        matches!(command, FakePlayerCommand::LoadFile(path) if path == &target)
                    })
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        server.close();
    }

    #[tokio::test]
    async fn current_item_removal_sends_index_even_when_its_number_is_unchanged() {
        let (state, mut server) = fixture().await;

        apply_playlist_edit_local(&state, false, |items, _| {
            items.remove(1);
            Ok(())
        })
        .await
        .unwrap();

        let playlist = server.next_received().await.unwrap();
        let index = server.next_received().await.unwrap();
        assert!(matches!(
            playlist,
            ProtocolMessage::Set { Set }
                if Set.playlist_change.is_some() && Set.playlist_index.is_none()
        ));
        assert!(matches!(
            index,
            ProtocolMessage::Set { Set }
                if Set.playlist_index.as_ref().and_then(|update| update.index) == Some(1)
        ));
    }

    async fn authenticated_state() -> (Arc<AppState>, FakeSyncplayServer) {
        let state = AppState::new();
        let server = FakeSyncplayServer::start().await.unwrap();
        let connection = Arc::new(Connection::new());
        connection
            .connect(server.host(), server.port())
            .await
            .unwrap();
        connection.set_authenticated();
        *state.connection.lock() = Some(connection);
        (state, server)
    }

    #[tokio::test]
    async fn initial_media_waits_for_the_room_snapshot_before_publishing() {
        let (state, mut server) = authenticated_state().await;
        let target = "https://media.example.test/new.m3u8".to_string();
        queue_initial_media(&state, target.clone());

        open_initial_media_if_ready(&state).await;
        assert!(state.pending_initial_media.lock().is_some());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), server.next_received())
                .await
                .is_err()
        );

        playback_runtime::server_playlist_and_index(
            &state,
            Some(vec!["existing.mkv".into()]),
            Some((Some(0), false)),
        )
        .await
        .unwrap();
        open_initial_media_if_ready(&state).await;

        let message = tokio::time::timeout(Duration::from_secs(1), server.next_received())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            message,
            ProtocolMessage::Set { Set }
                if Set.playlist_change.as_ref().map(|change| &change.files)
                    == Some(&vec!["existing.mkv".into(), target])
                && Set.playlist_index.is_none()
        ));
        assert!(state.pending_initial_media.lock().is_none());
        server.close();
    }

    #[tokio::test]
    async fn stale_initial_media_cannot_run_in_a_new_connection_generation() {
        let (state, server) = authenticated_state().await;
        queue_initial_media(&state, "https://media.example.test/old.m3u8".into());
        state
            .connection_session_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        playback_runtime::server_playlist_and_index(&state, Some(Vec::new()), None)
            .await
            .unwrap();

        open_initial_media_if_ready(&state).await;

        assert!(state.pending_initial_media.lock().is_some());
        assert!(state.playback.snapshot().playlist_items.is_empty());
        server.close();
    }

    #[tokio::test]
    async fn empty_shared_room_opens_the_initial_stream_after_its_snapshot() {
        let (state, server) = authenticated_state().await;
        let factory = Arc::new(FakePlayerFactory::default());
        *state.fake_player_factory.lock() = Some(factory.clone());
        state.server_features.lock().shared_playlists = false;
        let target = "https://media.example.test/episode.m3u8".to_string();
        queue_initial_media(&state, target.clone());
        playback_runtime::server_playlist_and_index(&state, Some(Vec::new()), None)
            .await
            .unwrap();

        open_initial_media_if_ready(&state).await;

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if factory.players().iter().any(|player| {
                    player.commands().iter().any(|command| {
                        matches!(command, FakePlayerCommand::LoadFile(path) if path == &target)
                    })
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        server.close();
    }

    #[tokio::test]
    async fn remote_reselection_does_not_reuse_the_initial_url_authorization() {
        let (state, server) = authenticated_state().await;
        let factory = Arc::new(FakePlayerFactory::default());
        *state.fake_player_factory.lock() = Some(factory.clone());
        let target = "https://media.example.test/episode.m3u8".to_string();
        let lease = || {
            playback_runtime::MediaLoadStartupLease::for_test(
                &state,
                crate::client::playback::LoadId(1),
                target.clone(),
            )
        };

        *state.user_initiated_url.lock() = Some(target.clone());
        playback_runtime::replace_playlist(&state, vec![target.clone()], None)
            .await
            .unwrap();
        playback_runtime::local_open(&state, 0, true).await.unwrap();
        crate::player::controller::load_media_by_name(
            &state,
            &target,
            true,
            crate::client::playback::LoadId(1),
            &lease(),
        )
        .await
        .unwrap();
        assert!(state.user_initiated_url.lock().is_none());
        let error = match crate::player::controller::load_media_by_name(
            &state,
            &target,
            true,
            crate::client::playback::LoadId(2),
            &lease(),
        )
        .await
        {
            Ok(_) => panic!("a later load reused the one-shot URL authorization"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), "URL is not trusted");
        assert_eq!(factory.launch_count(), 1);
        server.close();
    }

    #[tokio::test]
    async fn concurrent_edits_publish_complete_transactions_in_coordinator_order() {
        let (state, mut server) = fixture().await;
        let first_state = state.clone();
        let first = tokio::spawn(async move {
            apply_playlist_edit_local(&first_state, false, |items, _| {
                items.push("d.mkv".to_string());
                Ok(())
            })
            .await
        });
        let second_state = state.clone();
        let second = tokio::spawn(async move {
            apply_playlist_edit_local(&second_state, false, |items, _| {
                items.push("e.mkv".to_string());
                Ok(())
            })
            .await
        });
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();

        let mut playlist_lengths = Vec::new();
        for pair in 0..2 {
            let playlist = server.next_received().await.unwrap();
            let index = server.next_received().await.unwrap();
            let ProtocolMessage::Set { Set } = playlist else {
                panic!("expected playlist message for transaction {pair}");
            };
            playlist_lengths.push(Set.playlist_change.unwrap().files.len());
            assert!(matches!(
                index,
                ProtocolMessage::Set { Set } if Set.playlist_index.is_some()
            ));
        }
        assert_eq!(playlist_lengths, vec![4, 5]);
    }

    #[tokio::test]
    async fn reset_index_uses_the_advance_window_without_faking_a_rewind() {
        let (state, mut server) = fixture().await;
        assert!(state.last_advance_time.lock().is_none());
        assert!(state.last_rewind_time.lock().is_none());

        send_playlist_index(&state, 2, true).unwrap();

        assert!(state.last_advance_time.lock().is_some());
        assert!(state.last_rewind_time.lock().is_none());
        let index = server.next_received().await.unwrap();
        let playback = server.next_received().await.unwrap();
        assert!(matches!(
            index,
            ProtocolMessage::Set { Set }
                if Set.playlist_index.as_ref().and_then(|update| update.index) == Some(2)
        ));
        assert!(matches!(
            playback,
            ProtocolMessage::State { State }
                if matches!(
                    State.playstate,
                    Some(PlayState {
                        position,
                        paused: true,
                        ..
                    }) if position == 0.0
                )
        ));
    }
}
