pub mod persistence;
#[allow(dead_code)]
pub mod settings;

pub use persistence::{get_config_path, has_saved_config, load_config, save_config};
pub use settings::{
    ChatInputPosition, ChatOutputMode, PrivacyMode, ServerConfig, SyncplayConfig, UnpauseAction,
    UserPreferences, DEFAULT_MEDIA_INDEX_TIMEOUT_SECONDS,
};
