//! Единый тип ошибки для всего ядра.
//!
//! Библиотечные крейты в Rust принято делать со «своим» перечислением ошибок
//! (через `thiserror`), а `anyhow` оставлять приложению — ему удобно
//! оборачивать что угодно в `main`. Мы придерживаемся этого разделения.

use std::path::PathBuf;

/// Псевдоним, чтобы не писать `std::result::Result<T, yomi_core::Error>`.
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    #[error("не удалось разобрать конфигурацию {path}: {message}")]
    ConfigParse { path: PathBuf, message: String },

    #[error("не удалось определить домашний каталог пользователя")]
    NoHomeDir,

    #[error("источник «{0}» не найден")]
    SourceNotFound(String),

    // Поле нельзя назвать `source`: thiserror считает такое поле
    // источником ошибки и требует от него реализации std::error::Error.
    #[error("источник «{source_id}» не умеет: {feature}")]
    Unsupported { source_id: String, feature: String },

    #[error("не найдено: {0}")]
    NotFound(String),

    #[error("сеть недоступна: {0}")]
    Network(String),

    #[error("источник ответил неожиданно: {0}")]
    BadResponse(String),

    #[error("возможность ещё не реализована: {0}")]
    NotImplemented(&'static str),
}
