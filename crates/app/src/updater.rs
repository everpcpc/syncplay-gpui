use self_update::backends::github;
use self_update::{cargo_crate_version, VersionStatus};

const REPO_OWNER: &str = "everpcpc";
const REPO_NAME: &str = "syncplay-gpui";

// Release archives pack the binary under a syncplay/ directory.
#[cfg(windows)]
const BIN_PATH_IN_ARCHIVE: &str = "syncplay/syncplay.exe";
#[cfg(not(windows))]
const BIN_PATH_IN_ARCHIVE: &str = "syncplay/syncplay";

fn configure() -> github::UpdateBuilder {
    let mut builder = github::Update::configure();
    builder
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name("syncplay")
        .current_version(cargo_crate_version!())
        .bin_path_in_archive(BIN_PATH_IN_ARCHIVE)
        // The default Compatible strategy treats every 0.x minor bump as
        // incompatible and would settle for an older caret-matching release.
        .update_strategy(self_update::UpdateStrategy::Latest)
        // Without a ceiling a wedged connection parks the UI state forever;
        // seen in the wild with a stalled GitHub API connection.
        .timeout(std::time::Duration::from_secs(60))
        .no_confirm(true)
        .show_output(false);
    builder
}

/// Version of the newest release strictly newer than the running build.
pub async fn check_for_update() -> Result<Option<String>, String> {
    let updater = configure().build_async().map_err(|e| e.to_string())?;
    let release = updater
        .is_update_available_async()
        .await
        .map_err(|e| e.to_string())?;
    Ok(release.map(|release| release.version().to_string()))
}

/// Download and swap in the latest release; returns the installed version.
pub async fn install_update() -> Result<String, String> {
    let updater = configure().build_async().map_err(|e| e.to_string())?;
    let status = updater.update_async().await.map_err(|e| e.to_string())?;
    match status {
        VersionStatus::Updated(version) => Ok(version),
        VersionStatus::UpToDate(version) => Err(format!("already up to date ({version})")),
        _ => Err("unexpected update status".to_string()),
    }
}

pub fn restart_app() {
    match self_update::restart::restart() {
        Ok(infallible) => match infallible {},
        Err(error) => {
            tracing::error!("failed to restart after update: {error}");
            std::process::exit(1);
        }
    }
}
