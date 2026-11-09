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
    /// Speech-to-text model for recording transcripts (OpenAI-compatible services).
    #[serde(default = "whisper")] pub transcribe_model: String,
}

fn whisper() -> String { "whisper-1".into() }

impl Default for AiSettings {
    fn default() -> Self {
        Self { provider: Provider::Anthropic, base_url: "https://api.anthropic.com".into(), api_key: String::new(), model: "claude-sonnet-5-5".into(), transcribe_model: whisper() }
    }
}

impl AiSettings {
    pub fn ready(&self) -> bool { !self.model.trim().is_empty() && (!self.api_key.trim().is_empty() || self.local()) }

    /// A server on this computer or the home network (Ollama, LM Studio …): no key needed, and more patience.
    pub fn local(&self) -> bool {
        let u = self.base_url.trim();
        u.strip_prefix("http://").or_else(|| u.strip_prefix("https://")).is_some_and(crate::sync::home_host)
    }
}

/// Send one user turn (optional PNG image) and return the reply text.
pub fn ask(s: &AiSettings, system: &str, prompt: &str, png: Option<&[u8]>) -> Result<String, String> {
    use base64::Engine;
    let img = png.map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        // a model on this computer may first have to load: give it longer
        .timeout_global(Some(Duration::from_secs(if s.local() { 300 } else { 120 })))
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
    let mut resp = req.send_json(&body).map_err(|e| crate::tf!("Couldn't reach the AI service at {url}. Is it running, and is this device online? ({e})", url = url, e = e))?;
    let status = resp.status();
    let v: Value = resp.body_mut().read_json().map_err(|e| crate::tf!("The AI service sent an answer that couldn't be read ({status}: {e}).", status = status, e = e))?;
    if !status.is_success() {
        let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("").to_string();
        // what went wrong, in words that say what to do about it
        let hint = match status.as_u16() {
            401 | 403 => crate::tr("The AI service refused the key. Check it in Settings ▸ AI.").to_string(),
            404 => crate::tf!("The AI service doesn't know the model “{model}”, or the address is wrong. Check Settings ▸ AI.", model = s.model),
            429 => crate::tr("The AI service says there were too many requests, or the account has no credit left. Try again later.").to_string(),
            _ => crate::tf!("The AI service answered with an error ({status}).", status = status),
        };
        return Err(if msg.is_empty() { hint } else { format!("{hint} ({msg})") });
    }
    let (text, cut) = match s.provider {
        Provider::Anthropic => (v["content"].as_array().map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("")), v["stop_reason"] == "max_tokens"),
        Provider::OpenAiCompatible => (v.pointer("/choices/0/message/content").and_then(Value::as_str).map(String::from), v.pointer("/choices/0/finish_reason").and_then(Value::as_str) == Some("length")),
    };
    let text = text.filter(|t| !t.is_empty()).ok_or_else(|| crate::tr("The AI returned no text.").to_string())?;
    // a cut-off answer looks complete otherwise
    Ok(if cut { format!("{text}\n\n_{}_", crate::tr("(The answer was cut off here. Ask it to continue.)")) } else { text })
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

