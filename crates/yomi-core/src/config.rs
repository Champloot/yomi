//! Конфигурация приложения.
//!
//! Формат — TOML. Приоритет источников значений (позднее переопределяет раннее):
//!   1. значения по умолчанию, зашитые в коде;
//!   2. файл `~/.config/yomi/config.toml`;
//!   3. флаги командной строки.
//!
//! Отсутствующий файл — не ошибка: приложение обязано работать «из коробки».

use crate::{paths, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub general: General,
    pub library: Library,
    pub reader: Reader,
    pub download: Download,
    pub network: Network,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    /// Язык интерфейса.
    pub language: String,
    /// Предпочитаемые языки контента, в порядке убывания приоритета.
    pub content_languages: Vec<String>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            language: "ru".into(),
            content_languages: vec!["ru".into(), "en".into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Library {
    /// Каталоги, которые сканируются командой `yomi library scan`.
    pub paths: Vec<PathBuf>,
}

/// Способ вывода картинки в терминал.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Renderer {
    /// Определить возможности терминала автоматически (рекомендуется).
    #[default]
    Auto,
    /// Графический протокол kitty. Лучшее качество.
    Kitty,
    /// Протокол iTerm2 (поддерживают WezTerm, Konsole).
    Iterm2,
    /// Sixel — старый, но живой стандарт (foot, xterm с ключом).
    Sixel,
    /// Юникод-блоки: работает везде, качество низкое.
    Blocks,
}

/// Направление чтения.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ReadingDirection {
    /// Справа налево — как в японской манге.
    #[default]
    RightToLeft,
    LeftToRight,
    /// Вертикальная лента — манхва, маньхуа.
    Webtoon,
}

/// Как вписывать страницу в окно терминала.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Целиком в окно с сохранением пропорций.
    #[default]
    Contain,
    /// По ширине окна — обычный режим для вебтунов.
    Width,
    /// По высоте окна.
    Height,
    /// Один пиксель картинки — один пиксель экрана.
    Original,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Reader {
    pub renderer: Renderer,
    pub direction: ReadingDirection,
    /// Как вписывать страницу в окно.
    pub fit: Fit,
    /// Разрешить растягивать страницу сверх её собственного разрешения.
    /// По умолчанию выключено: увеличенная страница выглядит мыльной.
    pub upscale: bool,
    /// Разворот из двух страниц, если ширина терминала позволяет.
    pub double_page: bool,
    /// Сколько страниц подгружать вперёд.
    pub preload_pages: u8,
}

impl Default for Reader {
    fn default() -> Self {
        Self {
            renderer: Renderer::Auto,
            direction: ReadingDirection::RightToLeft,
            fit: Fit::Contain,
            upscale: false,
            double_page: false,
            preload_pages: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Download {
    /// Куда складывать скачанное. Пусто — каталог данных XDG.
    pub directory: Option<PathBuf>,
    /// Шаблон имени файла главы.
    pub filename_template: String,
    /// Одновременных загрузок страниц.
    pub concurrency: u8,
}

impl Default for Download {
    fn default() -> Self {
        Self {
            directory: None,
            filename_template: "{manga}/{volume}-{chapter} {title}.cbz".into(),
            concurrency: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Network {
    pub timeout_secs: u16,
    pub retries: u8,
    pub user_agent: String,
    /// Прокси вида `socks5://127.0.0.1:9050`. Актуально для части источников.
    pub proxy: Option<String>,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            retries: 3,
            user_agent: format!("yomi/{}", crate::VERSION),
            proxy: None,
        }
    }
}

impl Config {
    /// Читает конфигурацию. Если файла нет — возвращает значения по умолчанию.
    pub fn load(explicit_path: Option<&Path>) -> Result<Self> {
        let path = match explicit_path {
            Some(p) => p.to_path_buf(),
            None => paths::config_file()?,
        };

        if !path.exists() {
            tracing::debug!(path = %path.display(), "конфиг не найден, беру значения по умолчанию");
            return Ok(Self::default());
        }

        let raw = std::fs::read_to_string(&path)?;
        Self::from_toml(&raw).map_err(|e| Error::ConfigParse {
            path: path.clone(),
            message: e,
        })
    }

    /// Разбирает TOML. Ошибку возвращает строкой, чтобы не тащить
    /// тип ошибки `toml` в публичный интерфейс.
    pub fn from_toml(raw: &str) -> std::result::Result<Self, String> {
        toml::from_str(raw).map_err(|e| e.to_string())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("конфиг всегда сериализуем")
    }

    /// Записывает конфигурацию, создавая родительские каталоги.
    /// Существующий файл не трогает, если `overwrite == false`.
    pub fn save(&self, path: &Path, overwrite: bool) -> Result<bool> {
        if path.exists() && !overwrite {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            paths::ensure_dir(parent)?;
        }
        std::fs::write(path, self.to_toml())?;
        Ok(true)
    }

    /// Каталог загрузок с учётом значения по умолчанию.
    pub fn download_dir(&self) -> Result<PathBuf> {
        match &self.download.directory {
            Some(p) => Ok(p.clone()),
            None => Ok(paths::data_dir()?.join("downloads")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_does_not_upscale_by_default() {
        // Умолчание выбрано по жалобе на мыльную картинку в полноэкранном
        // терминале: лучше поля вокруг страницы, чем растянутые пиксели.
        let c = Config::default();
        assert!(!c.reader.upscale);
        assert_eq!(c.reader.fit, Fit::Contain);
    }

    #[test]
    fn fit_parses_from_kebab_case() {
        let c = Config::from_toml("[reader]\nfit = \"width\"\n").unwrap();
        assert_eq!(c.reader.fit, Fit::Width);
    }

    #[test]
    fn defaults_are_russian_first() {
        let c = Config::default();
        assert_eq!(c.general.language, "ru");
        assert_eq!(c.general.content_languages.first().unwrap(), "ru");
        assert_eq!(c.reader.direction, ReadingDirection::RightToLeft);
    }

    #[test]
    fn roundtrip_through_toml_is_lossless() {
        let original = Config::default();
        let parsed = Config::from_toml(&original.to_toml()).expect("должен разобраться");
        assert_eq!(original, parsed);
    }

    #[test]
    fn partial_config_merges_with_defaults() {
        let raw = r#"
            [reader]
            renderer = "kitty"
        "#;
        let c = Config::from_toml(raw).expect("частичный конфиг допустим");
        assert_eq!(c.reader.renderer, Renderer::Kitty);
        // Не указанное берётся из умолчаний:
        assert_eq!(c.reader.preload_pages, 2);
        assert_eq!(c.general.language, "ru");
    }

    #[test]
    fn unknown_key_is_an_error() {
        // Опечатка в конфиге должна быть замечена, а не проглочена.
        let raw = "[reader]\nrendrer = \"kitty\"\n";
        assert!(Config::from_toml(raw).is_err());
    }

    #[test]
    fn missing_file_yields_defaults() {
        let c = Config::load(Some(Path::new("/nonexistent/config.toml"))).unwrap();
        assert_eq!(c, Config::default());
    }
}
