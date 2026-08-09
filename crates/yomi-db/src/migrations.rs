//! Миграции схемы.
//!
//! Версия схемы хранится в `PRAGMA user_version` — встроенном в SQLite
//! счётчике, для которого не нужна отдельная таблица. Каждая миграция
//! применяется в транзакции: прерванное на середине обновление не оставит
//! базу в промежуточном состоянии.
//!
//! Миграции с первого дня — не перестраховка. Без них уже на третьем
//! изменении схемы пришлось бы просить пользователей удалять библиотеку
//! вместе с прогрессом чтения.

use crate::{Error, Result};
use rusqlite::Connection;

/// Версия схемы, которую понимает эта сборка.
pub const SCHEMA_VERSION: u32 = 2;

/// Одна миграция: SQL, поднимающий схему с `version - 1` до `version`.
struct Migration {
    version: u32,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: r#"
-- Тайтл. Для локальных файлов source = 'local', а external_id — путь
-- к каталогу тайтла: он и служит естественным ключом при пересканировании.
CREATE TABLE manga (
    id          INTEGER PRIMARY KEY,
    source      TEXT NOT NULL,
    external_id TEXT NOT NULL,
    title       TEXT NOT NULL,
    authors     TEXT NOT NULL DEFAULT '[]',
    genres      TEXT NOT NULL DEFAULT '[]',
    status      TEXT NOT NULL DEFAULT 'unknown',
    year        INTEGER,
    description TEXT,
    added_at    TEXT NOT NULL,
    UNIQUE (source, external_id)
);

-- Глава. Для локальных файлов external_id — путь к CBZ или каталогу.
CREATE TABLE chapters (
    id          INTEGER PRIMARY KEY,
    manga_id    INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    number      REAL,
    volume      INTEGER,
    title       TEXT,
    language    TEXT NOT NULL DEFAULT 'ru',
    scanlator   TEXT,
    page_count  INTEGER,
    UNIQUE (manga_id, external_id)
);

CREATE INDEX idx_chapters_manga ON chapters(manga_id);

-- Прогресс чтения. Отдельная таблица, а не колонка в chapters:
-- пересканирование библиотеки не должно задевать прогресс.
CREATE TABLE progress (
    chapter_id  INTEGER PRIMARY KEY REFERENCES chapters(id) ON DELETE CASCADE,
    page        INTEGER NOT NULL,
    total_pages INTEGER,
    completed   INTEGER NOT NULL DEFAULT 0,
    updated_at  TEXT NOT NULL
);

CREATE INDEX idx_progress_updated ON progress(updated_at DESC);
"#,
    },
    Migration {
        version: 2,
        // Тип файла: глава, том или одиночное изображение. Существующие
        // записи получают 'unknown' — пересканирование их обновит.
        sql: r#"
ALTER TABLE chapters ADD COLUMN kind TEXT NOT NULL DEFAULT 'unknown';
"#,
    },
];

/// Приводит базу к актуальной версии схемы.
pub fn migrate(conn: &mut Connection) -> Result<u32> {
    // Внешние ключи в SQLite выключены по умолчанию — без этого
    // ON DELETE CASCADE молча не сработает.
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    let current: u32 =
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? as u32;

    if current > SCHEMA_VERSION {
        return Err(Error::SchemaTooNew {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    if current == SCHEMA_VERSION {
        return Ok(current);
    }

    for m in MIGRATIONS.iter().filter(|m| m.version > current) {
        tracing::info!(version = m.version, "применяю миграцию");
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        // user_version не принимает параметры привязки, только литерал.
        tx.execute_batch(&format!("PRAGMA user_version = {};", m.version))?;
        tx.commit()?;
    }

    Ok(SCHEMA_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn migrates_empty_database_to_current_version() {
        let mut conn = memory();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v as u32, SCHEMA_VERSION);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let mut conn = memory();
        migrate(&mut conn).unwrap();
        // Повторный запуск не должен падать на «таблица уже существует».
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn refuses_to_touch_a_newer_schema() {
        let mut conn = memory();
        conn.execute_batch("PRAGMA user_version = 999;").unwrap();
        assert!(matches!(
            migrate(&mut conn),
            Err(Error::SchemaTooNew { found: 999, .. })
        ));
    }

    #[test]
    fn migration_two_adds_kind_column_to_existing_database() {
        // Поднимаем базу до первой версии, как будто она осталась от
        // прошлой сборки, и проверяем, что обновление её не сломает.
        let mut conn = memory();
        conn.execute_batch(MIGRATIONS[0].sql).unwrap();
        conn.execute_batch("PRAGMA user_version = 1;").unwrap();
        conn.execute_batch(
            "INSERT INTO manga (id, source, external_id, title, added_at)
                 VALUES (1, 'local', '/x', 'Тайтл', '2026-01-01T00:00:00Z');
             INSERT INTO chapters (id, manga_id, external_id) VALUES (1, 1, '/x/1.cbz');",
        )
        .unwrap();

        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);

        // Данные на месте, а у старой записи появилось значение по умолчанию.
        let kind: String = conn
            .query_row("SELECT kind FROM chapters WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kind, "unknown");
    }

    #[test]
    fn expected_tables_exist_after_migration() {
        let mut conn = memory();
        migrate(&mut conn).unwrap();
        for table in ["manga", "chapters", "progress"] {
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "таблица {table} должна существовать");
        }
    }

    #[test]
    fn deleting_manga_cascades_to_chapters_and_progress() {
        let mut conn = memory();
        migrate(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO manga (id, source, external_id, title, added_at)
                 VALUES (1, 'local', '/x', 'Тайтл', '2026-01-01T00:00:00Z');
             INSERT INTO chapters (id, manga_id, external_id) VALUES (1, 1, '/x/1.cbz');
             INSERT INTO progress (chapter_id, page, updated_at)
                 VALUES (1, 5, '2026-01-01T00:00:00Z');
             DELETE FROM manga WHERE id = 1;",
        )
        .unwrap();

        let chapters: i64 = conn
            .query_row("SELECT count(*) FROM chapters", [], |r| r.get(0))
            .unwrap();
        let progress: i64 = conn
            .query_row("SELECT count(*) FROM progress", [], |r| r.get(0))
            .unwrap();
        assert_eq!((chapters, progress), (0, 0), "каскад должен вычистить всё");
    }
}
