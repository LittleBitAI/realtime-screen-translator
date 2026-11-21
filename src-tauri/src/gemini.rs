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
    /// text is in `lang` (`ja` or `en`), each with its speaker and Korean, in
    /// reading order. Sound effects and text drawn into the art are left out.
    /// `context` holds the lines of the pages read just before this one, so
    /// each character keeps one voice across page turns.
    pub fn read_page(&self, image: &[u8], mime: &str, lang: &str, context: &[PageItem], may_send: &dyn Fn() -> bool) -> Result<Vec<PageItem>, GeminiError> {
        let data = base64::engine::general_purpose::STANDARD.encode(image);
        let parts = vec![json!({"text": page_prompt(lang, context)}), json!({"inline_data": {"mime_type": mime, "data": data}})];
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

/// Earlier lines given to the model, at most: a chapter so far (about 8 a
/// page), so names, speech levels and forms of address set pages back are
/// kept (ch256 p12, Rin's 아버지 from p7); a few thousand tokens.
pub const CONTEXT_LINES: usize = 160;

/// The reader's request text for one page.
pub fn page_prompt(lang: &str, context: &[PageItem]) -> String {
    let source = if lang == "en" { "English" } else { "Japanese" };
    // A manga page is read right to left whatever it is lettered in (an
    // English scan keeps the Japanese page), as the reader shares a line
    // out over its balloons (`share_out`).
    let order = "right to left, top to bottom";
    let mut text = format!(
        "This is one manga page. Find every speech balloon, thought bubble and narration box that holds \
         {source} dialogue, and every line a character speaks, shouts or thinks that is lettered straight \
         on the art with no balloon round it; each such line counts as a balloon here. Leave out sound \
         effects, onomatopoeia, signs and screens. Give exactly one item per balloon: never merge two balloons into one item, even \
         when they touch or one character says both, and never split one balloon into two items. For each \
         item give box_2d as [ymin, xmin, ymax, xmax] scaled to 0-1000 tightly around all of its text, from \
         the first letter of the first line to the last letter of the last line, and balloon_2d the same way \
         around the whole balloon or box up to its outline (for a line with no balloon, around its \
         lettering); speaker, the name of the character who says it \
         (as named on earlier pages, or a short description when the name is not known); the {source} text \
         as src; and as ko a natural Korean translation that fits the balloon, in the voice the character \
         uses there (casual only where the {source} is). Work out who speaks each balloon from its tail, the \
         panel and the exchange, and keep every character's voice: the speech level (banmal or jondaetmal) \
         and form of address each character uses toward each other one (a character who addresses a parent \
         or elder formally, as \"Father\" or with \"may I\", speaks jondaetmal and says 아버지), their \
         distinctive way of talking (baby talk, lisps and deliberate mispronunciations stay childlike or \
         misspoken in Korean; wordplay becomes Korean wordplay where it can), and the tone, rough language \
         included. Translate every clause of the line; never shorten it. A term of endearment follows who \
         says it to whom (a parent's \"honey\" to a child is 얘야 or 우리 딸, never 자기야). A line broken off \
         mid-phrase breaks off naturally in Korean, never inside a word. Write each name the same way every \
         time, as the earlier pages wrote it. List the items in reading \
         order ({order}). Answer only with JSON {{\"items\": [{{\"box_2d\": [...], \"balloon_2d\": [...], \
         \"speaker\": \"...\", \"src\": \"...\", \"ko\": \"...\"}}]}}."
    );
    let start = context.len().saturating_sub(CONTEXT_LINES);
    if start < context.len() {
        text.push_str(
            "\nThe chapter's pages before this one, in reading order, as speaker: source => Korean already \
             shown to the reader (speakers as guessed then). Keep the same names, speech levels, forms of \
             address and wording for recurring phrases:",
        );
        for it in &context[start..] {
            let speaker = if it.speaker.is_empty() { "?" } else { it.speaker.as_str() };
            text.push_str(&format!("\n{speaker}: {} => {}", it.src.replace('\n', " "), it.ko.replace('\n', " ")));
        }
    }
    text
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
/// Kept as given with each saved page, so its shapes can be laid out again.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PageItem {
    pub text: [f64; 4],
    pub balloon: Option<[f64; 4]>,
    #[serde(default)]
    pub speaker: String,
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

/// Prefill + continuation must read as JSON `{"items": [...]}`, every item
/// with a usable text box and translation; otherwise the answer is
/// unreadable and nothing of it is kept.
pub fn parse_page(full: &str) -> Option<Vec<PageItem>> {
    let full = with_prefill(PAGE_PREFILL, full);
    let items = match serde_json::from_str::<Value>(&full) {
        // Only balloons: any other entry (a null, a bare string) is one the
        // page would be kept short of.
        Ok(v) => Some(v["items"].as_array()?).filter(|a| a.iter().all(Value::is_object))?.clone(),
        Err(_) => salvage(&full)?,
    };
    // Every balloon or none: one without its Korean or its text box (left
    // out, blank, or not four numbers) fails the answer, as an unreadable
    // one does, so the page is not kept with that balloon's lettering
    // showing.
    items
        .iter()
        .map(|it| {
            let ko = it["ko"].as_str()?.trim().to_string();
            if ko.is_empty() {
                return None;
            }
            let field = |k: &str| it[k].as_str().unwrap_or("").trim().to_string();
            Some(PageItem { text: frac_box(&it["box_2d"])?, balloon: frac_box(&it["balloon_2d"]), speaker: field("speaker"), src: field("src"), ko })
        })
        .collect()
}

/// The items of a whole answer (its list closed) that is not JSON as a
/// whole: each item read on its own, a key left without a value before the
/// next key dropped (`"label": "balloon_2d": [...]`, seen once). An item
/// still unreadable fails the whole answer, as a cut-off one (the model ran
/// out of room) does: kept, the page would lack that balloon for good.
fn salvage(full: &str) -> Option<Vec<Value>> {
    let body = full.trim_end().strip_suffix("]}")?;
    let body = body.strip_prefix(PAGE_PREFILL)?;
    let (mut items, mut depth, mut start, mut in_str, mut esc) = (vec![], 0, 0, false, false);
    for (i, c) in body.char_indices() {
        // Between items only commas and spaces: anything else there (a
        // null, a bare string, an item begun and not finished) is an entry
        // the page would be kept short of.
        if depth == 0 && !(c.is_whitespace() || c == ',' || c == '{') {
            return None;
        }
        match c {
            _ if esc => esc = false,
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            '{' if !in_str => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            '}' if !in_str && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let obj = &body[start..=i];
                    items.push(serde_json::from_str(obj).ok().or_else(|| serde_json::from_str(&drop_bare_keys(obj)).ok())?);
                }
            }
            _ => {}
        }
    }
    (!items.is_empty() && depth == 0).then_some(items)
}

