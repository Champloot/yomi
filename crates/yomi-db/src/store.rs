//! Операции над библиотекой.
//!
//! Ключевое свойство: пересканирование не должно терять прогресс чтения.
//! Поэтому тайтлы и главы обновляются через UPSERT по естественному ключу
//! (путь на диске), а не «удалить всё и вставить заново» — при таком
//! удалении каскад унёс бы и прогресс.

use crate::migrations;
use crate::model::{LibraryChapter, LibraryManga, Progress, ScannedManga, LOCAL_SOURCE};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;

pub struct Store {
    conn: Connection,
}

/// Время в формате ISO-8601 без внешних зависимостей.
///
/// Полноценная работа с датами появится вместе с сетевыми источниками
/// (M3); пока хватает отметки «когда обновлено», по которой сортируется
/// список продолжения чтения.
fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Простой перевод в календарную дату: алгоритм Хиннанта.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn to_json(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
}

fn from_json(raw: &str, field: &'static str) -> Result<Vec<String>> {
    serde_json::from_str(raw).map_err(|e| Error::Decode {
        field,
        message: e.to_string(),
    })
}

impl Store {
    /// Открывает базу по пути, создавая файл и каталог при необходимости.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// База в памяти — для тестов.
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        migrations::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Записывает результат сканирования одного тайтла.
    ///
    /// Возвращает идентификатор тайтла и число добавленных глав.
    pub fn upsert_scanned(&mut self, scanned: &ScannedManga) -> Result<(i64, usize)> {
        let tx = self.conn.transaction()?;
        let now = now_iso8601();

        tx.execute(
            "INSERT INTO manga (source, external_id, title, authors, genres, status, year,
                                description, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (source, external_id) DO UPDATE SET
                 title = excluded.title,
                 authors = excluded.authors,
                 genres = excluded.genres,
                 status = excluded.status,
                 year = excluded.year,
                 description = excluded.description",
            params![
                LOCAL_SOURCE,
                scanned.external_id,
                scanned.title,
                to_json(&scanned.authors),
                to_json(&scanned.genres),
                scanned.status,
                scanned.year,
                scanned.description,
                now,
            ],
        )?;

        let manga_id: i64 = tx.query_row(
            "SELECT id FROM manga WHERE source = ?1 AND external_id = ?2",
            params![LOCAL_SOURCE, scanned.external_id],
            |r| r.get(0),
        )?;

        let mut added = 0usize;
        for ch in &scanned.chapters {
            let changed = tx.execute(
                "INSERT INTO chapters (manga_id, external_id, number, volume, title,
                                       language, scanlator, page_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (manga_id, external_id) DO UPDATE SET
                     number = excluded.number,
                     volume = excluded.volume,
                     title = excluded.title,
                     language = excluded.language,
                     scanlator = excluded.scanlator,
                     page_count = excluded.page_count",
                params![
                    manga_id,
                    ch.external_id,
                    ch.number,
                    ch.volume,
                    ch.title,
                    ch.language,
                    ch.scanlator,
                    ch.page_count,
                ],
            )?;
            added += changed;
        }

        tx.commit()?;
        Ok((manga_id, added))
    }

