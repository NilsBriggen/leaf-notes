//! Bring-your-own-key AI: OpenAI-compatible endpoints (OpenAI, OpenRouter, Ollama, LM Studio, Groq,
//! Mistral …) and Anthropic's Messages API. Supports SSE streaming, structured multi-turn history,
//! cooperative cancellation, and chunked speech-to-text transcription with progress.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
pub enum Provider {
    OpenAiCompatible,
    Anthropic,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct AiSettings {
    pub provider: Provider,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Speech-to-text model for recording transcripts (OpenAI-compatible services).
    #[serde(default = "whisper")]
    pub transcribe_model: String,
    /// Explicit opt-in to allow unencrypted `http://` on a non-loopback private LAN / `.local` host.
    #[serde(default)]
    pub allow_lan_http: bool,
}

fn whisper() -> String {
    "whisper-1".into()
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: String::new(),
            model: "claude-sonnet-5-5".into(),
            transcribe_model: whisper(),
            allow_lan_http: false,
        }
    }
}

impl AiSettings {
    pub fn ready(&self) -> bool {
        !self.model.trim().is_empty() && (!self.api_key.trim().is_empty() || self.local())
    }

    /// A server on this computer or the home network (Ollama, LM Studio …): no key needed, and more patience.
    pub fn local(&self) -> bool {
        let u = self.base_url.trim();
        u.strip_prefix("http://")
            .or_else(|| u.strip_prefix("https://"))
            .is_some_and(|r| crate::sync::is_loopback_host(r) || crate::sync::is_private_lan_host(r))
    }
}

/// Rich AI error carrying HTTP status code (when available) so callers only fall back from vision
/// on actual HTTP 400 / 415 / 422 payload rejections, never on 401 / 429 / 5xx / network errors.
#[derive(Debug, Clone)]
pub struct AiError {
    pub status: Option<u16>,
    pub message: String,
}

impl AiError {
    pub fn is_vision_rejected(&self) -> bool {
        matches!(self.status, Some(400 | 415 | 422))
    }
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Normalize Anthropic base URL so both `https://api.anthropic.com` and `https://api.anthropic.com/v1` work.
fn anthropic_messages_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    let b = b.strip_suffix("/v1").unwrap_or(b);
    format!("{b}/v1/messages")
}

/// Normalize OpenAI-compatible base URL: if no path component follows the host (e.g. `http://localhost:11434`),
/// insert `/v1`; otherwise append `suffix` directly (preserving `/openai/v1`, `/api/v1`, etc.).
fn openai_endpoint_url(base: &str, suffix: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    let rest = b.strip_prefix("https://").or_else(|| b.strip_prefix("http://")).unwrap_or(b);
    if !rest.contains('/') {
        format!("{b}/v1{suffix}")
    } else {
        format!("{b}{suffix}")
    }
}

fn status_error(status: u16, model: &str, body_text: &str) -> AiError {
    let msg = serde_json::from_str::<Value>(body_text)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.pointer("/message"))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .unwrap_or_default();
    let hint = match status {
        401 | 403 => crate::tr("The AI service refused the key. Check it in Settings ▸ AI.").to_string(),
        404 => crate::tf!(
            "The AI service doesn't know the model “{model}”, or the address is wrong. Check Settings ▸ AI.",
            model = model
        ),
        429 => crate::tr("The AI service says there were too many requests, or the account has no credit left. Try again later.").to_string(),
        _ => crate::tf!("The AI service answered with an error ({status}).", status = status),
    };
    let message = if msg.is_empty() { hint } else { format!("{hint} ({msg})") };
    AiError { status: Some(status), message }
}

