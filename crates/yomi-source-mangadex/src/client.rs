//! HTTP-клиент к API MangaDex: лимиты, повторы, отчёты о загрузках.

use crate::rate_limit::RateLimiter;
use crate::retry::{decide, Decision};
use serde::de::DeserializeOwned;
use std::time::Duration;
use yomi_core::{Error, Result};

pub const API_BASE: &str = "https://api.mangadex.org";
/// Отчёты о загрузках идут на другой домен — это не опечатка,
/// документация специально предупреждает не перепутать.
pub const REPORT_URL: &str = "https://api.mangadex.network/report";

/// Общий предел сервиса — пять запросов в секунду.
const REQUESTS_PER_SECOND: u32 = 5;
const MAX_ATTEMPTS: u32 = 4;
/// Отчёт — служебная мелочь, ждать его дольше нескольких секунд незачем.
const REPORT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct MangaDexClient {
    http: reqwest::Client,
    limiter: RateLimiter,
}

impl MangaDexClient {
    pub fn new(user_agent: &str, timeout: Duration) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(timeout)
            .build()
            .map_err(|e| Error::Network(format!("не удалось создать HTTP-клиент: {e}")))?;

        Ok(Self {
            http,
            limiter: RateLimiter::per_second(REQUESTS_PER_SECOND),
        })
    }

    /// Запрос к API с разбором JSON, соблюдением лимита и повторами.
    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let body = self.get_text(url).await?;
        serde_json::from_str(&body).map_err(|e| {
            // Ошибка разбора почти всегда означает, что сервис поменял
            // формат: сообщение должно вести к причине, а не к «ошибке».
            Error::BadResponse(format!("{url}: {e}"))
        })
    }

    async fn get_text(&self, url: &str) -> Result<String> {
        let mut attempt = 0;

        loop {
            self.limiter.acquire().await;
            tracing::debug!(url, attempt, "запрос к API");

            let response = self.http.get(url).send().await;

            let (status, retry_after, body) = match response {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok());
                    let body = resp.text().await.unwrap_or_default();
                    (status, retry_after, body)
                }
                Err(e) => {
                    // Сетевой сбой — повторяем на тех же основаниях,
                    // что и ошибку сервера.
                    if attempt + 1 >= MAX_ATTEMPTS {
                        return Err(Error::Network(e.to_string()));
                    }
                    let wait = crate::retry::backoff(attempt);
                    tracing::warn!(url, ?wait, error = %e, "сбой запроса, повторю");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                    continue;
                }
            };

            match decide(status, attempt, MAX_ATTEMPTS, retry_after) {
                Decision::Accept => return Ok(body),
                Decision::RetryAfter(wait) => {
                    tracing::warn!(url, status, ?wait, "повтор запроса");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                Decision::GiveUp => {
                    return Err(match status {
                        404 => Error::NotFound(url.to_string()),
                        // 429 после всех повторов — это тоже сетевая
                        // проблема с точки зрения пользователя.
                        _ => Error::Network(format!("{url} ответил кодом {status}")),
                    });
                }
            }
        }
    }

    /// Скачивает страницу и отчитывается о результате.
    ///
    /// Отчёт обязателен по условиям пользования сетью MangaDex@Home:
    /// по нему отслеживается здоровье узлов. Отправляется и при успехе,
    /// и при сбое, но только для сторонних узлов — для адресов самого
    /// mangadex.org отчёт не нужен.
    pub async fn download_page(&self, url: &str) -> Result<Vec<u8>> {
        let started = std::time::Instant::now();
        // На картинки нельзя слать заголовки авторизации: при обращении
        // к стороннему узлу это утечка токена третьей стороне.
        let response = self.http.get(url).send().await;

        let (bytes, success, cached) = match response {
            Ok(resp) => {
                let cached = resp
                    .headers()
                    .get("x-cache")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.starts_with("HIT"))
                    .unwrap_or(false);
                let ok = resp.status().is_success();
                let bytes = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
                (bytes, ok, cached)
            }
            Err(_) => (Vec::new(), false, false),
        };

        self.report(url, success, cached, bytes.len(), started.elapsed());

        tracing::debug!(
            url,
            bytes = bytes.len(),
            ms = started.elapsed().as_millis() as u64,
            cached,
            "страница загружена"
        );

        if success && !bytes.is_empty() {
            Ok(bytes)
        } else {
            Err(Error::NotFound(format!(
                "не удалось скачать страницу {url}"
            )))
        }
    }

    /// Отправляет отчёт о загрузке в фоне.
    ///
    /// Раньше отчёт ожидался перед выдачей страницы, и каждая картинка
    /// стоила лишнего запроса к другому домену — с общим тайм-аутом
    /// в тридцать секунд. На практике это давало около минуты на
    /// несколько страниц.
    ///
    /// Отчёт обязателен по условиям пользования сетью, но пользователю
    /// незачем его дожидаться: задача уходит в фон со своим коротким
    /// тайм-аутом.
    fn report(&self, url: &str, success: bool, cached: bool, bytes: usize, duration: Duration) {
        if url.contains("mangadex.org") {
            return;
        }
        let payload = serde_json::json!({
            "url": url,
            "success": success,
            "cached": cached,
            "bytes": bytes,
            "duration": duration.as_millis() as u64,
        });

        // Клонирование клиента дёшево: внутри общий пул соединений.
        let http = self.http.clone();
        tokio::spawn(async move {
            let request = http.post(REPORT_URL).json(&payload).send();
            match tokio::time::timeout(REPORT_TIMEOUT, request).await {
                Ok(Err(e)) => tracing::debug!(error = %e, "отчёт о загрузке не отправлен"),
                Err(_) => tracing::debug!("отчёт о загрузке не уложился в тайм-аут"),
                Ok(Ok(_)) => {}
            }
        });
    }
}
