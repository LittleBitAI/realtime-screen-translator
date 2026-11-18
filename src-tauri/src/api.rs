//! Request and response bodies of `docs/design/api.yaml`, P1..P11.

use serde::{Deserialize, Serialize};

/// An error code from the design (`not_on`, `key_missing`, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError(pub &'static str);

impl Serialize for ApiError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ApiError {}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> ApiError {
        eprintln!("database error: {e}");
        // A full disk is told as such (P9, P11), wherever the database meets it.
        match e.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DiskFull) => ApiError("storage_full"),
            _ => ApiError("internal"),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// A site address as typed: `https://` when no scheme is given; only web pages.
pub fn site_url(typed: &str) -> ApiResult<url::Url> {
    let typed = typed.trim();
    let full = if typed.contains("://") { typed.to_string() } else { format!("https://{typed}") };
    web_url(&full)
}

pub(crate) fn web_url(address: &str) -> ApiResult<url::Url> {
    let url: url::Url = address.parse().map_err(|_| ApiError("bad_url"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(ApiError("bad_url"));
    }
    Ok(url)
}

#[derive(Serialize, Debug)]
pub struct Status {
    pub on: bool,
    pub mode: &'static str,
    pub key_set: bool,
    pub key_tail: Option<String>,
    /// False where this host cannot capture other apps' screens at all.
    pub capture_supported: bool,
    pub capture_allowed: bool,
    pub overlay_allowed: bool,
    pub gemini_requests: u32,
    pub notice: Option<&'static str>,
    /// The language the reader translates from (`ja` or `en`).
    pub source_lang: String,
}

#[derive(Deserialize, Debug)]
pub struct KeyIn {
    pub key: String,
}

#[derive(Serialize, Debug)]
pub struct KeyOut {
    pub key_set: bool,
    pub key_tail: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct PermissionsOut {
    pub capture_allowed: bool,
    pub overlay_allowed: bool,
}

#[derive(Serialize, Debug)]
pub struct StartOut {
    pub on: bool,
    pub book_id: i64,
}

#[derive(Serialize, Debug)]
pub struct StopOut {
    pub on: bool,
    pub book_id: Option<i64>,
    pub picture_count: u32,
}

#[derive(Deserialize, Debug)]
pub struct FrameIn {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub thumb: Vec<u8>,
    #[serde(default)]
    pub captured_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub top: u32,
    pub bottom: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlacementOut {
    pub id: i64,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub radius: u16,
    pub ko: String,
    pub font_px: u16,
}

#[derive(Serialize, Debug)]
pub struct FrameOut {
    pub motion: &'static str,
    pub dy: i32,
    pub mode: &'static str,
    pub picture_id: i64,
    pub detect: Option<Range>,
    pub save: Option<Range>,
    pub clear: bool,
    pub placements: Vec<PlacementOut>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CropIn {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub kind: String,
    #[serde(default)]
    pub score: f32,
    pub image: Vec<u8>,
}

#[derive(Deserialize, Debug)]
pub struct CropsIn {
    pub picture_id: i64,
    pub frame_seq: u64,
    pub crops: Vec<CropIn>,
}

#[derive(Serialize, Debug)]
pub struct CropsOut {
    pub placed: Vec<PlacementOut>,
    pub pending: u32,
    pub skipped_effects: u32,
    pub discarded: u32,
}

#[derive(Deserialize, Debug, Default)]
pub struct PlacementsIn {
    #[serde(default)]
    pub since: Option<u64>,
}

#[derive(Serialize, Debug)]
pub struct PlacementsOut {
    pub revision: u64,
    pub offset_y: i64,
    pub placements: Vec<PlacementOut>,
}

#[derive(Deserialize, Debug)]
pub struct StripIn {
    pub picture_id: i64,
    pub frame_seq: u64,
    pub top: u32,
    pub bottom: u32,
    pub image: Vec<u8>,
}

#[derive(Serialize, Debug)]
pub struct StripOut {
    pub strip_id: i64,
    pub picture_height: u32,
}

fn default_limit() -> u32 {
    20
}

#[derive(Deserialize, Debug)]
pub struct BooksIn {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub before: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct PictureOut {
    pub id: i64,
    pub kind: String,
    pub width: u32,
    pub height: u32,
    pub closed: bool,
    pub preview_path: String,
}

#[derive(Serialize, Debug)]
pub struct BookOut {
    pub id: i64,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub pictures: Vec<PictureOut>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub imported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ChapterOut {
    pub title: String,
    pub pages: Vec<ChapterPageOut>,
}

#[derive(Serialize, Debug)]
pub struct ChapterPageOut {
    pub id: i64,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Debug)]
pub struct BooksOut {
    pub books: Vec<BookOut>,
    pub next_before: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct ReadingList {
    pub id: i64,
    pub name: String,
    pub entries: Vec<i64>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LibraryEdit {
    SaveLink { url: String, title: String },
    Create { name: String },
    Rename { list: i64, name: String },
    Delete { list: i64 },
    Add { list: i64, book: i64 },
    Remove { list: i64, book: i64 },
    MoveList { list: i64, up: bool },
    MoveEntry { list: i64, book: i64, up: bool },
}

fn default_format() -> String {
    "png".to_string()
}

#[derive(Deserialize, Debug)]
pub struct ExportIn {
    pub book_id: i64,
    #[serde(default = "default_format")]
    pub format: String,
}

#[derive(Serialize, Debug)]
pub struct ExportOut {
    pub files: Vec<String>,
    pub count: u32,
}

/// The language the reader translates from: `ja` or `en`.
#[derive(Serialize, Deserialize, Debug)]
pub struct LangIn {
    pub lang: String,
}

/// P12: one page picture of a site opened in the reader. `image` is the
/// picture as the page loaded it; without it the backend fetches `src`.
#[derive(Deserialize, Debug, Default)]
pub struct PageIn {
    pub src: String,
    #[serde(default)]
    pub image: Option<Vec<u8>>,
    pub page_url: String,
    #[serde(default)]
    pub title: String,
    /// Where the picture sits on the site page, so a book keeps reading order.
    #[serde(default)]
    pub index: i64,
}

/// A white shape and its Korean, in the page picture's own pixels.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PageBox {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
    pub radius: u16,
    pub font_px: u16,
    pub ko: String,
}

#[derive(Serialize, Debug)]
pub struct PageOut {
    pub width: u32,
    pub height: u32,
    /// Answered from the saved translation, with no Gemini request.
    pub saved: bool,
    pub boxes: Vec<PageBox>,
    /// Why the translation is not in the library (`storage_full`,
    /// `storage_denied`, `internal`), or none once it is. The answer is kept
    /// in memory, so the same call again saves it with no Gemini request.
    pub save_error: Option<&'static str>,
}
