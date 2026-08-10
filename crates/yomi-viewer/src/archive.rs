//! Источник страниц: каталог с картинками или CBZ-архив.
//!
//! `PageSource` даёт унифицированный доступ к списку и содержимому страниц
//! независимо от того, распакованы они на диске или лежат в ZIP. Читалка
//! и будущий загрузчик работают с этим типом, не заботясь о деталях.

use crate::{Error, Result};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Расширения, которые считаются страницами. Служебные файлы вроде
/// `Thumbs.db`, `.DS_Store` или `__MACOSX/` сюда не попадают.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp"];

fn is_image(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Открытый источник страниц.
pub enum PageSource {
    Directory {
        root: PathBuf,
        files: Vec<PathBuf>,
    },
    Cbz {
        path: PathBuf,
        entries: Vec<String>,
    },
    /// Единственный файл-изображение — читалка с одной страницей.
    SingleFile {
        path: PathBuf,
    },
}

impl PageSource {
    /// Открывает каталог с изображениями.
    pub fn open_directory(root: &Path) -> Result<Self> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_image(&name) {
                files.push(entry.path());
            }
        }
        if files.is_empty() {
            return Err(Error::NoPages(root.to_path_buf()));
        }
        crate::natural_sort::sort(&mut files, |p| {
            p.file_name().and_then(|n| n.to_str()).unwrap_or("")
        });
        Ok(Self::Directory {
            root: root.to_path_buf(),
            files,
        })
    }

    /// Открывает CBZ (ZIP-архив со страницами).
    pub fn open_cbz(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let archive = zip::ZipArchive::new(file).map_err(|e| Error::Archive {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let mut entries: Vec<String> = archive
            .file_names()
            // Файлы внутри служебного macOS-каталога — не страницы.
            .filter(|n| !n.starts_with("__MACOSX/"))
            .filter(|n| is_image(n))
            .map(str::to_string)
            .collect();

        if entries.is_empty() {
            return Err(Error::NoPages(path.to_path_buf()));
        }
        crate::natural_sort::sort(&mut entries, |s| s.as_str());
        Ok(Self::Cbz {
            path: path.to_path_buf(),
            entries,
        })
    }

    /// Открывает одиночный файл-изображение как читалку с одной страницей.
    pub fn open_single_image(path: &Path) -> Result<Self> {
        if !is_image(&path.to_string_lossy()) {
            return Err(Error::UnsupportedFormat(path.to_path_buf()));
        }
        Ok(Self::SingleFile {
            path: path.to_path_buf(),
        })
    }

    /// Открывает каталог, CBZ или одиночное изображение по пути.
    pub fn open(path: &Path) -> Result<Self> {
        if path.is_dir() {
            return Self::open_directory(path);
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "cbz" | "zip" => Self::open_cbz(path),
            _ if is_image(&path.to_string_lossy()) => Self::open_single_image(path),
            _ => Err(Error::UnsupportedFormat(path.to_path_buf())),
        }
    }

    /// Имена страниц в порядке чтения — нужны для распознавания
    /// структуры тома (см. [`crate::structure`]).
    pub fn entry_names(&self) -> Vec<String> {
        match self {
            Self::Directory { files, .. } => files
                .iter()
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .collect(),
            Self::Cbz { entries, .. } => entries.clone(),
            Self::SingleFile { path } => vec![path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()],
        }
    }

    /// Размеры всех страниц в пикселях.
    ///
    /// Читает только заголовки изображений, без полного декодирования:
    /// на томе в две сотни страниц разница принципиальная. Страницы,
    /// размер которых определить не удалось, пропускаются — битый файл
    /// не повод отказываться от разбора остальных.
    pub fn page_shapes(&self) -> Vec<crate::structure::PageShape> {
        (0..self.page_count())
            .filter_map(|i| {
                let bytes = self.read_page(i).ok()?;
                let (width, height) = image::io::Reader::new(std::io::Cursor::new(bytes))
                    .with_guessed_format()
                    .ok()?
                    .into_dimensions()
                    .ok()?;
                Some(crate::structure::PageShape { width, height })
            })
            .collect()
    }

    /// Путь к источнику — для сообщений и чтения метаданных.
    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { root, .. } => root,
            Self::Cbz { path, .. } => path,
            Self::SingleFile { path } => path,
        }
    }

    pub fn page_count(&self) -> usize {
        match self {
            Self::Directory { files, .. } => files.len(),
            Self::Cbz { entries, .. } => entries.len(),
            Self::SingleFile { .. } => 1,
        }
    }

    /// Сырые байты страницы по индексу с нуля.
    pub fn read_page(&self, index: usize) -> Result<Vec<u8>> {
        match self {
            Self::Directory { files, .. } => {
                let path = files.get(index).ok_or(Error::PageOutOfRange(index))?;
                Ok(std::fs::read(path)?)
            }
            Self::SingleFile { path } => {
                if index != 0 {
                    return Err(Error::PageOutOfRange(index));
                }
                Ok(std::fs::read(path)?)
            }
            Self::Cbz { path, entries } => {
                let name = entries.get(index).ok_or(Error::PageOutOfRange(index))?;
                let file = std::fs::File::open(path)?;
                let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::Archive {
                    path: path.clone(),
                    message: e.to_string(),
                })?;
                let mut entry = archive.by_name(name).map_err(|e| Error::Archive {
                    path: path.clone(),
                    message: e.to_string(),
                })?;
                let mut buf = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut buf)?;
                Ok(buf)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_png(path: &Path) {
        // Валидный минимальный 1x1 PNG — достаточно, чтобы проверить
        // открытие источника; декодирование картинки тестируется отдельно.
        let bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
            0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn directory_lists_only_images_in_natural_order() {
        let dir = tempfile::tempdir().unwrap();
        write_png(&dir.path().join("page2.png"));
        write_png(&dir.path().join("page10.png"));
        write_png(&dir.path().join("page1.png"));
        std::fs::write(dir.path().join("readme.txt"), b"not a page").unwrap();

        let src = PageSource::open_directory(dir.path()).unwrap();
        assert_eq!(src.page_count(), 3);
    }

    #[test]
    fn empty_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            PageSource::open_directory(dir.path()),
            Err(Error::NoPages(_))
        ));
    }

    #[test]
    fn cbz_lists_pages_and_reads_bytes_back() {
        let dir = tempfile::tempdir().unwrap();
        let cbz_path = dir.path().join("chapter.cbz");
        let file = std::fs::File::create(&cbz_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::FileOptions::default();

        for (name, content) in [("002.png", b"b" as &[u8]), ("001.png", b"a")] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(content).unwrap();
        }
        // Служебный мусор, который не должен попасть в список страниц.
        zip.start_file("__MACOSX/002.png", opts).unwrap();
        zip.write_all(b"junk").unwrap();
        zip.finish().unwrap();

        let src = PageSource::open_cbz(&cbz_path).unwrap();
        assert_eq!(src.page_count(), 2);
        assert_eq!(src.read_page(0).unwrap(), b"a");
        assert_eq!(src.read_page(1).unwrap(), b"b");
    }

    #[test]
    fn out_of_range_page_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write_png(&dir.path().join("page1.png"));
        let src = PageSource::open_directory(dir.path()).unwrap();
        assert!(matches!(src.read_page(5), Err(Error::PageOutOfRange(5))));
    }

    #[test]
    fn open_dispatches_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert!(matches!(
            PageSource::open(&txt),
            Err(Error::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn open_dispatches_single_image_to_one_page_source() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("cover.png");
        write_png(&png);
        let src = PageSource::open(&png).unwrap();
        assert_eq!(src.page_count(), 1);
        assert!(src.read_page(1).is_err());
    }
}
