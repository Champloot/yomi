//! Чтение PDF через извлечение встроенных изображений.
//!
//! Манга-сканы в PDF почти всегда устроены просто: одна растровая
//! картинка на страницу, без векторной графики. Вытащить её из структуры
//! файла на порядок легче, чем полноценно отрендерить страницу — а
//! полный рендер потребовал бы pdfium или poppler, тяжёлых зависимостей
//! с прекомпилированными библиотеками под каждую платформу, что рушит
//! идею единого статического бинарника.
//!
//! Поэтому здесь нет рендера вообще: только разбор структуры (`lopdf`) и
//! извлечение потоков изображений. У этого подхода есть цена — страницы
//! с векторной вёрсткой или экзотическим сжатием не читаются, — но за
//! него плачено честно: [`open`] отказывает сразу на всём файле, если
//! хоть одна страница не подходит, а не отдаёт часть страниц молча
//! пустыми.
//!
//! # Что поддержано
//!
//! - `DCTDecode` (JPEG) — подавляющее большинство сканов
//! - `FlateDecode` и несжатые данные с `DeviceGray`/`DeviceRGB`,
//!   8 бит на канал
//!
//! Всё остальное — `JPXDecode` (JPEG2000), `CCITTFaxDecode` (факсовое
//! чёрно-белое), индексированные палитры, произвольная глубина цвета —
//! встречается в сканах манги редко, и отказ на них честнее, чем
//! попытка угадать.

use crate::{Error, Result};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageKind {
    Jpeg,
    RasterGray,
    RasterRgb,
}

/// Открытый PDF: одно изображение на страницу, в порядке страниц.
pub struct PdfPages {
    path: PathBuf,
    doc: Document,
    /// На страницу: объект картинки, как её декодировать, ширина, высота.
    pages: Vec<(ObjectId, ImageKind, u32, u32)>,
}

// `lopdf::Document` не реализует `Debug`, поэтому вывод собираем вручную
// из того, что действительно полезно посмотреть при отладке или в
// сообщениях `unwrap_err` тестов — сам разобранный документ не нужен.
impl std::fmt::Debug for PdfPages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfPages")
            .field("path", &self.path)
            .field("pages", &self.pages.len())
            .finish()
    }
}

/// Достаёт словарь по значению, которое может быть как самим словарём,
/// так и ссылкой на него — в PDF оба варианта обычны.
fn resolve_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    if let Ok(dict) = obj.as_dict() {
        return Some(dict);
    }
    let id = obj.as_reference().ok()?;
    doc.get_dictionary(id).ok()
}

/// Изображения, на которые ссылается словарь ресурсов страницы.
fn collect_xobject_images(doc: &Document, resources: &Dictionary, out: &mut Vec<ObjectId>) {
    let Ok(xobjects_obj) = resources.get(b"XObject") else {
        return;
    };
    let Some(xobjects) = resolve_dict(doc, xobjects_obj) else {
        return;
    };
    for (_, value) in xobjects.iter() {
        let Ok(id) = value.as_reference() else {
            continue;
        };
        // Изображение — это всегда поток (Object::Stream), а не голый
        // словарь: у него, помимо метаданных, есть байты. get_dictionary
        // распознаёт только Object::Dictionary и на Stream молча
        // отказывает — нужно доставать словарь именно из потока.
        let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else {
            continue;
        };
        let is_image = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name_str)
            .map(|s| s == "Image")
            .unwrap_or(false);
        if is_image {
            out.push(id);
        }
    }
}

/// Определяет, можно ли декодировать поток, и как именно.
///
/// Возвращает `None` для всего, что не входит в поддержанный список —
/// это единственное место, где решается, честна ли будет попытка чтения.
fn classify(stream: &Stream) -> Option<ImageKind> {
    let filters = stream.filters().unwrap_or_default();
    match filters.as_slice() {
        // Отсутствие Filter означает несжатые сырые сэмплы.
        [] => classify_raster(stream),
        [f] if f == "FlateDecode" => classify_raster(stream),
        [f] if f == "DCTDecode" => Some(ImageKind::Jpeg),
        // JPXDecode, CCITTFaxDecode, JBIG2Decode, цепочки из нескольких
        // фильтров — встречаются редко, и лучше честно отказать, чем
        // угадывать формат данных внутри.
        _ => None,
    }
}