fn build_messages_body(
    s: &AiSettings,
    system: &str,
    turns: &[(bool, String)],
    png: Option<&[u8]>,
    stream: bool,
) -> (String, Value) {
    use base64::Engine;
    let img = png.map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    let last_user = turns.iter().rposition(|(is_ai, _)| !*is_ai);
    match s.provider {
        Provider::Anthropic => {
            let mut msgs = Vec::with_capacity(turns.len());
            for (idx, (is_ai, text)) in turns.iter().enumerate() {
                let role = if *is_ai { "assistant" } else { "user" };
                if Some(idx) == last_user {
                    if let Some(i) = &img {
                        msgs.push(json!({
                            "role": role,
                            "content": [
                                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": i}},
                                {"type": "text", "text": text}
                            ]
                        }));
                        continue;
                    }
                }
                msgs.push(json!({ "role": role, "content": text }));
            }
            let mut body = json!({
                "model": s.model.trim(),
                "max_tokens": 8192,
                "system": system,
                "messages": msgs
            });
            if stream {
                body["stream"] = json!(true);
            }
            (anthropic_messages_url(&s.base_url), body)
        }
        Provider::OpenAiCompatible => {
            let mut msgs = Vec::with_capacity(turns.len() + 1);
            if !system.is_empty() {
                msgs.push(json!({ "role": "system", "content": system }));
            }
            for (idx, (is_ai, text)) in turns.iter().enumerate() {
                let role = if *is_ai { "assistant" } else { "user" };
                if Some(idx) == last_user {
                    if let Some(i) = &img {
                        msgs.push(json!({
                            "role": role,
                            "content": [
                                {"type": "text", "text": text},
                                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{i}")}}
                            ]
                        }));
                        continue;
                    }
                }
                msgs.push(json!({ "role": role, "content": text }));
            }
            let mut body = json!({
                "model": s.model.trim(),
                "messages": msgs
            });
            if stream {
                body["stream"] = json!(true);
            }
            (openai_endpoint_url(&s.base_url, "/chat/completions"), body)
        }
    }
}

fn attach_auth<T>(mut req: ureq::RequestBuilder<T>, s: &AiSettings) -> ureq::RequestBuilder<T> {
    let key = s.api_key.trim();
    match s.provider {
        Provider::Anthropic => {
            if !key.is_empty() {
                req = req.header("x-api-key", key);
            }
            req.header("anthropic-version", "2023-06-01")
        }
        Provider::OpenAiCompatible => {
            if !key.is_empty() {
                req = req.header("authorization", format!("Bearer {key}"));
            }
            req
        }
    }
}

/// Extract text and truncation flag from a non-streaming JSON reply.
fn parse_complete_json(provider: Provider, v: &Value) -> Result<String, AiError> {
    let (text, cut) = match provider {
        Provider::Anthropic => (
            v["content"]
                .as_array()
                .map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("")),
            v["stop_reason"] == "max_tokens",
        ),
        Provider::OpenAiCompatible => (
            v.pointer("/choices/0/message/content").and_then(Value::as_str).map(String::from),
            v.pointer("/choices/0/finish_reason").and_then(Value::as_str) == Some("length"),
        ),
    };
    let text = text.filter(|t| !t.is_empty()).ok_or_else(|| AiError {
        status: None,
        message: crate::tr("The AI returned no text.").to_string(),
    })?;
    Ok(if cut {
        format!("{text}\n\n_{}_", crate::tr("(The answer was cut off here. Ask it to continue.)"))
    } else {
        text
    })
}

