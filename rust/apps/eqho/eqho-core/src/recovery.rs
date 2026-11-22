//! Bounded in-memory last-dictation recovery store (independent of History).

pub const DEFAULT_MAX_RECOVERY_CHARS: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct LastDictationBuffer {
    max_chars: usize,
    text: Option<String>,
}

impl Default for LastDictationBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_RECOVERY_CHARS)
    }
}

impl LastDictationBuffer {
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            text: None,
        }
    }

    pub fn store(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let stored: String = text.chars().take(self.max_chars).collect();
        self.text = Some(stored);
    }

    pub fn get(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn clear(&mut self) {
        self.text = None;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn stores_caps_and_clears() {
        let mut buf = LastDictationBuffer::new(5);
        assert_eq!(buf.get(), None);

        buf.store("hello world");
        assert_eq!(buf.get(), Some("hello"));

        buf.store("");
        assert_eq!(buf.get(), Some("hello"));

        buf.clear();
        assert_eq!(buf.get(), None);
    }
}
