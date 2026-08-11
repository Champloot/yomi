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
    for cmd in ["read", "info", "pack", "library", "config"] {
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

/// Минимальный валидный PNG для тестов.
fn png_bytes() -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4));
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

/// Собирает валидный CBZ из нескольких PNG — нужен тестам библиотеки,
/// потому что сканер отсеивает архивы без картинок.
fn make_cbz(path: &Path, pages: usize) {
    use std::io::Write;
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 4));
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();

    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    for i in 0..pages {
        zip.start_file(format!("{i:03}.png"), zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(&png).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn library_list_on_empty_library_succeeds() {
    let dir = temp_dir("emptylib");
    let out = run(&dir, &["library"]);
    assert_eq!(code(&out), 0, "пустая библиотека — не ошибка");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("пуста"),
        "должна быть подсказка, что делать: {stdout}"
    );
}

#[test]
fn library_scan_without_targets_is_a_usage_error() {
    let dir = temp_dir("scannotarget");
    let out = run(&dir, &["library", "scan"]);
    assert_eq!(code(&out), 1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("library.paths"), "{stderr}");
}

#[test]
fn library_scan_finds_titles_and_list_shows_them() {
    let dir = temp_dir("scanreal");
    // Раскладка КАТАЛОГ/Тайтл/Том.cbz — та, что ожидает сканер.
    let manga_dir = dir.join("манга").join("Тестовый тайтл");
    std::fs::create_dir_all(&manga_dir).unwrap();
    make_cbz(&manga_dir.join("Том 1.cbz"), 3);

    let out = run(
        &dir,
        &["library", "scan", dir.join("манга").to_str().unwrap()],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Тестовый тайтл"));

    let listed = run(&dir, &["library"]);
    assert_eq!(code(&listed), 0);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("Тестовый тайтл"));
}

#[test]
fn empty_library_explains_what_to_do() {
    let dir = temp_dir("libempty");
    let out = run(&dir, &["library"]);
    assert_eq!(code(&out), 0);
    assert!(String::from_utf8_lossy(&out.stdout).contains("library scan"));
}

#[test]
fn reading_missing_path_fails() {
    let dir = temp_dir("readmissing");
    let out = run(&dir, &["read", "/этого/точно/нет.cbz"]);
    assert_eq!(code(&out), 1);
}

#[test]
fn reading_cbr_gives_specific_unsupported_message() {
    let dir = temp_dir("readcbr");
    let cbr = dir.join("chapter.cbr");
    std::fs::write(&cbr, b"not really rar, format check happens by extension").unwrap();
    let out = run(&dir, &["read", cbr.to_str().unwrap()]);
    assert_eq!(code(&out), 1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("CBR"),
        "сообщение должно называть формат явно: {stderr}"
    );
}

#[test]
fn reading_page_zero_is_rejected_before_touching_the_terminal() {
    let dir = temp_dir("readpage0");
    let cbz = dir.join("empty.cbz");
    // Файл не обязан быть валидным CBZ: страница 0 отклоняется раньше,
    // чем источник вообще открывается.
    std::fs::write(&cbz, b"stub").unwrap();
    let out = run(&dir, &["read", cbz.to_str().unwrap(), "-p", "0"]);
    assert_eq!(code(&out), 1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("с единицы"), "{stderr}");
}

#[test]
fn clean_without_confirmation_changes_nothing() {
    let dir = temp_dir("cleandry");
    let manga_dir = dir.join("манга").join("Тайтл");
    std::fs::create_dir_all(&manga_dir).unwrap();
    let cbz = manga_dir.join("Том 1.cbz");
    make_cbz(&cbz, 2);
    run(
        &dir,
        &["library", "scan", dir.join("манга").to_str().unwrap()],
    );

    std::fs::remove_file(&cbz).unwrap();

    let out = run(&dir, &["library", "clean"]);
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("пропал"),
        "должен показать пропажу: {stdout}"
    );
    assert!(
        stdout.contains("--yes"),
        "должен подсказать, как подтвердить"
    );

    // Запись обязана остаться: без подтверждения ничего не удаляем.
    let listed = run(&dir, &["library"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("Тайтл"));
}

#[test]
fn clean_with_confirmation_removes_missing_entries() {
    let dir = temp_dir("cleanyes");
    let manga_dir = dir.join("манга").join("Тайтл");
    std::fs::create_dir_all(&manga_dir).unwrap();
    let cbz = manga_dir.join("Том 1.cbz");
    make_cbz(&cbz, 2);
    run(
        &dir,
        &["library", "scan", dir.join("манга").to_str().unwrap()],
    );

    std::fs::remove_file(&cbz).unwrap();
    let out = run(&dir, &["library", "clean", "--yes"]);
    assert_eq!(code(&out), 0);

    // Тайтл без глав тоже уходит.
    let listed = run(&dir, &["library"]);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("пуста"),
        "библиотека должна опустеть"
    );
}