/// Stream a multi-turn chat request using SSE (`stream: true`) for both Anthropic and OpenAI-compatible
/// endpoints. Falls back cleanly if the server replies with non-streaming JSON. Checks `cancel` throughout.
pub fn stream_chat(
    s: &AiSettings,
    system: &str,
    turns: &[(bool, String)],
    png: Option<&[u8]>,
    cancel: &AtomicBool,
    mut on_delta: impl FnMut(&str),
) -> Result<String, AiError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AiError { status: None, message: crate::tr("Stopped.").to_string() });
    }
    if let Err(e) = crate::sync::validate_http_url(&s.base_url, s.allow_lan_http) {
        return Err(AiError { status: None, message: e });
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(if s.local() { 300 } else { 120 })))
        .timeout_recv_body(Some(Duration::from_secs(if s.local() { 300 } else { 120 })))
        .build()
        .into();
    let (url, body) = build_messages_body(s, system, turns, png, true);
    let req = attach_auth(
        agent.post(&url).header("content-type", "application/json").header("accept", "text/event-stream, application/json"),
        s,
    );
    let mut resp = req.send_json(&body).map_err(|e| AiError {
        status: None,
        message: crate::tf!(
            "Couldn't reach the AI service at {url}. Is it running, and is this device online? ({e})",
            url = url,
            e = e
        ),
    })?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let raw = resp.body_mut().read_to_string().unwrap_or_default();
        return Err(status_error(status, &s.model, &raw));
    }
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !ct.contains("text/event-stream") {
        let v: Value = resp.body_mut().read_json().map_err(|e| AiError {
            status: Some(status),
            message: crate::tf!("The AI service sent an answer that couldn't be read ({status}: {e}).", status = status, e = e),
        })?;
        let text = parse_complete_json(s.provider, &v)?;
        on_delta(&text);
        return Ok(text);
    }

    let mut reader = BufReader::new(resp.into_body().into_reader());
    let mut line = String::new();
    let mut full = String::new();
    let mut cut = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError { status: None, message: crate::tr("Stopped.").to_string() });
        }
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| AiError {
            status: None,
            message: crate::tf!("The AI stream was interrupted: {e}", e = e),
        })?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            continue;
        }
        let Some(data) = trimmed.strip_prefix("data:").map(str::trim) else { continue };
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
        match s.provider {
            Provider::Anthropic => match v["type"].as_str().unwrap_or("") {
                "content_block_delta" => {
                    if let Some(t) = v.pointer("/delta/text").and_then(Value::as_str) {
                        if !t.is_empty() {
                            full.push_str(t);
                            on_delta(t);
                        }
                    }
                }
                "message_delta" => {
                    if v.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("max_tokens") {
                        cut = true;
                    }
                }
                "message_stop" => break,
                "error" => {
                    let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("stream error");
                    return Err(AiError { status: None, message: msg.to_string() });
                }
                _ => {}
            },
            Provider::OpenAiCompatible => {
                if let Some(err) = v.pointer("/error/message").and_then(Value::as_str) {
                    return Err(AiError { status: None, message: err.to_string() });
                }
                if let Some(t) = v.pointer("/choices/0/delta/content").and_then(Value::as_str) {
                    if !t.is_empty() {
                        full.push_str(t);
                        on_delta(t);
                    }
                }
                if v.pointer("/choices/0/finish_reason").and_then(Value::as_str) == Some("length") {
                    cut = true;
                }
            }
        }
    }
    if full.is_empty() {
        return Err(AiError { status: None, message: crate::tr("The AI returned no text.").to_string() });
    }
    if cut {
        let note = format!("\n\n_{}_", crate::tr("(The answer was cut off here. Ask it to continue.)"));
        on_delta(&note);
        full.push_str(&note);
    }
    Ok(full)
}

/// Send one user turn (optional PNG image) and return the reply text (non-streaming).
pub fn ask(s: &AiSettings, system: &str, prompt: &str, png: Option<&[u8]>, cancel: &AtomicBool) -> Result<String, AiError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AiError { status: None, message: crate::tr("Stopped.").to_string() });
    }
    if let Err(e) = crate::sync::validate_http_url(&s.base_url, s.allow_lan_http) {
        return Err(AiError { status: None, message: e });
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(if s.local() { 300 } else { 120 })))
        .build()
        .into();
    let turns = [(false, prompt.to_string())];
    let (url, body) = build_messages_body(s, system, &turns, png, false);
    let req = attach_auth(agent.post(&url).header("content-type", "application/json"), s);
    let mut resp = req.send_json(&body).map_err(|e| AiError {
        status: None,
        message: crate::tf!(
            "Couldn't reach the AI service at {url}. Is it running, and is this device online? ({e})",
            url = url,
            e = e
        ),
    })?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let raw = resp.body_mut().read_to_string().unwrap_or_default();
        return Err(status_error(status, &s.model, &raw));
    }
    let v: Value = resp.body_mut().read_json().map_err(|e| AiError {
        status: Some(status),
        message: crate::tf!("The AI service sent an answer that couldn't be read ({status}: {e}).", status = status, e = e),
    })?;
    parse_complete_json(s.provider, &v)
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

