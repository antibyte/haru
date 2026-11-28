pub mod audio;
pub mod backend;
pub mod cards;
pub mod commands;
pub mod error;
pub mod ipc;
pub mod locale;
pub mod messages;
pub mod model_fetch;
pub mod paths;
pub mod session;
pub mod settings;
pub mod test_hooks;
pub mod worker;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

use crate::ipc::STATE_EVENT;
use crate::session::{Core, CoreConfig};

/// Label of the one window (tauri.conf.json).
const MAIN_WINDOW: &str = "main";

/// What the window-state plugin saves and restores: size, position and maximized. Not VISIBLE (we show the window
/// ourselves), not DECORATIONS (always off) and not FULLSCREEN (the F toggle is a per-session choice).
fn window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

/// True when `state_json` (the plugin's state file: label -> state) holds a usable entry for `label`.
/// The plugin keeps its cache private and fills it with an all-zero default for every unknown window, so only the
/// file tells a saved state from a fresh start; an entry with a zero size is the default and does not count.
pub fn has_saved_window_state(state_json: &str, label: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(state_json) else { return false };
    let Some(entry) = v.get(label) else { return false };
    let dim = |k: &str| entry.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    dim("width") > 0 && dim("height") > 0
}

/// Restores the saved size/position/maximized state of the main window, or maximizes it on the first start, and
/// shows it (it starts hidden so there is no flicker). The window is shown even if restoring fails.
fn restore_or_maximize<R: tauri::Runtime>(app: &tauri::App<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return };
    let saved = app
        .path()
        .app_config_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join(app.handle().filename())).ok())
        .is_some_and(|json| has_saved_window_state(&json, MAIN_WINDOW));
    let ok = if saved {
        window.restore_state(window_state_flags()).is_ok()
    } else {
        window.maximize().is_ok()
    };
    if !ok {
        eprintln!("haru: could not restore the window state; showing the window as is");
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn run(context: tauri::Context<tauri::Wry>) {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .skip_initial_state(MAIN_WINDOW)
                .build(),
        )
        .setup(|app| {
            let path = app.path();
            let resource_backends = path.resource_dir().ok().map(|d| d.join("backends"));
            let dev_backends = cfg!(debug_assertions).then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("backends"));
            let backends_dir = paths::find_backends_dir(
                std::env::var_os("HARU_BACKENDS_DIR").map(PathBuf::from),
                resource_backends,
                dev_backends,
            );
            let cfg = CoreConfig {
                settings_path: path.app_config_dir()?.join("settings.json"),
                log_dir: path.app_log_dir()?,
                backends_dir,
                models_dir: paths::models_dir()?,
                model: paths::HARU_MODEL,
                hf_base: "https://huggingface.co".to_string(),
            };
            let handle = app.handle().clone();
            let core = Core::new(
                cfg,
                Arc::new(move |ev| {
                    let _ = handle.emit(STATE_EVENT, ev);
                }),
            );
            if let Some(io) = test_hooks::io_from_env() {
                core.set_test_io(io);
            }
            let hooks = test_hooks::TestHooks::from_env();
            app.manage(test_hooks::AutostartOnce::new(hooks.autostart));
            app.manage(hooks);
            app.manage(core);
            restore_or_maximize(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_settings,
            commands::save_settings,
            commands::list_audio_devices,
            commands::backend_options,
            commands::boot,
            commands::start_conversation,
            commands::start_battle,
            commands::stop_conversation,
            commands::set_muted,
            commands::play_cue,
            commands::test_hooks,
            commands::test_log,
        ])
        .build(context)
        .expect("error while building the Haru application");
    app.run(|app, event| {
        // The plugin saves on CloseRequested (into its cache) and on Exit (to disk); also write the file as soon as
        // the main window is asked to close, so the state survives an exit path that skips the plugin's Exit hook.
        if let tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { .. }, .. } = &event {
            if label == MAIN_WINDOW {
                let _ = app.save_window_state(window_state_flags());
            }
        }
        if let tauri::RunEvent::Exit = event {
            if let Some(core) = app.try_state::<Arc<Core>>() {
                core.shutdown();
            }
        }
    });
}

#[cfg(test)]
mod window_state_tests {
    use super::has_saved_window_state;

    const SAVED: &str = r#"{"main":{"width":1200,"height":800,"x":10,"y":20,"prev_x":0,"prev_y":0,"maximized":true,"visible":true,"decorated":false,"fullscreen":false}}"#;

    #[test]
    fn saved_state_is_detected() {
        assert!(has_saved_window_state(SAVED, "main"));
    }

    #[test]
    fn other_label_garbage_and_empty_do_not_count() {
        assert!(!has_saved_window_state(SAVED, "other"));
        assert!(!has_saved_window_state("not json", "main"));
        assert!(!has_saved_window_state("{}", "main"));
    }

    #[test]
    fn zero_sized_default_entry_does_not_count() {
        let d = r#"{"main":{"width":0,"height":0,"x":0,"y":0,"prev_x":0,"prev_y":0,"maximized":false,"visible":true,"decorated":true,"fullscreen":false}}"#;
        assert!(!has_saved_window_state(d, "main"));
    }
}
