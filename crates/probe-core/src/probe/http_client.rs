use super::types::{HttpProbeResponse, ProbeConfig, StreamProbeResponse};
use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE},
    Client,
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

pub struct ProbeHttpClient {
    client: Client,
    api_key: String,
    chat_completions_url: String,
}

impl ProbeHttpClient {
    pub fn new(config: &ProbeConfig) -> Result<Self> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(45))
            .connect_timeout(Duration::from_secs(15))
            .user_agent("apikey-probe-desktop/0.1");

        if let Some(proxy_url) = config
            .proxy_url
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            builder = builder.proxy(reqwest::Proxy::all(proxy_url).context("invalid proxy url")?);
        }

        Ok(Self {
            client: builder.build().context("failed to build http client")?,
            api_key: config.api_key.clone(),
            chat_completions_url: chat_completions_url(&config.base_url),
        })
    }

    pub async fn post_chat_completions(&self, payload: Value) -> Result<HttpProbeResponse> {
        self.post_json_bearer(&self.chat_completions_url, payload)
            .await
    }

    pub async fn post_json_bearer(&self, url: &str, payload: Value) -> Result<HttpProbeResponse> {
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await
            .context("request failed")?;

        let status = response.status().as_u16();
        let headers = collect_headers(response.headers());
        let body = response
            .text()
            .await
            .context("failed to read response body")?;
        let json = serde_json::from_str::<Value>(&body).ok();

        Ok(HttpProbeResponse {
            status,
            headers,
            body,
            json,
        })
    }

    pub async fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, String)],
        payload: Value,
    ) -> Result<HttpProbeResponse> {
        let mut request = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json");

        for (key, value) in headers {
            request = request.header(
                HeaderName::from_bytes(key.as_bytes()).context("invalid header name")?,
                HeaderValue::from_str(value).context("invalid header value")?,
            );
        }

        let response = request
            .json(&payload)
            .send()
            .await
            .context("request failed")?;

        let status = response.status().as_u16();
        let headers = collect_headers(response.headers());
        let body = response
            .text()
            .await
            .context("failed to read response body")?;
        let json = serde_json::from_str::<Value>(&body).ok();

        Ok(HttpProbeResponse {
            status,
            headers,
            body,
            json,
        })
    }

    pub async fn stream_chat_completions(&self, payload: Value) -> Result<StreamProbeResponse> {
        self.stream_json_bearer(&self.chat_completions_url, payload)
            .await
    }

    pub async fn stream_json_bearer(
        &self,
        url: &str,
        payload: Value,
    ) -> Result<StreamProbeResponse> {
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .header(CONTENT_TYPE, "application/json")
            .json(&payload)
            .send()
            .await
            .context("stream request failed")?;

        let status = response.status().as_u16();
        let headers = collect_headers(response.headers());
        let mut stream = response.bytes_stream();
        let mut chunks_seen = 0usize;
        let mut data_events_seen = 0usize;
        let mut done_seen = false;
        let mut invalid_json_events = 0usize;
        let mut buffer = String::new();
        let mut body_preview = String::new();

        while let Some(item) = stream.next().await {
            let bytes = item.context("failed to read stream chunk")?;
            chunks_seen += 1;
            let chunk = String::from_utf8_lossy(&bytes);

            if body_preview.len() < 4_000 {
                body_preview.push_str(&chunk);
                body_preview.truncate(4_000);
            }

            buffer.push_str(&chunk);

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if let Some(data) = line.strip_prefix("data:") {
                    let data = data.trim();
                    data_events_seen += 1;

                    if data == "[DONE]" {
                        done_seen = true;
                    } else if serde_json::from_str::<Value>(data).is_err() {
                        invalid_json_events += 1;
                    }
                }
            }

            if done_seen || chunks_seen >= 80 || body_preview.len() >= 4_000 {
                break;
            }
        }

        Ok(StreamProbeResponse {
            status,
            headers,
            chunks_seen,
            data_events_seen,
            done_seen,
            invalid_json_events,
            body_preview,
        })
    }

    pub async fn stream_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, String)],
        payload: Value,
    ) -> Result<StreamProbeResponse> {
        let mut request = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json");

        for (key, value) in headers {
            request = request.header(
                HeaderName::from_bytes(key.as_bytes()).context("invalid header name")?,
                HeaderValue::from_str(value).context("invalid header value")?,
            );
        }

        let response = request
            .json(&payload)
            .send()
            .await
            .context("stream request failed")?;

        read_sse_response(response).await
    }
}

async fn read_sse_response(response: reqwest::Response) -> Result<StreamProbeResponse> {
    let status = response.status().as_u16();
    let headers = collect_headers(response.headers());
    let mut stream = response.bytes_stream();
    let mut chunks_seen = 0usize;
    let mut data_events_seen = 0usize;
    let mut done_seen = false;
    let mut invalid_json_events = 0usize;
    let mut buffer = String::new();
    let mut body_preview = String::new();

    while let Some(item) = stream.next().await {
        let bytes = item.context("failed to read stream chunk")?;
        chunks_seen += 1;
        let chunk = String::from_utf8_lossy(&bytes);

        if body_preview.len() < 4_000 {
            body_preview.push_str(&chunk);
            body_preview.truncate(4_000);
        }

        buffer.push_str(&chunk);

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                data_events_seen += 1;

                if data == "[DONE]" {
                    done_seen = true;
                } else if serde_json::from_str::<Value>(data).is_err() {
                    invalid_json_events += 1;
                }
            }
        }

        if done_seen || chunks_seen >= 80 || body_preview.len() >= 4_000 {
            break;
        }
    }

    Ok(StreamProbeResponse {
        status,
        headers,
        chunks_seen,
        data_events_seen,
        done_seen,
        invalid_json_events,
        body_preview,
    })
}

fn chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');

    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn collect_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}