/// `"a": "b": 1` as `"b": 1`: a key followed straight by another key.
fn drop_bare_keys(obj: &str) -> String {
    let mut out = obj.to_string();
    while let Some((at, len)) = bare_key(&out) {
        out.replace_range(at..at + len, "");
    }
    out
}

fn bare_key(s: &str) -> Option<(usize, usize)> {
    let b = s.as_bytes();
    // The index of the quote closing the string opened at `open`.
    let close = |open: usize| {
        let mut i = open + 1;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                b'"' => return Some(i),
                _ => i += 1,
            }
        }
        None
    };
    let skip = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    };
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'"' {
            i += 1;
            continue;
        }
        // A string, a colon, then a string that is itself followed by a
        // colon: the first is a key with no value.
        let end = close(i)?;
        let j = skip(end + 1);
        if b.get(j) == Some(&b':') {
            let k = skip(j + 1);
            if b.get(k) == Some(&b'"') {
                if let Some(e2) = close(k) {
                    if b.get(skip(e2 + 1)) == Some(&b':') {
                        return Some((i, k - i));
                    }
                }
            }
        }
        i = end + 1;
    }
    None
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

    /// A chapter's lines so far go with the page (ch256 p12 needs Rin's
    /// "아버지" from p7), the newest kept when there are more.
    #[test]
    fn the_prompt_carries_the_chapter_so_far() {
        let line = |n: usize| PageItem { text: [0.0; 4], balloon: None, speaker: "Rin".into(), src: format!("LINE {n}"), ko: format!("줄 {n}") };
        let context: Vec<PageItem> = (0..CONTEXT_LINES + 5).map(line).collect();
        let prompt = page_prompt("en", &context);
        assert!(prompt.contains("\nRin: LINE 5 => 줄 5") && prompt.contains(&format!("LINE {}", CONTEXT_LINES + 4)));
        assert!(!prompt.contains("LINE 4 =>"));
        assert!(!page_prompt("en", &[]).contains("pages before this one"));
    }

    /// Dialogue lettered on the art with no balloon is asked for (the reader
    /// covers it with the art's tone), and only sound effects, signs and
    /// screens are left out: nothing tells the model to drop text drawn on
    /// the art, in either language.
    #[test]
    fn the_prompt_asks_for_dialogue_lettered_on_the_art() {
        for lang in ["en", "ja"] {
            let prompt = page_prompt(lang, &[]);
            assert!(prompt.contains("lettered straight on the art with no balloon"), "{lang}");
            assert!(prompt.contains("Leave out sound effects, onomatopoeia, signs and screens."), "{lang}");
            assert!(!prompt.contains("part of the art"), "{lang}");
            assert!(prompt.contains("in reading order (right to left, top to bottom)") && !prompt.contains("left to right"), "{lang}");
        }
    }

    #[test]
    fn prefill_continuation_parses_and_refusal_does_not() {
        assert_eq!(parse(READ_PREFILL, "こんにちは\",\"\"]}", "ja", 2), Some(vec!["こんにちは".into(), String::new()]));
        assert_eq!(parse(TRANSLATE_PREFILL, "{\"ko\":[\"안녕\"]}", "ko", 1), Some(vec!["안녕".into()]));
        assert_eq!(parse(TRANSLATE_PREFILL, "죄송하지만 도와드릴 수 없습니다.", "ko", 1), None);
        assert_eq!(parse(TRANSLATE_PREFILL, "안녕\"]}", "ko", 2), None);
        assert_eq!(parse(TRANSLATE_PREFILL, "안녕\"]} trailing", "ko", 1), None);
    }

    #[test]
    fn a_page_answer_is_read_into_its_balloons() {
        let page = parse_page("{\"box_2d\":[100,200,300,400],\"balloon_2d\":[50,150,350,450],\"speaker\":\"Rentaro\",\"src\":\"HI\",\"ko\":\"안녕\"}]}").unwrap();
        assert_eq!(page, vec![PageItem { text: [0.1, 0.2, 0.3, 0.4], balloon: Some([0.05, 0.15, 0.35, 0.45]), speaker: "Rentaro".into(), src: "HI".into(), ko: "안녕".into() }]);
        assert_eq!(parse_page("]}"), Some(vec![]));
        assert_eq!(parse_page("죄송합니다"), None);
        assert_eq!(parse(TRANSLATE_PREFILL, "안녕\"]}
", "ko", 1), Some(vec!["안녕".into()]));
    }

    /// A whole answer with one item written wrong (as Gemini once answered
    /// chapter 258, page 6) keeps every balloon; a cut-off one keeps none.
    #[test]
    fn a_whole_answer_with_a_broken_item_is_read_item_by_item() {
        let answer = r#"
  {"box_2d": [266, 852, 383, 975], "balloon_2d": [243, 842, 413, 985], "speaker": "Rentarou", "src": "OH, RIGHT.", "ko": "아, 맞다."},
  {"box_2d": [533, 852, 650, 975], "label": "balloon_2d": [510, 842, 680, 985], "speaker": "Nuremi", "src": "\"LIKE\" DOESN'T EVEN BEGIN TO COVER IT.", "ko": "좋아한다는 말로는 부족해."},
  {"box_2d": [533, 142, 650, 275], "src": "IT'S MORE LIKE...", "ko": "오히려..."}
]}"#;
        let page = parse_page(answer).unwrap();
        assert_eq!(page.iter().map(|p| p.ko.as_str()).collect::<Vec<_>>(), ["아, 맞다.", "좋아한다는 말로는 부족해.", "오히려..."]);
        assert_eq!(page[1].src, "\"LIKE\" DOESN'T EVEN BEGIN TO COVER IT.");
        assert_eq!(page[1].balloon, Some([0.51, 0.842, 0.68, 0.985]));
        let cut = r#"{"box_2d": [256, 825, 285, 915], "src": "Do take care.", "ko": "조심히 다녀오세요."}, {"box_2d": [26, 540, 276, 725], "ko": "안녕, 하카아아아"#;
        assert_eq!(parse_page(cut), None);
    }

    /// An answer is read whole or not at all: one item no repair reads (a
    /// value left unquoted), in the middle or at either end, fails the
    /// answer, so the page is not kept short of a balloon.
    #[test]
    fn an_answer_with_an_item_no_repair_reads_is_unreadable() {
        let good = |n: u32| format!(r#"{{"box_2d": [{n}, 100, {}, 200], "src": "LINE {n}", "ko": "줄 {n}"}}"#, n + 50);
        let broken = r#"{"box_2d": [400, 100, 450, 200], "src": "LINE", "ko": 줄}"#;
        for at in 0..3 {
            let mut items: Vec<String> = (0..2).map(|k| good(100 + 200 * k)).collect();
            items.insert(at, broken.to_string());
            let answer = format!("{}
]}}", items.join(",
"));
            assert_eq!(parse_page(&answer), None, "broken item at {at}: {answer}");
        }
    }

    /// Every balloon or none: an item without its Korean or its text box
    /// (left out, blank, not a string, not four numbers, no area), at either
    /// end or between two good ones, makes the answer unreadable, whether
    /// it is JSON or read item by item after a repair (a key left without a
    /// value in another item). With every item whole it is read.
    #[test]
    fn an_answer_with_a_balloon_missing_its_korean_or_box_is_unreadable() {
        let good = |n: u32| format!(r#"{{"box_2d": [{n}, 100, {}, 200], "src": "LINE {n}", "ko": "줄 {n}"}}"#, n + 50);
        let repaired = r#"{"box_2d": [700, 100, 750, 200], "label": "balloon_2d": [690, 90, 760, 210], "src": "LINE", "ko": "줄"}"#;
        let missing = [
            r#"{"box_2d": [400, 100, 450, 200], "src": "LINE"}"#,
            r#"{"box_2d": [400, 100, 450, 200], "src": "LINE", "ko": "  "}"#,
            r#"{"box_2d": [400, 100, 450, 200], "src": "LINE", "ko": 7}"#,
            r#"{"src": "LINE", "ko": "줄"}"#,
            r#"{"box_2d": [400, 100], "src": "LINE", "ko": "줄"}"#,
            r#"{"box_2d": [400, 100, "450", 200], "src": "LINE", "ko": "줄"}"#,
            r#"{"box_2d": [400, 200, 400, 200], "src": "LINE", "ko": "줄"}"#,
        ];
        for extra in ["", repaired] {
            let answer = |bad: &str, at: usize| {
                let mut parts = vec![good(100), good(300)];
                if !extra.is_empty() {
                    parts.push(extra.to_string());
                }
                if !bad.is_empty() {
                    parts.insert(at, bad.to_string());
                }
                format!("{}
]}}", parts.join(",
"))
            };
            assert_eq!(parse_page(&answer("", 0)).map(|p| p.len()), Some(2 + usize::from(!extra.is_empty())), "whole, repaired {}", !extra.is_empty());
            for bad in missing {
                for at in 0..3 {
                    assert_eq!(parse_page(&answer(bad, at)), None, "{bad} at {at}, repaired {}", !extra.is_empty());
                }
            }
        }
    }

    /// An answer holds whole balloons and nothing else between them: any
    /// other entry before, between or after them (a null, a bare value, an
    /// item begun and not finished, a stray brace), whether the answer is
    /// otherwise JSON or read item by item, makes it unreadable, so the
    /// page is not kept short of a balloon. With nothing else it is read.
    #[test]
    fn an_answer_with_anything_but_balloons_between_them_is_unreadable() {
        let good = |n: u32| format!(r#"{{"box_2d": [{n}, 100, {}, 200], "src": "LINE {n}", "ko": "줄 {n}"}}"#, n + 50);
        let entries = ["null", r#""text""#, "0", "[]", "}"];
        for trailing in ["", ","] {
            for at in 0..3 {
                let place = |entry: &str| {
                    let mut parts: Vec<String> = vec![good(100), good(300)];
                    if !entry.is_empty() {
                        parts.insert(at, entry.to_string());
                    }
                    format!("{}{trailing}
]}}", parts.join(", "))
                };
                assert_eq!(parse_page(&place("")).map(|p| p.len()), Some(2), "nothing else, at {at}, trailing {trailing:?}");
                for entry in entries {
                    assert_eq!(parse_page(&place(entry)), None, "{entry} at {at}, trailing {trailing:?}");
                }
            }
            // Begun and not finished, at the end.
            for cut in ["{", r#"{"box_2d": [200, 100"#, r#"{"src": "LINE", "ko": "줄"#, r#"{"src": "A}B""#] {
                let answer = format!("{}, {}, {cut}{trailing}
]}}", good(100), good(300));
                assert_eq!(parse_page(&answer), None, "{answer}");
            }
        }
    }
}
