mod commands;
mod payload;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
            commands::run_action,
            commands::find_duplicate_folders,
            commands::run_folder_action,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
