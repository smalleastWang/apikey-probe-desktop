pub mod probe;

pub use probe::{
    to_markdown,
    types::{
        CheckResult, CheckStatus, OverallConclusion, ProbeConfig, ProbeProgress, ProbeReport,
        RedactedProbeConfig, RiskAssessment, RiskLevel, RiskSeverity, RiskSignal, StepStatus,
    },
};

pub type ProgressCallback<'a> = dyn Fn(ProbeProgress) + Send + Sync + 'a;

pub async fn run_probe<'a>(
    config: ProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> anyhow::Result<ProbeReport> {
    probe::run_openai_compatible_probe(config, progress).await
}

pub fn to_json(report: &ProbeReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn to_summary(report: &ProbeReport) -> String {
    let mut lines = vec![
        format!("结论：{}", conclusion_string(report.conclusion)),
        format!("模型：{}", report.config.model),
        format!("协议：{}", report.config.protocol_type),
        format!(
            "风险评分：{}（{}）",
            report.risk.score,
            risk_level_string(report.risk.level)
        ),
        format!("结论说明：{}", report.conclusion_text),
        String::new(),
        "检测项：".to_string(),
    ];

    for check in &report.checks {
        lines.push(format!(
            "- [{}] {}: {}",
            check_status_string(check.status),
            check.label,
            check.summary
        ));
    }

    lines.join("\n")
}

pub fn infer_protocol_type(model: &str) -> Option<&'static str> {
    let normalized = model.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if has_model_prefix(&normalized, "claude") || normalized.contains("anthropic") {
        return Some("anthropic-messages");
    }

    if has_model_prefix(&normalized, "gemini") || normalized.contains("models/gemini") {
        return Some("google-gemini");
    }

    if is_gpt_5_family(&normalized) {
        return Some("openai-responses");
    }

    for prefix in [
        "gpt", "o1", "o3", "o4", "chatgpt", "deepseek", "qwen", "qwq", "moonshot", "kimi", "glm",
        "doubao", "yi", "llama", "mistral",
    ] {
        if has_model_prefix(&normalized, prefix) {
            return Some("openai-compatible");
        }
    }

    None
}

fn conclusion_string(value: OverallConclusion) -> &'static str {
    match value {
        OverallConclusion::Pass => "PASS",
        OverallConclusion::Warn => "WARN",
        OverallConclusion::Fail => "FAIL",
    }
}

fn check_status_string(value: CheckStatus) -> &'static str {
    match value {
        CheckStatus::Pass => "PASS",
        CheckStatus::Warn => "WARN",
        CheckStatus::Fail => "FAIL",
    }
}

fn risk_level_string(value: RiskLevel) -> &'static str {
    match value {
        RiskLevel::Low => "LOW",
        RiskLevel::Medium => "MEDIUM",
        RiskLevel::High => "HIGH",
    }
}

fn has_model_prefix(model: &str, prefix: &str) -> bool {
    if model == prefix || model.starts_with(&format!("{prefix}-")) {
        return true;
    }

    model
        .split(['/', '_', ':'])
        .any(|part| part == prefix || part.starts_with(&format!("{prefix}-")))
}

fn is_gpt_5_family(model: &str) -> bool {
    model
        .split(['/', '_', ':'])
        .any(|part| part == "gpt-5" || part.starts_with("gpt-5.") || part.starts_with("gpt-5-"))
}

#[cfg(test)]
mod tests {
    use super::infer_protocol_type;

    #[test]
    fn infers_gpt_5_family_as_openai_responses() {
        assert_eq!(infer_protocol_type("gpt-5.5"), Some("openai-responses"));
        assert_eq!(
            infer_protocol_type("openai/gpt-5-mini"),
            Some("openai-responses")
        );
    }

    #[test]
    fn infers_other_common_families() {
        assert_eq!(infer_protocol_type("gpt-4o"), Some("openai-compatible"));
        assert_eq!(
            infer_protocol_type("claude-3-5-sonnet-latest"),
            Some("anthropic-messages")
        );
        assert_eq!(infer_protocol_type("gemini-1.5-pro"), Some("google-gemini"));
    }
}
