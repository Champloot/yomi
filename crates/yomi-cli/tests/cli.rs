//! Интеграционные тесты: запускают настоящий бинарник и проверяют
//! его вывод и код возврата.
//!
//! Внешние крейты (`assert_cmd`) сознательно не используются: путь к
//! собранному бинарнику Cargo сам передаёт через переменную времени
//! компиляции `CARGO_BIN_EXE_<имя>`, а этого достаточно.
//!
//! Все тесты переопределяют XDG-каталоги на временные: тест не имеет
//! права трогать реальный `~/.config` того, кто его запускает.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Путь к бинарнику `yomi`, собранному этим же прогоном Cargo.
const BIN: &str = env!("CARGO_BIN_EXE_yomi");

/// Создаёт уникальный временный каталог для одного теста.
fn temp_dir(tag: &str) -> PathBuf {
    let unique = format!(
        "yomi-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).expect("создание временного каталога");
    dir
}

/// Запускает `yomi` с изолированным окружением.
fn run(sandbox: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env("YOMI_CONFIG_DIR", sandbox.join("config"))
        .env("YOMI_DATA_DIR", sandbox.join("data"))
        .env("YOMI_CACHE_DIR", sandbox.join("cache"))
        // Чтобы логи не зависели от окружения разработчика.
        .env_remove("YOMI_LOG")
        .output()
        .expect("запуск бинарника yomi")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("процесс завершился сигналом")
}

#[test]
fn version_flag_prints_version() {
    let dir = temp_dir("version");
    let out = run(&dir, &["--version"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_is_available() {
    let dir = temp_dir("help");
    let out = run(&dir, &["--help"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    for cmd in ["read", "search", "download", "library", "sources", "config"] {
        assert!(text.contains(cmd), "в справке нет команды {cmd}");
    }
}

#[test]
fn without_command_exits_with_usage_error() {
    let dir = temp_dir("nocmd");
    let out = run(&dir, &[]);
    assert_eq!(
        code(&out),
        2,
        "clap возвращает 2 при неверном использовании"
    );
}

#[test]
fn sources_list_shows_demo_source() {
    let dir = temp_dir("sources");
    let out = run(&dir, &["sources", "list"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("demo"));
}

#[test]
fn search_finds_demo_entry() {
    let dir = temp_dir("search");
    let out = run(&dir, &["search", "Пример"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(text.contains("Пример первый"));
    assert!(text.contains("Найдено: 2"));
}

#[test]
fn search_in_unknown_source_exits_not_found() {
    let dir = temp_dir("badsource");
    let out = run(&dir, &["search", "x", "--source", "не-существует"]);
    assert_eq!(code(&out), 4);
}

#[test]
fn config_init_creates_file_and_is_idempotent() {
    let dir = temp_dir("config");

    let first = run(&dir, &["config", "init"]);
    assert_eq!(code(&first), 0);
    let config_file = dir.join("config").join("config.toml");
    assert!(config_file.exists(), "файл конфигурации не создан");

    // Повторный запуск не должен молча затирать пользовательский файл.
    std::fs::write(&config_file, "[general]\nlanguage = \"en\"\n").unwrap();
    let second = run(&dir, &["config", "init"]);
    assert_eq!(code(&second), 0);
    let content = std::fs::read_to_string(&config_file).unwrap();
    assert!(
        content.contains("en"),
        "существующий конфиг был перезаписан"
    );
}

#[test]
fn config_show_reflects_user_file() {
    let dir = temp_dir("configshow");
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(
        dir.join("config").join("config.toml"),
        "[reader]\nrenderer = \"sixel\"\n",
    )
    .unwrap();

    let out = run(&dir, &["config", "show"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).contains("sixel"));
}

#[test]
fn broken_config_exits_with_usage_code() {
    let dir = temp_dir("brokenconfig");
    std::fs::create_dir_all(dir.join("config")).unwrap();
    std::fs::write(dir.join("config").join("config.toml"), "это не toml =====").unwrap();

    let out = run(&dir, &["sources", "list"]);
    assert_eq!(code(&out), 2, "битый конфиг — ошибка использования");
}

#[test]
fn unimplemented_command_exits_with_six() {
    let dir = temp_dir("notimpl");
    let out = run(&dir, &["library", "list"]);
    assert_eq!(code(&out), 6);
}

#[test]
fn reading_missing_path_fails() {
    let dir = temp_dir("readmissing");
    let out = run(&dir, &["read", "/этого/точно/нет.cbz"]);
    assert_eq!(code(&out), 1);
}