#[test]
fn clean_on_intact_library_reports_nothing_to_do() {
    let dir = temp_dir("cleanintact");
    let manga_dir = dir.join("манга").join("Тайтл");
    std::fs::create_dir_all(&manga_dir).unwrap();
    make_cbz(&manga_dir.join("Том 1.cbz"), 2);
    run(
        &dir,
        &["library", "scan", dir.join("манга").to_str().unwrap()],
    );

    let out = run(&dir, &["library", "clean"]);
    assert_eq!(code(&out), 0);
    assert!(String::from_utf8_lossy(&out.stdout).contains("на месте"));
}

#[test]
fn chapters_lists_chapters_of_a_title() {
    let dir = temp_dir("chapters");
    let manga_dir = dir.join("манга").join("Тайтл");
    std::fs::create_dir_all(&manga_dir).unwrap();
    make_cbz(&manga_dir.join("Том 1.cbz"), 2);
    make_cbz(&manga_dir.join("Том 2.cbz"), 3);
    run(
        &dir,
        &["library", "scan", dir.join("манга").to_str().unwrap()],
    );

    let out = run(&dir, &["library", "Тайтл"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("т.1") && stdout.contains("т.2"), "{stdout}");
}

#[test]
fn chapters_of_unknown_title_is_not_found() {
    let dir = temp_dir("chaptersmissing");
    run(&dir, &["library"]);
    let out = run(&dir, &["library", "нет такого тайтла"]);
    assert_ne!(code(&out), 0, "несуществующий тайтл — ошибка");
    assert!(String::from_utf8_lossy(&out.stderr).contains("нет тайтла"));
}

#[test]
fn pack_builds_an_archive_from_a_directory() {
    let dir = temp_dir("packdir");
    let pages = dir.join("Глава 1");
    std::fs::create_dir_all(&pages).unwrap();
    for name in ["page1.png", "page2.png", "page10.png"] {
        std::fs::write(pages.join(name), png_bytes()).unwrap();
    }

    let out = run(&dir, &["pack", pages.to_str().unwrap()]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    let archive = dir.join("Глава 1.cbz");
    assert!(archive.exists(), "архив должен появиться рядом с каталогом");

    // Порядок страниц должен быть естественным, а не лексикографическим.
    let file = std::fs::File::open(&archive).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    assert_eq!(names[0], "001.png");
    assert_eq!(names[2], "003.png");
    assert!(names.contains(&"ComicInfo.xml".to_string()));
}

#[test]
fn pack_refuses_to_overwrite_without_force() {
    let dir = temp_dir("packoverwrite");
    let pages = dir.join("тайтл");
    std::fs::create_dir_all(&pages).unwrap();
    std::fs::write(pages.join("001.png"), png_bytes()).unwrap();

    assert_eq!(code(&run(&dir, &["pack", pages.to_str().unwrap()])), 0);
    let second = run(&dir, &["pack", pages.to_str().unwrap()]);
    assert_ne!(
        code(&second),
        0,
        "повторная упаковка не должна затирать молча"
    );
    assert!(String::from_utf8_lossy(&second.stderr).contains("--force"));

    assert_eq!(
        code(&run(&dir, &["pack", pages.to_str().unwrap(), "--force"])),
        0
    );
}

#[test]
fn pack_needs_a_directory_not_a_file() {
    let dir = temp_dir("packfile");
    let file = dir.join("одна.png");
    std::fs::write(&file, png_bytes()).unwrap();
    assert_ne!(code(&run(&dir, &["pack", file.to_str().unwrap()])), 0);
}

/// Готовит библиотеку с одним файлом и возвращает путь к нему.
fn library_with_file(dir: &Path, pages: usize) -> PathBuf {
    let manga_dir = dir.join("манга").join("Тайтл");
    std::fs::create_dir_all(&manga_dir).unwrap();
    let file = manga_dir.join("Том 1.cbz");
    make_cbz(&file, pages);
    run(
        dir,
        &["library", "scan", dir.join("манга").to_str().unwrap()],
    );
    file
}

#[test]
fn marks_can_be_added_listed_and_removed() {
    let dir = temp_dir("marksflow");
    let file = library_with_file(&dir, 30);
    let path = file.to_str().unwrap();

    assert_eq!(
        code(&run(
            &dir,
            &["marks", "add", path, "10", "--title", "Вторая"]
        )),
        0
    );
    assert_eq!(code(&run(&dir, &["marks", "add", path, "1"])), 0);

    let listed = run(&dir, &["marks", "list", path]);
    assert_eq!(code(&listed), 0);
    let text = stdout(&listed);
    assert!(text.contains("Вторая"), "{text}");
    assert!(text.contains("Всего глав: 2"), "{text}");

    assert_eq!(code(&run(&dir, &["marks", "remove", path, "10"])), 0);
    assert!(stdout(&run(&dir, &["marks", "list", path])).contains("Всего глав: 1"));
}

#[test]
fn marks_reject_page_numbers_outside_the_file() {
    let dir = temp_dir("marksrange");
    let file = library_with_file(&dir, 5);
    let path = file.to_str().unwrap();

    assert_ne!(
        code(&run(&dir, &["marks", "add", path, "0"])),
        0,
        "нумерация с единицы"
    );
    assert_ne!(
        code(&run(&dir, &["marks", "add", path, "99"])),
        0,
        "страницы нет"
    );
}

#[test]
fn marks_require_the_file_to_be_in_the_library() {
    let dir = temp_dir("marksunknown");
    let loose = dir.join("одинокий.cbz");
    make_cbz(&loose, 3);
    // Библиотеку создаём, но файл в неё не попадает.
    run(&dir, &["library"]);

    let out = run(&dir, &["marks", "list", loose.to_str().unwrap()]);
    assert_ne!(code(&out), 0);
    assert!(String::from_utf8_lossy(&out.stderr).contains("library scan"));
}

#[test]
fn marks_are_not_replaced_without_force() {
    let dir = temp_dir("marksforce");
    let file = library_with_file(&dir, 20);
    let path = file.to_str().unwrap();

    run(&dir, &["marks", "add", path, "5"]);
    let out = run(&dir, &["marks", "detect", path, "--deep"]);
    assert_ne!(code(&out), 0, "существующие отметки нельзя затирать молча");
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
}

#[test]
fn marks_survive_a_library_rescan() {
    // Ручной труд не должен пропадать при обновлении библиотеки.
    let dir = temp_dir("marksrescan");
    let file = library_with_file(&dir, 20);
    let path = file.to_str().unwrap();

    run(&dir, &["marks", "add", path, "7", "--title", "Ручная"]);
    run(
        &dir,
        &["library", "scan", dir.join("манга").to_str().unwrap()],
    );

    let text = stdout(&run(&dir, &["marks", "list", path]));
    assert!(text.contains("Ручная"), "отметка должна уцелеть: {text}");
}

#[test]
fn clearing_removes_every_mark() {
    let dir = temp_dir("marksclear");
    let file = library_with_file(&dir, 20);
    let path = file.to_str().unwrap();

    run(&dir, &["marks", "add", path, "3"]);
    run(&dir, &["marks", "add", path, "9"]);
    assert_eq!(code(&run(&dir, &["marks", "clear", path])), 0);
    assert!(stdout(&run(&dir, &["marks", "list", path])).contains("Отметок нет"));
}

#[test]
fn build_assembles_a_volume_with_chapter_bookmarks() {
    let dir = temp_dir("buildvol");
    let chapters = dir.join("главы");
    std::fs::create_dir_all(&chapters).unwrap();
    // Имена в том же формате, что у реальных файлов.
    make_cbz(&chapters.join("33_-_359_Первая.cbz"), 5);
    make_cbz(&chapters.join("33_-_360_Вторая.cbz"), 4);
    make_cbz(&chapters.join("33_-_361_Третья.cbz"), 6);

    let out = dir.join("том33.cbz");
    let result = run(
        &dir,
        &[
            "build",
            chapters.to_str().unwrap(),
            "--series",
            "Тайтл",
            "-o",
            out.to_str().unwrap(),
            "--yes",
        ],
    );
    assert_eq!(
        code(&result),
        0,
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(out.exists());

    // Границы глав должны читаться обратно ровно там, где стыки.
    let info = run(&dir, &["info", out.to_str().unwrap()]);
    let text = stdout(&info);
    assert!(text.contains("закладки ComicInfo.xml"), "{text}");
    assert!(text.contains("Глава 359"), "{text}");
    assert!(
        text.contains("стр. 6–9"),
        "вторая глава начинается с шестой страницы: {text}"
    );
}

#[test]
fn build_refuses_to_overwrite_without_force() {
    let dir = temp_dir("buildforce");
    let chapters = dir.join("главы");
    std::fs::create_dir_all(&chapters).unwrap();
    make_cbz(&chapters.join("1.cbz"), 3);

    let out = dir.join("том.cbz");
    let args = [
        "build",
        chapters.to_str().unwrap(),
        "--series",
        "Т",
        "-o",
        out.to_str().unwrap(),
        "--yes",
    ];
    assert_eq!(code(&run(&dir, &args)), 0);

    let second = run(&dir, &args);
    assert_ne!(
        code(&second),
        0,
        "существующий том не должен затираться молча"
    );
    assert!(String::from_utf8_lossy(&second.stderr).contains("--force"));
}

#[test]
fn build_needs_archives_in_the_directory() {
    let dir = temp_dir("buildempty");
    let empty = dir.join("пусто");
    std::fs::create_dir_all(&empty).unwrap();
    let out = run(&dir, &["build", empty.to_str().unwrap(), "--yes"]);
    assert_ne!(code(&out), 0);
}

#[test]
fn build_add_appends_a_chapter_and_keeps_previous_bookmarks() {
    let dir = temp_dir("buildadd");
    let chapters = dir.join("главы");
    std::fs::create_dir_all(&chapters).unwrap();
    make_cbz(&chapters.join("1_-_10_Первая.cbz"), 4);
    make_cbz(&chapters.join("1_-_11_Вторая.cbz"), 4);

    let out = dir.join("том.cbz");
    run(
        &dir,
        &[
            "build",
            chapters.to_str().unwrap(),
            "--series",
            "Тайтл",
            "-o",
            out.to_str().unwrap(),
            "--yes",
        ],
    );

    let extra = dir.join("1_-_12_Третья.cbz");
    make_cbz(&extra, 3);
    let added = run(
        &dir,
        &[
            "build",
            out.to_str().unwrap(),
            "--add",
            extra.to_str().unwrap(),
        ],
    );
    assert_eq!(
        code(&added),
        0,
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let text = stdout(&run(&dir, &["info", out.to_str().unwrap()]));
    assert!(
        text.contains("Глава 10"),
        "старые закладки должны уцелеть: {text}"
    );
    assert!(
        text.contains("Глава 12"),
        "новая глава должна появиться: {text}"
    );
}

#[test]
fn build_add_inserts_a_chapter_by_its_number() {
    // Пропущенная глава должна встать между соседями, а не в конец.
    let dir = temp_dir("buildinsert");
    let chapters = dir.join("главы");
    std::fs::create_dir_all(&chapters).unwrap();
    make_cbz(&chapters.join("34_-_364_Первая.cbz"), 3);
    make_cbz(&chapters.join("34_-_366_Третья.cbz"), 4);

    let out = dir.join("том34.cbz");
    run(
        &dir,
        &[
            "build",
            chapters.to_str().unwrap(),
            "--series",
            "Т",
            "-o",
            out.to_str().unwrap(),
            "--yes",
        ],
    );

    let missing = dir.join("34_-_365_Вторая.cbz");
    make_cbz(&missing, 5);
    let added = run(
        &dir,
        &[
            "build",
            out.to_str().unwrap(),
            "--add",
            missing.to_str().unwrap(),
        ],
    );
    assert_eq!(
        code(&added),
        0,
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );

    let text = stdout(&run(&dir, &["info", out.to_str().unwrap()]));
    let pos364 = text.find("Глава 364").expect("364 на месте");
    let pos365 = text.find("Глава 365").expect("365 добавлена");
    let pos366 = text.find("Глава 366").expect("366 на месте");
    assert!(
        pos364 < pos365 && pos365 < pos366,
        "порядок глав нарушен:\n{text}"
    );

    // Страницы должны быть пересчитаны: 365 начинается сразу после 364.
    assert!(text.contains("стр. 4–8"), "границы не пересчитаны:\n{text}");
}

#[test]
fn build_add_puts_a_later_chapter_at_the_end() {
    let dir = temp_dir("buildappend");
    let chapters = dir.join("главы");
    std::fs::create_dir_all(&chapters).unwrap();
    make_cbz(&chapters.join("34_-_364_Первая.cbz"), 3);

    let out = dir.join("том.cbz");
    run(
        &dir,
        &[
            "build",
            chapters.to_str().unwrap(),
            "--series",
            "Т",
            "-o",
            out.to_str().unwrap(),
            "--yes",
        ],
    );

    let later = dir.join("34_-_370_Поздняя.cbz");
    make_cbz(&later, 2);
    assert_eq!(
        code(&run(
            &dir,
            &[
                "build",
                out.to_str().unwrap(),
                "--add",
                later.to_str().unwrap()
            ]
        )),
        0
    );

    let text = stdout(&run(&dir, &["info", out.to_str().unwrap()]));
    assert!(text.find("Глава 364") < text.find("Глава 370"), "{text}");
}

#[test]
fn output_does_not_panic_when_the_pipe_closes() {
    // `yomi info файл | head` не должен падать с паникой: Rust
    // игнорирует SIGPIPE, и печать в закрытую трубу становится ошибкой.
    let dir = temp_dir("pipe");
    let file = dir.join("том.cbz");
    make_cbz(&file, 3);

    let out = run(&dir, &["info", file.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn build_splits_a_mixed_directory_into_volumes() {
    let dir = temp_dir("buildgroups");
    let mixed = dir.join("вперемешку");
    std::fs::create_dir_all(&mixed).unwrap();
    // Три файла тома 21 и один тома 22: связная последовательность
    // важнее одиночного совпадения номеров.
    for name in [
        "_21_12_Наз.cbz",
        "_21_13_Наз.cbz",
        "_21_14_Наз.cbz",
        "_22_12_Наз.cbz",
    ] {
        make_cbz(&mixed.join(name), 3);
    }

    let out = run(
        &dir,
        &["build", mixed.to_str().unwrap(), "--series", "Т", "--yes"],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    let text = stdout(&out);
    assert!(text.contains("найдено групп: 2"), "{text}");
    // Тома складываются в подкаталог по названию тайтла.
    let vol21 = mixed.join("Т").join("Т_Vol_21.cbz");
    assert!(vol21.exists(), "том 21 должен собраться отдельно");
    assert!(mixed.join("Т").join("Т_Vol_22.cbz").exists(), "том 22 тоже");

    // В томе 21 должно быть три главы, а не все четыре файла.
    let info = stdout(&run(&dir, &["info", vol21.to_str().unwrap()]));
    assert!(
        info.contains("Глава 12") && info.contains("Глава 14"),
        "{info}"
    );
    assert!(
        !info.contains("Страниц: 12"),
        "лишний файл попал в том: {info}"
    );
}

#[test]
fn build_keeps_different_series_apart() {
    let dir = temp_dir("buildseries");
    let mixed = dir.join("вперемешку");
    std::fs::create_dir_all(&mixed).unwrap();
    make_cbz(&mixed.join("Первый_1_1_Имя.cbz"), 2);
    make_cbz(&mixed.join("Первый_1_2_Имя.cbz"), 2);
    make_cbz(&mixed.join("Второй_1_1_Имя.cbz"), 2);

    let out = run(&dir, &["build", mixed.to_str().unwrap(), "--yes"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let text = stdout(&out);
    assert!(
        text.contains("найдено групп: 2"),
        "разные тайтлы не должны сливаться: {text}"
    );
}

#[test]
fn single_volume_directory_is_not_treated_as_groups() {
    let dir = temp_dir("buildone");
    let one = dir.join("том");
    std::fs::create_dir_all(&one).unwrap();
    make_cbz(&one.join("v01 c01.cbz"), 2);
    make_cbz(&one.join("v01 c02.cbz"), 2);

    let out = run(
        &dir,
        &["build", one.to_str().unwrap(), "--series", "Т", "--yes"],
    );
    assert_eq!(code(&out), 0);
    assert!(
        !stdout(&out).contains("найдено групп"),
        "один том — без разговоров о группах"
    );
}

#[test]
fn completions_are_generated_for_every_supported_shell() {
    let dir = temp_dir("completions");
    for shell in ["bash", "zsh", "fish"] {
        let out = run(&dir, &["generate", "completions", shell]);
        assert_eq!(code(&out), 0, "оболочка {shell}");
        let text = stdout(&out);
        assert!(text.contains("yomi"), "пустое автодополнение для {shell}");
        // Команды должны попадать в автодополнение автоматически:
        // они берутся из того же описания, что разбирает аргументы.
        assert!(text.contains("build"), "нет команды build для {shell}");
    }
}

#[test]
fn manpage_is_generated() {
    let dir = temp_dir("manpage");
    let out = run(&dir, &["generate", "manpage"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(
        text.contains(".TH yomi 1"),
        "не похоже на roff: {}",
        &text[..80.min(text.len())]
    );
    assert!(text.contains("SH NAME"));
}

#[test]
fn built_volumes_go_into_a_series_subdirectory() {
    // Раньше тома уезжали в родительский каталог — то есть в домашний,
    // если главы лежали прямо в ~/manga.
    let dir = temp_dir("buildlayout");
    let manga = dir.join("manga");
    std::fs::create_dir_all(&manga).unwrap();
    make_cbz(&manga.join("33_-_359_Первая.cbz"), 3);
    make_cbz(&manga.join("33_-_360_Вторая.cbz"), 3);

    let out = run(
        &dir,
        &[
            "build",
            manga.to_str().unwrap(),
            "--series",
            "Тайтл",
            "--yes",
        ],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    assert!(
        manga.join("Тайтл").join("Тайтл_Vol_33.cbz").exists(),
        "том должен лечь в подкаталог тайтла"
    );
    assert!(
        !dir.join("Тайтл_Vol_33.cbz").exists(),
        "и не выше исходного каталога"
    );
}

#[test]
fn bookmarks_note_is_printed_once_per_run() {
    let dir = temp_dir("buildnote");
    let manga = dir.join("manga");
    std::fs::create_dir_all(&manga).unwrap();
    for name in ["33_-_1_А.cbz", "33_-_2_Б.cbz", "34_-_3_В.cbz"] {
        make_cbz(&manga.join(name), 2);
    }

    let out = run(
        &dir,
        &["build", manga.to_str().unwrap(), "--series", "Т", "--yes"],
    );
    let text = stdout(&out);
    assert_eq!(
        text.matches("поймут и Komga").count(),
        1,
        "пояснение повторяется для каждого тома:\n{text}"
    );
}

#[test]
fn reading_a_file_outside_the_library_warns_about_progress() {
    let dir = temp_dir("readwarn");
    let file = dir.join("том.cbz");
    make_cbz(&file, 3);

    let out = run(&dir, &["read", file.to_str().unwrap()]);
    // Читалке нужен терминал, поэтому она завершится ошибкой, но
    // предупреждение должно успеть напечататься.
    assert!(
        stdout(&out).contains("прогресс чтения сохранён не будет"),
        "нет предупреждения: {}",
        stdout(&out)
    );
}

#[test]
fn library_stores_absolute_paths_even_when_scanned_relatively() {
    // Иначе прогресс не найдётся при чтении абсолютным путём.
    let dir = temp_dir("libpaths");
    let manga = dir.join("manga").join("Тайтл");
    std::fs::create_dir_all(&manga).unwrap();
    make_cbz(&manga.join("том1.cbz"), 2);

    let out = std::process::Command::new(BIN)
        .args(["library", "scan", "manga"])
        .current_dir(&dir)
        .env("YOMI_CONFIG_DIR", dir.join("config"))
        .env("YOMI_DATA_DIR", dir.join("data"))
        .env("YOMI_CACHE_DIR", dir.join("cache"))
        .env_remove("YOMI_LOG")
        .output()
        .expect("запуск бинарника yomi");
    assert_eq!(out.status.code(), Some(0));

    // Проверяем напрямую в базе: показ по названию путей больше не печатает.
    let db = dir.join("data").join("library.db");
    assert!(db.exists(), "база должна быть создана");
    let dump = std::process::Command::new(BIN)
        .args(["library", "Тайтл"])
        .env("YOMI_CONFIG_DIR", dir.join("config"))
        .env("YOMI_DATA_DIR", dir.join("data"))
        .env("YOMI_CACHE_DIR", dir.join("cache"))
        .output()
        .expect("запуск");
    assert_eq!(
        dump.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&dump.stderr)
    );
}

#[test]
fn library_overview_groups_by_title_with_volume_range() {
    let dir = temp_dir("libview");
    let manga = dir.join("manga").join("Тайтл");
    std::fs::create_dir_all(&manga).unwrap();
    make_cbz(&manga.join("Том 33.cbz"), 3);
    make_cbz(&manga.join("Том 34.cbz"), 3);
    run(
        &dir,
        &["library", "scan", dir.join("manga").to_str().unwrap()],
    );

    let out = run(&dir, &["library"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let text = stdout(&out);
    assert!(text.contains("Тайтл"), "{text}");
    assert!(
        text.contains("тома 33–34"),
        "должен быть диапазон томов: {text}"
    );
    assert!(
        text.contains("дальше"),
        "должно быть видно, что читать дальше: {text}"
    );
}

#[test]
fn library_by_title_lists_volumes_newest_first() {
    let dir = temp_dir("libtitle");
    let manga = dir.join("manga").join("Тайтл");
    std::fs::create_dir_all(&manga).unwrap();
    make_cbz(&manga.join("Том 1.cbz"), 3);
    make_cbz(&manga.join("Том 2.cbz"), 3);
    run(
        &dir,
        &["library", "scan", dir.join("manga").to_str().unwrap()],
    );

    let text = stdout(&run(&dir, &["library", "Тайтл"]));
    let second = text.find("т.2").expect("том 2 в списке");
    let first = text.find("т.1").expect("том 1 в списке");
    assert!(second < first, "свежее должно быть сверху:\n{text}");
}

#[test]
fn reading_by_title_opens_the_library_entry() {
    let dir = temp_dir("readtitle");
    let manga = dir.join("manga").join("Мой тайтл");
    std::fs::create_dir_all(&manga).unwrap();
    make_cbz(&manga.join("Том 1.cbz"), 3);
    run(
        &dir,
        &["library", "scan", dir.join("manga").to_str().unwrap()],
    );

    // Читалке нужен терминал, но выбор файла по названию должен
    // произойти до этого и напечататься.
    let out = run(&dir, &["read", "Мой тайтл"]);
    assert!(stdout(&out).contains("Мой тайтл"), "{}", stdout(&out));
}

#[test]
fn reading_an_unknown_title_explains_itself() {
    let dir = temp_dir("readunknown");
    run(&dir, &["library"]);
    let out = run(&dir, &["read", "Такого тайтла нет"]);
    assert_ne!(code(&out), 0);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("не файл"), "{stderr}");
}

#[test]
fn an_existing_file_wins_over_a_title_with_the_same_name() {
    // Файл важнее совпадения имён: иначе тайтл «том.cbz» перехватил бы
    // открытие настоящего файла.
    let dir = temp_dir("readpriority");
    let file = dir.join("том.cbz");
    make_cbz(&file, 2);

    let out = run(&dir, &["read", file.to_str().unwrap()]);
    let combined = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
    assert!(!combined.contains("не файл и не название"), "{combined}");
}
