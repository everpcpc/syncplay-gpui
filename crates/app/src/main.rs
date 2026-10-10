#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod chat;
mod connection;
mod events;
mod playlist;
mod rooms;
mod root;
mod settings;
mod store;
mod updater;
mod users;

use std::sync::Arc;

use gpui_kit::component::TitleBar;
use gpui_kit::{px, size, AppContext as _, WindowBounds};
use syncplay_core::app_state::AppState;
use syncplay_core::config::load_config;
use syncplay_core::player::controller::spawn_player_state_loop;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::root::RootView;
use crate::store::{apply_theme, AppStore, UiEvent};

fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "syncplay=info,syncplay_core=info,self_update=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // The core spawns background work on a tokio runtime that outlives any
    // gpui context; keep the runtime alive on its own thread.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let tokio_handle = runtime.handle().clone();
    syncplay_core::set_runtime_handle(tokio_handle.clone());
    std::thread::Builder::new()
        .name("syncplay-tokio".to_string())
        .spawn(move || runtime.block_on(std::future::pending::<()>()))
        .expect("failed to spawn tokio runtime thread");

    let core = AppState::new();
    let config = load_config().unwrap_or_else(|error| {
        tracing::error!("Failed to load config: {error}");
        syncplay_core::config::SyncplayConfig::default()
    });
    *core.config.lock() = config.clone();
    core.sync_engine.lock().update_from_config(&config.user);
    core.media_index.update_settings(
        config.player.media_directories.clone(),
        config.player.media_index_timeout_seconds,
    );
    core.media_index.clone().spawn_indexer(core.clone());
    if !config.player.media_directories.is_empty() {
        core.media_index.clone().request_refresh(core.clone());
    }
    tokio_handle.spawn({
        let core = core.clone();
        async move { spawn_player_state_loop(core) }
    });

    let (ui_tx, ui_rx) = async_channel::unbounded::<UiEvent>();
    core.set_event_sink(Arc::new({
        let ui_tx = ui_tx.clone();
        move |name, payload| {
            let _ = ui_tx.try_send(UiEvent::Core {
                name: name.to_string(),
                payload,
            });
        }
    }));

    let theme = config.user.theme.clone();
    let window_size = match (config.user.window_width, config.user.window_height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            size(px(width as f32), px(height as f32))
        }
        _ => size(px(1200.), px(800.)),
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            apply_theme(&theme, None, cx);

            let mut window_options = TitleBar::window_options();
            window_options.window_bounds = Some(WindowBounds::centered(window_size, cx));
            if let Some(titlebar) = window_options.titlebar.as_mut() {
                titlebar.title = Some("Syncplay".into());
            }

            gpui_kit::open_window(window_options, cx, |window, cx| {
                let store = cx.new(|cx| {
                    AppStore::new(
                        core.clone(),
                        tokio_handle.clone(),
                        ui_tx.clone(),
                        ui_rx.clone(),
                        config.clone(),
                        window,
                        cx,
                    )
                });
                cx.new(|cx| RootView::new(store, window, cx))
            })
            .expect("failed to open window");

            // The player is a separate child process, so quitting the app
            // alone leaves it running; tear down the session (which stops
            // the player) before the process exits.
            cx.on_app_quit({
                let core = core.clone();
                let tokio_handle = tokio_handle.clone();
                move |_| {
                    let teardown = tokio_handle.spawn({
                        let core = core.clone();
                        async move {
                            if let Err(error) =
                                syncplay_core::commands::connection::disconnect_from_server_state(
                                    &core,
                                )
                                .await
                            {
                                tracing::warn!("Failed to disconnect during shutdown: {error}");
                            }
                        }
                    });
                    async move {
                        let _ = teardown.await;
                    }
                }
            })
            .detach();

            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        });
}
