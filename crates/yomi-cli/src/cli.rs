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
    long_about = "yomi — чтение манги прямо из терминала.\n\
                  Локальные файлы, библиотека с прогрессом, сборка CBZ.\n\
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

    /// Что внутри файла: том или глава, сколько страниц, есть ли разбиение
    Info(InfoArgs),

    /// Собрать CBZ из каталога с картинками
    Pack(PackArgs),

    /// Отметки начала глав внутри файла
    #[command(subcommand)]
    Marks(MarksCommand),

    /// Локальная библиотека
    #[command(subcommand)]
    Library(LibraryCommand),

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

    /// Как вписывать страницу в окно
    #[arg(long, value_enum)]
    pub fit: Option<FitArg>,

    /// Разрешить увеличивать страницу сверх её разрешения
    #[arg(long)]
    pub upscale: bool,

    /// Направление чтения
    #[arg(long, value_enum)]
    pub direction: Option<DirectionArg>,
}

#[derive(Debug, Args)]
pub struct PackArgs {
    /// Каталог с изображениями (страницы в естественном порядке имён)
    #[arg(value_name = "КАТАЛОГ")]
    pub path: PathBuf,

    /// Куда сохранить архив. По умолчанию — рядом, по имени каталога
    #[arg(long, short = 'o', value_name = "ФАЙЛ")]
    pub output: Option<PathBuf>,

    /// Название тайтла для метаданных. По умолчанию — имя каталога
    #[arg(long)]
    pub series: Option<String>,

    /// Номер тома
    #[arg(long)]
    pub volume: Option<u16>,

    /// Номер главы
    #[arg(long)]
    pub chapter: Option<f32>,

    /// Название главы
    #[arg(long)]
    pub title: Option<String>,

    /// Перезаписать существующий архив
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct InfoArgs {
    /// Путь к CBZ-архиву, каталогу или изображению
    #[arg(value_name = "ПУТЬ")]
    pub path: PathBuf,

    /// Искать границы глав по пропорциям страниц, если разметки нет.
    /// Читает размеры всех страниц, поэтому заметно медленнее
    #[arg(long)]
    pub deep: bool,
}

#[derive(Debug, Subcommand)]
pub enum LibraryCommand {
    /// Просканировать каталоги и обновить базу
    Scan {
        /// Каталог для сканирования; можно повторять. По умолчанию — из конфига
        #[arg(value_name = "КАТАЛОГ")]
        paths: Vec<PathBuf>,
    },
    /// Продолжить чтение с того места, где остановились
    Resume,
    /// Показать главы тайтла (идентификатор берётся из `library list`)
    Chapters {
        /// Идентификатор тайтла
        #[arg(value_name = "ID")]
        manga_id: i64,
    },
    /// Убрать из библиотеки записи, файлов которых больше нет
    Clean {
        /// Действительно удалить. Без этого флага только показывает список
        #[arg(long)]
        yes: bool,
    },
    /// Показать содержимое библиотеки
    List {
        /// Фильтр по названию
        #[arg(long, short = 'f')]
        filter: Option<String>,
    },
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

#[derive(Debug, Subcommand)]
pub enum MarksCommand {
    /// Показать отметки файла
    List {
        /// Путь к файлу
        #[arg(value_name = "ПУТЬ")]
        path: PathBuf,
    },
    /// Поставить отметку на страницу (нумерация с единицы)
    Add {
        #[arg(value_name = "ПУТЬ")]
        path: PathBuf,
        #[arg(value_name = "СТРАНИЦА")]
        page: u32,
        /// Название главы
        #[arg(long)]
        title: Option<String>,
    },
    /// Снять отметку со страницы
    Remove {
        #[arg(value_name = "ПУТЬ")]
        path: PathBuf,
        #[arg(value_name = "СТРАНИЦА")]
        page: u32,
    },
    /// Заполнить отметки тем, что нашла автоматика (разметка или развороты)
    Detect {
        #[arg(value_name = "ПУТЬ")]
        path: PathBuf,
        /// Искать границы по пропорциям страниц, если разметки нет
        #[arg(long)]
        deep: bool,
        /// Заменить существующие отметки
        #[arg(long)]
        force: bool,
    },
    /// Снять все отметки файла
    Clear {
        #[arg(value_name = "ПУТЬ")]
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DirectionArg {
    /// Справа налево — традиционная манга
    Rtl,
    /// Слева направо — комиксы, переведённые издания
    Ltr,
    /// Вертикальная лента — манхва, маньхуа
    Webtoon,
}

impl From<DirectionArg> for yomi_core::config::ReadingDirection {
    fn from(d: DirectionArg) -> Self {
        use yomi_core::config::ReadingDirection;
        match d {
            DirectionArg::Rtl => ReadingDirection::RightToLeft,
            DirectionArg::Ltr => ReadingDirection::LeftToRight,
            DirectionArg::Webtoon => ReadingDirection::Webtoon,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FitArg {
    Contain,
    Width,
    Height,
    Original,
}

impl From<FitArg> for yomi_core::config::Fit {
    fn from(f: FitArg) -> Self {
        use yomi_core::config::Fit;
        match f {
            FitArg::Contain => Fit::Contain,
            FitArg::Width => Fit::Width,
            FitArg::Height => Fit::Height,
            FitArg::Original => Fit::Original,
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
    fn read_accepts_fit_and_upscale() {
        let cli =
            Cli::try_parse_from(["yomi", "read", "a.cbz", "--fit", "width", "--upscale"]).unwrap();
        match cli.command {
            Command::Read(a) => {
                assert_eq!(a.fit, Some(FitArg::Width));
                assert!(a.upscale);
            }
            _ => panic!("ожидалась команда read"),
        }
    }

    #[test]
    fn read_accepts_direction() {
        let cli = Cli::try_parse_from(["yomi", "read", "a.cbz", "--direction", "ltr"]).unwrap();
        match cli.command {
            Command::Read(a) => assert_eq!(a.direction, Some(DirectionArg::Ltr)),
            _ => panic!("ожидалась команда read"),
        }
    }

    #[test]
    fn verbose_flag_counts() {
        let cli = Cli::try_parse_from(["yomi", "-vv", "info", "a.cbz"]).unwrap();
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn quiet_and_verbose_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["yomi", "-q", "-v", "info", "a.cbz"]).is_err());
    }

    #[test]
    fn command_is_required() {
        assert!(Cli::try_parse_from(["yomi"]).is_err());
    }
}