fn classify_raster(stream: &Stream) -> Option<ImageKind> {
    let bpc = stream
        .dict
        .get(b"BitsPerComponent")
        .and_then(Object::as_i64)
        .unwrap_or(8);
    if bpc != 8 {
        return None;
    }
    let colorspace = stream
        .dict
        .get(b"ColorSpace")
        .and_then(Object::as_name_str)
        .unwrap_or("DeviceRGB");
    match colorspace {
        "DeviceGray" | "CalGray" => Some(ImageKind::RasterGray),
        "DeviceRGB" | "CalRGB" => Some(ImageKind::RasterRgb),
        // CMYK, индексированные палитры, ICC-профили — не поддержаны.
        _ => None,
    }
}

fn stream_dimensions(stream: &Stream) -> Option<(u32, u32)> {
    let width = stream.dict.get(b"Width").and_then(Object::as_i64).ok()?;
    let height = stream.dict.get(b"Height").and_then(Object::as_i64).ok()?;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((width as u32, height as u32))
}

impl PdfPages {
    /// Открывает PDF и проверяет, что у каждой страницы есть ровно одно
    /// подходящее изображение.
    ///
    /// Проверка сразу для всего файла, а не лениво по мере чтения:
    /// частично читаемый PDF хуже честного отказа с одним понятным
    /// сообщением, где именно страница не подошла.
    pub fn open(path: &Path) -> Result<Self> {
        let doc = Document::load(path).map_err(|e| Error::Pdf {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let page_ids = doc.get_pages();
        if page_ids.is_empty() {
            return Err(Error::NoPages(path.to_path_buf()));
        }

        let mut pages = Vec::with_capacity(page_ids.len());

        for (page_number, page_id) in &page_ids {
            let (direct, indirect_ids) = doc.get_page_resources(*page_id);

            let mut candidates = Vec::new();
            if let Some(dict) = direct {
                collect_xobject_images(&doc, dict, &mut candidates);
            }
            for id in indirect_ids {
                if let Ok(dict) = doc.get_dictionary(id) {
                    collect_xobject_images(&doc, dict, &mut candidates);
                }
            }
            candidates.sort_unstable();
            candidates.dedup();

            if candidates.is_empty() {
                return Err(Error::UnsupportedPdfPage {
                    path: path.to_path_buf(),
                    page: *page_number as usize,
                    reason: "нет изображений — похоже на векторную или текстовую страницу"
                        .to_string(),
                });
            }

            // Из нескольких картинок на странице (например, фон и штамп
            // сканера) берём самую крупную по площади: это почти всегда
            // и есть страница, а не декоративный элемент.
            let mut best: Option<(ObjectId, ImageKind, u32, u32)> = None;
            let mut saw_unsupported = false;

            for id in candidates {
                let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else {
                    continue;
                };
                let Some((width, height)) = stream_dimensions(stream) else {
                    continue;
                };
                match classify(stream) {
                    Some(kind) => {
                        let area = u64::from(width) * u64::from(height);
                        let current_area = best
                            .map(|(_, _, w, h)| u64::from(w) * u64::from(h))
                            .unwrap_or(0);
                        if area > current_area {
                            best = Some((id, kind, width, height));
                        }
                    }
                    None => saw_unsupported = true,
                }
            }

            match best {
                Some(entry) => pages.push(entry),
                None => {
                    let reason = if saw_unsupported {
                        "формат изображения не поддержан (нужен DCTDecode/JPEG \
                         либо несжатые/FlateDecode DeviceGray или DeviceRGB, 8 бит)"
                    } else {
                        "изображение без размеров — повреждённый или необычный поток"
                    };
                    return Err(Error::UnsupportedPdfPage {
                        path: path.to_path_buf(),
                        page: *page_number as usize,
                        reason: reason.to_string(),
                    });
                }
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            doc,
            pages,
        })
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Размеры страниц — уже известны из словарей `Width`/`Height`,
    /// декодировать картинки для этого не нужно.
    pub fn page_shapes(&self) -> Vec<(u32, u32)> {
        self.pages.iter().map(|(_, _, w, h)| (*w, *h)).collect()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Байты страницы, уже готовые для [`image::load_from_memory`]:
    /// JPEG как есть, растровые данные — перекодированы в PNG.
    pub fn read_page(&self, index: usize) -> Result<Vec<u8>> {
        let &(id, kind, width, height) =
            self.pages.get(index).ok_or(Error::PageOutOfRange(index))?;

        let stream = self
            .doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|e| Error::Pdf {
                path: self.path.clone(),
                message: e.to_string(),
            })?;

        if kind == ImageKind::Jpeg {
            return Ok(stream.content.clone());
        }

        let filters = stream.filters().unwrap_or_default();
        let raw: Vec<u8> = if filters.iter().any(|f| f == "FlateDecode") {
            let mut decoder = flate2::read::ZlibDecoder::new(stream.content.as_slice());
            let mut buf = Vec::new();
            decoder.read_to_end(&mut buf).map_err(|e| Error::Pdf {
                path: self.path.clone(),
                message: format!("распаковка страницы {}: {e}", index + 1),
            })?;
            buf
        } else {
            stream.content.clone()
        };

        let image = build_raster(kind, width, height, &raw).ok_or_else(|| Error::Pdf {
            path: self.path.clone(),
            message: format!(
                "страница {}: данных меньше, чем требуют размеры изображения",
                index + 1
            ),
        })?;

        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(Error::Decode)?;
        Ok(png)
    }
}

fn build_raster(
    kind: ImageKind,
    width: u32,
    height: u32,
    raw: &[u8],
) -> Option<image::DynamicImage> {
    match kind {
        ImageKind::RasterGray => {
            let needed = (width as usize).checked_mul(height as usize)?;
            let data = raw.get(..needed)?.to_vec();
            image::GrayImage::from_raw(width, height, data).map(image::DynamicImage::ImageLuma8)
        }
        ImageKind::RasterRgb => {
            let needed = (width as usize)
                .checked_mul(height as usize)?
                .checked_mul(3)?;
            let data = raw.get(..needed)?.to_vec();
            image::RgbImage::from_raw(width, height, data).map(image::DynamicImage::ImageRgb8)
        }
        ImageKind::Jpeg => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Document as LoDocument};

    /// Кодирует однотонный квадрат в JPEG — минимальный валидный поток
    /// для проверки пути DCTDecode.
    fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            w,
            h,
            image::Rgb([120, 150, 90]),
        ));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        buf
    }

