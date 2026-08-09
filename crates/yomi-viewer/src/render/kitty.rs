//! Кодирование изображения в escape-последовательности графического
//! протокола kitty.
//!
//! Формат команды: `ESC _G <ключи через запятую> ; <данные-base64> ESC \`.
//! Спецификация: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
//!
//! Ключевое ограничение протокола: полезная нагрузка одного чанка не может
//! превышать 4096 байт base64-текста, поэтому большая картинка режется на
//! несколько последовательностей, соединённых ключом `m=1` (есть продолжение)
//! и завершённых `m=0` (последний чанк).
//!
//! Проверить правильность формирования байтов можно и без живого терминала:
//! декодировать base64 обратно и свериться с оригиналом, посчитать чанки.
//! Увидеть картинку — уже вопрос реального запуска в kitty-совместимом
//! терминале.

use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Максимальный размер payload одного чанка по спецификации протокола.
const CHUNK_SIZE: usize = 4096;

/// Собирает последовательность escape-команд, показывающих PNG-картинку
/// протоколом kitty. `png_bytes` — уже закодированный в PNG буфер:
/// формат `f=100` требует именно PNG, а не сырые пиксели.
pub fn encode_png(png_bytes: &[u8], cols: u16, rows: u16) -> String {
    let encoded = STANDARD.encode(png_bytes);
    let chunks: Vec<&[u8]> = encoded.as_bytes().chunks(CHUNK_SIZE).collect();

    let mut out = String::new();
    let total = chunks.len();

    for (i, chunk) in chunks.iter().enumerate() {
        let is_first = i == 0;
        let is_last = i + 1 == total;
        let more = if is_last { 0 } else { 1 };

        out.push_str("\x1b_G");
        if is_first {
            // a=T: transmit and display. f=100: данные в формате PNG.
            // c/r: сколько ячеек терминала должна занять картинка —
            // масштабирование делает сам терминал.
            out.push_str(&format!("a=T,f=100,c={cols},r={rows},m={more}"));
        } else {
            out.push_str(&format!("m={more}"));
        }
        out.push(';');
        // SAFETY-нет: chunk — срез валидных ASCII-байт base64-алфавита,
        // from_utf8 здесь не может провалиться.
        out.push_str(std::str::from_utf8(chunk).expect("base64 всегда ASCII"));
        out.push_str("\x1b\\");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_payload_produces_single_chunk() {
        let seq = encode_png(b"testovye baity png", 10, 20);
        assert_eq!(seq.matches("\x1b_G").count(), 1);
        assert!(
            seq.contains("m=0"),
            "единственный чанк должен быть помечен как последний"
        );
    }

    #[test]
    fn first_chunk_carries_display_keys() {
        let seq = encode_png(b"data", 40, 20);
        assert!(seq.starts_with("\x1b_Ga=T,f=100,c=40,r=20,m=0;"));
    }

    #[test]
    fn every_command_is_properly_terminated() {
        let seq = encode_png(b"data", 1, 1);
        assert!(seq.ends_with("\x1b\\"));
    }

    #[test]
    fn large_payload_splits_into_multiple_chunks_with_continuation() {
        // Достаточно байт, чтобы после base64 (примерно x4/3) выйти
        // за пределы одного чанка в 4096 символов.
        let big = vec![0xABu8; 6000];
        let seq = encode_png(&big, 5, 5);
        let commands = seq.matches("\x1b_G").count();
        assert!(
            commands >= 2,
            "большая картинка должна резаться на несколько команд"
        );
        assert!(seq.contains("m=1"), "не последний чанк должен нести m=1");
        assert!(seq.contains("m=0"), "последний чанк должен нести m=0");
    }

    #[test]
    fn base64_payload_round_trips_to_original_bytes() {
        let original = b"proverka bytes 0123456789 png stub";
        let seq = encode_png(original, 3, 3);

        // Вырезаем всё содержимое между `;` и завершающим ESC\, склеиваем
        // чанки обратно и убеждаемся, что декодированные байты совпадают.
        let mut payload = String::new();
        for part in seq.split("\x1b_G").skip(1) {
            let data = part.split(';').nth(1).unwrap();
            let data = data.trim_end_matches("\x1b\\");
            payload.push_str(data);
        }
        let decoded = STANDARD.decode(payload).unwrap();
        assert_eq!(decoded, original);
    }
}
