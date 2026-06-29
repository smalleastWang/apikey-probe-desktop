mod http_client;
mod report;
mod risk;
pub mod types;

use self::{
    http_client::ProbeHttpClient,
    report::{conclusion_for, conclusion_text},
    risk::assess_risk,
    types::{
        CheckResult, CheckStatus, HttpProbeResponse, OverallConclusion, ProbeConfig, ProbeProgress,
        ProbeReport, RedactedProbeConfig, RiskAssessment, StepStatus, StreamProbeResponse,
    },
};
use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};

pub use report::to_markdown;
type ProgressCallback<'a> = dyn Fn(ProbeProgress) + Send + Sync + 'a;

pub async fn run_openai_compatible_probe<'a>(
    config: ProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> Result<ProbeReport> {
    match config.protocol_type.as_str() {
        "openai-compatible" => run_openai_chat_probe(config, progress).await,
        "openai-responses" => run_openai_responses_probe(config, progress).await,
        "anthropic-messages" => run_anthropic_messages_probe(config, progress).await,
        "google-gemini" => run_gemini_probe(config, progress).await,
        _ => Ok(single_fail_report(
            &config,
            "protocol",
            "协议类型",
            format!("暂不支持的协议类型：{}", config.protocol_type),
        )),
    }
}

async fn run_openai_chat_probe<'a>(
    config: ProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> Result<ProbeReport> {
    let client = ProbeHttpClient::new(&config)?;
    let mut checks = Vec::new();
    let mut http_responses: Vec<HttpProbeResponse> = Vec::new();

    progress(step(
        "chat",
        "基础聊天",
        StepStatus::Running,
        "正在测试基础 Chat Completions",
    ));
    let (chat_check, chat_response) = probe_chat(&client, &config).await;
    progress(step_from_check(&chat_check));
    if let Some(response) = chat_response {
        http_responses.push(response);
    }
    checks.push(chat_check);

    progress(step(
        "tools",
        "Tools / Function Calling",
        StepStatus::Running,
        "正在强制模型调用 get_weather",
    ));
    let (tools_check, tools_response) = probe_tools(&client, &config).await;
    progress(step_from_check(&tools_check));
    if let Some(response) = tools_response {
        http_responses.push(response);
    }
    checks.push(tools_check);

    progress(step(
        "stream",
        "Stream 流式",
        StepStatus::Running,
        "正在检测 SSE 流式格式",
    ));
    let (stream_check, stream_result) = probe_stream(&client, &config).await;
    progress(step_from_check(&stream_check));
    let stream_response = stream_result;
    checks.push(stream_check);

    progress(step(
        "json_mode",
        "JSON Mode",
        StepStatus::Running,
        "正在检测 response_format json_object",
    ));
    let (json_mode_check, json_mode_response) = probe_json_mode(&client, &config).await;
    progress(step_from_check(&json_mode_check));
    if let Some(response) = json_mode_response {
        http_responses.push(response);
    }
    checks.push(json_mode_check);

    progress(step(
        "error_format",
        "错误格式",
        StepStatus::Running,
        "正在检测错误响应是否接近官方格式",
    ));
    let (error_check, error_response) = probe_error_format(&client).await;
    progress(step_from_check(&error_check));
    if let Some(response) = error_response {
        http_responses.push(response);
    }
    checks.push(error_check);

    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Running,
        "正在汇总风险信号",
    ));
    let response_refs = http_responses.iter().collect::<Vec<_>>();
    let risk = assess_risk(&config, &checks, &response_refs, stream_response.as_ref());

    let report = build_report(&config, checks, risk);

    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Pass,
        "风险评分完成",
    ));
    Ok(report)
}

async fn run_openai_responses_probe<'a>(
    config: ProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> Result<ProbeReport> {
    let client = ProbeHttpClient::new(&config)?;
    let url = responses_url(&config.base_url);
    let mut checks = Vec::new();
    let mut http_responses: Vec<HttpProbeResponse> = Vec::new();

    progress(step(
        "chat",
        "基础聊天",
        StepStatus::Running,
        "正在测试 OpenAI Responses API",
    ));
    let (chat_check, chat_response) = probe_responses_chat(&client, &config, &url).await;
    progress(step_from_check(&chat_check));
    if let Some(response) = chat_response {
        http_responses.push(response);
    }
    checks.push(chat_check);

    progress(step(
        "tools",
        "Tools / Function Calling",
        StepStatus::Running,
        "正在检测 Responses function_call",
    ));
    let (tools_check, tools_response) = probe_responses_tools(&client, &config, &url).await;
    progress(step_from_check(&tools_check));
    if let Some(response) = tools_response {
        http_responses.push(response);
    }
    checks.push(tools_check);

    progress(step(
        "stream",
        "Stream 流式",
        StepStatus::Running,
        "正在检测 Responses SSE",
    ));
    let (stream_check, stream_response) = probe_responses_stream(&client, &config, &url).await;
    progress(step_from_check(&stream_check));
    checks.push(stream_check);

    progress(step(
        "json_mode",
        "JSON Mode",
        StepStatus::Running,
        "正在检测 Responses JSON schema",
    ));
    let (json_check, json_response) = probe_responses_json_mode(&client, &config, &url).await;
    progress(step_from_check(&json_check));
    if let Some(response) = json_response {
        http_responses.push(response);
    }
    checks.push(json_check);

    progress(step(
        "error_format",
        "错误格式",
        StepStatus::Running,
        "正在检测 Responses 错误格式",
    ));
    let (error_check, error_response) = probe_openai_style_error(&client, &url).await;
    progress(step_from_check(&error_check));
    if let Some(response) = error_response {
        http_responses.push(response);
    }
    checks.push(error_check);

    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Running,
        "正在汇总风险信号",
    ));
    let response_refs = http_responses.iter().collect::<Vec<_>>();
    let risk = assess_risk(&config, &checks, &response_refs, stream_response.as_ref());
    let report = build_report(&config, checks, risk);
    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Pass,
        "风险评分完成",
    ));
    Ok(report)
}

