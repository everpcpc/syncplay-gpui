use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::OnceLock;

const APP_CACHE_DIR: &str = "com.everpcpc.syncplay";

const RESOURCES: [(&str, &[u8]); 3] = [
    (
        "placeholder.png",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/placeholder.png"
        )),
    ),
    (
        "syncplay.lua",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/syncplay.lua"
        )),
    ),
    (
        "syncplayintf.lua",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/syncplayintf.lua"
        )),
    ),
];

static RESOURCES_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Media players consume these assets as real files, so the embedded copies
/// are materialized into the cache directory on first use.
pub fn ensure_resources_dir() -> Result<PathBuf> {
    if let Some(dir) = RESOURCES_DIR.get() {
        return Ok(dir.clone());
    }
    let dir = materialize_resources()?;
    Ok(RESOURCES_DIR.get_or_init(|| dir).clone())
}

fn materialize_resources() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .map(|dir| dir.join(APP_CACHE_DIR).join("resources"))
        .context("Failed to resolve cache directory")?;
    std::fs::create_dir_all(&dir).context("Failed to create resources directory")?;
    for (name, bytes) in RESOURCES {
        let path = dir.join(name);
        if std::fs::read(&path)
            .map(|existing| existing == bytes)
            .unwrap_or(false)
        {
            continue;
        }
        std::fs::write(&path, bytes).with_context(|| format!("Failed to write resource {name}"))?;
    }
    Ok(dir)
}