    /// Все тайтлы библиотеки, с необязательным фильтром по названию.
    pub fn list_manga(&self, filter: Option<&str>) -> Result<Vec<LibraryManga>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, external_id, title, authors, genres, status, year, description
             FROM manga
             WHERE ?1 IS NULL OR title LIKE '%' || ?1 || '%'
             ORDER BY title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![filter], row_to_manga)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .collect::<Result<Vec<_>>>()
    }

    pub fn manga_by_id(&self, id: i64) -> Result<LibraryManga> {
        self.conn
            .query_row(
                "SELECT id, source, external_id, title, authors, genres, status, year, description
                 FROM manga WHERE id = ?1",
                params![id],
                row_to_manga,
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("тайтл {id}")))?
    }

    /// Главы тайтла в порядке чтения.
    pub fn chapters_of(&self, manga_id: i64) -> Result<Vec<LibraryChapter>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, manga_id, external_id, number, volume, title, language,
                    scanlator, page_count
             FROM chapters
             WHERE manga_id = ?1
             ORDER BY volume NULLS LAST, number NULLS LAST, external_id",
        )?;
        let rows = stmt.query_map(params![manga_id], row_to_chapter)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn chapter_by_path(&self, path: &str) -> Result<Option<LibraryChapter>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, manga_id, external_id, number, volume, title, language,
                        scanlator, page_count
                 FROM chapters WHERE external_id = ?1",
                params![path],
                row_to_chapter,
            )
            .optional()?)
    }

    /// Сохраняет прогресс. Глава считается прочитанной, когда достигнута
    /// последняя страница.
    pub fn save_progress(
        &self,
        chapter_id: i64,
        page: u32,
        total_pages: Option<u32>,
    ) -> Result<()> {
        let completed = match total_pages {
            Some(total) if total > 0 => page + 1 >= total,
            _ => false,
        };
        self.conn.execute(
            "INSERT INTO progress (chapter_id, page, total_pages, completed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (chapter_id) DO UPDATE SET
                 page = excluded.page,
                 total_pages = excluded.total_pages,
                 completed = excluded.completed,
                 updated_at = excluded.updated_at",
            params![
                chapter_id,
                page,
                total_pages,
                completed as i64,
                now_iso8601()
            ],
        )?;
        Ok(())
    }

    pub fn progress_of(&self, chapter_id: i64) -> Result<Option<Progress>> {
        Ok(self
            .conn
            .query_row(
                "SELECT chapter_id, page, total_pages, completed FROM progress
                 WHERE chapter_id = ?1",
                params![chapter_id],
                |r| {
                    Ok(Progress {
                        chapter_id: r.get(0)?,
                        page: r.get::<_, i64>(1)? as u32,
                        total_pages: r.get::<_, Option<i64>>(2)?.map(|v| v as u32),
                        completed: r.get::<_, i64>(3)? != 0,
                    })
                },
            )
            .optional()?)
    }

    /// Последняя недочитанная глава — для команды продолжения чтения.
    pub fn last_unfinished(&self) -> Result<Option<(LibraryChapter, Progress)>> {
        let found = self
            .conn
            .query_row(
                "SELECT c.id, c.manga_id, c.external_id, c.number, c.volume, c.title,
                        c.language, c.scanlator, c.page_count,
                        p.page, p.total_pages, p.completed
                 FROM progress p
                 JOIN chapters c ON c.id = p.chapter_id
                 WHERE p.completed = 0
                 ORDER BY p.updated_at DESC
                 LIMIT 1",
                [],
                |r| {
                    let chapter = row_to_chapter(r)?;
                    let progress = Progress {
                        chapter_id: chapter.id,
                        page: r.get::<_, i64>(9)? as u32,
                        total_pages: r.get::<_, Option<i64>>(10)?.map(|v| v as u32),
                        completed: r.get::<_, i64>(11)? != 0,
                    };
                    Ok((chapter, progress))
                },
            )
            .optional()?;
        Ok(found)
    }

    pub fn manga_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM manga", [], |r| r.get(0))?)
    }

    pub fn chapter_count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM chapters", [], |r| r.get(0))?)
    }
}

fn row_to_manga(r: &Row<'_>) -> rusqlite::Result<Result<LibraryManga>> {
    let authors_raw: String = r.get(4)?;
    let genres_raw: String = r.get(5)?;
    let manga = LibraryManga {
        id: r.get(0)?,
        source: r.get(1)?,
        external_id: r.get(2)?,
        title: r.get(3)?,
        authors: Vec::new(),
        genres: Vec::new(),
        status: r.get(6)?,
        year: r.get::<_, Option<i64>>(7)?.map(|v| v as u16),
        description: r.get(8)?,
    };
    Ok((|| {
        Ok(LibraryManga {
            authors: from_json(&authors_raw, "authors")?,
            genres: from_json(&genres_raw, "genres")?,
            ..manga
        })
    })())
}