async fn run_anthropic_messages_probe<'a>(
    config: ProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> Result<ProbeReport> {
    let client = ProbeHttpClient::new(&config)?;
    let url = anthropic_messages_url(&config.base_url);
    let headers = anthropic_headers(&config);
    let mut checks = Vec::new();
    let mut http_responses: Vec<HttpProbeResponse> = Vec::new();

    progress(step(
        "chat",
        "基础聊天",
        StepStatus::Running,
        "正在测试 Anthropic Messages API",
    ));
    let (chat_check, chat_response) = probe_anthropic_chat(&client, &config, &url, &headers).await;
    progress(step_from_check(&chat_check));
    if let Some(response) = chat_response {
        http_responses.push(response);
    }
    checks.push(chat_check);

    progress(step(
        "tools",
        "Tools / Function Calling",
        StepStatus::Running,
        "正在检测 Anthropic tool_use",
    ));
    let (tools_check, tools_response) =
        probe_anthropic_tools(&client, &config, &url, &headers).await;
    progress(step_from_check(&tools_check));
    if let Some(response) = tools_response {
        http_responses.push(response);
    }
    checks.push(tools_check);

    progress(step(
        "stream",
        "Stream 流式",
        StepStatus::Running,
        "正在检测 Anthropic SSE",
    ));
    let (stream_check, stream_response) =
        probe_anthropic_stream(&client, &config, &url, &headers).await;
    progress(step_from_check(&stream_check));
    checks.push(stream_check);

    progress(step(
        "json_mode",
        "JSON Mode",
        StepStatus::Running,
        "正在检测 Anthropic JSON 等效输出",
    ));
    let (json_check, json_response) =
        probe_anthropic_json_mode(&client, &config, &url, &headers).await;
    progress(step_from_check(&json_check));
    if let Some(response) = json_response {
        http_responses.push(response);
    }
    checks.push(json_check);

    progress(step(
        "error_format",
        "错误格式",
        StepStatus::Running,
        "正在检测 Anthropic 错误格式",
    ));
    let (error_check, error_response) = probe_anthropic_error(&client, &url, &headers).await;
    progress(step_from_check(&error_check));
    if let Some(response) = error_response {
        http_responses.push(response);
    }
    checks.push(error_check);

    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Running,
        "正在汇总风险信号",
    ));
    let response_refs = http_responses.iter().collect::<Vec<_>>();
    let risk = assess_risk(&config, &checks, &response_refs, stream_response.as_ref());
    let report = build_report(&config, checks, risk);
    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Pass,
        "风险评分完成",
    ));
    Ok(report)
}

