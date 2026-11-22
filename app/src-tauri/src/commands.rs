//! #[tauri::command] adapters onto Core (Task 1.8).

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::audio::devices;
use crate::cards::Persona;
use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::ipc::{AudioDevices, BackendOptions, BootInfo, DownloadEvent, SaveSettingsResult, StateEvent, VizFrame};
use crate::session::conversation::VizSink;
use crate::session::{Core, DownloadSink};
use crate::settings::Settings;

type CoreState<'a> = State<'a, Arc<Core>>;

fn join_err(e: tauri::Error) -> HaruError {
    HaruError::new(ErrorCode::Internal, MsgKey::Internal).with_detail(e.to_string())
}

#[tauri::command]
pub async fn get_status(core: CoreState<'_>) -> Result<StateEvent, HaruError> {
    Ok(core.status())
}

#[tauri::command]
pub async fn get_settings(core: CoreState<'_>) -> Result<Settings, HaruError> {
    Ok(core.settings())
}

#[tauri::command]
pub async fn save_settings(core: CoreState<'_>, settings: Settings) -> Result<SaveSettingsResult, HaruError> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.save_settings(settings)).await.map_err(join_err)?
}

#[tauri::command]
pub async fn list_audio_devices() -> Result<AudioDevices, HaruError> {
    tauri::async_runtime::spawn_blocking(devices::list).await.map_err(join_err)?
}

#[tauri::command]
pub async fn backend_options(core: CoreState<'_>) -> Result<BackendOptions, HaruError> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.backend_options()).await.map_err(join_err)?
}

#[tauri::command]
pub async fn boot(core: CoreState<'_>, on_download: Channel<DownloadEvent>) -> Result<BootInfo, HaruError> {
    let core = core.inner().clone();
    let sink: DownloadSink = Arc::new(move |e| {
        let _ = on_download.send(e);
    });
    core.boot(sink).await
}

#[tauri::command]
pub async fn start_conversation(core: CoreState<'_>, persona: Persona, on_viz: Channel<VizFrame>) -> Result<(), HaruError> {
    let core = core.inner().clone();
    let sink: VizSink = Arc::new(move |f| {
        let _ = on_viz.send(f);
    });
    tauri::async_runtime::spawn_blocking(move || core.start(persona, sink)).await.map_err(join_err)?
}

#[tauri::command]
pub async fn stop_conversation(core: CoreState<'_>) -> Result<(), HaruError> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.stop()).await.map_err(join_err)?
}

#[tauri::command]
pub async fn set_muted(core: CoreState<'_>, muted: bool) -> Result<(), HaruError> {
    core.set_muted(muted);
    Ok(())
}

/// Dev/test switches from the environment (see `test_hooks`); all false in normal use.
#[tauri::command]
pub async fn test_hooks(
    hooks: State<'_, crate::test_hooks::TestHooks>,
    autostart: State<'_, crate::test_hooks::AutostartOnce>,
) -> Result<crate::test_hooks::TestHooks, HaruError> {
    let mut h = hooks.inner().clone();
    // once per process: a webview reload must not start a second conversation
    h.autostart = autostart.take();
    Ok(h)
}

/// Writes a UI diagnostic line to stderr; only acts when `HARU_TEST_DEBUG` is set.
#[tauri::command]
pub async fn test_log(hooks: State<'_, crate::test_hooks::TestHooks>, msg: String) -> Result<(), HaruError> {
    if hooks.debug {
        eprintln!("[ui] {msg}");
    }
    Ok(())
}
