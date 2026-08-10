//! Ограничитель частоты запросов.
//!
//! У MangaDex общий предел — пять запросов в секунду, а у отдельных
//! эндпоинтов свои, более строгие. Превышение даёт ответ 429, а
//! настойчивое превышение — бан по адресу. Страдает при этом не только
//! наш пользователь: страдает репутация проекта в глазах сервиса,
//! который раздаёт данные бесплатно.
//!
//! Реализован простейший вариант — минимальный интервал между запросами.
//! Скользящее окно пропускало бы всплески, но всплеск и есть то, за что
//! банят; ровный поток безопаснее и проще для понимания.

use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

pub struct RateLimiter {
    interval: Duration,
    /// Момент, раньше которого следующий запрос отправлять нельзя.
    next_allowed: Mutex<Option<Instant>>,
}

impl RateLimiter {
    /// `per_second` — сколько запросов в секунду допустимо.
    pub fn per_second(requests: u32) -> Self {
        let requests = requests.max(1);
        Self {
            interval: Duration::from_micros(1_000_000 / requests as u64),
            next_allowed: Mutex::new(None),
        }
    }

    /// Ждёт, пока отправлять запрос станет можно.
    ///
    /// Блокировка держится всё время ожидания намеренно: так очередь
    /// запросов выстраивается по одному, а не просыпается разом.
    pub async fn acquire(&self) {
        let mut next = self.next_allowed.lock().await;
        let now = Instant::now();

        let allowed_at = match *next {
            Some(t) if t > now => {
                let wait = t - now;
                tracing::trace!(?wait, "жду, чтобы не превысить лимит");
                tokio::time::sleep(wait).await;
                t
            }
            _ => now,
        };

        *next = Some(allowed_at + self.interval);
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn first_request_goes_through_immediately() {
        let limiter = RateLimiter::per_second(5);
        let start = Instant::now();
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn subsequent_requests_are_spaced_out() {
        let limiter = RateLimiter::per_second(5); // 200 мс между запросами
        let start = Instant::now();

        for _ in 0..5 {
            limiter.acquire().await;
        }

        // Пять запросов — четыре промежутка по 200 мс.
        assert_eq!(start.elapsed(), Duration::from_millis(800));
    }

    #[tokio::test(start_paused = true)]
    async fn pause_between_calls_is_not_wasted() {
        let limiter = RateLimiter::per_second(5);
        limiter.acquire().await;

        // Пользователь думал целую секунду — ждать сверх этого не нужно.
        tokio::time::sleep(Duration::from_secs(1)).await;

        let start = Instant::now();
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[test]
    fn interval_matches_requested_rate() {
        assert_eq!(
            RateLimiter::per_second(5).interval(),
            Duration::from_millis(200)
        );
        assert_eq!(
            RateLimiter::per_second(1).interval(),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn zero_rate_is_treated_as_one_per_second() {
        // Ноль запросов в секунду означал бы деление на ноль и вечное
        // ожидание; выбираем самый осторожный разумный вариант.
        assert_eq!(
            RateLimiter::per_second(0).interval(),
            Duration::from_secs(1)
        );
    }
}