async fn run_gemini_probe<'a>(
    config: ProbeConfig,
    progress: &'a ProgressCallback<'a>,
) -> Result<ProbeReport> {
    let client = ProbeHttpClient::new(&config)?;
    let url = gemini_url(&config, "generateContent");
    let stream_url = gemini_url(&config, "streamGenerateContent");
    let mut checks = Vec::new();
    let mut http_responses: Vec<HttpProbeResponse> = Vec::new();

    progress(step(
        "chat",
        "基础聊天",
        StepStatus::Running,
        "正在测试 Gemini generateContent",
    ));
    let (chat_check, chat_response) = probe_gemini_chat(&client, &config, &url).await;
    progress(step_from_check(&chat_check));
    if let Some(response) = chat_response {
        http_responses.push(response);
    }
    checks.push(chat_check);

    progress(step(
        "tools",
        "Tools / Function Calling",
        StepStatus::Running,
        "正在检测 Gemini functionCall",
    ));
    let (tools_check, tools_response) = probe_gemini_tools(&client, &config, &url).await;
    progress(step_from_check(&tools_check));
    if let Some(response) = tools_response {
        http_responses.push(response);
    }
    checks.push(tools_check);

    progress(step(
        "stream",
        "Stream 流式",
        StepStatus::Running,
        "正在检测 Gemini SSE",
    ));
    let (stream_check, stream_response) = probe_gemini_stream(&client, &config, &stream_url).await;
    progress(step_from_check(&stream_check));
    checks.push(stream_check);

    progress(step(
        "json_mode",
        "JSON Mode",
        StepStatus::Running,
        "正在检测 Gemini responseMimeType",
    ));
    let (json_check, json_response) = probe_gemini_json_mode(&client, &config, &url).await;
    progress(step_from_check(&json_check));
    if let Some(response) = json_response {
        http_responses.push(response);
    }
    checks.push(json_check);

    progress(step(
        "error_format",
        "错误格式",
        StepStatus::Running,
        "正在检测 Gemini 错误格式",
    ));
    let (error_check, error_response) = probe_gemini_error(&client, &config).await;
    progress(step_from_check(&error_check));
    if let Some(response) = error_response {
        http_responses.push(response);
    }
    checks.push(error_check);

    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Running,
        "正在汇总风险信号",
    ));
    let response_refs = http_responses.iter().collect::<Vec<_>>();
    let risk = assess_risk(&config, &checks, &response_refs, stream_response.as_ref());
    let report = build_report(&config, checks, risk);
    progress(step(
        "risk",
        "逆向风险评分",
        StepStatus::Pass,
        "风险评分完成",
    ));
    Ok(report)
}

