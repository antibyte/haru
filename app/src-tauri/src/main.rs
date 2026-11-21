#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    haru_lib::run(tauri::generate_context!())
}
