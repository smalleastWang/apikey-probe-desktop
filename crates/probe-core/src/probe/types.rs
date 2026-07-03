use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub protocol_type: String,
    pub provider_name: Option<String>,
    pub note: Option<String>,
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedProbeConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub protocol_type: String,
    pub provider_name: Option<String>,
    pub note: Option<String>,
    pub proxy_url: Option<String>,
}

impl From<&ProbeConfig> for RedactedProbeConfig {
    fn from(config: &ProbeConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            api_key: redact_api_key(&config.api_key),
            model: config.model.clone(),
            protocol_type: config.protocol_type.clone(),
            provider_name: config.provider_name.clone(),
            note: config.note.clone(),
            proxy_url: config.proxy_url.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeProgress {
    pub step: String,
    pub label: String,
    pub status: StepStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    Running,
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OverallConclusion {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub key: String,
    pub label: String,
    pub status: CheckStatus,
    pub summary: String,
    pub evidence: Vec<String>,
    pub raw_preview: Option<String>,
}

impl CheckResult {
    pub fn pass(key: &str, label: &str, summary: impl Into<String>) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            status: CheckStatus::Pass,
            summary: summary.into(),
            evidence: Vec::new(),
            raw_preview: None,
        }
    }

    pub fn warn(key: &str, label: &str, summary: impl Into<String>) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            status: CheckStatus::Warn,
            summary: summary.into(),
            evidence: Vec::new(),
            raw_preview: None,
        }
    }

    pub fn fail(key: &str, label: &str, summary: impl Into<String>) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            status: CheckStatus::Fail,
            summary: summary.into(),
            evidence: Vec::new(),
            raw_preview: None,
        }
    }

    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence.push(evidence.into());
        self
    }

    pub fn with_raw_preview(mut self, raw_preview: impl Into<String>) -> Self {
        self.raw_preview = Some(raw_preview.into());
        self
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskSignal {
    pub key: String,
    pub label: String,
    pub severity: RiskSeverity,
    pub score: u32,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskSeverity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskAssessment {
    pub score: u32,
    pub level: RiskLevel,
    pub signals: Vec<RiskSignal>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub generated_at: DateTime<Utc>,
    pub config: RedactedProbeConfig,
    pub conclusion: OverallConclusion,
    pub conclusion_text: String,
    pub checks: Vec<CheckResult>,
    pub risk: RiskAssessment,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiProtocolProbeConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub protocol_types: Vec<String>,
    pub provider_name: Option<String>,
    pub note: Option<String>,
    pub proxy_url: Option<String>,
}

impl MultiProtocolProbeConfig {
    pub fn to_single(&self, protocol_type: &str) -> ProbeConfig {
        ProbeConfig {
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            protocol_type: protocol_type.to_string(),
            provider_name: self.provider_name.clone(),
            note: self.note.clone(),
            proxy_url: self.proxy_url.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiProtocolProbeReport {
    pub generated_at: DateTime<Utc>,
    pub model: String,
    pub provider_name: Option<String>,
    pub conclusion: OverallConclusion,
    pub conclusion_text: String,
    pub best_protocol: Option<String>,
    pub results: Vec<ProbeReport>,
}

#[derive(Debug, Clone)]
pub struct HttpProbeResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    pub json: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct StreamProbeResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub chunks_seen: usize,
    pub data_events_seen: usize,
    pub done_seen: bool,
    pub invalid_json_events: usize,
    pub body_preview: String,
}

pub fn redact_api_key(api_key: &str) -> String {
    if api_key.len() <= 8 {
        return "***".to_string();
    }

    let prefix: String = api_key.chars().take(4).collect();
    let suffix: String = api_key
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}
