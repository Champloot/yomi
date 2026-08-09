//! `yomi read` — читалка. Основная работа этапа M1.
//!
//! Уже сейчас команда делает полезное: проверяет, что путь существует и
//! опознаётся как поддерживаемый формат. Это позволяет тестировать
//! определение формата отдельно от рендеринга.

use super::{not_implemented, Ctx};
use crate::cli::ReadArgs;
use anyhow::{bail, Result};
use std::path::Path;

/// Что нам подсунули.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// Каталог с изображениями.
    Directory,
    /// Архив CBZ/ZIP.
    Cbz,
    /// Архив CBR/RAR — распаковка отложена, см. ADR-0006.
    Cbr,
    /// Одиночное изображение.
    Image,
    Unsupported,
}

pub fn detect_kind(path: &Path) -> InputKind {
    if path.is_dir() {
        return InputKind::Directory;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "cbz" | "zip" => InputKind::Cbz,
        "cbr" | "rar" => InputKind::Cbr,
        "png" | "jpg" | "jpeg" | "webp" | "avif" | "gif" => InputKind::Image,
        _ => InputKind::Unsupported,
    }
}

pub async fn run(ctx: &Ctx, args: &ReadArgs) -> Result<()> {
    if !args.path.exists() {
        bail!("путь не существует: {}", args.path.display());
    }

    let kind = detect_kind(&args.path);
    let renderer = args
        .renderer
        .map(Into::into)
        .unwrap_or(ctx.config.reader.renderer);

    tracing::info!(
        path = %args.path.display(),
        ?kind,
        ?renderer,
        direction = ?ctx.config.reader.direction,
        page = args.page,
        "запуск читалки"
    );

    match kind {
        InputKind::Unsupported => {
            bail!(
                "неподдерживаемый формат: {}. Поддерживаются каталоги, CBZ и изображения",
                args.path.display()
            )
        }
        InputKind::Cbr => bail!("CBR/RAR пока не поддерживается, см. docs/adr/0006-formats.md"),
        _ => Err(not_implemented("чтение и вывод страниц", "M1")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_cbz_regardless_of_case() {
        assert_eq!(detect_kind(&PathBuf::from("a.CBZ")), InputKind::Cbz);
        assert_eq!(detect_kind(&PathBuf::from("a.cbz")), InputKind::Cbz);
    }

    #[test]
    fn detects_images() {
        assert_eq!(detect_kind(&PathBuf::from("page.jpeg")), InputKind::Image);
        assert_eq!(detect_kind(&PathBuf::from("page.webp")), InputKind::Image);
    }

    #[test]
    fn detects_directory() {
        assert_eq!(detect_kind(&PathBuf::from("/tmp")), InputKind::Directory);
    }

    #[test]
    fn unknown_extension_is_unsupported() {
        assert_eq!(
            detect_kind(&PathBuf::from("notes.txt")),
            InputKind::Unsupported
        );
        assert_eq!(detect_kind(&PathBuf::from("noext")), InputKind::Unsupported);
    }
}
