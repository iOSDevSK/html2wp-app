//! Signed desktop updates. The release-only public repository exposes binaries
//! and latest.json; no GitHub credential is ever bundled in the application.
use crate::{model::*, AppState};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;

#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> Result<Value> {
    let update = app.updater().map_err(err)?.check().await.map_err(err)?;
    Ok(match update {
        Some(update) => json!({"available":true,"version":update.version,"currentVersion":update.current_version,"notes":update.body}),
        None => json!({"available":false,"currentVersion":env!("CARGO_PKG_VERSION")}),
    })
}

#[tauri::command]
pub async fn install_app_update(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    // Hold the same lock used by conversion steps until installation and
    // restart. New work cannot start between the busy check and replacement.
    let _guard = state.try_lock_environment("Finish or stop every running conversion before installing the app update")?;
    let update = app.updater().map_err(err)?.check().await.map_err(err)?
        .ok_or("The app is already up to date")?;
    let mut downloaded = 0_u64;
    update.download_and_install(
        |chunk, total| {
            downloaded = downloaded.saturating_add(chunk as u64);
            let _ = app.emit("app-update-progress",json!({"downloaded":downloaded,"total":total}));
        },
        || { let _ = app.emit("app-update-progress",json!({"installed":true})); },
    ).await.map_err(err)?;
    app.restart();
}
