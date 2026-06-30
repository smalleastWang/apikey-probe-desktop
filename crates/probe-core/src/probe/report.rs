use super::types::{CheckStatus, OverallConclusion, ProbeReport, RiskLevel};

pub fn conclusion_for(report: &ProbeReport) -> OverallConclusion {
    if report
        .checks
        .iter()
        .any(|check| check.key == "chat" && check.status == CheckStatus::Fail)
        || report.risk.level == RiskLevel::High
    {
        return OverallConclusion::Fail;
    }

    if report
        .checks
        .iter()
        .any(|check| check.status != CheckStatus::Pass)
        || report.risk.level == RiskLevel::Medium
    {
        return OverallConclusion::Warn;
    }

    OverallConclusion::Pass
}

pub fn conclusion_text(conclusion: OverallConclusion) -> String {
    match conclusion {
        OverallConclusion::Pass => {
            "建议接入：基础聊天、tools、stream、JSON mode 等核心能力通过，逆向风险较低。"
                .to_string()
        }
        OverallConclusion::Warn => {
            "谨慎接入：基础能力可能可用，但存在能力缺失、格式不标准或中等风险。".to_string()
        }
        OverallConclusion::Fail => {
            "不建议接入：基础聊天失败，或存在高逆向/中转/不稳定供货风险。".to_string()
        }
    }
}

pub fn to_markdown(report: &ProbeReport) -> String {
    let mut lines = Vec::new();
    lines.push("# 上游 API Key / 模型验货报告".to_string());
    lines.push(String::new());
    lines.push(format!("- 生成时间：{}", report.generated_at));
    lines.push(format!("- 结论：{:?}", report.conclusion));
    lines.push(format!("- 说明：{}", report.conclusion_text));
    lines.push(String::new());

    lines.push("## 配置".to_string());
    lines.push(String::new());
    lines.push(format!("- Base URL：{}", report.config.base_url));
    lines.push(format!("- API Key：{}", report.config.api_key));
    lines.push(format!("- 模型名：{}", report.config.model));
    lines.push(format!("- 协议类型：{}", report.config.protocol_type));
    if let Some(provider_name) = &report.config.provider_name {
        lines.push(format!("- 供应商名称：{provider_name}"));
    }
    if let Some(note) = &report.config.note {
        lines.push(format!("- 备注：{note}"));
    }
    lines.push(String::new());

    lines.push("## 检测项".to_string());
    lines.push(String::new());
    for check in &report.checks {
        lines.push(format!("### {} - {:?}", check.label, check.status));
        lines.push(String::new());
        lines.push(check.summary.clone());
        if !check.evidence.is_empty() {
            lines.push(String::new());
            lines.push("证据：".to_string());
            for evidence in &check.evidence {
                lines.push(format!("- {evidence}"));
            }
        }
        if let Some(raw_preview) = &check.raw_preview {
            lines.push(String::new());
            lines.push("响应预览：".to_string());
            lines.push("```text".to_string());
            lines.push(raw_preview.clone());
            lines.push("```".to_string());
        }
        lines.push(String::new());
    }

    lines.push("## 逆向风险".to_string());
    lines.push(String::new());
    lines.push(format!("- 风险分：{}", report.risk.score));
    lines.push(format!("- 风险等级：{:?}", report.risk.level));
    if report.risk.signals.is_empty() {
        lines.push("- 未发现明显逆向/中转风险信号。".to_string());
    } else {
        for signal in &report.risk.signals {
            lines.push(format!(
                "- {}：{:?}，+{}，{}",
                signal.label, signal.severity, signal.score, signal.evidence
            ));
        }
    }

    lines.join("\n")
}
