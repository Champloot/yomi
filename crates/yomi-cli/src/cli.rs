//! Описание интерфейса командной строки.
//!
//! Структура команд задана декларативно через `clap`. Держим её отдельно
//! от исполнения: так справку можно проверять тестами, а команды —
//! добавлять, не трогая `main`.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "yomi",
    version,
    about = "Терминальная читалка манги",
    long_about = "yomi — чтение, поиск и загрузка манги прямо из терминала.\n\
                  Документация: docs/ в репозитории проекта.",
    propagate_version = true
)]
pub struct Cli {
    /// Путь к файлу конфигурации (по умолчанию ~/.config/yomi/config.toml)
    #[arg(
        long,
        short = 'c',
        value_name = "ФАЙЛ",
        global = true,
        env = "YOMI_CONFIG"
    )]
    pub config: Option<PathBuf>,

    /// Подробный вывод: -v информация, -vv отладка, -vvv трассировка
    #[arg(long, short = 'v', action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Только ошибки
    #[arg(long, short = 'q', global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Машиночитаемый вывод JSON вместо таблиц
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Читать мангу: файл, каталог или тайтл из библиотеки
    Read(ReadArgs),

    /// Локальная библиотека
    #[command(subcommand)]
    Library(LibraryCommand),

    /// Искать в источниках
    Search(SearchArgs),

    /// Скачать главы
    Download(DownloadArgs),

    /// Источники манги
    #[command(subcommand)]
    Sources(SourcesCommand),

    /// Конфигурация
    #[command(subcommand)]
    Config(ConfigCommand),
}

#[derive(Debug, Args)]
pub struct ReadArgs {
    /// Путь к CBZ-архиву или каталогу с изображениями
    #[arg(value_name = "ПУТЬ")]
    pub path: PathBuf,

    /// Начать с указанной страницы (нумерация с единицы)
    #[arg(long, short = 'p', default_value_t = 1)]
    pub page: u32,

    /// Переопределить способ вывода изображений
    #[arg(long, value_enum)]
    pub renderer: Option<RendererArg>,
}

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    /// Просканировать каталоги и обновить базу
    Scan {
        /// Каталог для сканирования; можно повторять. По умолчанию — из конфига
        #[arg(value_name = "КАТАЛОГ")]
        paths: Vec<PathBuf>,
    },
    /// Показать содержимое библиотеки
    List {
        /// Фильтр по названию
        #[arg(long, short = 'f')]
        filter: Option<String>,
    },
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Поисковый запрос
    #[arg(value_name = "ЗАПРОС")]
    pub query: Option<String>,

    /// Идентификатор источника
    #[arg(long, short = 's', default_value = "demo")]
    pub source: String,

    /// Жанр; можно повторять
    #[arg(long, short = 'g', value_name = "ЖАНР")]
    pub genre: Vec<String>,

    /// Исключить жанр; можно повторять
    #[arg(long, value_name = "ЖАНР")]
    pub exclude_genre: Vec<String>,

    /// Автор
    #[arg(long, short = 'a')]
    pub author: Vec<String>,

    /// Способ сортировки
    #[arg(long, value_enum, default_value_t = SortArg::Relevance)]
    pub sort: SortArg,

    /// Номер страницы результатов
    #[arg(long, default_value_t = 1)]
    pub page: u32,

    /// Результатов на страницу
    #[arg(long, default_value_t = 20)]
    pub limit: u32,
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Идентификатор тайтла в источнике
    #[arg(value_name = "ID")]
    pub manga_id: String,

    #[arg(long, short = 's', default_value = "demo")]
    pub source: String,

    /// Главы: "5", "1-10", "all"
    ///
    /// Короткого -c здесь нет: он занят глобальным --config.
    #[arg(long, default_value = "all")]
    pub chapters: String,

    /// Каталог назначения
    #[arg(long, short = 'o', value_name = "КАТАЛОГ")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum SourcesCommand {
    /// Список доступных источников и их возможностей
    List,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Создать файл конфигурации со значениями по умолчанию
    Init {
        /// Перезаписать существующий файл
        #[arg(long)]
        force: bool,
    },
    /// Показать текущую конфигурацию
    Show,
    /// Показать пути, которыми пользуется программа
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RendererArg {
    Auto,
    Kitty,
    Iterm2,
    Sixel,
    Blocks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortArg {
    Relevance,
    Popularity,
    Rating,
    Updated,
    Created,
    Title,
}

impl From<SortArg> for yomi_core::model::SortBy {
    fn from(s: SortArg) -> Self {
        use yomi_core::model::SortBy;
        match s {
            SortArg::Relevance => SortBy::Relevance,
            SortArg::Popularity => SortBy::Popularity,
            SortArg::Rating => SortBy::Rating,
            SortArg::Updated => SortBy::Updated,
            SortArg::Created => SortBy::Created,
            SortArg::Title => SortBy::Title,
        }
    }
}

impl From<RendererArg> for yomi_core::config::Renderer {
    fn from(r: RendererArg) -> Self {
        use yomi_core::config::Renderer;
        match r {
            RendererArg::Auto => Renderer::Auto,
            RendererArg::Kitty => Renderer::Kitty,
            RendererArg::Iterm2 => Renderer::Iterm2,
            RendererArg::Sixel => Renderer::Sixel,
            RendererArg::Blocks => Renderer::Blocks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Ловит ошибки в объявлении аргументов (конфликты имён, кривые значения
    /// по умолчанию) на этапе тестов, а не при запуске у пользователя.
    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_read_with_page() {
        let cli = Cli::try_parse_from(["yomi", "read", "/tmp/x.cbz", "-p", "7"]).unwrap();
        match cli.command {
            Command::Read(a) => {
                assert_eq!(a.page, 7);
                assert_eq!(a.path, PathBuf::from("/tmp/x.cbz"));
            }
            _ => panic!("ожидалась команда read"),
        }
    }

    #[test]
    fn repeated_genre_flags_accumulate() {
        let cli =
            Cli::try_parse_from(["yomi", "search", "тест", "-g", "драма", "-g", "школа"]).unwrap();
        match cli.command {
            Command::Search(a) => assert_eq!(a.genre, vec!["драма", "школа"]),
            _ => panic!("ожидалась команда search"),
        }
    }

    #[test]
    fn verbose_flag_counts() {
        let cli = Cli::try_parse_from(["yomi", "-vv", "sources", "list"]).unwrap();
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn quiet_and_verbose_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["yomi", "-q", "-v", "sources", "list"]).is_err());
    }

    #[test]
    fn command_is_required() {
        assert!(Cli::try_parse_from(["yomi"]).is_err());
    }
}
