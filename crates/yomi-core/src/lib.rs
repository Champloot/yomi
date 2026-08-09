//! # yomi-core
//!
//! Ядро приложения. Здесь нет ни одного `println!` и ни одной строчки,
//! завязанной на терминал: этот крейт обязан оставаться пригодным для
//! использования из TUI, из тестов и из будущего HTTP-демона.
//!
//! Правило зависимостей: `yomi` знает про `yomi-core`, но никогда наоборот.

pub mod config;
pub mod error;
pub mod model;
pub mod paths;
pub mod source;
pub mod sources;

pub use error::{Error, Result};

/// Версия крейта, подставляется Cargo на этапе компиляции.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
