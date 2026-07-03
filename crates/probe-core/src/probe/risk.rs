use super::types::{
    CheckResult, CheckStatus, HttpProbeResponse, ProbeConfig, RiskAssessment, RiskLevel,
    RiskSeverity, RiskSignal, StreamProbeResponse,
};

pub fn assess_risk(
    config: &ProbeConfig,
    checks: &[CheckResult],
    http_responses: &[&HttpProbeResponse],
    stream_response: Option<&StreamProbeResponse>,
) -> RiskAssessment {
    let mut signals = Vec::new();

    // 每类风险信号最多计一次：对所有 HTTP 响应做聚合判断，避免同一特征按响应个数重复累加。
    let bodies_lower = http_responses
        .iter()
        .map(|response| response.body.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let header_blobs_lower = http_responses
        .iter()
        .map(|response| {
            response
                .headers
                .iter()
                .map(|(key, value)| format!("{key}: {value}"))
                .collect::<Vec<_>>()
                .join("\n")
                .to_ascii_lowercase()
        })
        .collect::<Vec<_>>();

    if bodies_lower
        .iter()
        .any(|body| looks_like_html_or_challenge(body))
    {
        signals.push(RiskSignal {
            key: "html_or_challenge".to_string(),
            label: "返回 HTML / 验证页".to_string(),
            severity: RiskSeverity::High,
            score: 35,
            evidence: "响应正文包含 html、cloudflare、captcha、login 等页面特征".to_string(),
        });
    }

    if bodies_lower.iter().any(|body| {
        body.contains("session")
            || body.contains("cookie")
            || body.contains("captcha")
            || body.contains("login")
    }) {
        signals.push(RiskSignal {
            key: "browser_session_artifact".to_string(),
            label: "出现浏览器会话特征".to_string(),
            severity: RiskSeverity::High,
            score: 25,
            evidence: "响应内容出现 session/cookie/captcha/login 相关关键词".to_string(),
        });
    }

    if header_blobs_lower.iter().any(|header_blob| {
        header_blob.contains("cloudflare")
            || header_blob.contains("cf-ray")
            || header_blob.contains("openresty")
            || header_blob.contains("nginx")
            || header_blob.contains("vercel")
            || header_blob.contains("x-powered-by")
    }) {
        signals.push(RiskSignal {
            key: "proxy_headers".to_string(),
            label: "响应头暴露中转服务".to_string(),
            severity: RiskSeverity::Medium,
            score: 12,
            evidence: "响应头中存在 cloudflare/openresty/nginx/vercel/x-powered-by 等服务特征"
                .to_string(),
        });
    }

    if checks
        .iter()
        .any(|check| check.key == "error_format" && check.status != CheckStatus::Pass)
    {
        signals.push(RiskSignal {
            key: "non_standard_error".to_string(),
            label: "错误格式不像官方".to_string(),
            severity: RiskSeverity::Medium,
            score: 15,
            evidence: "错误探针没有返回 OpenAI-compatible 的标准 error 对象".to_string(),
        });
    }

    if let Some(stream) = stream_response {
        if stream.status >= 400 || stream.data_events_seen == 0 || stream.invalid_json_events > 0 {
            signals.push(RiskSignal {
                key: "non_standard_sse".to_string(),
                label: "SSE 流式不标准".to_string(),
                severity: RiskSeverity::Medium,
                score: 15,
                evidence: format!(
                    "status={}, data_events={}, invalid_json_events={}",
                    stream.status, stream.data_events_seen, stream.invalid_json_events
                ),
            });
        }
    }

    // 仅在基础聊天通过（即 tools 确实被检测过）时才评估该信号，
    // 避免把"因聊天失败而跳过的 tools"误判为"高级模型不支持 tools"。
    let chat_failed = checks
        .iter()
        .any(|check| check.key == "chat" && check.status == CheckStatus::Fail);
    let claims_advanced_model = looks_like_advanced_model(&config.model);
    let tools_failed = checks
        .iter()
        .any(|check| check.key == "tools" && check.status != CheckStatus::Pass);
    if !chat_failed && claims_advanced_model && tools_failed {
        signals.push(RiskSignal {
            key: "advanced_model_without_tools".to_string(),
            label: "高级模型不支持 tools".to_string(),
            severity: RiskSeverity::High,
            score: 25,
            evidence: format!("模型名 `{}` 疑似高级模型，但 tools 探针失败", config.model),
        });
    }

    let score = signals
        .iter()
        .map(|signal| signal.score)
        .sum::<u32>()
        .min(100);
    let level = if score >= 50 {
        RiskLevel::High
    } else if score >= 20 {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };

    RiskAssessment {
        score,
        level,
        signals,
    }
}

fn looks_like_html_or_challenge(body_lower: &str) -> bool {
    body_lower.contains("<html")
        || body_lower.contains("<!doctype")
        || body_lower.contains("cloudflare")
        || body_lower.contains("cf-browser-verification")
        || body_lower.contains("captcha")
        || body_lower.contains("just a moment")
        || body_lower.contains("please enable cookies")
}

fn looks_like_advanced_model(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    [
        "gpt-4",
        "gpt-4o",
        "gpt-5",
        "o1",
        "o3",
        "claude",
        "gemini",
        "deepseek-r1",
        "qwen-max",
        "kimi-k2",
    ]
    .iter()
    .any(|keyword| model.contains(keyword))
}
