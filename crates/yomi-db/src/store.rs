//! Операции над библиотекой.
//!
//! Ключевое свойство: пересканирование не должно терять прогресс чтения.
//! Поэтому тайтлы и главы обновляются через UPSERT по естественному ключу
//! (путь на диске), а не «удалить всё и вставить заново» — при таком
//! удалении каскад унёс бы и прогресс.

use crate::migrations;
use crate::model::{
    ChapterMark, LibraryChapter, LibraryManga, Progress, ScannedManga, LOCAL_SOURCE,
};
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
                                       language, scanlator, page_count, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (manga_id, external_id) DO UPDATE SET
                     number = excluded.number,
                     volume = excluded.volume,
                     title = excluded.title,
                     language = excluded.language,
                     scanlator = excluded.scanlator,
                     page_count = excluded.page_count,
                     kind = excluded.kind",
                params![
                    manga_id,
                    ch.external_id,
                    ch.number,
                    ch.volume,
                    ch.title,
                    ch.language,
                    ch.scanlator,
                    ch.page_count,
                    ch.kind,
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
                    scanlator, page_count, kind
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
                        scanlator, page_count, kind
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
                        c.language, c.scanlator, c.page_count, c.kind,
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
                        page: r.get::<_, i64>(10)? as u32,
                        total_pages: r.get::<_, Option<i64>>(11)?.map(|v| v as u32),
                        completed: r.get::<_, i64>(12)? != 0,
                    };
                    Ok((chapter, progress))
                },
            )
            .optional()?;
        Ok(found)
    }

    /// Все главы библиотеки — для проверки, что файлы ещё на месте.
    pub fn all_chapters(&self) -> Result<Vec<LibraryChapter>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, manga_id, external_id, number, volume, title, language,
                    scanlator, page_count, kind
             FROM chapters ORDER BY manga_id, volume, number",
        )?;
        let rows = stmt.query_map([], row_to_chapter)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Удаляет главы по идентификаторам. Прогресс уходит вместе с ними
    /// каскадом — это верно только когда файла действительно больше нет.
    pub fn delete_chapters(&mut self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.transaction()?;
        let mut removed = 0usize;
        {
            let mut stmt = tx.prepare("DELETE FROM chapters WHERE id = ?1")?;
            for id in ids {
                removed += stmt.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Убирает тайтлы, у которых не осталось ни одной главы.
    pub fn delete_empty_manga(&mut self) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM manga
             WHERE id NOT IN (SELECT DISTINCT manga_id FROM chapters)",
            [],
        )?)
    }

    /// Отметки глав внутри файла, по возрастанию страницы.
    pub fn marks_of(&self, chapter_id: i64) -> Result<Vec<ChapterMark>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, chapter_id, page, title FROM chapter_marks
             WHERE chapter_id = ?1 ORDER BY page",
        )?;
        let rows = stmt.query_map(params![chapter_id], |r| {
            Ok(ChapterMark {
                id: r.get(0)?,
                chapter_id: r.get(1)?,
                page: r.get::<_, i64>(2)? as u32,
                title: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Ставит отметку. Повторная установка на ту же страницу обновляет
    /// название, а не создаёт вторую отметку.
    pub fn add_mark(&self, chapter_id: i64, page: u32, title: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chapter_marks (chapter_id, page, title) VALUES (?1, ?2, ?3)
             ON CONFLICT (chapter_id, page) DO UPDATE SET title = excluded.title",
            params![chapter_id, page, title],
        )?;
        Ok(())
    }

    /// Снимает отметку. Возвращает true, если она там была.
    pub fn remove_mark(&self, chapter_id: i64, page: u32) -> Result<bool> {
        let removed = self.conn.execute(
            "DELETE FROM chapter_marks WHERE chapter_id = ?1 AND page = ?2",
            params![chapter_id, page],
        )?;
        Ok(removed > 0)
    }

    /// Ставит отметку, если её не было, и снимает, если была.
    ///
    /// Нужна читалке: одна клавиша и для установки, и для снятия —
    /// иначе пользователю пришлось бы помнить состояние текущей страницы.
    pub fn toggle_mark(&self, chapter_id: i64, page: u32, title: Option<&str>) -> Result<bool> {
        if self.remove_mark(chapter_id, page)? {
            return Ok(false);
        }
        self.add_mark(chapter_id, page, title)?;
        Ok(true)
    }

    pub fn clear_marks(&self, chapter_id: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM chapter_marks WHERE chapter_id = ?1",
            params![chapter_id],
        )?)
    }

    /// На чём остановились в этом тайтле.
    ///
    /// Сначала ищется недочитанная глава с самой свежей отметкой
    /// прогресса — это буквально «где я был». Если такой нет, берётся
    /// первая непрочитанная по порядку: значит, тайтл либо не начат,
    /// либо дочитан до конца очередного тома и пора браться за следующий.
    pub fn resume_target(
        &self,
        manga_id: i64,
    ) -> Result<Option<(LibraryChapter, Option<Progress>)>> {
        let in_progress = self
            .conn
            .query_row(
                "SELECT c.id, c.manga_id, c.external_id, c.number, c.volume, c.title,
                        c.language, c.scanlator, c.page_count, c.kind,
                        p.page, p.total_pages, p.completed
                 FROM progress p
                 JOIN chapters c ON c.id = p.chapter_id
                 WHERE c.manga_id = ?1 AND p.completed = 0
                 ORDER BY p.updated_at DESC
                 LIMIT 1",
                params![manga_id],
                |r| {
                    let chapter = row_to_chapter(r)?;
                    let progress = Progress {
                        chapter_id: chapter.id,
                        page: r.get::<_, i64>(10)? as u32,
                        total_pages: r.get::<_, Option<i64>>(11)?.map(|v| v as u32),
                        completed: r.get::<_, i64>(12)? != 0,
                    };
                    Ok((chapter, Some(progress)))
                },
            )
            .optional()?;

        if in_progress.is_some() {
            return Ok(in_progress);
        }

        // Ничего не начато или всё начатое дочитано: первая глава,
        // которую ещё не закрыли.
        for chapter in self.chapters_of(manga_id)? {
            let done = self
                .progress_of(chapter.id)?
                .map(|p| p.completed)
                .unwrap_or(false);
            if !done {
                return Ok(Some((chapter, None)));
            }
        }
        Ok(None)
    }

    /// Диапазон томов тайтла — для краткой строки в списке библиотеки.
    pub fn volume_range(&self, manga_id: i64) -> Result<Option<(u16, u16)>> {
        let mut volumes: Vec<u16> = self
            .chapters_of(manga_id)?
            .into_iter()
            .filter_map(|c| c.volume)
            .collect();
        if volumes.is_empty() {
            return Ok(None);
        }
        volumes.sort_unstable();
        Ok(Some((volumes[0], *volumes.last().unwrap())))
    }

    /// Удаляет тайтл вместе с главами, прогрессом и отметками.
    pub fn delete_manga(&mut self, manga_id: i64) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM manga WHERE id = ?1", params![manga_id])?)
    }

    /// Удаляет запись о файле по пути.
    pub fn delete_chapter_by_path(&mut self, path: &str) -> Result<bool> {
        let removed = self
            .conn
            .execute("DELETE FROM chapters WHERE external_id = ?1", params![path])?;
        Ok(removed > 0)
    }

    /// Полная очистка библиотеки.
    ///
    /// Прогресс и ручные отметки уходят вместе с записями: восстановить
    /// их неоткуда, поэтому вызывающий код обязан спросить подтверждение.
    pub fn clear_all(&mut self) -> Result<(usize, usize)> {
        let tx = self.conn.transaction()?;
        let chapters = tx.execute("DELETE FROM chapters", [])?;
        let manga = tx.execute("DELETE FROM manga", [])?;
        tx.commit()?;
        Ok((manga, chapters))
    }

    /// Записи, чей путь начинается с указанного каталога.
    pub fn chapters_under(&self, prefix: &str) -> Result<Vec<LibraryChapter>> {
        Ok(self
            .all_chapters()?
            .into_iter()
            .filter(|c| c.external_id.starts_with(prefix))
            .collect())
    }

    /// Ищет том тайтла по номеру.
    pub fn chapter_by_volume(&self, manga_id: i64, volume: u16) -> Result<Option<LibraryChapter>> {
        Ok(self
            .chapters_of(manga_id)?
            .into_iter()
            .find(|c| c.volume == Some(volume)))
    }

    /// Ищет, в каком файле лежит глава с указанным номером.
    ///
    /// Отвечает на вопрос «мне сказали про 234-ю главу, где она?» —
    /// иначе пришлось бы открывать тома наугад. Сначала смотрим на
    /// номер самой записи, потом на закладки внутри собранного тома.
    pub fn locate_chapter(
        &self,
        manga_id: i64,
        number: f32,
    ) -> Result<Option<(LibraryChapter, Option<u32>)>> {
        for chapter in self.chapters_of(manga_id)? {
            if chapter.number == Some(number) {
                return Ok(Some((chapter, None)));
            }
        }

        // Внутри тома главы размечены отметками; их подписи хранят номер.
        for chapter in self.chapters_of(manga_id)? {
            for mark in self.marks_of(chapter.id)? {
                let Some(title) = &mark.title else { continue };
                if label_number(title) == Some(number) {
                    return Ok(Some((chapter, Some(mark.page))));
                }
            }
        }
        Ok(None)
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

/// Достаёт номер главы из подписи закладки вида «Глава 234 — Название».
fn label_number(label: &str) -> Option<f32> {
    let digits: String = label
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.trim_end_matches('.').parse().ok()
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
        kind: r.get(9)?,
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
            kind: "chapter".into(),
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
    fn resume_target_prefers_the_chapter_in_progress() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![
                chapter("/м/Т/1.cbz", 1.0),
                chapter("/м/Т/2.cbz", 2.0),
                chapter("/м/Т/3.cbz", 3.0),
            ]))
            .unwrap();
        let chapters = s.chapters_of(manga_id).unwrap();

        s.save_progress(chapters[0].id, 19, Some(20)).unwrap(); // дочитана
        s.save_progress(chapters[1].id, 5, Some(20)).unwrap(); // брошена на середине

        let (chapter, progress) = s.resume_target(manga_id).unwrap().unwrap();
        assert_eq!(chapter.id, chapters[1].id);
        assert_eq!(progress.unwrap().page, 5);
    }

    #[test]
    fn resume_target_falls_back_to_the_first_unread() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![
                chapter("/м/Т/1.cbz", 1.0),
                chapter("/м/Т/2.cbz", 2.0),
            ]))
            .unwrap();
        let chapters = s.chapters_of(manga_id).unwrap();
        s.save_progress(chapters[0].id, 19, Some(20)).unwrap();

        let (chapter, progress) = s.resume_target(manga_id).unwrap().unwrap();
        assert_eq!(chapter.id, chapters[1].id, "пора браться за следующий том");
        assert!(progress.is_none());
    }

    #[test]
    fn resume_target_is_none_when_everything_is_read() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/м/Т/1.cbz", 1.0)]))
            .unwrap();
        let id = s.chapters_of(manga_id).unwrap()[0].id;
        s.save_progress(id, 19, Some(20)).unwrap();
        assert!(s.resume_target(manga_id).unwrap().is_none());
    }

    #[test]
    fn volume_range_covers_all_chapters() {
        let mut s = Store::open_in_memory().unwrap();
        let mut a = chapter("/м/Т/33.cbz", 1.0);
        a.volume = Some(33);
        let mut b = chapter("/м/Т/35.cbz", 2.0);
        b.volume = Some(35);
        let (manga_id, _) = s.upsert_scanned(&scanned(vec![a, b])).unwrap();
        assert_eq!(s.volume_range(manga_id).unwrap(), Some((33, 35)));
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
    fn deleting_chapters_removes_them_and_their_progress() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![
                chapter("/манга/Тайтл/1.cbz", 1.0),
                chapter("/манга/Тайтл/2.cbz", 2.0),
            ]))
            .unwrap();
        let chapters = s.chapters_of(manga_id).unwrap();
        s.save_progress(chapters[0].id, 3, Some(20)).unwrap();

        assert_eq!(s.delete_chapters(&[chapters[0].id]).unwrap(), 1);
        assert_eq!(s.chapter_count().unwrap(), 1);
        assert!(s.progress_of(chapters[0].id).unwrap().is_none());
        // Второй главы это касаться не должно.
        assert!(s
            .chapters_of(manga_id)
            .unwrap()
            .iter()
            .any(|c| c.id == chapters[1].id));
    }

    #[test]
    fn deleting_nothing_is_allowed() {
        let mut s = Store::open_in_memory().unwrap();
        assert_eq!(s.delete_chapters(&[]).unwrap(), 0);
    }

    #[test]
    fn empty_manga_is_removed_only_after_all_chapters_are_gone() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![
                chapter("/манга/Тайтл/1.cbz", 1.0),
                chapter("/манга/Тайтл/2.cbz", 2.0),
            ]))
            .unwrap();
        let chapters = s.chapters_of(manga_id).unwrap();

        s.delete_chapters(&[chapters[0].id]).unwrap();
        assert_eq!(s.delete_empty_manga().unwrap(), 0, "одна глава осталась");
        assert_eq!(s.manga_count().unwrap(), 1);

        s.delete_chapters(&[chapters[1].id]).unwrap();
        assert_eq!(s.delete_empty_manga().unwrap(), 1);
        assert_eq!(s.manga_count().unwrap(), 0);
    }

    #[test]
    fn all_chapters_returns_everything_across_titles() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/1.cbz", 1.0)]))
            .unwrap();
        let mut other = scanned(vec![chapter("/манга/Другой/1.cbz", 1.0)]);
        other.external_id = "/манга/Другой".into();
        other.title = "Другой".into();
        s.upsert_scanned(&other).unwrap();

        assert_eq!(s.all_chapters().unwrap().len(), 2);
    }

    #[test]
    fn marks_are_stored_and_returned_in_page_order() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/том1.cbz", 1.0)]))
            .unwrap();
        let file = s.chapters_of(manga_id).unwrap()[0].id;

        s.add_mark(file, 40, Some("Глава 3")).unwrap();
        s.add_mark(file, 0, Some("Глава 1")).unwrap();
        s.add_mark(file, 20, None).unwrap();

        let marks = s.marks_of(file).unwrap();
        assert_eq!(
            marks.iter().map(|m| m.page).collect::<Vec<_>>(),
            vec![0, 20, 40]
        );
        assert_eq!(marks[0].title.as_deref(), Some("Глава 1"));
    }

    #[test]
    fn marking_the_same_page_twice_updates_the_title() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/том1.cbz", 1.0)]))
            .unwrap();
        let file = s.chapters_of(manga_id).unwrap()[0].id;

        s.add_mark(file, 10, Some("Старое")).unwrap();
        s.add_mark(file, 10, Some("Новое")).unwrap();

        let marks = s.marks_of(file).unwrap();
        assert_eq!(marks.len(), 1, "вторая отметка на той же странице не нужна");
        assert_eq!(marks[0].title.as_deref(), Some("Новое"));
    }

    #[test]
    fn toggle_adds_then_removes() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/том1.cbz", 1.0)]))
            .unwrap();
        let file = s.chapters_of(manga_id).unwrap()[0].id;

        assert!(
            s.toggle_mark(file, 5, None).unwrap(),
            "первое нажатие ставит"
        );
        assert_eq!(s.marks_of(file).unwrap().len(), 1);

        assert!(!s.toggle_mark(file, 5, None).unwrap(), "второе снимает");
        assert!(s.marks_of(file).unwrap().is_empty());
    }

    #[test]
    fn rescanning_preserves_manual_marks() {
        // Главное свойство: ручной труд не должен пропадать при
        // обновлении библиотеки. То же правило, что и для прогресса.
        let mut s = Store::open_in_memory().unwrap();
        let data = scanned(vec![chapter("/манга/Тайтл/том1.cbz", 1.0)]);
        let (manga_id, _) = s.upsert_scanned(&data).unwrap();
        let file = s.chapters_of(manga_id).unwrap()[0].id;

        s.add_mark(file, 24, Some("Глава 2")).unwrap();
        s.upsert_scanned(&data).unwrap();

        let marks = s.marks_of(file).unwrap();
        assert_eq!(marks.len(), 1, "отметка должна уцелеть");
        assert_eq!(marks[0].page, 24);
    }

    #[test]
    fn marks_disappear_with_the_file_they_belong_to() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/манга/Тайтл/том1.cbz", 1.0)]))
            .unwrap();
        let file = s.chapters_of(manga_id).unwrap()[0].id;
        s.add_mark(file, 3, None).unwrap();

        s.delete_chapters(&[file]).unwrap();
        assert!(s.marks_of(file).unwrap().is_empty());
    }

    #[test]
    fn unnamed_marks_get_numbered_labels() {
        let mark = crate::model::ChapterMark {
            id: 1,
            chapter_id: 1,
            page: 0,
            title: None,
        };
        assert_eq!(mark.label(0), "Глава 1");
        assert_eq!(mark.label(4), "Глава 5");

        let named = crate::model::ChapterMark {
            id: 2,
            chapter_id: 1,
            page: 10,
            title: Some("Особая".into()),
        };
        assert_eq!(named.label(0), "Особая");
    }

    #[test]
    fn deleting_a_manga_takes_its_chapters_and_progress() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/м/Т/1.cbz", 1.0)]))
            .unwrap();
        let file = s.chapters_of(manga_id).unwrap()[0].id;
        s.save_progress(file, 3, Some(20)).unwrap();
        s.add_mark(file, 5, None).unwrap();

        assert_eq!(s.delete_manga(manga_id).unwrap(), 1);
        assert_eq!(s.manga_count().unwrap(), 0);
        assert_eq!(s.chapter_count().unwrap(), 0);
        assert!(s.progress_of(file).unwrap().is_none());
        assert!(s.marks_of(file).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_single_file_leaves_the_rest() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_scanned(&scanned(vec![
            chapter("/м/Т/1.cbz", 1.0),
            chapter("/м/Т/2.cbz", 2.0),
        ]))
        .unwrap();

        assert!(s.delete_chapter_by_path("/м/Т/1.cbz").unwrap());
        assert_eq!(s.chapter_count().unwrap(), 1);
        assert!(!s.delete_chapter_by_path("/м/Т/нет.cbz").unwrap());
    }

    #[test]
    fn clearing_removes_everything() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_scanned(&scanned(vec![chapter("/м/Т/1.cbz", 1.0)]))
            .unwrap();
        let (manga, chapters) = s.clear_all().unwrap();
        assert_eq!((manga, chapters), (1, 1));
        assert_eq!(s.manga_count().unwrap(), 0);
    }

    #[test]
    fn chapters_under_filters_by_directory() {
        let mut s = Store::open_in_memory().unwrap();
        s.upsert_scanned(&scanned(vec![chapter("/м/Т/1.cbz", 1.0)]))
            .unwrap();
        let mut other = scanned(vec![chapter("/другое/1.cbz", 1.0)]);
        other.external_id = "/другое".into();
        other.title = "Другое".into();
        s.upsert_scanned(&other).unwrap();

        assert_eq!(s.chapters_under("/м/").unwrap().len(), 1);
        assert_eq!(s.chapters_under("/").unwrap().len(), 2);
    }

    #[test]
    fn locates_a_chapter_by_its_own_number() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![
                chapter("/м/Т/1.cbz", 359.0),
                chapter("/м/Т/2.cbz", 360.0),
            ]))
            .unwrap();

        let (found, page) = s.locate_chapter(manga_id, 360.0).unwrap().unwrap();
        assert!(found.external_id.ends_with("2.cbz"));
        assert_eq!(page, None, "отдельный файл открывается с начала");
    }

    #[test]
    fn locates_a_chapter_inside_an_assembled_volume() {
        // Собранный том — одна запись, главы внутри размечены отметками.
        let mut s = Store::open_in_memory().unwrap();
        let mut volume = chapter("/м/Т/том33.cbz", 1.0);
        volume.number = None;
        volume.volume = Some(33);
        let (manga_id, _) = s.upsert_scanned(&scanned(vec![volume])).unwrap();
        let file = s.chapters_of(manga_id).unwrap()[0].id;

        s.add_mark(file, 0, Some("Глава 359 — Первая")).unwrap();
        s.add_mark(file, 22, Some("Глава 360 — Вторая")).unwrap();

        let (found, page) = s.locate_chapter(manga_id, 360.0).unwrap().unwrap();
        assert_eq!(found.id, file);
        assert_eq!(page, Some(22), "открыть надо сразу нужную страницу");
    }

    #[test]
    fn missing_chapter_is_not_found() {
        let mut s = Store::open_in_memory().unwrap();
        let (manga_id, _) = s
            .upsert_scanned(&scanned(vec![chapter("/м/Т/1.cbz", 1.0)]))
            .unwrap();
        assert!(s.locate_chapter(manga_id, 999.0).unwrap().is_none());
    }

    #[test]
    fn finds_a_volume_by_number() {
        let mut s = Store::open_in_memory().unwrap();
        let mut a = chapter("/м/Т/33.cbz", 1.0);
        a.volume = Some(33);
        let mut b = chapter("/м/Т/34.cbz", 2.0);
        b.volume = Some(34);
        let (manga_id, _) = s.upsert_scanned(&scanned(vec![a, b])).unwrap();

        let found = s.chapter_by_volume(manga_id, 34).unwrap().unwrap();
        assert!(found.external_id.ends_with("34.cbz"));
        assert!(s.chapter_by_volume(manga_id, 99).unwrap().is_none());
    }

    #[test]
    fn label_number_reads_the_chapter_number() {
        assert_eq!(label_number("Глава 234 — Название"), Some(234.0));
        assert_eq!(label_number("Глава 10.5"), Some(10.5));
        assert_eq!(label_number("Послесловие"), None);
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