    /// Собирает минимальный PDF с одной страницей на изображение.
    ///
    /// `image` — уже готовый поток: (данные, словарь дополнительных
    /// полей вроде Filter/ColorSpace/BitsPerComponent), либо `None` —
    /// тогда на странице не будет изображений вовсе (для проверки
    /// отказа на "векторных" страницах).
    struct PageSpec {
        image: Option<(Vec<u8>, Dictionary)>,
    }

    fn build_pdf(pages: Vec<PageSpec>) -> Vec<u8> {
        let mut doc = LoDocument::with_version("1.5");
        let pages_id = doc.new_object_id();

        let mut kids = Vec::new();
        for spec in pages {
            let mut resources = dictionary! {};

            let content = if let Some((data, mut image_dict)) = spec.image {
                image_dict.set("Type", "XObject");
                image_dict.set("Subtype", "Image");
                let image_id = doc.add_object(Object::Stream(Stream::new(image_dict, data)));

                let xobjects = dictionary! { "Im0" => image_id };
                resources.set("XObject", Object::Dictionary(xobjects));
                b"q /Im0 Do Q".to_vec()
            } else {
                Vec::new()
            };

            let content_id = doc.add_object(Stream::new(dictionary! {}, content));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Contents" => content_id,
            });
            kids.push(page_id.into());
        }

        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.max_id = doc.objects.keys().map(|(id, _)| *id).max().unwrap_or(0);

        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }

    fn jpeg_page(w: u32, h: u32) -> PageSpec {
        PageSpec {
            image: Some((
                jpeg_bytes(w, h),
                dictionary! {
                    "Width" => w as i64,
                    "Height" => h as i64,
                    "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8,
                    "Filter" => "DCTDecode",
                },
            )),
        }
    }

    fn flate_rgb_page(w: u32, h: u32) -> PageSpec {
        use std::io::Write;
        let raw = vec![200u8; (w * h * 3) as usize];
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        let compressed = encoder.finish().unwrap();
        PageSpec {
            image: Some((
                compressed,
                dictionary! {
                    "Width" => w as i64,
                    "Height" => h as i64,
                    "ColorSpace" => "DeviceRGB",
                    "BitsPerComponent" => 8,
                    "Filter" => "FlateDecode",
                },
            )),
        }
    }

    fn vector_page() -> PageSpec {
        PageSpec { image: None }
    }

    fn write_pdf(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("том.pdf");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn extracts_jpeg_pages_in_order() {
        let pdf = build_pdf(vec![jpeg_page(40, 30), jpeg_page(20, 60)]);
        let (_dir, path) = write_pdf(&pdf);

        let pages = PdfPages::open(&path).unwrap();
        assert_eq!(pages.page_count(), 2);
        assert_eq!(pages.page_shapes(), vec![(40, 30), (20, 60)]);

        // Байты должны быть валидным JPEG нужного размера — не просто
        // непустыми, а действительно декодируемыми.
        let bytes = pages.read_page(0).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (40, 30));
    }

    #[test]
    fn decompresses_flate_raster_into_valid_png() {
        let pdf = build_pdf(vec![flate_rgb_page(10, 8)]);
        let (_dir, path) = write_pdf(&pdf);

        let pages = PdfPages::open(&path).unwrap();
        let bytes = pages.read_page(0).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (10, 8));
        // Проверяем сам факт декодирования, а не потерянные при сборке
        // теста данные — значит путь Flate -> PNG действительно рабочий.
        assert_eq!(decoded.to_rgb8().get_pixel(0, 0).0, [200, 200, 200]);
    }

    #[test]
    fn page_without_images_is_rejected_with_its_number() {
        let pdf = build_pdf(vec![jpeg_page(10, 10), vector_page()]);
        let (_dir, path) = write_pdf(&pdf);

        let err = PdfPages::open(&path).unwrap_err();
        match err {
            Error::UnsupportedPdfPage { page, reason, .. } => {
                assert_eq!(page, 2, "должна быть указана именно вторая страница");
                assert!(reason.contains("векторную"), "{reason}");
            }
            other => panic!("ожидалась UnsupportedPdfPage, получено {other:?}"),
        }
    }

    #[test]
    fn unsupported_filter_is_rejected_honestly() {
        let mut dict = dictionary! {
            "Width" => 10i64,
            "Height" => 10i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "JPXDecode",
        };
        dict.set("Type", "XObject");
        dict.set("Subtype", "Image");
        let pdf = build_pdf(vec![PageSpec {
            image: Some((vec![0u8; 4], dict)),
        }]);
        let (_dir, path) = write_pdf(&pdf);

        let err = PdfPages::open(&path).unwrap_err();
        assert!(
            matches!(&err, Error::UnsupportedPdfPage { reason, .. } if reason.contains("не поддержан")),
            "{err:?}"
        );
    }

    #[test]
    fn one_bad_page_refuses_the_whole_file_not_just_that_page() {
        // Первая страница отличная, вторая без картинок — весь файл
        // должен быть отклонён, а не открыт частично.
        let pdf = build_pdf(vec![jpeg_page(10, 10), vector_page(), jpeg_page(10, 10)]);
        let (_dir, path) = write_pdf(&pdf);
        assert!(PdfPages::open(&path).is_err());
    }

    #[test]
    fn largest_image_on_a_page_wins_over_a_small_stamp() {
        let mut doc = LoDocument::with_version("1.5");
        let pages_id = doc.new_object_id();

        let big = jpeg_bytes(80, 80);
        let mut big_dict = dictionary! {
            "Width" => 80i64, "Height" => 80i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        };
        big_dict.set("Type", "XObject");
        big_dict.set("Subtype", "Image");
        let big_id = doc.add_object(Object::Stream(Stream::new(big_dict, big)));

        let small = jpeg_bytes(5, 5);
        let mut small_dict = dictionary! {
            "Width" => 5i64, "Height" => 5i64,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode",
        };
        small_dict.set("Type", "XObject");
        small_dict.set("Subtype", "Image");
        let small_id = doc.add_object(Object::Stream(Stream::new(small_dict, small)));

        let resources = dictionary! {
            "XObject" => dictionary! { "Big" => big_id, "Stamp" => small_id },
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, b"q /Big Do Q".to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => content_id,
        });

        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        doc.max_id = doc.objects.keys().map(|(id, _)| *id).max().unwrap_or(0);

        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        let (_dir, path) = write_pdf(&buf);

        let pages = PdfPages::open(&path).unwrap();
        assert_eq!(
            pages.page_shapes(),
            vec![(80, 80)],
            "должна победить крупная картинка"
        );
    }

    #[test]
    fn broken_file_gives_a_clear_error_not_a_panic() {
        let (_dir, path) = write_pdf(b"not a pdf at all");
        assert!(PdfPages::open(&path).is_err());
    }

    #[test]
    fn out_of_range_page_is_an_error() {
        let pdf = build_pdf(vec![jpeg_page(10, 10)]);
        let (_dir, path) = write_pdf(&pdf);
        let pages = PdfPages::open(&path).unwrap();
        assert!(matches!(pages.read_page(5), Err(Error::PageOutOfRange(5))));
    }
}
