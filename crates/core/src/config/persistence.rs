// Persistence module
// Configuration storage as a JSON file

use super::settings::SyncplayConfig;
use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

const STORE_PATH: &str = "syncplay.store.json";
const CONFIG_KEY: &str = "config";
#[cfg(not(test))]
const APP_CONFIG_DIR: &str = "com.everpcpc.syncplay";

/// Get the configuration store path
pub fn get_config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join(STORE_PATH))
}

#[cfg(not(test))]
fn config_dir() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join(APP_CONFIG_DIR))
        .context("Failed to resolve config directory")
}

// Unit tests share one throwaway directory so incidental config writes from
// connection-flow tests never touch the real user config.
#[cfg(test)]
fn config_dir() -> Result<PathBuf> {
    use std::sync::OnceLock;
    static TEST_CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
    Ok(TEST_CONFIG_DIR
        .get_or_init(|| {
            std::env::temp_dir().join(format!("syncplay-core-test-config-{}", std::process::id()))
        })
        .clone())
}

/// Load configuration from the store
pub fn load_config() -> Result<SyncplayConfig> {
    load_config_from(&get_config_path()?)
}

/// Save configuration to the store
pub fn save_config(config: &SyncplayConfig) -> Result<()> {
    save_config_to(&get_config_path()?, config)
}

pub(crate) fn load_config_from(path: &Path) -> Result<SyncplayConfig> {
    let store = read_store(path)?;
    if let Some(value) = store.get(CONFIG_KEY) {
        if let Ok(config) = serde_json::from_value::<SyncplayConfig>(value.clone()) {
            return Ok(config);
        }
        tracing::warn!("Failed to deserialize config, resetting to defaults");
    }

    let config = SyncplayConfig::default();
    save_config_to(path, &config)?;
    Ok(config)
}

pub(crate) fn save_config_to(path: &Path, config: &SyncplayConfig) -> Result<()> {
    let value = serde_json::to_value(config).context("Failed to serialize config")?;
    let mut store = read_store(path)?;
    store.insert(CONFIG_KEY.to_string(), value);
    write_store(path, &store)
}

fn read_store(path: &Path) -> Result<Map<String, Value>> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e).context("Failed to read config store"),
    };
    let value: Value = serde_json::from_str(&contents).context("Failed to parse config store")?;
    Ok(value.as_object().cloned().unwrap_or_default())
}

fn write_store(path: &Path, store: &Map<String, Value>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create config directory")?;
    }
    let contents =
        serde_json::to_string_pretty(store).context("Failed to serialize config store")?;
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, contents).context("Failed to write config store")?;
    std::fs::rename(&tmp_path, path).context("Failed to replace config store")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STORE_PATH);
        (dir, path)
    }

    #[test]
    fn test_save_and_load_config() {
        let (_dir, path) = temp_store_path();
        let mut config = SyncplayConfig::default();
        config.user.username = "testuser".to_string();
        config.server.host = "example.com".to_string();
        config.server.port = 9000;

        save_config_to(&path, &config).unwrap();
        let loaded = load_config_from(&path).unwrap();

        assert_eq!(loaded.server.host, "example.com");
        assert_eq!(loaded.server.port, 9000);
        assert_eq!(loaded.user.username, "testuser");
    }

    #[test]
    fn test_config_path() {
        let path = get_config_path().unwrap();
        assert!(path.to_string_lossy().contains("syncplay"));
        assert!(path.to_string_lossy().ends_with(STORE_PATH));
    }

    #[test]
    fn test_load_nonexistent_config() {
        let (_dir, path) = temp_store_path();
        let config = load_config_from(&path).unwrap();
        assert_eq!(config.server.host, "syncplay.pl");
        assert!(path.exists());
    }

    #[test]
    fn legacy_store_format_roundtrips() {
        let (_dir, path) = temp_store_path();
        let mut config = SyncplayConfig::default();
        config.user.username = "legacy".to_string();
        let mut store = Map::new();
        store.insert(
            CONFIG_KEY.to_string(),
            serde_json::to_value(&config).unwrap(),
        );
        store.insert("unrelated".to_string(), Value::from(42));
        std::fs::write(&path, serde_json::to_string_pretty(&store).unwrap()).unwrap();

        let loaded = load_config_from(&path).unwrap();
        assert_eq!(loaded.user.username, "legacy");

        let mut updated = loaded.clone();
        updated.server.host = "example.org".to_string();
        save_config_to(&path, &updated).unwrap();

        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["unrelated"], Value::from(42));
        assert_eq!(written[CONFIG_KEY]["server"]["host"], "example.org");
        assert_eq!(
            serde_json::from_value::<SyncplayConfig>(written[CONFIG_KEY].clone())
                .unwrap()
                .user
                .username,
            "legacy"
        );
    }

    #[test]
    fn invalid_store_json_errors() {
        let (_dir, path) = temp_store_path();
        std::fs::write(&path, "not json").unwrap();
        assert!(load_config_from(&path).is_err());
    }
}
