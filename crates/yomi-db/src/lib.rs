//! # yomi-db
//!
//! Хранилище: локальная библиотека, метаданные и прогресс чтения.
//!
//! Правило то же, что у остальных крейтов: здесь нет ни разбора аргументов,
//! ни вывода в терминал. Только SQLite и типы из `yomi-core`.

pub mod error;
pub mod migrations;
pub mod model;
pub mod store;

pub use error::{Error, Result};
pub use store::Store;
