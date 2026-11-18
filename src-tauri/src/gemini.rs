//! A5: Gemini `generateContent` requests. One request reads the Japanese
//! text of a batch of crops (R26), a second translates that text with no
//! pictures (R27). The reader sends a whole page in one request that finds
//! the balloons and translates them ([`Client::read_page`]). All turn every
//! safety filter `OFF` (R9) and end with a `model` turn holding the start of
//! the JSON answer (R10).

use base64::Engine;
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "gemini-3.1-flash-lite";
pub const DEFAULT_BASE: &str = "https://generativelanguage.googleapis.com";
pub const READ_PREFILL: &str = "{\"ja\":[\"";
pub const TRANSLATE_PREFILL: &str = "{\"ko\":[\"";
pub const PAGE_PREFILL: &str = "{\"items\":[";

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

    /// The dialogue balloons of one manga page (`mime` picture bytes) whose
    /// text is in `lang` (`ja` or `en`), each with its Korean, in reading
    /// order. Sound effects and text drawn into the art are left out.
    pub fn read_page(&self, image: &[u8], mime: &str, lang: &str, may_send: &dyn Fn() -> bool) -> Result<Vec<PageItem>, GeminiError> {
        let source = if lang == "en" { "English" } else { "Japanese" };
        let order = if lang == "en" { "left to right, top to bottom" } else { "right to left, top to bottom" };
        let text = format!(
            "This is one manga page. Find every speech balloon, thought bubble and narration box that holds \
             {source} dialogue. Leave out sound effects, onomatopoeia, signs, screens and any text drawn as \
             part of the art. For each one give box_2d as [ymin, xmin, ymax, xmax] scaled to 0-1000 around all \
             of its text, every line included, and balloon_2d the same way around the whole balloon or box up \
             to its outline, the {source} text as src, and as ko a natural, casual Korean translation that fits \
             the balloon, keeping the tone, rough language included. List them in reading order ({order}). \
             Answer only with JSON {{\"items\": [{{\"box_2d\": [...], \"balloon_2d\": [...], \"src\": \"...\", \"ko\": \"...\"}}]}}."
        );
        let data = base64::engine::general_purpose::STANDARD.encode(image);
        let parts = vec![json!({"text": text}), json!({"inline_data": {"mime_type": mime, "data": data}})];
        let full = self.send(parts, PAGE_PREFILL, may_send)?;
        parse_page(&full).ok_or(GeminiError::Unreadable)
    }

    fn generate(&self, parts: Vec<Value>, prefill: &str, key: &str, n: usize, may_send: &dyn Fn() -> bool) -> Result<Vec<String>, GeminiError> {
        let full = self.send(parts, prefill, may_send)?;
        parse(prefill, &full, key, n).ok_or(GeminiError::Unreadable)
    }

    /// The model's whole answer, prefill included.
    fn send(&self, parts: Vec<Value>, prefill: &str, may_send: &dyn Fn() -> bool) -> Result<String, GeminiError> {
        let url = format!("{}/v1beta/models/{}:generateContent", self.base.trim_end_matches('/'), self.model);
        let mut threshold = "OFF";
        loop {
            let body = json!({
                "contents": [
                    {"role": "user", "parts": parts},
                    {"role": "model", "parts": [{"text": prefill}]}
                ],
                "safetySettings": safety(threshold),
                "generationConfig": {"temperature": 0.2, "maxOutputTokens": 8192}
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
                200 => return continuation(&text).map(|c| with_prefill(prefill, &c)).ok_or(GeminiError::Unreadable),
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

/// The answer as written: the model may or may not repeat the prefill.
fn with_prefill(prefill: &str, continuation: &str) -> String {
    if continuation.trim_start().starts_with(prefill) {
        continuation.trim_start().to_string()
    } else {
        format!("{prefill}{continuation}")
    }
}

/// One balloon of a page: boxes as fractions (0..1) of the page, `[top, left, bottom, right]`.
#[derive(Clone, Debug, PartialEq)]
pub struct PageItem {
    pub text: [f64; 4],
    pub balloon: Option<[f64; 4]>,
    pub src: String,
    pub ko: String,
}

/// A `[ymin, xmin, ymax, xmax]` box on the 0-1000 scale as fractions, if it is one.
fn frac_box(v: &Value) -> Option<[f64; 4]> {
    let a = v.as_array().filter(|a| a.len() == 4)?;
    let mut b = [0.0; 4];
    for (o, x) in b.iter_mut().zip(a) {
        *o = (x.as_f64()? / 1000.0).clamp(0.0, 1.0);
    }
    (b[2] > b[0] && b[3] > b[1]).then_some(b)
}

/// Prefill + continuation must read as JSON `{"items": [...]}`. An item
/// without a usable text box or translation is dropped, not the page.
pub fn parse_page(full: &str) -> Option<Vec<PageItem>> {
    let full = with_prefill(PAGE_PREFILL, full);
    let v: Value = serde_json::from_str(&full).ok()?;
    let items = v["items"].as_array()?;
    Some(
        items
            .iter()
            .filter_map(|it| {
                let ko = it["ko"].as_str()?.trim().to_string();
                if ko.is_empty() {
                    return None;
                }
                Some(PageItem { text: frac_box(&it["box_2d"])?, balloon: frac_box(&it["balloon_2d"]), src: it["src"].as_str().unwrap_or("").trim().to_string(), ko })
            })
            .collect(),
    )
}

/// Prefill + continuation must read as JSON `{key: [n strings]}`.
pub fn parse(prefill: &str, continuation: &str, key: &str, n: usize) -> Option<Vec<String>> {
    let full = with_prefill(prefill, continuation);
    // The whole answer is the JSON, nothing after it (T13: anything else failed).
    let v: Value = serde_json::from_str(&full).ok()?;
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
        assert_eq!(parse(TRANSLATE_PREFILL, "안녕\"]} trailing", "ko", 1), None);
    }

    #[test]
    fn a_page_answer_keeps_whole_balloons_only() {
        let page = parse_page("{\"box_2d\":[100,200,300,400],\"balloon_2d\":[50,150,350,450],\"src\":\"HI\",\"ko\":\"안녕\"},{\"box_2d\":[1,2],\"ko\":\"x\"},{\"box_2d\":[0,0,9,9],\"ko\":\"\"}]}").unwrap();
        assert_eq!(page, vec![PageItem { text: [0.1, 0.2, 0.3, 0.4], balloon: Some([0.05, 0.15, 0.35, 0.45]), src: "HI".into(), ko: "안녕".into() }]);
        assert_eq!(parse_page("]}"), Some(vec![]));
        assert_eq!(parse_page("죄송합니다"), None);
        assert_eq!(parse(TRANSLATE_PREFILL, "안녕\"]}
", "ko", 1), Some(vec!["안녕".into()]));
    }
}
