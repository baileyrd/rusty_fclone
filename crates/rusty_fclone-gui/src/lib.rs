mod commands;
mod payload;
mod preview;
mod profiles;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::start_scan,
            commands::run_action,
            commands::choose_keep,
            commands::find_duplicate_folders,
            commands::run_folder_action,
            commands::read_preview,
            commands::list_scan_profiles,
            commands::save_scan_profile,
            commands::delete_scan_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