fn row_to_chapter(r: &Row<'_>) -> rusqlite::Result<LibraryChapter> {
    Ok(LibraryChapter {
        id: r.get(0)?,
        manga_id: r.get(1)?,
        external_id: r.get(2)?,
        number: r.get::<_, Option<f64>>(3)?.map(|v| v as f32),
        volume: r.get::<_, Option<i64>>(4)?.map(|v| v as u16),
        title: r.get(5)?,
        language: r.get(6)?,
        scanlator: r.get(7)?,
        page_count: r.get::<_, Option<i64>>(8)?.map(|v| v as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScannedChapter;

    fn chapter(path: &str, number: f32) -> ScannedChapter {
        ScannedChapter {
            external_id: path.into(),
            number: Some(number),
            volume: Some(1),
            title: None,
            language: "ru".into(),
            scanlator: None,
            page_count: Some(20),
        }
    }

    fn scanned(chapters: Vec<ScannedChapter>) -> ScannedManga {
        ScannedManga {
            external_id: "/манга/Тайтл".into(),
            title: "Тайтл".into(),
            authors: vec!["Автор А.".into()],
            genres: vec!["сэйнэн".into(), "драма".into()],
            status: "ongoing".into(),
            year: Some(2020),
            description: Some("Описание".into()),
            chapters,
        }
    }

    #[test]
    fn upsert_inserts_manga_with_chapters() {
        let mut s = Store::open_in_memory().unwrap();
        let (id, added) = s
            .upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/1.cbz", 1.0)]))
            .unwrap();
        assert!(id > 0);
        assert_eq!(added, 1);
        assert_eq!(s.manga_count().unwrap(), 1);
        assert_eq!(s.chapter_count().unwrap(), 1);
    }

    #[test]
    fn lists_and_round_trips_json_fields() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_scanned(&scanned(vec![])).unwrap();
        let list = s.list_manga(None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].authors, vec!["Автор А."]);
        assert_eq!(list[0].genres, vec!["сэйнэн", "драма"]);
        assert_eq!(list[0].year, Some(2020));
    }

    #[test]
    fn rescanning_does_not_duplicate_anything() {
        let mut s = Store::open_in_memory().unwrap();
        let data = scanned(vec![
            chapter("/манга/Тайтл/1.cbz", 1.0),
            chapter("/манга/Тайтл/2.cbz", 2.0),
        ]);
        s.upsert_scanned(&data).unwrap();
        s.upsert_scanned(&data).unwrap();
        assert_eq!(s.manga_count().unwrap(), 1);
        assert_eq!(s.chapter_count().unwrap(), 2);
    }

    #[test]
    fn rescanning_preserves_reading_progress() {
        // Это главное свойство хранилища: обновление библиотеки не имеет
        // права стирать то, до какой страницы человек дочитал.
        let mut s = Store::open_in_memory().unwrap();
        let data = scanned(vec![chapter("/манга/Тайтл/1.cbz", 1.0)]);
        let (manga_id, _) = s.upsert_scanned(&data).unwrap();

        let chapter_id = s.chapters_of(manga_id).unwrap()[0].id;
        s.save_progress(chapter_id, 12, Some(20)).unwrap();

        s.upsert_scanned(&data).unwrap();

        let progress = s
            .progress_of(chapter_id)
            .unwrap()
            .expect("прогресс должен уцелеть");
        assert_eq!(progress.page, 12);
        assert!(!progress.completed);
    }

    #[test]
    fn reaching_the_last_page_marks_chapter_completed() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/1.cbz", 1.0)]))
            .unwrap();
        let chapter_id = s.chapters_of(manga_id).unwrap()[0].id;

        s.save_progress(chapter_id, 18, Some(20)).unwrap();
        assert!(!s.progress_of(chapter_id).unwrap().unwrap().completed);

        s.save_progress(chapter_id, 19, Some(20)).unwrap();
        assert!(s.progress_of(chapter_id).unwrap().unwrap().completed);
    }

    #[test]
    fn chapters_are_ordered_by_volume_then_number() {
        let mut s = Store::open_in_memory().unwrap();
        let mut ch10 = chapter("/манга/Тайтл/10.cbz", 10.0);
        ch10.volume = Some(1);
        let mut ch2 = chapter("/манга/Тайтл/2.cbz", 2.0);
        ch2.volume = Some(1);
        let mut ch1v2 = chapter("/манга/Тайтл/v2-1.cbz", 1.0);
        ch1v2.volume = Some(2);

        let (manga_id, _) = s.upsert_scanned(&scanned(vec![ch10, ch1v2, ch2])).unwrap();
        let order: Vec<f32> = s
            .chapters_of(manga_id)
            .unwrap()
            .iter()
            .filter_map(|c| c.number)
            .collect();
        assert_eq!(order, vec![2.0, 10.0, 1.0], "т1гл2, т1гл10, затем т2гл1");
    }

    #[test]
    fn last_unfinished_returns_most_recent_and_skips_completed() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![
                chapter("/манга/Тайтл/1.cbz", 1.0),
                chapter("/манга/Тайтл/2.cbz", 2.0),
            ]))
            .unwrap();
        let chapters = s.chapters_of(manga_id).unwrap();

        // Первую дочитали до конца, вторую бросили на середине.
        s.save_progress(chapters[0].id, 19, Some(20)).unwrap();
        s.save_progress(chapters[1].id, 5, Some(20)).unwrap();

        let (chapter, progress) = s.last_unfinished().unwrap().expect("есть недочитанная");
        assert_eq!(chapter.id, chapters[1].id);
        assert_eq!(progress.page, 5);
    }

    #[test]
    fn last_unfinished_is_none_when_everything_is_read() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/1.cbz", 1.0)]))
            .unwrap();
        let id = s.chapters_of(manga_id).unwrap()[0].id;
        s.save_progress(id, 19, Some(20)).unwrap();
        assert!(s.last_unfinished().unwrap().is_none());
    }

    #[test]
    fn filter_narrows_the_list() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_scanned(&scanned(vec![])).unwrap();
        let mut other = scanned(vec![]);
        other.external_id = "/манга/Другой".into();
        other.title = "Другой тайтл".into();
        s.upsert_scanned(&other).unwrap();

        assert_eq!(s.list_manga(None).unwrap().len(), 2);
        assert_eq!(s.list_manga(Some("Друг")).unwrap().len(), 1);
        assert_eq!(s.list_manga(Some("нет такого")).unwrap().len(), 0);
    }

    #[test]
    fn timestamp_looks_like_iso8601() {
        let ts = now_iso8601();
        assert_eq!(ts.len(), 20, "{ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
        // Год должен быть правдоподобным, а не 1970.
        let year: u32 = ts[..4].parse().unwrap();
        assert!(year >= 2024, "получен год {year}");
    }

    #[test]
    fn store_survives_reopening_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("вложенный").join("library.db");
        {
            let mut s = Store::open(&path).unwrap();
            s.upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/1.cbz", 1.0)]))
                .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.manga_count().unwrap(), 1);
    }
}
