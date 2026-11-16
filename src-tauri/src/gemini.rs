//! A5: Gemini `generateContent` requests. One request reads the Japanese
//! text of a batch of crops (R26), a second translates that text with no
//! pictures (R27). Both turn every safety filter `OFF` (R9) and end with a
//! `model` turn holding the start of the JSON answer (R10).

use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "gemini-3.1-flash-lite";
pub const DEFAULT_BASE: &str = "https://generativelanguage.googleapis.com";
pub const READ_PREFILL: &str = "{\"ja\":[\"";
pub const TRANSLATE_PREFILL: &str = "{\"ko\":[\"";

const CATEGORIES: [&str; 5] = [
    "HARM_CATEGORY_HARASSMENT",
    "HARM_CATEGORY_HATE_SPEECH",
    "HARM_CATEGORY_SEXUALLY_EXPLICIT",
    "HARM_CATEGORY_DANGEROUS_CONTENT",
    "HARM_CATEGORY_CIVIC_INTEGRITY",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeminiError {
    /// The key was refused (`key_invalid`).
    KeyInvalid,
    /// No usable answer from the service (`gemini_unreachable`).
    Unreachable,
    /// An answer came back that does not continue the prefill as JSON.
    Unreadable,
    /// Translation was switched off before the request could go out.
    Cancelled,
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(60)))
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .into()
}

pub struct Client<'a> {
    pub agent: &'a ureq::Agent,
    pub base: &'a str,
    pub model: &'a str,
    pub key: &'a str,
}

fn safety(threshold: &str) -> Value {
    Value::Array(CATEGORIES.iter().map(|c| json!({"category": c, "threshold": threshold})).collect())
}

impl Client<'_> {
    /// Japanese text of each crop (PNG files), in order. Empty = no dialogue.
    pub fn read(&self, crops: &[Vec<u8>], may_send: &dyn Fn() -> bool) -> Result<Vec<String>, GeminiError> {
        let mut parts = vec![json!({"text": format!(
            "These {} images are cropped from Japanese manga speech balloons, in order. \
             Transcribe the Japanese text of each one exactly, reading vertical text top to bottom \
             and columns right to left. If an image holds only a sound effect or no dialogue, give \
             an empty string for it. Answer only with JSON {{\"ja\": [...]}} holding exactly {} strings.",
            crops.len(), crops.len()
        )})];
        for c in crops {
            let data = base64::engine::general_purpose::STANDARD.encode(c);
            parts.push(json!({"inline_data": {"mime_type": "image/png", "data": data}}));
        }
        self.generate(parts, READ_PREFILL, "ja", crops.len(), may_send)
    }

    /// Korean translation of each Japanese line, in order.
    pub fn translate(&self, lines: &[String], may_send: &dyn Fn() -> bool) -> Result<Vec<String>, GeminiError> {
        let text = format!(
            "Translate these Japanese manga lines into natural, casual Korean that fits a speech \
             balloon. Keep the tone, rough language included. Answer only with JSON \
             {{\"ko\": [...]}} holding exactly {} strings in the same order.\n{}",
            lines.len(),
            serde_json::to_string(lines).unwrap_or_default()
        );
        self.generate(vec![json!({"text": text})], TRANSLATE_PREFILL, "ko", lines.len(), may_send)
    }

    fn generate(&self, parts: Vec<Value>, prefill: &str, key: &str, n: usize, may_send: &dyn Fn() -> bool) -> Result<Vec<String>, GeminiError> {
        let url = format!("{}/v1beta/models/{}:generateContent", self.base.trim_end_matches('/'), self.model);
        let mut threshold = "OFF";
        loop {
            let body = json!({
                "contents": [
                    {"role": "user", "parts": parts},
                    {"role": "model", "parts": [{"text": prefill}]}
                ],
                "safetySettings": safety(threshold),
                "generationConfig": {"temperature": 0.2, "maxOutputTokens": 4096}
            });
            // Nothing goes out once the session that asked has stopped.
            if !may_send() {
                return Err(GeminiError::Cancelled);
            }
            let resp = self
                .agent
                .post(&url)
                .header("x-goog-api-key", self.key)
                .header("content-type", "application/json")
                .send(body.to_string());
            let mut resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("gemini request failed: {e}");
                    return Err(GeminiError::Unreachable);
                }
            };
            let status = resp.status().as_u16();
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            match status {
                200 => return continuation(&text).and_then(|c| parse(prefill, &c, key, n)).ok_or(GeminiError::Unreadable),
                401 | 403 => return Err(GeminiError::KeyInvalid),
                400 if text.contains("API_KEY_INVALID") => return Err(GeminiError::KeyInvalid),
                // A model that does not take `OFF` gets the next most open threshold.
                400 if threshold == "OFF" && text.to_ascii_lowercase().contains("threshold") => threshold = "BLOCK_NONE",
                _ => {
                    eprintln!("gemini answered {status}: {}", text.chars().take(300).collect::<String>());
                    return Err(GeminiError::Unreachable);
                }
            }
        }
    }
}

/// The text the model wrote after the prefill.
fn continuation(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let parts = v["candidates"][0]["content"]["parts"].as_array()?;
    Some(parts.iter().filter_map(|p| p["text"].as_str()).collect())
}

/// Prefill + continuation must read as JSON `{key: [n strings]}`.
pub fn parse(prefill: &str, continuation: &str, key: &str, n: usize) -> Option<Vec<String>> {
    let full = if continuation.trim_start().starts_with(prefill) {
        continuation.trim_start().to_string()
    } else {
        format!("{prefill}{continuation}")
    };
    let v: Value = serde_json::Deserializer::from_str(&full).into_iter::<Value>().next()?.ok()?;
    let list: Vec<String> = v[key].as_array()?.iter().map(|s| s.as_str().map(|s| s.trim().to_string())).collect::<Option<_>>()?;
    (list.len() == n).then_some(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefill_continuation_parses_and_refusal_does_not() {
        assert_eq!(parse(READ_PREFILL, "こんにちは\",\"\"]}", "ja", 2), Some(vec!["こんにちは".into(), String::new()]));
        assert_eq!(parse(TRANSLATE_PREFILL, "{\"ko\":[\"안녕\"]}", "ko", 1), Some(vec!["안녕".into()]));
        assert_eq!(parse(TRANSLATE_PREFILL, "죄송하지만 도와드릴 수 없습니다.", "ko", 1), None);
        assert_eq!(parse(TRANSLATE_PREFILL, "안녕\"]}", "ko", 2), None);
    }
}