/// Speech to text with timestamps: `(seconds from the start, text)` per sentence or phrase.
/// Uses the OpenAI-style `/audio/transcriptions` endpoint, ten minutes at a time, with progress & cancel support.
pub fn transcribe(
    s: &AiSettings,
    samples: &[i16],
    rate: u32,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<Vec<(f32, String)>, String> {
    if s.provider == Provider::Anthropic {
        return Err(crate::tr("Turning speech into text needs an OpenAI-compatible service (OpenAI, Groq or a local Whisper server). Anthropic doesn't offer it.").into());
    }
    crate::sync::validate_http_url(&s.base_url, s.allow_lan_http)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let url = openai_endpoint_url(&s.base_url, "/audio/transcriptions");
    let chunk = (rate as usize * 600).max(1);
    let total = samples.len().div_ceil(chunk).max(1);
    let mut out = vec![];
    for (k, part) in samples.chunks(chunk).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return if out.is_empty() {
                Err(crate::tr("Stopped.").into())
            } else {
                Ok(out)
            };
        }
        on_progress(k, total);
        let offset = (k * chunk) as f32 / rate as f32;
        let boundary = format!("leafnotes{}", crate::model::new_id());
        let field = |name: &str, value: &str| {
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
        };
        let mut body = (field("model", s.transcribe_model.trim()) + &field("response_format", "verbose_json")).into_bytes();
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"recording.wav\"\r\nContent-Type: audio/wav\r\n\r\n").bytes());
        body.extend(crate::audio::wav_bytes(part, rate));
        body.extend(format!("\r\n--{boundary}--\r\n").bytes());
        let stop = |e: String, out: Vec<(f32, String)>| -> Result<Vec<(f32, String)>, String> {
            if out.is_empty() {
                return Err(e);
            }
            let mut out = out;
            out.push((offset, crate::tf!("[The transcript stops here, at {m} min: {e}]", m = format!("{:.0}", offset / 60.0), e = e)));
            Ok(out)
        };
        let mut req = agent
            .post(&url)
            .header("content-type", format!("multipart/form-data; boundary={boundary}"));
        let key = s.api_key.trim();
        if !key.is_empty() {
            req = req.header("authorization", format!("Bearer {key}"));
        }
        let mut resp = match req.send(&body[..]) {
            Ok(r) => r,
            Err(e) => return stop(format!("Could not reach {url}: {e}"), out),
        };
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let raw = resp.body_mut().read_to_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&raw)
                .ok()
                .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(String::from))
                .unwrap_or_else(|| "transcription failed".into());
            return stop(format!("{status}: {msg}"), out);
        }
        let v: Value = match resp.body_mut().read_json() {
            Ok(v) => v,
            Err(e) => return stop(format!("Bad reply ({status}): {e}"), out),
        };
        match v["segments"].as_array() {
            Some(segs) => {
                for seg in segs {
                    let t = seg["start"].as_f64().unwrap_or(0.0) as f32 + offset;
                    let text = seg["text"].as_str().unwrap_or("").trim().to_string();
                    if !text.is_empty() {
                        out.push((t, text));
                    }
                }
            }
            None => {
                if let Some(text) = v["text"].as_str().filter(|t| !t.trim().is_empty()) {
                    out.push((offset, text.trim().to_string()));
                }
            }
        }
        on_progress(k + 1, total);
    }
    Ok(out)
}

