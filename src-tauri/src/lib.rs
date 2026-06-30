mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::run_openai_compatible_probe,
            commands::export_report_json,
            commands::export_report_markdown,
            commands::infer_protocol_type,
            commands::save_report_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
