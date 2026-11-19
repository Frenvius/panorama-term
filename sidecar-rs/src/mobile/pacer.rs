use tokio::time::{Duration, Instant};

pub struct Pacer {
    rate: f64,
    burst: f64,
    tokens: f64,
    updated: Instant,
}
impl Pacer {
    pub fn new(rate_bytes_per_sec: usize, burst_bytes: usize) -> Self {
        assert!(rate_bytes_per_sec > 0 && burst_bytes > 0);
        Self {
            rate: rate_bytes_per_sec as f64,
            burst: burst_bytes as f64,
            tokens: burst_bytes as f64,
            updated: Instant::now(),
        }
    }
    pub async fn take(&mut self, n: usize) {
        let mut remaining = n as f64;
        while remaining > 0.0 {
            let now = Instant::now();
            self.tokens = (self.tokens
                + now.duration_since(self.updated).as_secs_f64() * self.rate)
                .min(self.burst);
            self.updated = now;
            let taken = self.tokens.min(remaining);
            self.tokens -= taken;
            remaining -= taken;
            if remaining > 0.0 {
                tokio::time::sleep(Duration::from_secs_f64(
                    remaining.min(self.burst) / self.rate,
                ))
                .await;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    #[tokio::test(start_paused = true)]
    async fn burst_and_refill() {
        let mut p = super::Pacer::new(100, 200);
        let start = tokio::time::Instant::now();
        p.take(200).await;
        assert_eq!(start.elapsed(), std::time::Duration::ZERO);
        p.take(300).await;
        assert_eq!(start.elapsed(), std::time::Duration::from_secs(3));
    }
}
