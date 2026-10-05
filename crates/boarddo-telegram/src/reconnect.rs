use std::time::Duration;

/// Finite reconnect schedule. After the last delay the account pauses in Error.
pub struct ReconnectBackoff {
    step: usize,
}

impl ReconnectBackoff {
    pub const DELAYS_MS: [u64; 6] = [1_000, 2_000, 5_000, 10_000, 30_000, 60_000];

    pub fn new() -> Self {
        Self { step: 0 }
    }

    pub fn next_delay(&mut self) -> Option<Duration> {
        if self.step >= Self::DELAYS_MS.len() {
            return None;
        }
        let ms = Self::DELAYS_MS[self.step];
        self.step += 1;
        Some(Duration::from_millis(ms))
    }

    pub fn reset(&mut self) {
        self.step = 0;
    }
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self::new()
    }
}
