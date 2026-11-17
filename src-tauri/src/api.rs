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
}

#[derive(Serialize, Debug)]
pub struct BooksOut {
    pub books: Vec<BookOut>,
    pub next_before: Option<i64>,
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
