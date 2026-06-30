use apikey_probe_core::{self as probe_core, ProbeConfig, ProbeProgress, ProbeReport};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn run_openai_compatible_probe(
    app: AppHandle,
    config: ProbeConfig,
) -> Result<ProbeReport, String> {
    let emit_progress = |progress: ProbeProgress| {
        let _ = app.emit("probe-progress", progress);
    };

    probe_core::run_probe(config, &emit_progress)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_report_json(report: ProbeReport) -> Result<String, String> {
    probe_core::to_json(&report).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_report_markdown(report: ProbeReport) -> Result<String, String> {
    Ok(probe_core::to_markdown(&report))
}

#[tauri::command]
pub fn infer_protocol_type(model: String) -> Option<String> {
    probe_core::infer_protocol_type(&model).map(str::to_string)
}

#[tauri::command]
pub fn save_report_file(
    directory: String,
    filename: String,
    content: String,
) -> Result<String, String> {
    let safe_filename = sanitize_filename(&filename)?;
    let mut path = PathBuf::from(directory);

    if !path.is_dir() {
        return Err("选择的路径不是文件夹".to_string());
    }

    path.push(safe_filename);
    std::fs::write(&path, content).map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

fn sanitize_filename(filename: &str) -> Result<String, String> {
    let trimmed = filename.trim();
    if trimmed.is_empty() {
        return Err("文件名不能为空".to_string());
    }

    let sanitized = trimmed
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect::<String>();

    Ok(sanitized)
}
