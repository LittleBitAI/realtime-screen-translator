//! On-device SQLite (`Setting`, `Translation`, `Book`, `Picture`, `Strip`,
//! `Placement` of the design's data diagram). Picture coordinates: `y` grows
//! down the picture, 0 is the top of the first screen seen in it.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;

pub type Db = Connection;
pub type Res<T> = rusqlite::Result<T>;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
-- Flushed to disk at checkpoints, not at every commit: a frame's writes do
-- not wait on the disk (R7). WAL keeps the file whole either way, and what
-- was committed survives the app closing or crashing; only a power cut may
-- take back the last commits.
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS Setting (
    name TEXT PRIMARY KEY,
    value BLOB NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS Translation (
    fingerprint TEXT PRIMARY KEY,
    aspect REAL NOT NULL,
    source_ja TEXT,
    translation_ko TEXT,
    status TEXT NOT NULL CHECK (status IN ('ready', 'skip', 'failed')),
    model TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS Book (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER
);
CREATE TABLE IF NOT EXISTS Picture (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES Book(id),
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('stitched', 'page')),
    width INTEGER NOT NULL,
    covered_top INTEGER NOT NULL DEFAULT 0,
    covered_bottom INTEGER NOT NULL DEFAULT 0,
    thumb_fingerprint TEXT,
    closed_at INTEGER
);
CREATE TABLE IF NOT EXISTS Strip (
    id INTEGER PRIMARY KEY,
    picture_id INTEGER NOT NULL REFERENCES Picture(id),
    y_start INTEGER NOT NULL,
    height INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS Placement (
    id INTEGER PRIMARY KEY,
    picture_id INTEGER NOT NULL REFERENCES Picture(id),
    fingerprint TEXT NOT NULL REFERENCES Translation(fingerprint),
    x INTEGER NOT NULL,
    y INTEGER NOT NULL,
    w INTEGER NOT NULL,
    h INTEGER NOT NULL,
    radius INTEGER NOT NULL,
    font_px INTEGER NOT NULL
);
-- A page the reader translated: `key` is the picture's SHA-256 and the
-- source language, so the same page is never sent to Gemini twice.
CREATE TABLE IF NOT EXISTS ReaderPage (
    key TEXT PRIMARY KEY,
    picture_id INTEGER NOT NULL REFERENCES Picture(id),
    page_url TEXT NOT NULL
);
-- An imported picture exists before translation and can share a cached
-- translation with a different picture. The link selects its export overlay.
CREATE TABLE IF NOT EXISTS ChapterPage (
    picture_id INTEGER PRIMARY KEY REFERENCES Picture(id),
    translated_key TEXT REFERENCES ReaderPage(key)
);
CREATE TABLE IF NOT EXISTS SavedLink (
    book_id INTEGER PRIMARY KEY REFERENCES Book(id),
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ReadingList (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    position INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ListEntry (
    list_id INTEGER NOT NULL REFERENCES ReadingList(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES Book(id),
    position INTEGER NOT NULL,
    PRIMARY KEY (list_id, book_id)
);
CREATE INDEX IF NOT EXISTS ListEntry_order ON ListEntry(list_id, position);
CREATE INDEX IF NOT EXISTS ReaderPage_picture ON ReaderPage(picture_id);
CREATE VIEW IF NOT EXISTS LibraryBook AS
    SELECT b.id, COALESCE(l.title, b.title) AS title, b.started_at, b.ended_at, COALESCE(l.url, (
        SELECT r.page_url FROM Picture p JOIN ReaderPage r ON r.picture_id = p.id
        WHERE p.book_id = b.id AND (r.page_url LIKE 'https://%' OR r.page_url LIKE 'http://%')
        ORDER BY p.seq, p.id LIMIT 1
    )) AS url
    FROM Book b LEFT JOIN SavedLink l ON l.book_id = b.id
    WHERE l.book_id IS NOT NULL OR EXISTS (
        SELECT 1 FROM Picture p JOIN Strip s ON s.picture_id = p.id WHERE p.book_id = b.id
    );
CREATE INDEX IF NOT EXISTS ReaderPage_url ON ReaderPage(page_url);
CREATE INDEX IF NOT EXISTS Picture_book ON Picture(book_id, seq);
CREATE INDEX IF NOT EXISTS Strip_picture ON Strip(picture_id, y_start);
CREATE INDEX IF NOT EXISTS Placement_picture ON Placement(picture_id);
"#;

pub fn open(dir: &Path, now: i64) -> Res<Db> {
    let _ = std::fs::create_dir_all(dir);
    let db = Connection::open(dir.join("translator.sqlite3"))?;
    db.execute_batch(SCHEMA)?;
    // Added after the first libraries were made: Gemini's answer for a
    // reader page, kept so the page's shapes can be laid out again.
    if !column_exists(&db, "ReaderPage", "items")? {
        db.execute_batch("ALTER TABLE ReaderPage ADD COLUMN items TEXT")?;
    }
    if !column_exists(&db, "Placement", "cover")? {
        db.execute_batch("ALTER TABLE Placement ADD COLUMN cover TEXT")?;
    }
    // A book left open by a crash or a kill ends where it stopped.
    db.execute("UPDATE Picture SET closed_at = ?1 WHERE closed_at IS NULL", [now])?;
    db.execute("UPDATE Book SET ended_at = ?1 WHERE ended_at IS NULL", [now])?;
    Ok(db)
}

fn column_exists(db: &Db, table: &str, column: &str) -> Res<bool> {
    db.query_row(&format!("SELECT EXISTS (SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"), [column], |r| r.get(0))
}

// ---- Setting

pub fn setting(db: &Db, name: &str) -> Res<Option<Vec<u8>>> {
    db.query_row("SELECT value FROM Setting WHERE name = ?1", [name], |r| r.get(0)).optional()
}

pub fn put_setting(db: &Db, name: &str, value: &[u8], now: i64) -> Res<()> {
    db.execute(
        "INSERT INTO Setting (name, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![name, value, now],
    )?;
    Ok(())
}

pub fn delete_setting(db: &Db, name: &str) -> Res<()> {
    db.execute("DELETE FROM Setting WHERE name = ?1", [name])?;
    Ok(())
}

// ---- Translation

#[derive(Clone, Debug)]
pub struct Translation {
    pub fingerprint: String,
    pub aspect: f32,
    pub source_ja: Option<String>,
    pub translation_ko: Option<String>,
    pub status: String,
}

pub fn translations(db: &Db) -> Res<Vec<Translation>> {
    let mut st = db.prepare("SELECT fingerprint, aspect, source_ja, translation_ko, status FROM Translation")?;
    let rows = st.query_map([], |r| {
        Ok(Translation {
            fingerprint: r.get(0)?,
            aspect: r.get::<_, f64>(1)? as f32,
            source_ja: r.get(2)?,
            translation_ko: r.get(3)?,
            status: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// Store a translation. A `ready` row is never overwritten: placements read
/// their Korean from it, so saved books keep the sentence they were shown.
/// Returns whether the row now holds `t`.
pub fn put_translation(db: &Db, t: &Translation, model: &str, now: i64) -> Res<bool> {
    let n = db.execute(
        "INSERT INTO Translation (fingerprint, aspect, source_ja, translation_ko, status, model, created_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(fingerprint) DO UPDATE SET aspect = excluded.aspect, source_ja = excluded.source_ja,
             translation_ko = excluded.translation_ko, status = excluded.status, model = excluded.model,
             last_seen_at = excluded.last_seen_at
         WHERE Translation.status <> 'ready'",
        params![t.fingerprint, t.aspect as f64, t.source_ja, t.translation_ko, t.status, model, now],
    )?;
    Ok(n > 0)
}

pub fn touch_translation(db: &Db, fingerprint: &str, now: i64) -> Res<()> {
    db.execute("UPDATE Translation SET last_seen_at = ?2 WHERE fingerprint = ?1", params![fingerprint, now])?;
    Ok(())
}

// ---- Book, Picture, Strip, Placement

pub fn create_book(db: &Db, title: &str, now: i64) -> Res<i64> {
    db.execute("INSERT INTO Book (title, started_at) VALUES (?1, ?2)", params![title, now])?;
    Ok(db.last_insert_rowid())
}

pub fn end_book(db: &Db, id: i64, now: i64) -> Res<()> {
    db.execute("UPDATE Book SET ended_at = ?2 WHERE id = ?1 AND ended_at IS NULL", params![id, now])?;
    Ok(())
}

pub fn create_picture(db: &Db, book_id: i64, kind: &str, width: u32) -> Res<i64> {
    db.execute(
        "INSERT INTO Picture (book_id, seq, kind, width)
         VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM Picture WHERE book_id = ?1), ?2, ?3)",
        params![book_id, kind, width],
    )?;
    Ok(db.last_insert_rowid())
}

pub fn set_picture_kind(db: &Db, id: i64, kind: &str) -> Res<()> {
    db.execute("UPDATE Picture SET kind = ?2 WHERE id = ?1", params![id, kind])?;
    Ok(())
}

pub fn set_picture_thumb(db: &Db, id: i64, thumb_fingerprint: Option<&str>) -> Res<()> {
    db.execute("UPDATE Picture SET thumb_fingerprint = ?2 WHERE id = ?1", params![id, thumb_fingerprint])?;
    Ok(())
}

pub fn set_covered(db: &Db, id: i64, top: i64, bottom: i64) -> Res<()> {
    db.execute("UPDATE Picture SET covered_top = ?2, covered_bottom = ?3 WHERE id = ?1", params![id, top, bottom])?;
    Ok(())
}

pub fn close_picture(db: &Db, id: i64, now: i64) -> Res<()> {
    db.execute("UPDATE Picture SET closed_at = ?2 WHERE id = ?1 AND closed_at IS NULL", params![id, now])?;
    Ok(())
}

pub fn reopen_picture(db: &Db, id: i64) -> Res<()> {
    db.execute("UPDATE Picture SET closed_at = NULL WHERE id = ?1", [id])?;
    Ok(())
}

pub fn insert_strip(db: &Db, picture_id: i64, y_start: i64, height: u32, file_path: &str, now: i64) -> Res<i64> {
    db.execute(
        "INSERT INTO Strip (picture_id, y_start, height, file_path, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![picture_id, y_start, height, file_path, now],
    )?;
    Ok(db.last_insert_rowid())
}

#[derive(Clone, Debug)]
pub struct PlacementRow {
    pub id: i64,
    pub fingerprint: String,
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
    pub radius: u16,
    pub font_px: u16,
    pub ko: String,
    /// A reader shape's outline and text area (JSON of `layout::Cover`);
    /// none for a plain rounded rectangle.
    pub cover: Option<String>,
}

pub fn insert_placement(db: &Db, picture_id: i64, p: &PlacementRow) -> Res<i64> {
    db.execute(
        "INSERT INTO Placement (picture_id, fingerprint, x, y, w, h, radius, font_px, cover) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![picture_id, p.fingerprint, p.x, p.y, p.w, p.h, p.radius, p.font_px, p.cover],
    )?;
    Ok(db.last_insert_rowid())
}

pub fn delete_placement(db: &Db, id: i64) -> Res<()> {
    db.execute("DELETE FROM Placement WHERE id = ?1", [id])?;
    Ok(())
}

pub fn placements(db: &Db, picture_id: i64) -> Res<Vec<PlacementRow>> {
    picture_placements(db, picture_id, None)
}

fn picture_placements(db: &Db, picture_id: i64, prefix: Option<&str>) -> Res<Vec<PlacementRow>> {
    let mut st = db.prepare(
        "SELECT p.id, p.fingerprint, p.x, p.y, p.w, p.h, p.radius, p.font_px, COALESCE(t.translation_ko, ''), p.cover
         FROM Placement p JOIN Translation t ON t.fingerprint = p.fingerprint
         WHERE p.picture_id = ?1 AND (?2 IS NULL OR p.fingerprint GLOB ?2) ORDER BY p.id",
    )?;
    let rows = st.query_map(params![picture_id, prefix], |r| {
        Ok(PlacementRow {
            id: r.get(0)?,
            fingerprint: r.get(1)?,
            x: r.get(2)?,
            y: r.get(3)?,
            w: r.get(4)?,
            h: r.get(5)?,
            radius: r.get(6)?,
            font_px: r.get(7)?,
            ko: r.get(8)?,
            cover: r.get(9)?,
        })
    })?;
    rows.collect()
}

// ---- ReaderPage

/// The picture a page with this key was saved as, if it was.
pub fn reader_page(db: &Db, key: &str) -> Res<Option<i64>> {
    db.query_row("SELECT picture_id FROM ReaderPage WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub fn reader_placements(db: &Db, key: &str) -> Res<Vec<PlacementRow>> {
    match reader_page(db, key)? {
        Some(id) => picture_placements(db, id, Some(&format!("page:{key}:*"))),
        None => Ok(vec![]),
    }
}

/// The book of the site page `page_url`, if one of its pictures was saved.
pub fn reader_book(db: &Db, page_url: &str) -> Res<Option<i64>> {
    db.query_row(
        "SELECT book_id FROM SavedLink WHERE url = ?1 UNION ALL
         SELECT p.book_id FROM ReaderPage r JOIN Picture p ON p.id = r.picture_id
         WHERE r.page_url = ?1 AND NOT EXISTS (SELECT 1 FROM ChapterPage c WHERE c.picture_id = p.id) LIMIT 1",
        [page_url],
        |r| r.get(0),
    )
    .optional()
}

pub fn insert_reader_page(db: &Db, key: &str, picture_id: i64, page_url: &str, items: &str) -> Res<()> {
    db.execute("INSERT INTO ReaderPage (key, picture_id, page_url, items) VALUES (?1, ?2, ?3, ?4)", params![key, picture_id, page_url, items])?;
    Ok(())
}

/// Gemini's answer (JSON) kept for a reader page, if it was kept.
pub fn reader_items(db: &Db, key: &str) -> Res<Option<String>> {
    db.query_row("SELECT items FROM ReaderPage WHERE key = ?1", [key], |r| r.get(0)).optional().map(Option::flatten)
}

/// Keeps Gemini's answer (JSON) for a reader page saved without one.
pub fn set_reader_items(db: &Db, key: &str, items: &str) -> Res<()> {
    db.execute("UPDATE ReaderPage SET items = ?2 WHERE key = ?1", params![key, items])?;
    Ok(())
}

/// A reader page's placements, in order, each with the source text of its
/// translation: what a page saved before its answer was kept still holds.
pub fn reader_lines(db: &Db, key: &str) -> Res<Vec<(PlacementRow, String)>> {
    let Some(id) = reader_page(db, key)? else { return Ok(vec![]) };
    let sources: HashMap<String, String> = {
        let mut st = db.prepare("SELECT t.fingerprint, COALESCE(t.source_ja, '') FROM Placement p JOIN Translation t ON t.fingerprint = p.fingerprint WHERE p.picture_id = ?1")?;
        let rows = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Res<_>>()?
    };
    Ok(reader_placements(db, key)?.into_iter().map(|p| {
        let src = sources.get(&p.fingerprint).cloned().unwrap_or_default();
        (p, src)
    }).collect())
}

/// The kept answers (JSON) of the pages read just before `seq` in `book`,
/// nearest last: a site page's own, or the page an imported picture shares.
pub fn earlier_items(db: &Db, book: i64, seq: i64, pages: u32) -> Res<Vec<String>> {
    let mut st = db.prepare(
        "SELECT items FROM (
             SELECT p.seq, r.items FROM Picture p LEFT JOIN ChapterPage c ON c.picture_id = p.id
             JOIN ReaderPage r ON r.key = c.translated_key OR (c.picture_id IS NULL AND r.picture_id = p.id)
             WHERE p.book_id = ?1 AND p.seq < ?2 AND r.items IS NOT NULL ORDER BY p.seq DESC LIMIT ?3
         ) ORDER BY seq",
    )?;
    let rows = st.query_map(params![book, seq, pages], |r| r.get(0))?;
    rows.collect()
}

/// The book and place of a picture.
pub fn picture_place(db: &Db, picture_id: i64) -> Res<Option<(i64, i64)>> {
    db.query_row("SELECT book_id, seq FROM Picture WHERE id = ?1", [picture_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()
}

/// The reader page's shapes, laid out again: its placements are replaced.
pub fn replace_reader_placements(db: &Db, key: &str, rows: &[PlacementRow]) -> Res<()> {
    let Some(pic) = reader_page(db, key)? else { return Ok(()) };
    db.execute("DELETE FROM Placement WHERE picture_id = ?1 AND fingerprint GLOB ?2", params![pic, format!("page:{key}:*")])?;
    for row in rows {
        insert_placement(db, pic, row)?;
    }
    Ok(())
}

pub fn insert_chapter_page(db: &Db, picture_id: i64) -> Res<()> {
    db.execute("INSERT INTO ChapterPage (picture_id) VALUES (?1)", [picture_id])?;
    Ok(())
}

pub fn is_chapter(db: &Db, book_id: i64) -> Res<bool> {
    db.query_row(
        "SELECT EXISTS (SELECT 1 FROM Picture p JOIN ChapterPage c ON c.picture_id = p.id WHERE p.book_id = ?1)",
        [book_id],
        |r| r.get(0),
    )
}

pub fn chapter_page(db: &Db, picture_id: i64) -> Res<Option<(String, u32, u32)>> {
    db.query_row(
        "SELECT s.file_path, p.width, s.height FROM ChapterPage c JOIN Picture p ON p.id = c.picture_id
         JOIN Strip s ON s.picture_id = p.id WHERE c.picture_id = ?1",
        [picture_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
}

pub fn chapter_translation(db: &Db, picture_id: i64) -> Res<Option<String>> {
    db.query_row("SELECT translated_key FROM ChapterPage WHERE picture_id = ?1", [picture_id], |r| r.get(0))
        .optional()
        .map(Option::flatten)
}

pub fn set_chapter_translation(db: &Db, picture_id: i64, key: &str) -> Res<()> {
    db.execute("UPDATE ChapterPage SET translated_key = ?2 WHERE picture_id = ?1 AND translated_key IS NOT ?2", params![picture_id, key])?;
    Ok(())
}

pub fn set_picture_seq(db: &Db, id: i64, seq: i64) -> Res<()> {
    db.execute("UPDATE Picture SET seq = ?1 WHERE id = ?2", params![seq, id])?;
    Ok(())
}

#[derive(Clone, Debug)]
pub struct BookRow {
    pub id: i64,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct PictureRow {
    pub id: i64,
    pub kind: String,
    pub width: u32,
    pub covered_top: i64,
    pub covered_bottom: i64,
    pub closed: bool,
}

#[derive(Clone, Debug)]
pub struct StripRow {
    pub y_start: i64,
    pub height: u32,
    pub file_path: String,
}

fn book_row(r: &rusqlite::Row) -> Res<BookRow> {
    Ok(BookRow { id: r.get(0)?, title: r.get(1)?, started_at: r.get(2)?, ended_at: r.get(3)? })
}

pub fn book(db: &Db, id: i64) -> Res<Option<BookRow>> {
    db.query_row("SELECT id, title, started_at, ended_at FROM Book WHERE id = ?1", [id], book_row).optional()
}

/// The cursor is a book id in both the newest-first library and a manually ordered list.
pub fn books(db: &Db, limit: u32, before: Option<i64>, list: Option<i64>, search: &str) -> Res<Vec<(BookRow, Option<String>)>> {
    let mut st = db.prepare(
        "SELECT b.id, b.title, b.started_at, b.ended_at, b.url FROM LibraryBook b
         LEFT JOIN ListEntry e ON e.book_id = b.id AND e.list_id = ?3
         WHERE (?3 IS NULL OR e.book_id IS NOT NULL)
           AND (?1 IS NULL OR CASE WHEN ?3 IS NULL THEN b.id < ?1 ELSE
               e.position > (SELECT position FROM ListEntry WHERE list_id = ?3 AND book_id = ?1) END)
           AND (?4 = '' OR instr(lower(b.title), lower(?4)) > 0 OR instr(lower(b.url), lower(?4)) > 0)
         ORDER BY CASE WHEN ?3 IS NULL THEN -b.id ELSE e.position END LIMIT ?2",
    )?;
    let rows = st.query_map(params![before, limit, list, search], |r| Ok((book_row(r)?, r.get(4)?)))?;
    rows.collect()
}

/// Pictures of a book that hold at least one strip, in reading order. The
/// range is taken from the strips themselves, so it always holds all of them
/// (a database written before strips and range were one commit included).
pub fn pictures(db: &Db, book_id: i64) -> Res<Vec<PictureRow>> {
    let mut st = db.prepare(
        "SELECT p.id, p.kind, p.width, MIN(s.y_start), MAX(s.y_start + s.height), p.closed_at IS NOT NULL
         FROM Picture p JOIN Strip s ON s.picture_id = p.id
         WHERE p.book_id = ?1 GROUP BY p.id ORDER BY p.seq",
    )?;
    let rows = st.query_map([book_id], |r| {
        Ok(PictureRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            width: r.get(2)?,
            covered_top: r.get(3)?,
            covered_bottom: r.get(4)?,
            closed: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// Strips in the order they were saved: a later strip is drawn over an
/// earlier one, as when content scrolls into rows first saved under a
/// status bar not yet known as fixed.
pub fn strips(db: &Db, picture_id: i64) -> Res<Vec<StripRow>> {
    let mut st = db.prepare("SELECT y_start, height, file_path FROM Strip WHERE picture_id = ?1 ORDER BY id")?;
    let rows = st.query_map([picture_id], |r| Ok(StripRow { y_start: r.get(0)?, height: r.get(1)?, file_path: r.get(2)? }))?;
    rows.collect()
}