/// Speech to text with timestamps: (seconds from the start, text) per sentence or phrase.
/// Uses the OpenAI-style /audio/transcriptions endpoint, ten minutes at a time.
pub fn transcribe(s: &AiSettings, samples: &[i16], rate: u32) -> Result<Vec<(f32, String)>, String> {
    if s.provider == Provider::Anthropic {
        return Err(crate::tr("Turning speech into text needs an OpenAI-compatible service (OpenAI, Groq or a local Whisper server). Anthropic doesn't offer it.").into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(600))).build().into();
    let url = format!("{}/audio/transcriptions", s.base_url.trim_end_matches('/'));
    let chunk = rate as usize * 600;
    let mut out = vec![];
    for (k, part) in samples.chunks(chunk.max(1)).enumerate() {
        let offset = (k * chunk) as f32 / rate as f32;
        let boundary = format!("leafnotes{}", crate::model::new_id());
        let field = |name: &str, value: &str| format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n");
        let mut body = (field("model", s.transcribe_model.trim()) + &field("response_format", "verbose_json")).into_bytes();
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"recording.wav\"\r\nContent-Type: audio/wav\r\n\r\n").bytes());
        body.extend(crate::audio::wav_bytes(part));
        body.extend(format!("\r\n--{boundary}--\r\n").bytes());
        // a part that fails after others worked: keep those, and say where it stopped
        let stop = |e: String, out: Vec<(f32, String)>| -> Result<Vec<(f32, String)>, String> {
            if out.is_empty() { return Err(e); }
            let mut out = out;
            out.push((offset, crate::tf!("[The transcript stops here, at {m} min: {e}]", m = format!("{:.0}", offset / 60.0), e = e)));
            Ok(out)
        };
        let sent = agent.post(&url).header("authorization", format!("Bearer {}", s.api_key.trim()))
            .header("content-type", format!("multipart/form-data; boundary={boundary}")).send(&body[..]);
        let mut resp = match sent { Ok(r) => r, Err(e) => return stop(format!("Could not reach {url}: {e}"), out) };
        let status = resp.status();
        let v: Value = match resp.body_mut().read_json() { Ok(v) => v, Err(e) => return stop(format!("Bad reply ({status}): {e}"), out) };
        if !status.is_success() { return stop(format!("{status}: {}", v["error"]["message"].as_str().unwrap_or("transcription failed")), out); }
        match v["segments"].as_array() {
            Some(segs) => for seg in segs {
                let t = seg["start"].as_f64().unwrap_or(0.0) as f32 + offset;
                let text = seg["text"].as_str().unwrap_or("").trim().to_string();
                if !text.is_empty() { out.push((t, text)); }
            },
            None => if let Some(text) = v["text"].as_str().filter(|t| !t.trim().is_empty()) { out.push((offset, text.trim().to_string())); },
        }
    }
    Ok(out)
}

/// Consecutive pieces of a transcript joined into paragraphs of about half a minute.
pub fn paragraphs(segs: &[(f32, String)]) -> Vec<(f32, String)> {
    let mut out: Vec<(f32, String)> = vec![];
    for (t, text) in segs {
        match out.last_mut() {
            Some((start, p)) if t - *start < 30.0 && p.len() < 500 => { p.push(' '); p.push_str(text); }
            _ => out.push((*t, text.clone())),
        }
    }
    out
}

#[cfg(test)]
mod transcribe_tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn transcribes_in_ten_minute_pieces() {
        // a stand-in for the speech-to-text service, inside this test
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
                        let len: usize = text[..h].lines().find_map(|l| l.to_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                        if req.len() >= h + 4 + len { break; }
                    }
                    if n == 0 { break; }
                }
                seen.push(String::from_utf8_lossy(&req).into_owned());
                let body = r#"{"segments":[{"start":1.5,"text":" Hello there."},{"start":4.0,"text":"Second part."}]}"#;
                write!(c, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            seen
        });
        let s = AiSettings { provider: Provider::OpenAiCompatible, base_url: format!("http://127.0.0.1:{port}/v1"), api_key: "k".into(), model: "m".into(), transcribe_model: "whisper-1".into() };
        let rate = 100; // a tiny rate keeps the test fast: 10 minutes = 60 000 samples
        let samples = vec![0i16; 60_000 + 10];
        let got = transcribe(&s, &samples, rate).unwrap();
        let reqs = server.join().unwrap();
        assert!(reqs[0].starts_with("POST /v1/audio/transcriptions") && reqs[0].contains("whisper-1") && reqs[0].contains("verbose_json") && reqs[0].contains("RIFF"));
        assert_eq!(got.len(), 4);
        assert_eq!((got[0].0, got[2].0), (1.5, 601.5), "the second piece starts ten minutes in");
        assert_eq!(paragraphs(&got).len(), 2, "pieces within half a minute join into one paragraph");
        let anthropic = AiSettings { provider: Provider::Anthropic, ..s };
        assert!(transcribe(&anthropic, &samples, rate).is_err());
    }
}
