use apikey_probe_core::{
    self as probe_core, MultiProtocolProbeConfig, MultiProtocolProbeReport, ProbeConfig,
    ProbeProgress, ProbeReport,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

/// 前端主动取消检测时返回的标记信息（前端据此显示为"已取消"而非报错）。
pub const CANCELED_MESSAGE: &str = "PROBE_CANCELED";

/// 按 session_id 记录进行中的检测任务，供主动取消使用。
///
/// 每次运行分配一个自增 run_id，用于在任务结束时判断登记项是否仍属于本次运行，
/// 避免误删同一 session 下的后续任务。
#[derive(Default)]
pub struct ProbeCancellation {
    tokens: Mutex<HashMap<String, (u64, CancellationToken)>>,
    next_id: AtomicU64,
}

impl ProbeCancellation {
    /// 注册一个新任务并返回其运行编号与取消令牌。若同 session 已有旧任务，先取消旧任务。
    fn register(&self, session_id: &str) -> (u64, CancellationToken) {
        let token = CancellationToken::new();
        let run_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if !session_id.is_empty() {
            let mut tokens = self.tokens.lock().expect("cancellation lock poisoned");
            if let Some((_, previous)) =
                tokens.insert(session_id.to_string(), (run_id, token.clone()))
            {
                previous.cancel();
            }
        }
        (run_id, token)
    }

    /// 任务结束后移除登记（仅当仍是本次运行时才移除，避免误删同 session 的新任务）。
    fn finish(&self, session_id: &str, run_id: u64) {
        if session_id.is_empty() {
            return;
        }
        let mut tokens = self.tokens.lock().expect("cancellation lock poisoned");
        if tokens
            .get(session_id)
            .map(|(id, _)| *id == run_id)
            .unwrap_or(false)
        {
            tokens.remove(session_id);
        }
    }

    fn cancel(&self, session_id: &str) {
        let tokens = self.tokens.lock().expect("cancellation lock poisoned");
        if let Some((_, token)) = tokens.get(session_id) {
            token.cancel();
        }
    }
}

#[tauri::command]
pub async fn run_openai_compatible_probe(
    app: AppHandle,
    cancellation: State<'_, ProbeCancellation>,
    config: ProbeConfig,
    session_id: String,
) -> Result<ProbeReport, String> {
    let event = progress_event_name(&session_id);
    let emit_progress = move |progress: ProbeProgress| {
        let _ = app.emit(event.as_str(), progress);
    };

    let (run_id, token) = cancellation.register(&session_id);
    let result = tokio::select! {
        biased;
        _ = token.cancelled() => Err(CANCELED_MESSAGE.to_string()),
        result = probe_core::run_probe(config, &emit_progress) => {
            result.map_err(|error| error.to_string())
        }
    };
    cancellation.finish(&session_id, run_id);
    result
}

#[tauri::command]
pub async fn run_multi_protocol_probe(
    app: AppHandle,
    cancellation: State<'_, ProbeCancellation>,
    config: MultiProtocolProbeConfig,
    session_id: String,
) -> Result<MultiProtocolProbeReport, String> {
    let event = progress_event_name(&session_id);
    let emit_progress = move |progress: ProbeProgress| {
        let _ = app.emit(event.as_str(), progress);
    };

    let (run_id, token) = cancellation.register(&session_id);
    let result = tokio::select! {
        biased;
        _ = token.cancelled() => Err(CANCELED_MESSAGE.to_string()),
        result = probe_core::run_multi_protocol_probe(config, &emit_progress) => {
            result.map_err(|error| error.to_string())
        }
    };
    cancellation.finish(&session_id, run_id);
    result
}

/// 主动取消指定 session 正在进行的检测。
#[tauri::command]
pub fn cancel_probe(cancellation: State<'_, ProbeCancellation>, session_id: String) {
    cancellation.cancel(&session_id);
}

fn progress_event_name(session_id: &str) -> String {
    if session_id.is_empty() {
        "probe-progress".to_string()
    } else {
        format!("probe-progress:{session_id}")
    }
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
pub fn export_multi_report_json(report: MultiProtocolProbeReport) -> Result<String, String> {
    probe_core::multi_to_json(&report).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_multi_report_markdown(report: MultiProtocolProbeReport) -> Result<String, String> {
    Ok(probe_core::multi_to_markdown(&report))
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
