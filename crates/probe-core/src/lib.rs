pub mod probe;

pub use probe::{
    to_markdown,
    types::{
        CheckResult, CheckStatus, MultiProtocolProbeConfig, MultiProtocolProbeReport,
        OverallConclusion, ProbeConfig, ProbeProgress, ProbeReport, RedactedProbeConfig,
        RiskAssessment, RiskLevel, RiskSeverity, RiskSignal, StepStatus,
    },
};

use chrono::Utc;

pub type ProgressCallback<'a> = dyn Fn(ProbeProgress) + Send + Sync + 'a;

pub async fn run_probe<'a>(
    config: ProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> anyhow::Result<ProbeReport> {
    probe::run_openai_compatible_probe(config, progress).await
}

/// 依次对多个协议运行探针，返回聚合报告。
///
/// 协议之间保持串行（避免对同一上游瞬时并发过多请求），
/// 每个协议内部的 tools/stream/JSON 检测仍然并行。
pub async fn run_multi_protocol_probe<'a>(
    config: MultiProtocolProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> anyhow::Result<MultiProtocolProbeReport> {
    let mut results: Vec<ProbeReport> = Vec::with_capacity(config.protocol_types.len());

    for protocol in &config.protocol_types {
        let protocol_label = protocol.clone();
        let single = config.to_single(protocol);
        let wrapped = move |mut update: ProbeProgress| {
            update.protocol = Some(protocol_label.clone());
            progress(update);
        };
        let report = run_probe(single, &wrapped).await?;
        results.push(report);
    }

    Ok(build_multi_protocol_report(config, results))
}

fn build_multi_protocol_report(
    config: MultiProtocolProbeConfig,
    results: Vec<ProbeReport>,
) -> MultiProtocolProbeReport {
    let (conclusion, best_protocol) = aggregate_conclusion(&results);
    let conclusion_text = multi_conclusion_text(conclusion, &results);
    MultiProtocolProbeReport {
        generated_at: Utc::now(),
        model: config.model,
        provider_name: config.provider_name,
        conclusion,
        conclusion_text,
        best_protocol,
        results,
    }
}

fn aggregate_conclusion(results: &[ProbeReport]) -> (OverallConclusion, Option<String>) {
    let mut best: Option<(&ProbeReport, u8)> = None;
    for report in results {
        let rank = conclusion_rank(report.conclusion);
        if best.map(|(_, best_rank)| rank > best_rank).unwrap_or(true) {
            best = Some((report, rank));
        }
    }

    match best {
        Some((report, _)) => {
            // 全部协议均为 FAIL 时，不再标注"表现最佳协议"，避免"最佳却失败"的矛盾。
            let best_protocol = if report.conclusion == OverallConclusion::Fail {
                None
            } else {
                Some(report.config.protocol_type.clone())
            };
            (report.conclusion, best_protocol)
        }
        None => (OverallConclusion::Fail, None),
    }
}

fn conclusion_rank(value: OverallConclusion) -> u8 {
    match value {
        OverallConclusion::Pass => 2,
        OverallConclusion::Warn => 1,
        OverallConclusion::Fail => 0,
    }
}

fn multi_conclusion_text(conclusion: OverallConclusion, results: &[ProbeReport]) -> String {
    match conclusion {
        OverallConclusion::Pass => {
            "建议接入：至少一种协议下基础聊天、tools、stream、JSON mode 等核心能力通过。"
                .to_string()
        }
        OverallConclusion::Warn => {
            "谨慎接入：所选协议下基础能力可能可用，但存在能力缺失、格式不标准或中等风险。"
                .to_string()
        }
        OverallConclusion::Fail => {
            let chat_failed = results
                .iter()
                .filter(|report| {
                    report
                        .checks
                        .iter()
                        .any(|check| check.key == "chat" && check.status == CheckStatus::Fail)
                })
                .count();
            let high_risk = results
                .iter()
                .filter(|report| report.risk.level == RiskLevel::High)
                .count();

            let mut reasons = Vec::new();
            if chat_failed > 0 {
                reasons.push(format!("{chat_failed} 个协议基础聊天失败"));
            }
            if high_risk > 0 {
                reasons.push(format!("{high_risk} 个协议逆向/中转风险偏高"));
            }

            if reasons.is_empty() {
                "不建议接入：所选协议均未通过。".to_string()
            } else {
                format!("不建议接入：所选协议均未通过（{}）。", reasons.join("，"))
            }
        }
    }
}

pub fn to_json(report: &ProbeReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn multi_to_json(report: &MultiProtocolProbeReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn multi_to_markdown(report: &MultiProtocolProbeReport) -> String {
    let mut lines = Vec::new();
    lines.push("# 上游 API Key / 模型多协议验货报告".to_string());
    lines.push(String::new());
    lines.push(format!("- 生成时间：{}", report.generated_at));
    lines.push(format!("- 模型名：{}", report.model));
    if let Some(provider_name) = &report.provider_name {
        lines.push(format!("- 供应商名称：{provider_name}"));
    }
    lines.push(format!("- 综合结论：{}", conclusion_string(report.conclusion)));
    if let Some(best) = &report.best_protocol {
        lines.push(format!("- 表现最佳协议：{best}"));
    }
    lines.push(format!("- 说明：{}", report.conclusion_text));
    lines.push(String::new());

    lines.push("## 各协议结论概览".to_string());
    lines.push(String::new());
    lines.push("| 协议 | 结论 | 风险分 |".to_string());
    lines.push("| --- | --- | --- |".to_string());
    for result in &report.results {
        lines.push(format!(
            "| {} | {} | {} |",
            result.config.protocol_type,
            conclusion_string(result.conclusion),
            result.risk.score
        ));
    }
    lines.push(String::new());

    for result in &report.results {
        lines.push(format!("## 协议：{}", result.config.protocol_type));
        lines.push(String::new());
        lines.push(to_markdown(result));
        lines.push(String::new());
    }

    lines.join("\n")
}

pub fn multi_to_summary(report: &MultiProtocolProbeReport) -> String {
    let mut lines = vec![
        format!("综合结论：{}", conclusion_string(report.conclusion)),
        format!("模型：{}", report.model),
    ];

    if let Some(best) = &report.best_protocol {
        lines.push(format!("表现最佳协议：{best}"));
    }
    lines.push(format!("结论说明：{}", report.conclusion_text));
    lines.push(String::new());
    lines.push("各协议结论：".to_string());

    for result in &report.results {
        lines.push(format!(
            "- [{}] {}（风险 {}，{}）",
            conclusion_string(result.conclusion),
            result.config.protocol_type,
            result.risk.score,
            risk_level_string(result.risk.level)
        ));
    }

    lines.join("\n")
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