/// Consecutive pieces of a transcript joined into paragraphs of about half a minute.
pub fn paragraphs(segs: &[(f32, String)]) -> Vec<(f32, String)> {
    let mut out: Vec<(f32, String)> = vec![];
    for (t, text) in segs {
        match out.last_mut() {
            Some((start, p)) if t - *start < 30.0 && p.len() < 500 => {
                p.push(' ');
                p.push_str(text);
            }
            _ => out.push((*t, text.clone())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn cleans_fences() {
        assert_eq!(super::clean("```latex\n\\frac{1}{2}\n```"), "\\frac{1}{2}");
        assert_eq!(super::clean("$$x^2$$"), "x^2");
        assert_eq!(super::clean("hello"), "hello");
    }

    #[test]
    fn normalizes_base_urls_and_enforces_http_policy() {
        assert_eq!(anthropic_messages_url("https://api.anthropic.com"), "https://api.anthropic.com/v1/messages");
        assert_eq!(anthropic_messages_url("https://api.anthropic.com/v1/"), "https://api.anthropic.com/v1/messages");
        assert_eq!(openai_endpoint_url("http://localhost:11434", "/chat/completions"), "http://localhost:11434/v1/chat/completions");
        assert_eq!(openai_endpoint_url("http://localhost:11434/v1", "/chat/completions"), "http://localhost:11434/v1/chat/completions");
        assert_eq!(openai_endpoint_url("https://api.groq.com/openai/v1/", "/chat/completions"), "https://api.groq.com/openai/v1/chat/completions");
        // Public http:// rejected; private LAN requires opt-in
        let pub_http = AiSettings { provider: Provider::OpenAiCompatible, base_url: "http://api.openai.com/v1".into(), api_key: "k".into(), model: "m".into(), ..Default::default() };
        assert!(ask(&pub_http, "", "hi", None, &AtomicBool::new(false)).is_err());
        let lan_http = AiSettings { base_url: "http://192.168.1.50:11434/v1".into(), allow_lan_http: false, ..pub_http };
        assert!(ask(&lan_http, "", "hi", None, &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn streams_sse_for_openai_and_anthropic_and_omits_empty_bearer() {
        // OpenAI SSE mock
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = c.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Hello \"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"world\"}}]}\n\n",
                "data: [DONE]\n\n"
            );
            write!(c, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            req
        });
        let s = AiSettings {
            provider: Provider::OpenAiCompatible,
            base_url: format!("http://127.0.0.1:{port}"),
            api_key: String::new(),
            model: "local".into(),
            ..Default::default()
        };
        let mut chunks = vec![];
        let full = stream_chat(&s, "sys", &[(false, "hi".into())], None, &AtomicBool::new(false), |d| chunks.push(d.to_string())).unwrap();
        let req = server.join().unwrap();
        assert_eq!(full, "Hello world");
        assert_eq!(chunks, vec!["Hello ", "world"]);
        assert!(req.starts_with("POST /v1/chat/completions"), "{req}");
        assert!(!req.to_ascii_lowercase().contains("authorization:"), "empty key must omit Authorization header");

        // Anthropic SSE mock
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let body = concat!(
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hi \"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Claude\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            );
            write!(c, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let s2 = AiSettings {
            provider: Provider::Anthropic,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "test".into(),
            model: "claude".into(),
            ..Default::default()
        };
        let mut chunks2 = vec![];
        let full2 = stream_chat(&s2, "sys", &[(false, "hi".into())], None, &AtomicBool::new(false), |d| chunks2.push(d.to_string())).unwrap();
        server.join().unwrap();
        assert_eq!(full2, "Hi Claude");
        assert_eq!(chunks2, vec!["Hi ", "Claude"]);
    }

    #[test]
    fn non_json_502_reports_http_status_not_json_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf);
            let body = "<html>502 Bad Gateway</html>";
            write!(c, "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let s = AiSettings {
            provider: Provider::OpenAiCompatible,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "k".into(),
            model: "m".into(),
            ..Default::default()
        };
        let err = ask(&s, "", "hi", None, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(err.status, Some(502));
        assert!(!err.is_vision_rejected());
        assert!(err.message.contains("502"), "{}", err.message);
    }
}

#[cfg(test)]
mod transcribe_tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn transcribes_in_ten_minute_pieces() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut seen = vec![];
            for _ in 0..2 {
                let (mut c, _) = listener.accept().unwrap();
                let mut req = vec![];
                let mut buf = [0u8; 65536];
                loop {
                    let n = c.read(&mut buf).unwrap();
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req);
                    if let Some(h) = text.find("\r\n\r\n") {
                        let len: usize = text[..h]
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse().unwrap()))
                            .unwrap_or(0);
                        if req.len() >= h + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                seen.push(String::from_utf8_lossy(&req).into_owned());
                let body = r#"{"segments":[{"start":1.5,"text":" Hello there."},{"start":4.0,"text":"Second part."}]}"#;
                write!(c, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            seen
        });
        let s = AiSettings {
            provider: Provider::OpenAiCompatible,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "k".into(),
            model: "m".into(),
            transcribe_model: "whisper-1".into(),
            allow_lan_http: false,
        };
        let rate = 100;
        let samples = vec![0i16; 60_000 + 10];
        let mut progress = vec![];
        let got = transcribe(&s, &samples, rate, &AtomicBool::new(false), |d, t| progress.push((d, t))).unwrap();
        let reqs = server.join().unwrap();
        assert!(reqs[0].starts_with("POST /v1/audio/transcriptions") && reqs[0].contains("whisper-1") && reqs[0].contains("verbose_json") && reqs[0].contains("RIFF"));
        assert_eq!(got.len(), 4);
        assert_eq!((got[0].0, got[2].0), (1.5, 601.5), "the second piece starts ten minutes in");
        assert_eq!(paragraphs(&got).len(), 2, "pieces within half a minute join into one paragraph");
        assert_eq!(progress.last(), Some(&(2, 2)));
        let anthropic = AiSettings { provider: Provider::Anthropic, ..s };
        assert!(transcribe(&anthropic, &samples, rate, &AtomicBool::new(false), |_, _| {}).is_err());
    }
}
