//! Политика повторов.
//!
//! Логика «стоит ли повторять и через сколько» вынесена в чистые функции:
//! без этого проверить её можно было бы только настоящими сбоями сети,
//! то есть никак.

use std::time::Duration;

/// Что делать с полученным ответом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Всё хорошо.
    Accept,
    /// Повторить через указанное время.
    RetryAfter(Duration),
    /// Повторять бессмысленно.
    GiveUp,
}

/// Базовая задержка; удваивается с каждой попыткой.
const BASE_DELAY: Duration = Duration::from_millis(500);
/// Больше этого не ждём: пользователь сидит перед терминалом.
const MAX_DELAY: Duration = Duration::from_secs(30);

/// Экспоненциальная выдержка: 0.5с, 1с, 2с, 4с...
pub fn backoff(attempt: u32) -> Duration {
    let factor = 2u32.saturating_pow(attempt.min(16));
    BASE_DELAY.saturating_mul(factor).min(MAX_DELAY)
}

/// Решает по коду ответа, что делать дальше.
///
/// `retry_after` — значение одноимённого заголовка в секундах, если он
/// пришёл. Сервер лучше нас знает, сколько ждать, поэтому его указание
/// перекрывает нашу выдержку.
pub fn decide(status: u16, attempt: u32, max_attempts: u32, retry_after: Option<u64>) -> Decision {
    if (200..300).contains(&status) {
        return Decision::Accept;
    }

    let retryable = match status {
        // Превышен лимит частоты — самый ожидаемый случай.
        429 => true,
        // Временные неполадки на стороне сервиса.
        500 | 502 | 503 | 504 => true,
        // Остальное повторять бесполезно: 404 не станет 200,
        // а 403 означает, что мы делаем что-то не то.
        _ => false,
    };

    if !retryable || attempt + 1 >= max_attempts {
        return Decision::GiveUp;
    }

    let delay = match retry_after {
        Some(secs) => Duration::from_secs(secs).min(MAX_DELAY),
        None => backoff(attempt),
    };
    Decision::RetryAfter(delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_is_accepted() {
        assert_eq!(decide(200, 0, 3, None), Decision::Accept);
        assert_eq!(decide(204, 0, 3, None), Decision::Accept);
    }

    #[test]
    fn rate_limit_is_retried() {
        assert!(matches!(decide(429, 0, 3, None), Decision::RetryAfter(_)));
    }

    #[test]
    fn server_errors_are_retried() {
        for status in [500, 502, 503, 504] {
            assert!(
                matches!(decide(status, 0, 3, None), Decision::RetryAfter(_)),
                "{status} должен повторяться"
            );
        }
    }

    #[test]
    fn client_errors_are_not_retried() {
        // 404 не станет 200 от повторения, а 403 значит, что мы
        // делаем что-то неправильно, и надо разбираться, а не долбить.
        for status in [400, 401, 403, 404] {
            assert_eq!(decide(status, 0, 3, None), Decision::GiveUp, "{status}");
        }
    }

    #[test]
    fn last_attempt_gives_up_even_on_retryable_status() {
        assert_eq!(decide(429, 2, 3, None), Decision::GiveUp);
    }

    #[test]
    fn server_supplied_delay_wins_over_our_backoff() {
        assert_eq!(
            decide(429, 0, 3, Some(7)),
            Decision::RetryAfter(Duration::from_secs(7))
        );
    }

    #[test]
    fn absurd_retry_after_is_capped() {
        // Сервер может попросить подождать час — столько мы ждать
        // не будем, человек сидит перед терминалом.
        assert_eq!(
            decide(429, 0, 3, Some(3600)),
            Decision::RetryAfter(MAX_DELAY)
        );
    }

    #[test]
    fn backoff_grows_and_then_stops_growing() {
        assert_eq!(backoff(0), Duration::from_millis(500));
        assert_eq!(backoff(1), Duration::from_secs(1));
        assert_eq!(backoff(2), Duration::from_secs(2));
        assert_eq!(backoff(20), MAX_DELAY, "рост должен упираться в потолок");
    }
}
