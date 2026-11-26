//! Model metadata, GGML filename & HuggingFace URL mapping, and memory checks.

use std::path::{Path, PathBuf};

pub const LOW_MEMORY_LOAD_TEXT: &str = "Not enough free memory to load the speech model. Close some programs, or pick a smaller model in Eqho → Models.";

const MEMORY_ERROR_MARKERS: [&str; 5] = [
    "memoryerror",
    "mkl_malloc",
    "out of memory",
    "bad_alloc",
    "failed to allocate",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub size_mb: u64,
    pub ggml_name: &'static str,
    pub ggml_filename: &'static str,
    pub download_url: &'static str,
    pub english_only: bool,
    pub quantized: bool,
}

pub const MODELS: &[ModelInfo] = &[
    // English-optimized (4 — clean 2x2 grid)
    ModelInfo {
        key: "distil-large-v3",
        label: "Distil Large v3 (~1.5 GB, English-optimized)",
        size_mb: 1510,
        ggml_name: "large-v3-turbo",
        ggml_filename: "ggml-large-v3-turbo.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        english_only: true,
        quantized: false,
    },
    ModelInfo {
        key: "distil-medium.en",
        label: "Distil Medium EN (~750 MB, fast English)",
        size_mb: 750,
        ggml_name: "medium.en",
        ggml_filename: "ggml-medium.en.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin",
        english_only: true,
        quantized: false,
    },
    ModelInfo {
        key: "distil-small.en",
        label: "Distil Small EN (~330 MB, fast English)",
        size_mb: 330,
        ggml_name: "small.en",
        ggml_filename: "ggml-small.en.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        english_only: true,
        quantized: false,
    },
    ModelInfo {
        key: "small.en-q5",
        label: "Small EN Q5 (~190 MB, CPU-fast quantized English)",
        size_mb: 190,
        ggml_name: "small.en-q5_1",
        ggml_filename: "ggml-small.en-q5_1.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q5_1.bin",
        english_only: true,
        quantized: true,
    },
    // Multilingual (9)
    ModelInfo {
        key: "large-v3-turbo",
        label: "Large v3 Turbo (~1.6 GB, multilingual, near-large accuracy)",
        size_mb: 1620,
        ggml_name: "large-v3-turbo",
        ggml_filename: "ggml-large-v3-turbo.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        key: "large-v3-turbo-q5",
        label: "Large v3 Turbo Q5 (~574 MB, quantized fast, low RAM/VRAM)",
        size_mb: 574,
        ggml_name: "large-v3-turbo-q5_0",
        ggml_filename: "ggml-large-v3-turbo-q5_0.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        key: "medium",
        label: "Medium (~1.5 GB, multilingual)",
        size_mb: 1530,
        ggml_name: "medium",
        ggml_filename: "ggml-medium.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        key: "small",
        label: "Small (~950 MB, multilingual)",
        size_mb: 970,
        ggml_name: "small",
        ggml_filename: "ggml-small.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        key: "small-q5",
        label: "Small Q5 (~190 MB, CPU-fast quantized multilingual)",
        size_mb: 190,
        ggml_name: "small-q5_1",
        ggml_filename: "ggml-small-q5_1.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        key: "base",
        label: "Base (~300 MB, multilingual)",
        size_mb: 290,
        ggml_name: "base",
        ggml_filename: "ggml-base.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        key: "base-q5",
        label: "Base Q5 (~60 MB, ultra-lightweight CPU)",
        size_mb: 60,
        ggml_name: "base-q5_1",
        ggml_filename: "ggml-base-q5_1.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
        english_only: false,
        quantized: true,
    },
    ModelInfo {
        key: "tiny",
        label: "Tiny (~150 MB, fastest, least accurate)",
        size_mb: 150,
        ggml_name: "tiny",
        ggml_filename: "ggml-tiny.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        english_only: false,
        quantized: false,
    },
    ModelInfo {
        key: "large-v3",
        label: "Large v3 (~3.1 GB, highest accuracy)",
        size_mb: 3090,
        ggml_name: "large-v3",
        ggml_filename: "ggml-large-v3.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        english_only: false,
        quantized: false,
    },
];

