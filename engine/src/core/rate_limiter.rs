use std::collections::HashMap;
use std::pin::Pin;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// Trait for rate limiting backends.
///
/// Implement this trait to plug in a custom rate limiter (e.g. Redis-backed)
/// for distributed environments where multiple server instances share state.
pub trait RateLimiter: Send + Sync + 'static {
    /// Returns `true` if the request identified by `key` is allowed, `false` if rate-limited.
    ///
    /// `max_requests` is the maximum number of requests allowed within `window_secs` seconds.
    fn check(
        &self,
        key: &str,
        max_requests: u32,
        window_secs: u64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

/// In-memory rate limiter using a per-IP token counter with sliding window.
///
/// Suitable for single-instance deployments and local testing.
/// For distributed setups (e.g. Kubernetes), implement [`RateLimiter`] with
/// a shared backend like Redis.
pub struct InMemoryRateLimiter {
    store: Mutex<HashMap<String, (u32, Instant)>>,
}

impl InMemoryRateLimiter {
    /// Creates a new empty in-memory rate limiter.
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter for InMemoryRateLimiter {
    fn check(
        &self,
        key: &str,
        max_requests: u32,
        window_secs: u64,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        let key = key.to_string();
        Box::pin(async move {
            let now = Instant::now();
            let window = Duration::from_secs(window_secs);

            let mut map = self.store.lock().await;
            let entry = map.entry(key).or_insert((0, now));

            if now.duration_since(entry.1) > window {
                *entry = (1, now);
                return true;
            }

            entry.0 += 1;
            let allowed = entry.0 <= max_requests;

            if map.len() > 10_000 {
                map.retain(|_, (_, ts)| now.duration_since(*ts) <= window);
            }

            allowed
        })
    }
}
