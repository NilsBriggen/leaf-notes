//! Bring-your-own-key AI: OpenAI-compatible endpoints (OpenAI, OpenRouter, Ollama, LM Studio, Groq,
//! Mistral …) and Anthropic's Messages API. Calls run on a worker thread.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub enum Provider { OpenAiCompatible, Anthropic }

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AiSettings {
    pub provider: Provider,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self { provider: Provider::Anthropic, base_url: "https://api.anthropic.com".into(), api_key: String::new(), model: "claude-sonnet-5-5".into() }
    }
}

impl AiSettings {
    pub fn ready(&self) -> bool { !self.model.trim().is_empty() && (!self.api_key.trim().is_empty() || self.base_url.contains("localhost") || self.base_url.contains("127.0.0.1")) }
}

/// Send one user turn (optional PNG image) and return the reply text.
pub fn ask(s: &AiSettings, system: &str, prompt: &str, png: Option<&[u8]>) -> Result<String, String> {
    use base64::Engine;
    let img = png.map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let base = s.base_url.trim_end_matches('/');
    let (url, body) = match s.provider {
        Provider::Anthropic => {
            let mut content = vec![];
            if let Some(i) = &img { content.push(json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":i}})); }
            content.push(json!({"type":"text","text":prompt}));
            (format!("{base}/v1/messages"), json!({"model":s.model,"max_tokens":2048,"system":system,"messages":[{"role":"user","content":content}]}))
        }
        Provider::OpenAiCompatible => {
            let mut content = vec![json!({"type":"text","text":prompt})];
            if let Some(i) = &img { content.push(json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{i}")}})); }
            (format!("{base}/chat/completions"), json!({"model":s.model,"messages":[{"role":"system","content":system},{"role":"user","content":content}]}))
        }
    };
    let mut req = agent.post(&url).header("content-type", "application/json");
    req = match s.provider {
        Provider::Anthropic => req.header("x-api-key", s.api_key.trim()).header("anthropic-version", "2023-06-01"),
        Provider::OpenAiCompatible => req.header("authorization", format!("Bearer {}", s.api_key.trim())),
    };
    let mut resp = req.send_json(&body).map_err(|e| format!("Could not reach {url}: {e}"))?;
    let status = resp.status();
    let v: Value = resp.body_mut().read_json().map_err(|e| format!("Bad reply ({status}): {e}"))?;
    if !status.is_success() {
        let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("unknown error");
        return Err(format!("AI error {status}: {msg}"));
    }
    let text = match s.provider {
        Provider::Anthropic => v["content"].as_array().map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("")),
        Provider::OpenAiCompatible => v.pointer("/choices/0/message/content").and_then(Value::as_str).map(String::from),
    };
    text.filter(|t| !t.is_empty()).ok_or_else(|| "The AI returned no text".into())
}

/// Remove code fences and $ delimiters a model may wrap around its answer.
pub fn clean(reply: &str) -> String {
    let mut t = reply.trim();
    if t.starts_with("```") {
        t = t.trim_start_matches('`');
        t = t.split_once('\n').map(|(_, r)| r).unwrap_or(t);
        t = t.trim_end().trim_end_matches('`');
    }
    t.trim().trim_matches('$').trim().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn cleans_fences() {
        assert_eq!(super::clean("```latex\n\\frac{1}{2}\n```"), "\\frac{1}{2}");
        assert_eq!(super::clean("$$x^2$$"), "x^2");
        assert_eq!(super::clean("hello"), "hello");
    }
}