pub const MODEL_SIZES_MB: [(&str, u64); 13] = [
    ("distil-large-v3", 1510),
    ("distil-medium.en", 750),
    ("distil-small.en", 330),
    ("small.en-q5", 190),
    ("large-v3-turbo", 1620),
    ("large-v3-turbo-q5", 574),
    ("medium", 1530),
    ("small", 970),
    ("small-q5", 190),
    ("base", 290),
    ("base-q5", 60),
    ("tiny", 150),
    ("large-v3", 3090),
];

pub fn model_info(model_key: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.key == model_key)
}

impl ModelInfo {
    /// Human-browsable Hugging Face model file page (`blob/main/...`) so clicking opens
    /// a web page with file details/checksums rather than triggering a blind binary download.
    #[must_use]
    pub fn browse_url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/blob/main/{}",
            self.ggml_filename
        )
    }
}

pub fn whispercpp_name(model_key: &str) -> &'static str {
    model_info(model_key)
        .map(|m| m.ggml_name)
        .unwrap_or("base")
}

pub fn whispercpp_models_dir(model_dir: &Path) -> PathBuf {
    model_dir.join("whispercpp")
}

pub fn whispercpp_model_path(model_dir: &Path, model_key: &str) -> PathBuf {
    let name = whispercpp_name(model_key);
    whispercpp_models_dir(model_dir).join(format!("ggml-{name}.bin"))
}

pub fn required_memory_mb(model_key: &str) -> u64 {
    let size = model_info(model_key).map(|m| m.size_mb).unwrap_or(1000);
    (size as f64 * 1.3 + 300.0).round() as u64
}

pub fn low_memory_warning(free_mb: u64, need_mb: u64) -> String {
    format!(
        "Your computer is short on free memory (about {:.1} GB free; this speech model needs about {:.1} GB). Close some programs, or pick a smaller model in Eqho → Models.",
        free_mb as f64 / 1024.0,
        need_mb as f64 / 1024.0
    )
}

pub fn looks_like_memory_error(text: &str) -> bool {
    let low = text.to_lowercase();
    MEMORY_ERROR_MARKERS.iter().any(|m| low.contains(m))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn model_table_and_paths() {
        assert_eq!(MODELS.len(), 13);
        let dir = Path::new("/tmp/models");
        assert_eq!(
            whispercpp_model_path(dir, "distil-large-v3"),
            PathBuf::from("/tmp/models/whispercpp/ggml-large-v3-turbo.bin")
        );
        assert_eq!(
            whispercpp_model_path(dir, "large-v3-turbo-q5"),
            PathBuf::from("/tmp/models/whispercpp/ggml-large-v3-turbo-q5_0.bin")
        );
        assert_eq!(
            whispercpp_model_path(dir, "small.en-q5"),
            PathBuf::from("/tmp/models/whispercpp/ggml-small.en-q5_1.bin")
        );
        assert_eq!(
            whispercpp_model_path(dir, "unknown-model"),
            PathBuf::from("/tmp/models/whispercpp/ggml-base.bin")
        );
        assert_eq!(
            model_info("tiny").unwrap().download_url,
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
        );
        assert_eq!(
            model_info("distil-large-v3").unwrap().browse_url(),
            "https://huggingface.co/ggerganov/whisper.cpp/blob/main/ggml-large-v3-turbo.bin"
        );
    }

    #[test]
    fn memory_checks() {
        assert_eq!(required_memory_mb("tiny"), 495);
        assert!(looks_like_memory_error("std::bad_alloc in whisper"));
        assert!(!looks_like_memory_error("network timeout"));
        let msg = low_memory_warning(1024, 2048);
        assert!(msg.contains("1.0 GB free"));
        assert!(msg.contains("2.0 GB"));
    }
}