async fn probe_chat(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "messages": [
            {"role": "system", "content": "你是用于 API 兼容性验货的测试助手。"},
            {"role": "user", "content": "请只回复：probe-ok"}
        ],
        "temperature": 0
    });

    match client.post_chat_completions(payload).await {
        Ok(response) => {
            let preview = preview(&response.body);
            let check = if response.status < 400
                && response
                    .json
                    .as_ref()
                    .and_then(|json| json.pointer("/choices/0/message/content"))
                    .is_some()
            {
                CheckResult::pass("chat", "基础聊天", "Chat Completions 基础聊天可用")
                    .with_evidence(format!("HTTP {}", response.status))
                    .with_raw_preview(preview)
            } else {
                CheckResult::fail("chat", "基础聊天", "基础聊天失败，不能说明该上游可接入")
                    .with_evidence(format!("HTTP {}", response.status))
                    .with_raw_preview(preview)
            };
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::fail("chat", "基础聊天", "请求失败，基础能力不可用")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_tools(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "messages": [
            {"role": "user", "content": "请调用 get_weather 工具查询北京天气。不要直接回答自然语言。"}
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "获取指定城市的天气",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string", "description": "城市名称"}
                        },
                        "required": ["city"]
                    }
                }
            }
        ],
        "tool_choice": {
            "type": "function",
            "function": {"name": "get_weather"}
        },
        "temperature": 0
    });

    match client.post_chat_completions(payload).await {
        Ok(response) => {
            let preview = preview(&response.body);
            let validation = validate_tool_call(response.json.as_ref());
            let check = match validation {
                Ok(evidence) => CheckResult::pass(
                    "tools",
                    "Tools / Function Calling",
                    "支持 tools/tool_choice，并能强制返回标准 tool_calls",
                )
                .with_evidence(evidence)
                .with_raw_preview(preview),
                Err(reason) => CheckResult::warn(
                    "tools",
                    "Tools / Function Calling",
                    "不支持或未正确实现 tools，不能视为满血版",
                )
                .with_evidence(format!("HTTP {}", response.status))
                .with_evidence(reason)
                .with_raw_preview(preview),
            };
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn(
                "tools",
                "Tools / Function Calling",
                "tools 探针请求失败，不能视为满血版",
            )
            .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_stream(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
) -> (CheckResult, Option<StreamProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "messages": [
            {"role": "user", "content": "请用一句话回复 stream-probe-ok"}
        ],
        "stream": true,
        "temperature": 0
    });

    match client.stream_chat_completions(payload).await {
        Ok(response) => {
            let preview = preview(&response.body_preview);
            let check = if response.status < 400
                && response.data_events_seen > 0
                && response.invalid_json_events == 0
                && response.done_seen
            {
                CheckResult::pass(
                    "stream",
                    "Stream 流式",
                    "支持标准 SSE data 事件和 [DONE] 结束",
                )
                .with_evidence(format!(
                    "chunks={}, data_events={}",
                    response.chunks_seen, response.data_events_seen
                ))
                .with_raw_preview(preview)
            } else if response.status < 400
                && response.data_events_seen > 0
                && response.invalid_json_events == 0
            {
                CheckResult::warn(
                    "stream",
                    "Stream 流式",
                    "可收到 SSE data 事件，但结束标记不完整",
                )
                .with_evidence(format!(
                    "done_seen={}, chunks={}, data_events={}",
                    response.done_seen, response.chunks_seen, response.data_events_seen
                ))
                .with_raw_preview(preview)
            } else {
                CheckResult::warn(
                    "stream",
                    "Stream 流式",
                    "stream 不标准，前端实时体验存在风险",
                )
                .with_evidence(format!(
                    "HTTP {}, data_events={}, invalid_json_events={}",
                    response.status, response.data_events_seen, response.invalid_json_events
                ))
                .with_raw_preview(preview)
            };
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("stream", "Stream 流式", "stream 探针请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_json_mode(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "messages": [
            {"role": "system", "content": "你必须输出合法 JSON，不要输出 Markdown。"},
            {"role": "user", "content": "输出一个 JSON 对象，字段 ok=true，city='北京'。"}
        ],
        "response_format": {"type": "json_object"},
        "temperature": 0
    });

    match client.post_chat_completions(payload).await {
        Ok(response) => {
            let preview = preview(&response.body);
            let content = response
                .json
                .as_ref()
                .and_then(|json| json.pointer("/choices/0/message/content"))
                .and_then(Value::as_str);

            let check = if response.status < 400
                && content
                    .and_then(|content| serde_json::from_str::<Value>(content).ok())
                    .is_some()
            {
                CheckResult::pass(
                    "json_mode",
                    "JSON Mode",
                    "支持 response_format=json_object 且内容为合法 JSON",
                )
                .with_raw_preview(preview)
            } else {
                CheckResult::warn(
                    "json_mode",
                    "JSON Mode",
                    "JSON Mode 不稳定或不支持，结构化任务存在风险",
                )
                .with_evidence(format!("HTTP {}", response.status))
                .with_raw_preview(preview)
            };
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("json_mode", "JSON Mode", "JSON Mode 探针请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_error_format(client: &ProbeHttpClient) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": "__apikey_probe_missing_model__",
        "messages": [
            {"role": "user", "content": "error-format-probe"}
        ]
    });

    match client.post_chat_completions(payload).await {
        Ok(response) => {
            let preview = preview(&response.body);
            let error = response.json.as_ref().and_then(|json| json.get("error"));
            let has_official_shape = response.status >= 400
                && error.and_then(|error| error.get("message")).is_some()
                && error
                    .and_then(|error| error.as_object())
                    .map(|object| object.contains_key("type") || object.contains_key("code"))
                    .unwrap_or(false);

            let check = if has_official_shape {
                CheckResult::pass(
                    "error_format",
                    "错误格式",
                    "错误响应接近 OpenAI 官方 error 对象",
                )
                .with_evidence(format!("HTTP {}", response.status))
                .with_raw_preview(preview)
            } else if response.status < 400 {
                CheckResult::warn(
                    "error_format",
                    "错误格式",
                    "无效模型仍返回成功，错误处理不可信",
                )
                .with_raw_preview(preview)
            } else {
                CheckResult::warn(
                    "error_format",
                    "错误格式",
                    "错误响应格式不像 OpenAI 官方接口",
                )
                .with_evidence(format!("HTTP {}", response.status))
                .with_raw_preview(preview)
            };
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("error_format", "错误格式", "错误格式探针请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_responses_chat(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "input": "请只回复：probe-ok",
        "temperature": 0
    });

    match client.post_json_bearer(url, payload).await {
        Ok(response) => {
            let text = responses_text(response.json.as_ref());
            let check = if response.status < 400 && text.is_some() {
                CheckResult::pass("chat", "基础聊天", "OpenAI Responses 基础输出可用")
                    .with_evidence(format!("HTTP {}", response.status))
                    .with_raw_preview(preview(&response.body))
            } else {
                CheckResult::fail("chat", "基础聊天", "OpenAI Responses 基础输出失败")
                    .with_evidence(format!("HTTP {}", response.status))
                    .with_raw_preview(preview(&response.body))
            };
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::fail("chat", "基础聊天", "OpenAI Responses 请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_responses_tools(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "input": "请调用 get_weather 工具查询北京天气。不要直接回答自然语言。",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "获取指定城市的天气",
            "parameters": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        }],
        "tool_choice": {"type": "function", "name": "get_weather"},
        "temperature": 0
    });

    match client.post_json_bearer(url, payload).await {
        Ok(response) => {
            let validation = validate_responses_function_call(response.json.as_ref());
            let check = match validation {
                Ok(evidence) => CheckResult::pass(
                    "tools",
                    "Tools / Function Calling",
                    "Responses API 支持 function_call",
                )
                .with_evidence(evidence),
                Err(reason) => CheckResult::warn(
                    "tools",
                    "Tools / Function Calling",
                    "Responses function_call 不完整，不能视为满血版",
                )
                .with_evidence(reason),
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn(
                "tools",
                "Tools / Function Calling",
                "Responses tools 请求失败",
            )
            .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_responses_stream(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<StreamProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "input": "请用一句话回复 stream-probe-ok",
        "stream": true,
        "temperature": 0
    });
    stream_check_from_result(
        "stream",
        "Stream 流式",
        client.stream_json_bearer(url, payload).await,
    )
}

async fn probe_responses_json_mode(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "input": "输出一个 JSON 对象，字段 ok=true，city='北京'。",
        "text": {
            "format": {
                "type": "json_schema",
                "name": "probe_result",
                "schema": {
                    "type": "object",
                    "properties": {
                        "ok": {"type": "boolean"},
                        "city": {"type": "string"}
                    },
                    "required": ["ok", "city"],
                    "additionalProperties": false
                },
                "strict": true
            }
        },
        "temperature": 0
    });

    match client.post_json_bearer(url, payload).await {
        Ok(response) => {
            let ok = responses_text(response.json.as_ref())
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .is_some();
            let check = if response.status < 400 && ok {
                CheckResult::pass(
                    "json_mode",
                    "JSON Mode",
                    "Responses JSON schema 输出为合法 JSON",
                )
            } else {
                CheckResult::warn(
                    "json_mode",
                    "JSON Mode",
                    "Responses JSON schema 不支持或输出不稳定",
                )
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("json_mode", "JSON Mode", "Responses JSON Mode 请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_openai_style_error(
    client: &ProbeHttpClient,
    url: &str,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": "__apikey_probe_missing_model__",
        "input": "error-format-probe"
    });
    match client.post_json_bearer(url, payload).await {
        Ok(response) => {
            let error = response.json.as_ref().and_then(|json| json.get("error"));
            let ok = response.status >= 400
                && error.and_then(|error| error.get("message")).is_some()
                && error
                    .and_then(|error| error.as_object())
                    .map(|object| object.contains_key("type") || object.contains_key("code"))
                    .unwrap_or(false);
            let check = if ok {
                CheckResult::pass(
                    "error_format",
                    "错误格式",
                    "错误响应接近 OpenAI 官方 error 对象",
                )
            } else {
                CheckResult::warn(
                    "error_format",
                    "错误格式",
                    "错误响应格式不像 OpenAI 官方接口",
                )
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("error_format", "错误格式", "错误格式探针请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_anthropic_chat(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
    headers: &[(&str, String)],
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "请只回复：probe-ok"}]
    });
    match client.post_json_with_headers(url, headers, payload).await {
        Ok(response) => {
            let ok = response.status < 400
                && response
                    .json
                    .as_ref()
                    .and_then(|json| json.pointer("/content/0/text"))
                    .is_some();
            let check = if ok {
                CheckResult::pass("chat", "基础聊天", "Anthropic Messages 基础输出可用")
            } else {
                CheckResult::fail("chat", "基础聊天", "Anthropic Messages 基础输出失败")
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::fail("chat", "基础聊天", "Anthropic Messages 请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_anthropic_tools(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
    headers: &[(&str, String)],
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "max_tokens": 256,
        "messages": [{"role": "user", "content": "请调用 get_weather 工具查询北京天气。"}],
        "tools": [{
            "name": "get_weather",
            "description": "获取指定城市的天气",
            "input_schema": {
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }
        }],
        "tool_choice": {"type": "tool", "name": "get_weather"}
    });
    match client.post_json_with_headers(url, headers, payload).await {
        Ok(response) => {
            let validation = validate_anthropic_tool_use(response.json.as_ref());
            let check = match validation {
                Ok(evidence) => CheckResult::pass(
                    "tools",
                    "Tools / Function Calling",
                    "Anthropic Messages 支持 tool_use",
                )
                .with_evidence(evidence),
                Err(reason) => CheckResult::warn(
                    "tools",
                    "Tools / Function Calling",
                    "Anthropic tool_use 不完整，不能视为满血版",
                )
                .with_evidence(reason),
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn(
                "tools",
                "Tools / Function Calling",
                "Anthropic tools 请求失败",
            )
            .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_anthropic_stream(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
    headers: &[(&str, String)],
) -> (CheckResult, Option<StreamProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "max_tokens": 64,
        "stream": true,
        "messages": [{"role": "user", "content": "请用一句话回复 stream-probe-ok"}]
    });
    stream_check_from_result(
        "stream",
        "Stream 流式",
        client.stream_json_with_headers(url, headers, payload).await,
    )
}

async fn probe_anthropic_json_mode(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
    headers: &[(&str, String)],
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": config.model.as_str(),
        "max_tokens": 128,
        "messages": [{"role": "user", "content": "只输出合法 JSON：{\"ok\":true,\"city\":\"北京\"}，不要 Markdown。"}]
    });
    match client.post_json_with_headers(url, headers, payload).await {
        Ok(response) => {
            let ok = response
                .json
                .as_ref()
                .and_then(|json| json.pointer("/content/0/text"))
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .is_some();
            let check = if response.status < 400 && ok {
                CheckResult::pass(
                    "json_mode",
                    "JSON Mode",
                    "Anthropic 等效 JSON 输出为合法 JSON",
                )
            } else {
                CheckResult::warn(
                    "json_mode",
                    "JSON Mode",
                    "Anthropic 没有 OpenAI 式 JSON Mode，等效 JSON 输出不稳定",
                )
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("json_mode", "JSON Mode", "Anthropic JSON 等效探针请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_anthropic_error(
    client: &ProbeHttpClient,
    url: &str,
    headers: &[(&str, String)],
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "model": "__apikey_probe_missing_model__",
        "max_tokens": 16,
        "messages": [{"role": "user", "content": "error-format-probe"}]
    });
    match client.post_json_with_headers(url, headers, payload).await {
        Ok(response) => {
            let ok = response.status >= 400
                && response
                    .json
                    .as_ref()
                    .and_then(|json| json.pointer("/error/message"))
                    .is_some();
            let check = if ok {
                CheckResult::pass(
                    "error_format",
                    "错误格式",
                    "错误响应接近 Anthropic 官方 error 对象",
                )
            } else {
                CheckResult::warn(
                    "error_format",
                    "错误格式",
                    "错误响应格式不像 Anthropic 官方接口",
                )
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("error_format", "错误格式", "Anthropic 错误格式探针请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_gemini_chat(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "contents": [{"role": "user", "parts": [{"text": "请只回复：probe-ok"}]}]
    });
    match client.post_json_with_headers(url, &[], payload).await {
        Ok(response) => {
            let ok = response.status < 400
                && response
                    .json
                    .as_ref()
                    .and_then(|json| json.pointer("/candidates/0/content/parts/0/text"))
                    .is_some();
            let check = if ok {
                CheckResult::pass("chat", "基础聊天", "Gemini generateContent 基础输出可用")
            } else {
                CheckResult::fail("chat", "基础聊天", "Gemini generateContent 基础输出失败")
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_evidence(format!("model={}", config.model))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::fail("chat", "基础聊天", "Gemini 请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_gemini_tools(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "contents": [{"role": "user", "parts": [{"text": "请调用 get_weather 工具查询北京天气。"}]}],
        "tools": [{
            "functionDeclarations": [{
                "name": "get_weather",
                "description": "获取指定城市的天气",
                "parameters": {
                    "type": "OBJECT",
                    "properties": {"city": {"type": "STRING"}},
                    "required": ["city"]
                }
            }]
        }],
        "toolConfig": {
            "functionCallingConfig": {
                "mode": "ANY",
                "allowedFunctionNames": ["get_weather"]
            }
        }
    });
    match client.post_json_with_headers(url, &[], payload).await {
        Ok(response) => {
            let validation = validate_gemini_function_call(response.json.as_ref());
            let check = match validation {
                Ok(evidence) => CheckResult::pass(
                    "tools",
                    "Tools / Function Calling",
                    "Gemini 支持 functionCall",
                )
                .with_evidence(evidence),
                Err(reason) => CheckResult::warn(
                    "tools",
                    "Tools / Function Calling",
                    "Gemini functionCall 不完整，不能视为满血版",
                )
                .with_evidence(reason),
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_evidence(format!("model={}", config.model))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("tools", "Tools / Function Calling", "Gemini tools 请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_gemini_stream(
    client: &ProbeHttpClient,
    _config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<StreamProbeResponse>) {
    let payload = json!({
        "contents": [{"role": "user", "parts": [{"text": "请用一句话回复 stream-probe-ok"}]}]
    });
    stream_check_from_result(
        "stream",
        "Stream 流式",
        client.stream_json_with_headers(url, &[], payload).await,
    )
}

async fn probe_gemini_json_mode(
    client: &ProbeHttpClient,
    _config: &ProbeConfig,
    url: &str,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let payload = json!({
        "contents": [{"role": "user", "parts": [{"text": "输出一个 JSON 对象，字段 ok=true，city='北京'。"}]}],
        "generationConfig": {"responseMimeType": "application/json"}
    });
    match client.post_json_with_headers(url, &[], payload).await {
        Ok(response) => {
            let ok = response
                .json
                .as_ref()
                .and_then(|json| json.pointer("/candidates/0/content/parts/0/text"))
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .is_some();
            let check = if response.status < 400 && ok {
                CheckResult::pass(
                    "json_mode",
                    "JSON Mode",
                    "Gemini responseMimeType JSON 输出合法",
                )
            } else {
                CheckResult::warn(
                    "json_mode",
                    "JSON Mode",
                    "Gemini JSON Mode 不支持或输出不稳定",
                )
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("json_mode", "JSON Mode", "Gemini JSON Mode 请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

async fn probe_gemini_error(
    client: &ProbeHttpClient,
    config: &ProbeConfig,
) -> (CheckResult, Option<HttpProbeResponse>) {
    let mut invalid_config = config.clone();
    invalid_config.model = "__apikey_probe_missing_model__".to_string();
    let url = gemini_url(&invalid_config, "generateContent");
    let payload =
        json!({"contents": [{"role": "user", "parts": [{"text": "error-format-probe"}]}]});
    match client.post_json_with_headers(&url, &[], payload).await {
        Ok(response) => {
            let ok = response.status >= 400
                && response
                    .json
                    .as_ref()
                    .and_then(|json| json.pointer("/error/message"))
                    .is_some();
            let check = if ok {
                CheckResult::pass(
                    "error_format",
                    "错误格式",
                    "错误响应接近 Gemini 官方 error 对象",
                )
            } else {
                CheckResult::warn(
                    "error_format",
                    "错误格式",
                    "错误响应格式不像 Gemini 官方接口",
                )
            }
            .with_evidence(format!("HTTP {}", response.status))
            .with_raw_preview(preview(&response.body));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn("error_format", "错误格式", "Gemini 错误格式探针请求失败")
                .with_evidence(error.to_string()),
            None,
        ),
    }
}

fn validate_tool_call(json: Option<&Value>) -> std::result::Result<String, String> {
    let json = json.ok_or_else(|| "响应不是合法 JSON".to_string())?;
    let tool_call = json
        .pointer("/choices/0/message/tool_calls/0")
        .ok_or_else(|| "缺少 choices[0].message.tool_calls[0]".to_string())?;

    let name = tool_call
        .pointer("/function/name")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 function.name".to_string())?;
    if name != "get_weather" {
        return Err(format!("function.name 不是 get_weather，而是 {name}"));
    }

    let arguments = tool_call
        .pointer("/function/arguments")
        .ok_or_else(|| "缺少 function.arguments".to_string())?;

    let parsed_arguments = if let Some(arguments) = arguments.as_str() {
        serde_json::from_str::<Value>(arguments)
            .map_err(|error| format!("function.arguments 不是合法 JSON 字符串：{error}"))?
    } else if arguments.is_object() {
        arguments.clone()
    } else {
        return Err("function.arguments 既不是 JSON 字符串也不是对象".to_string());
    };

    let city = parsed_arguments
        .get("city")
        .and_then(Value::as_str)
        .ok_or_else(|| "arguments.city 缺失或不是字符串".to_string())?;
    if !city.contains("北京") {
        return Err(format!("arguments.city 没有包含 北京，实际为 {city}"));
    }

    Ok("tool_calls[0].function.name=get_weather，arguments.city 包含 北京".to_string())
}

fn step(key: &str, label: &str, status: StepStatus, message: &str) -> ProbeProgress {
    ProbeProgress {
        step: key.to_string(),
        label: label.to_string(),
        status,
        message: message.to_string(),
    }
}

fn step_from_check(check: &CheckResult) -> ProbeProgress {
    let status = match check.status {
        CheckStatus::Pass => StepStatus::Pass,
        CheckStatus::Warn => StepStatus::Warn,
        CheckStatus::Fail => StepStatus::Fail,
    };

    step(&check.key, &check.label, status, &check.summary)
}

fn preview(body: &str) -> String {
    let mut value = body.trim().to_string();
    if value.len() > 1_200 {
        value.truncate(1_200);
        value.push_str("\n...");
    }
    value
}

fn super_placeholder_conclusion() -> OverallConclusion {
    OverallConclusion::Warn
}

fn build_report(
    config: &ProbeConfig,
    checks: Vec<CheckResult>,
    risk: RiskAssessment,
) -> ProbeReport {
    let mut report = ProbeReport {
        generated_at: Utc::now(),
        config: RedactedProbeConfig::from(config),
        conclusion: super_placeholder_conclusion(),
        conclusion_text: String::new(),
        checks,
        risk,
    };
    report.conclusion = conclusion_for(&report);
    report.conclusion_text = conclusion_text(report.conclusion);
    report
}

fn single_fail_report(
    config: &ProbeConfig,
    key: &str,
    label: &str,
    summary: impl Into<String>,
) -> ProbeReport {
    build_report(
        config,
        vec![CheckResult::fail(key, label, summary)],
        RiskAssessment {
            score: 0,
            level: super::probe::types::RiskLevel::Low,
            signals: Vec::new(),
        },
    )
}

fn stream_check_from_result(
    key: &str,
    label: &str,
    result: Result<StreamProbeResponse>,
) -> (CheckResult, Option<StreamProbeResponse>) {
    match result {
        Ok(response) => {
            let check = if response.status < 400
                && response.data_events_seen > 0
                && response.invalid_json_events == 0
            {
                let summary = if response.done_seen {
                    "支持标准 SSE data 事件和结束信号"
                } else {
                    "可收到 SSE data 事件，但结束标记不完整"
                };
                CheckResult::pass(key, label, summary)
            } else {
                CheckResult::warn(key, label, "stream 不标准，实时体验存在风险")
            }
            .with_evidence(format!(
                "HTTP {}, chunks={}, data_events={}, invalid_json_events={}, done_seen={}",
                response.status,
                response.chunks_seen,
                response.data_events_seen,
                response.invalid_json_events,
                response.done_seen
            ))
            .with_evidence(format!("headers={}", response.headers.len()))
            .with_raw_preview(preview(&response.body_preview));
            (check, Some(response))
        }
        Err(error) => (
            CheckResult::warn(key, label, "stream 探针请求失败").with_evidence(error.to_string()),
            None,
        ),
    }
}

fn responses_text(json: Option<&Value>) -> Option<String> {
    let json = json?;
    if let Some(text) = json.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    json.get("output")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find_map(|item| {
                item.get("content")
                    .and_then(Value::as_array)
                    .and_then(|content| {
                        content.iter().find_map(|part| {
                            part.get("text")
                                .or_else(|| part.get("content"))
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                        })
                    })
            })
        })
}

fn validate_responses_function_call(json: Option<&Value>) -> std::result::Result<String, String> {
    let json = json.ok_or_else(|| "响应不是合法 JSON".to_string())?;
    let output = json
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 output 数组".to_string())?;
    let call = output
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .ok_or_else(|| "缺少 type=function_call 的 output 项".to_string())?;
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 function_call.name".to_string())?;
    if name != "get_weather" {
        return Err(format!("function_call.name 不是 get_weather，而是 {name}"));
    }
    let arguments = call
        .get("arguments")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 function_call.arguments 字符串".to_string())?;
    validate_city_argument(arguments)
}

fn validate_anthropic_tool_use(json: Option<&Value>) -> std::result::Result<String, String> {
    let json = json.ok_or_else(|| "响应不是合法 JSON".to_string())?;
    let content = json
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 content 数组".to_string())?;
    let tool_use = content
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("tool_use"))
        .ok_or_else(|| "缺少 type=tool_use 的 content 项".to_string())?;
    let name = tool_use
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 tool_use.name".to_string())?;
    if name != "get_weather" {
        return Err(format!("tool_use.name 不是 get_weather，而是 {name}"));
    }
    let city = tool_use
        .pointer("/input/city")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 input.city".to_string())?;
    if !city.contains("北京") {
        return Err(format!("input.city 没有包含 北京，实际为 {city}"));
    }
    Ok("content 中存在 tool_use=get_weather，input.city 包含 北京".to_string())
}

fn validate_gemini_function_call(json: Option<&Value>) -> std::result::Result<String, String> {
    let json = json.ok_or_else(|| "响应不是合法 JSON".to_string())?;
    let parts = json
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .ok_or_else(|| "缺少 candidates[0].content.parts".to_string())?;
    let call = parts
        .iter()
        .find_map(|part| part.get("functionCall"))
        .ok_or_else(|| "缺少 functionCall".to_string())?;
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 functionCall.name".to_string())?;
    if name != "get_weather" {
        return Err(format!("functionCall.name 不是 get_weather，而是 {name}"));
    }
    let city = call
        .pointer("/args/city")
        .and_then(Value::as_str)
        .ok_or_else(|| "缺少 functionCall.args.city".to_string())?;
    if !city.contains("北京") {
        return Err(format!("args.city 没有包含 北京，实际为 {city}"));
    }
    Ok("functionCall.name=get_weather，args.city 包含 北京".to_string())
}

fn validate_city_argument(arguments: &str) -> std::result::Result<String, String> {
    let parsed = serde_json::from_str::<Value>(arguments)
        .map_err(|error| format!("arguments 不是合法 JSON 字符串：{error}"))?;
    let city = parsed
        .get("city")
        .and_then(Value::as_str)
        .ok_or_else(|| "arguments.city 缺失或不是字符串".to_string())?;
    if !city.contains("北京") {
        return Err(format!("arguments.city 没有包含 北京，实际为 {city}"));
    }
    Ok("function arguments.city 包含 北京".to_string())
}

fn responses_url(base_url: &str) -> String {
    endpoint_url(base_url, "/responses")
}

fn anthropic_messages_url(base_url: &str) -> String {
    endpoint_url(base_url, "/messages")
}

fn endpoint_url(base_url: &str, endpoint: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with(endpoint) {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}{endpoint}")
    } else {
        format!("{trimmed}/v1{endpoint}")
    }
}

fn anthropic_headers(config: &ProbeConfig) -> Vec<(&'static str, String)> {
    vec![
        ("x-api-key", config.api_key.clone()),
        ("anthropic-version", "2023-06-01".to_string()),
    ]
}

fn gemini_url(config: &ProbeConfig, action: &str) -> String {
    let trimmed = config.base_url.trim().trim_end_matches('/');
    let base = if trimmed.ends_with("/v1beta") || trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1beta")
    };
    let model = encode_path_segment(&config.model);
    let separator = if action == "streamGenerateContent" {
        "?alt=sse&key="
    } else {
        "?key="
    };
    format!(
        "{base}/models/{model}:{action}{separator}{}",
        config.api_key
    )
}

fn encode_path_segment(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('/', "%2F")
        .replace(' ', "%20")
        .replace(':', "%3A")
}
