//! Пути по стандарту XDG Base Directory.
//!
//! Никогда не пишем в `~/.yomi`. Пользователь Linux ожидает:
//!   конфиг  → `~/.config/yomi/config.toml`
//!   данные  → `~/.local/share/yomi/` (база SQLite, скачанное)
//!   кэш     → `~/.cache/yomi/` (обложки, миниатюры — можно смело удалять)
//!
//! Любой из путей можно переопределить переменной окружения — это критично
//! для тестов, которые не должны трогать реальный домашний каталог.

use crate::{Error, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

pub const APP_NAME: &str = "yomi";

pub const ENV_CONFIG_DIR: &str = "YOMI_CONFIG_DIR";
pub const ENV_DATA_DIR: &str = "YOMI_DATA_DIR";
pub const ENV_CACHE_DIR: &str = "YOMI_CACHE_DIR";

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", APP_NAME).ok_or(Error::NoHomeDir)
}

fn from_env(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Каталог конфигурации.
pub fn config_dir() -> Result<PathBuf> {
    if let Some(p) = from_env(ENV_CONFIG_DIR) {
        return Ok(p);
    }
    Ok(project_dirs()?.config_dir().to_path_buf())
}

/// Полный путь к файлу конфигурации.
pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// Каталог данных: база, скачанные главы.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(p) = from_env(ENV_DATA_DIR) {
        return Ok(p);
    }
    Ok(project_dirs()?.data_dir().to_path_buf())
}

/// Каталог кэша: всё, что не жалко потерять.
pub fn cache_dir() -> Result<PathBuf> {
    if let Some(p) = from_env(ENV_CACHE_DIR) {
        return Ok(p);
    }
    Ok(project_dirs()?.cache_dir().to_path_buf())
}

/// Путь к базе SQLite (появится на этапе M2).
pub fn database_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("library.db"))
}

/// Создаёт каталог вместе с родителями, если его ещё нет.
pub fn ensure_dir(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        tracing::debug!(path = %path.display(), "создаю каталог");
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins() {
        std::env::set_var(ENV_CONFIG_DIR, "/tmp/yomi-test-config");
        assert_eq!(
            config_dir().unwrap(),
            PathBuf::from("/tmp/yomi-test-config")
        );
        assert_eq!(
            config_file().unwrap(),
            PathBuf::from("/tmp/yomi-test-config/config.toml")
        );
        std::env::remove_var(ENV_CONFIG_DIR);
    }

    #[test]
    fn empty_env_is_ignored() {
        std::env::set_var(ENV_CACHE_DIR, "");
        // Пустая переменная не должна давать пустой путь.
        assert!(cache_dir()
            .map(|p| !p.as_os_str().is_empty())
            .unwrap_or(true));
        std::env::remove_var(ENV_CACHE_DIR);
    }
}
