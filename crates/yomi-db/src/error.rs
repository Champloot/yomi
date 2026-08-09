//! Ошибки хранилища.

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ошибка базы данных: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("не удалось разобрать сохранённое поле {field}: {message}")]
    Decode {
        field: &'static str,
        message: String,
    },

    #[error("схема базы новее, чем понимает эта версия yomi: {found} > {supported}")]
    SchemaTooNew { found: u32, supported: u32 },

    #[error("не найдено: {0}")]
    NotFound(String),
}
