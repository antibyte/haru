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

use crate::ipc::STATE_EVENT;
use crate::session::{Core, CoreConfig};

pub fn run(context: tauri::Context<tauri::Wry>) {
    let app = tauri::Builder::default()
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
            commands::stop_conversation,
            commands::set_muted,
            commands::test_hooks,
            commands::test_log,
        ])
        .build(context)
        .expect("error while building the Haru application");
    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(core) = app.try_state::<Arc<Core>>() {
                core.shutdown();
            }
        }
    });
}
