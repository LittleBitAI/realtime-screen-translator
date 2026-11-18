use super::*;
use rusqlite::{params, OptionalExtension};

impl Backend {
    pub fn lists(&self) -> ApiResult<Vec<ReadingList>> {
        let st = self.lock();
        let mut query = st.db.prepare(
            "SELECT l.id, l.name, e.book_id FROM ReadingList l
             LEFT JOIN ListEntry e ON e.list_id = l.id ORDER BY l.position, e.position",
        )?;
        let mut rows = query.query([])?;
        let mut lists: Vec<ReadingList> = Vec::new();
        while let Some(row) = rows.next()? {
            let id = row.get(0)?;
            if lists.last().map(|l| l.id) != Some(id) {
                lists.push(ReadingList { id, name: row.get(1)?, entries: vec![] });
            }
            if let Some(book) = row.get(2)? {
                lists.last_mut().unwrap().entries.push(book);
            }
        }
        Ok(lists)
    }

    pub fn edit_library(&self, edit: LibraryEdit) -> ApiResult<()> {
        use LibraryEdit::*;
        let mut st = self.lock();
        let tx = st.db.transaction()?;
        match edit {
            SaveLink { url, title } => {
                let url = site_url(&url)?;
                let title = name(&title)?;
                let book = match store::reader_book(&tx, url.as_str())? {
                    Some(book) => book,
                    None => {
                        let now = now_ms();
                        let book = store::create_book(&tx, title, now)?;
                        store::end_book(&tx, book, now)?;
                        book
                    }
                };
                tx.execute(
                    "INSERT INTO SavedLink (book_id, url, title) VALUES (?1, ?2, ?3)
                     ON CONFLICT(book_id) DO UPDATE SET title = excluded.title",
                    params![book, url.as_str(), title],
                )?;
            }
            Create { name: input } => {
                tx.execute("INSERT INTO ReadingList (name, position) SELECT ?1, COALESCE(MAX(position), 0) + 1 FROM ReadingList", [name(&input)?])?;
            }
            Rename { list, name: input } => {
                existing(tx.execute("UPDATE ReadingList SET name = ?1 WHERE id = ?2", params![name(&input)?, list])?)?;
            }
            Delete { list } => {
                tx.execute("DELETE FROM ReadingList WHERE id = ?1", [list])?;
            }
            Add { list, book } => {
                tx.execute(
                    "INSERT INTO ListEntry (list_id, book_id, position)
                     SELECT ?1, ?2, COALESCE(MAX(position), 0) + 1 FROM ListEntry WHERE list_id = ?1
                     ON CONFLICT(list_id, book_id) DO NOTHING",
                    params![list, book],
                )?;
            }
            Remove { list, book } => {
                tx.execute("DELETE FROM ListEntry WHERE list_id = ?1 AND book_id = ?2", params![list, book])?;
            }
            MoveList { list, up } => move_one(&tx, None, list, up)?,
            MoveEntry { list, book, up } => move_one(&tx, Some(list), book, up)?,
        }
        tx.commit()?;
        Ok(())
    }
}

fn name(input: &str) -> ApiResult<&str> {
    let name = input.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(ApiError("bad_name"));
    }
    Ok(name)
}

fn existing(count: usize) -> ApiResult<()> {
    if count == 0 { Err(ApiError("not_found")) } else { Ok(()) }
}

/// Swap two adjacent positions inside the edit transaction. Gaps left by removals are fine.
fn move_one(db: &Db, list: Option<i64>, id: i64, up: bool) -> ApiResult<()> {
    let (table, key, scope) = match list {
        Some(_) => ("ListEntry", "book_id", "list_id = ?1"),
        None => ("ReadingList", "id", "?1 IS NULL"),
    };
    let position: i64 = db.query_row(
        &format!("SELECT position FROM {table} WHERE {scope} AND {key} = ?2"),
        params![list, id], |r| r.get(0),
    ).optional()?.ok_or(ApiError("not_found"))?;
    let (cmp, order) = if up { ("<", "DESC") } else { (">", "ASC") };
    let neighbor: Option<i64> = db.query_row(
        &format!("SELECT position FROM {table} WHERE {scope} AND position {cmp} ?2 ORDER BY position {order} LIMIT 1"),
        params![list, position], |r| r.get(0),
    ).optional()?;
    if let Some(other) = neighbor {
        db.execute(
            &format!("UPDATE {table} SET position = CASE position WHEN ?2 THEN ?3 ELSE ?2 END WHERE {scope} AND position IN (?2, ?3)"),
            params![list, position, other],
        )?;
    }
    Ok(())
}
