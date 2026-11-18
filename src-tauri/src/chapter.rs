use super::reader::ReaderSource;
use super::*;
use crate::api::{ChapterOut, ChapterPageOut, PageOut};
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

impl Backend {
    pub fn import_folder(&self, folder: &Path) -> ApiResult<i64> {
        let title = folder.file_name().unwrap_or(folder.as_os_str()).to_string_lossy();
        let mut pages = vec![];
        for entry in fs::read_dir(folder).map_err(source_error)? {
            let entry = entry.map_err(source_error)?;
            if entry.file_type().map_err(source_error)?.is_file() {
                pages.push((entry.file_name().to_string_lossy().into_owned(), entry.path()));
            }
        }
        self.import_chapter(&title, pages, |source, dest| {
            let mut source = File::open(source).map_err(source_error)?.take(MAX_PAGE_BYTES + 1);
            let mut dest = File::create(dest).map_err(storage_error)?;
            io::copy(&mut source, &mut dest).map_err(storage_error)?;
            Ok(())
        })
    }

    pub fn import_chapter<T>(
        &self,
        title: &str,
        mut pages: Vec<(String, T)>,
        mut copy: impl FnMut(T, &Path) -> ApiResult<()>,
    ) -> ApiResult<i64> {
        pages.retain(|(name, _)| supported(name));
        if pages.is_empty() {
            return Err(ApiError("empty_folder"));
        }
        pages.sort_by(|(left, _), (right, _)| page_order(left, right));

        let strips = self.data_dir.join("strips");
        fs::create_dir_all(&strips).map_err(storage_error)?;
        let staged = tempfile::Builder::new().prefix("chapter-").tempdir_in(strips).map_err(storage_error)?;
        let dir = staged.path().file_name().unwrap().to_string_lossy();
        let mut images = Vec::with_capacity(pages.len());
        for (idx, (_, source)) in pages.into_iter().enumerate() {
            let name = idx.to_string();
            let dest = staged.path().join(&name);
            copy(source, &dest)?;
            let bytes = read_image(&dest)?;
            let img = export::decode_page(&bytes)?;
            // Surface deferred space errors before the database can publish the chapter.
            File::options().write(true).open(&dest).and_then(|file| file.sync_all()).map_err(storage_error)?;
            images.push((img.w, img.h, format!("strips/{dir}/{name}")));
        }

        let st = self.lock();
        let tx = st.db.unchecked_transaction()?;
        let now = now_ms();
        let title: String = title.chars().take(120).collect();
        let book = store::create_book(&tx, &title, now)?;
        for (width, height, path) in images {
            let pic = store::create_picture(&tx, book, "page", width)?;
            store::insert_strip(&tx, pic, 0, height, &path, now)?;
            store::set_covered(&tx, pic, 0, height as i64)?;
            store::close_picture(&tx, pic, now)?;
            store::insert_chapter_page(&tx, pic)?;
        }
        store::end_book(&tx, book, now)?;
        tx.commit()?;
        let _ = staged.keep();
        Ok(book)
    }

    pub fn chapter(&self, book: i64) -> ApiResult<ChapterOut> {
        let st = self.lock();
        let book = store::book(&st.db, book)?.ok_or(ApiError("not_found"))?;
        if !store::is_chapter(&st.db, book.id)? {
            return Err(ApiError("not_found"));
        }
        let pages = store::pictures(&st.db, book.id)?.into_iter().map(|p| ChapterPageOut {
            id: p.id, width: p.width, height: p.covered_bottom as u32,
        }).collect();
        Ok(ChapterOut { title: book.title, pages })
    }

    fn chapter_file(&self, picture: i64) -> ApiResult<(PathBuf, u32, u32)> {
        let (path, width, height) = store::chapter_page(&self.lock().db, picture)?.ok_or(ApiError("not_found"))?;
        Ok((self.data_dir.join(path), width, height))
    }

    pub fn chapter_image(&self, picture: i64) -> ApiResult<Vec<u8>> {
        let (path, _, _) = self.chapter_file(picture)?;
        read_image(&path)
    }

    pub fn read_chapter_page(&self, picture: i64) -> ApiResult<PageOut> {
        let (path, width, height) = self.chapter_file(picture)?;
        let bytes = read_image(&path)?;
        self.read_picture(ReaderSource::Chapter(picture), &bytes, width, height)
    }
}

fn supported(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default();
    ["jpg", "jpeg", "png", "webp"].iter().any(|format| ext.eq_ignore_ascii_case(format))
}

fn read_image(path: &Path) -> ApiResult<Vec<u8>> {
    let mut data = vec![];
    File::open(path).map_err(source_error)?.take(MAX_PAGE_BYTES + 1).read_to_end(&mut data).map_err(source_error)?;
    if data.len() as u64 > MAX_PAGE_BYTES {
        return Err(ApiError("image_too_large"));
    }
    Ok(data)
}

fn source_error(e: io::Error) -> ApiError {
    eprintln!("chapter: {e}");
    ApiError("image_unreachable")
}

fn page_order(left: &str, right: &str) -> Ordering {
    let same_kind = |a: &u8, b: &u8| a.is_ascii_digit() == b.is_ascii_digit();
    let mut a = left.as_bytes().chunk_by(same_kind);
    let mut b = right.as_bytes().chunk_by(same_kind);
    loop {
        match (a.next(), b.next()) {
            (Some(a), Some(b)) => {
                let order = if a[0].is_ascii_digit() && b[0].is_ascii_digit() {
                    let a = &a[a.iter().take_while(|&&c| c == b'0').count()..];
                    let b = &b[b.iter().take_while(|&&c| c == b'0').count()..];
                    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
                } else {
                    a.cmp(b)
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}
