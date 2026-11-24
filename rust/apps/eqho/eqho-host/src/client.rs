//! Parent-process [`ModelHost`] handle with inactivity-based load timeout,
//! transcribe timeout, kill-on-switch/kill-on-timeout, and truthful error
//! propagation.

use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_core::modelstore::looks_like_memory_error;

use crate::error::ModelHostError;
use crate::protocol::{self, HostRequest, HostResponse, HostSegment};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Configuration for spawning and supervising the out-of-process model host.
#[derive(Debug, Clone)]
pub struct ModelHostConfig {
    pub child_exe: PathBuf,
    pub child_args: Vec<String>,
    pub dll_dir: PathBuf,
    pub load_inactivity_timeout: Duration,
    pub transcribe_timeout: Duration,
}

impl ModelHostConfig {
    /// Create a configuration with `["--model-host"]` and 120 s default timeouts.
    pub fn new(child_exe: PathBuf, dll_dir: PathBuf) -> Self {
        Self {
            child_exe,
            child_args: vec!["--model-host".into()],
            dll_dir,
            load_inactivity_timeout: DEFAULT_TIMEOUT,
            transcribe_timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// Metadata reported by the child process once a model finishes loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub backend: String,
    pub model: String,
    pub device: String,
    pub whisper_version: String,
}

#[derive(Debug)]
struct ChildSession {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    rx: Receiver<Result<HostResponse, String>>,
    reader_thread: Option<JoinHandle<()>>,
}

impl ChildSession {
    fn spawn(config: &ModelHostConfig) -> Result<Self, ModelHostError> {
        let mut cmd = Command::new(&config.child_exe);
        cmd.args(&config.child_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| ModelHostError::SpawnFailed(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ModelHostError::SpawnFailed("child stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ModelHostError::SpawnFailed("child stdout unavailable".into()))?;

        let (tx, rx) = mpsc::channel();
        let reader_thread = thread::Builder::new()
            .name("eqho-host-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    match protocol::read_response(&mut reader) {
                        Ok(Some(resp)) => {
                            if tx.send(Ok(resp)).is_err() {
                                break;
                            }
                        }
                        Ok(None) => {
                            let _ = tx.send(Err("model host process exited (EOF)".into()));
                            break;
                        }
                        Err(err) => {
                            let _ = tx.send(Err(err.to_string()));
                            break;
                        }
                    }
                }
            })
            .map_err(|e| ModelHostError::SpawnFailed(format!("reader thread spawn failed: {e}")))?;

        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            rx,
            reader_thread: Some(reader_thread),
        })
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn kill_and_reap(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        drop(self.stdin);
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Parent-process handle to the single-model-per-process `eqho-host` child.
#[derive(Debug)]
pub struct ModelHost {
    config: ModelHostConfig,
    session: Option<ChildSession>,
    current_model: Option<String>,
    current_use_gpu: bool,
    info: Option<ModelInfo>,
    last_load_error: Option<String>,
}

impl Drop for ModelHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl ModelHost {
    /// Construct a new [`ModelHost`] handle without spawning a child yet.
    pub fn new(config: ModelHostConfig) -> Self {
        Self {
            config,
            session: None,
            current_model: None,
            current_use_gpu: false,
            info: None,
            last_load_error: None,
        }
    }

    /// Return the error message from the most recent failed [`Self::load_model`], if any.
    pub fn last_load_error(&self) -> Option<&str> {
        self.last_load_error.as_deref()
    }

    /// Return metadata for the currently loaded model, if ready.
    pub fn info(&self) -> Option<&ModelInfo> {
        self.info.as_ref()
    }

    /// Return `true` if `model_key` with `use_gpu` is currently loaded in a live child.
    pub fn is_ready(&mut self, model_key: &str, use_gpu: bool) -> bool {
        if self.current_model.as_deref() != Some(model_key) || self.current_use_gpu != use_gpu {
            return false;
        }
        let Some(sess) = self.session.as_mut() else {
            return false;
        };
        if sess.is_alive() {
            true
        } else {
            self.kill_child();
            false
        }
    }

    fn kill_child(&mut self) {
        if let Some(sess) = self.session.take() {
            sess.kill_and_reap();
        }
        self.current_model = None;
        self.info = None;
    }

    /// Ensure `model_key` (with `use_gpu`) is loaded in a fresh child process.
    ///
    /// Switching models or changing `use_gpu` kills any existing child first.
    /// The wait is inactivity-based: every `Progress` frame from the child
    /// resets the `load_inactivity_timeout` deadline.
    pub fn load_model(
        &mut self,
        model_key: &str,
        model_dir: &Path,
        use_gpu: bool,
        threads: u32,
        on_progress: &mut dyn FnMut(&str, Option<f32>),
    ) -> Result<ModelInfo, ModelHostError> {
        if self.is_ready(model_key, use_gpu)
            && let Some(info) = self.info.clone()
        {
            self.last_load_error = None;
            return Ok(info);
        }

        self.last_load_error = None;
        self.kill_child();

        let mut sess = ChildSession::spawn(&self.config)?;
        let req = HostRequest::Load {
            model_key: model_key.to_owned(),
            model_dir: model_dir.to_string_lossy().into_owned(),
            dll_dir: self.config.dll_dir.to_string_lossy().into_owned(),
            use_gpu,
            threads,
        };
        if let Err(err) = protocol::write_request(&mut sess.stdin, &req, None) {
            let msg = err.to_string();
            sess.kill_and_reap();
            self.last_load_error = Some(msg.clone());
            return Err(ModelHostError::LoadFailed(msg));
        }

        let mut deadline = Instant::now() + self.config.load_inactivity_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let msg = "no progress from model host before load inactivity timeout".to_owned();
                sess.kill_and_reap();
                self.last_load_error = Some(msg.clone());
                return Err(ModelHostError::Timeout(msg));
            }

            match sess.rx.recv_timeout(remaining) {
                Ok(Ok(HostResponse::Progress { message, fraction })) => {
                    on_progress(&message, fraction);
                    deadline = Instant::now() + self.config.load_inactivity_timeout;
                }
                Ok(Ok(HostResponse::Ready {
                    backend,
                    model,
                    device,
                    whisper_version,
                })) => {
                    let info = ModelInfo {
                        backend,
                        model,
                        device,
                        whisper_version,
                    };
                    self.session = Some(sess);
                    self.current_model = Some(model_key.to_owned());
                    self.current_use_gpu = use_gpu;
                    self.info = Some(info.clone());
                    self.last_load_error = None;
                    return Ok(info);
                }
                Ok(Ok(HostResponse::Error { message, is_oom })) => {
                    sess.kill_and_reap();
                    self.last_load_error = Some(message.clone());
                    return Err(classify_load_error(message, is_oom));
                }
                Ok(Ok(other)) => {
                    let msg = format!("unexpected response during load: {other:?}");
                    sess.kill_and_reap();
                    self.last_load_error = Some(msg.clone());
                    return Err(ModelHostError::LoadFailed(msg));
                }
                Ok(Err(err_msg)) => {
                    sess.kill_and_reap();
                    self.last_load_error = Some(err_msg.clone());
                    return Err(classify_load_error(err_msg, false));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let msg = "no progress from model host before load inactivity timeout".to_owned();
                    sess.kill_and_reap();
                    self.last_load_error = Some(msg.clone());
                    return Err(ModelHostError::Timeout(msg));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let msg = "model host reader disconnected during load".to_owned();
                    sess.kill_and_reap();
                    self.last_load_error = Some(msg.clone());
                    return Err(ModelHostError::LoadFailed(msg));
                }
            }
        }
    }

    /// Transcribe mono 16 kHz `f32` PCM audio in the active child process.
    ///
    /// On timeout, EOF, or child error, the child is immediately killed and
    /// reaped so no stale reply can ever be read by a subsequent request.
    pub fn transcribe(
        &mut self,
        audio: &[f32],
        language: &str,
        beam_size: u32,
        initial_prompt: &str,
    ) -> Result<Vec<HostSegment>, ModelHostError> {
        let Some(sess) = self.session.as_mut() else {
            return Err(ModelHostError::NotReady);
        };
        if !sess.is_alive() {
            self.kill_child();
            return Err(ModelHostError::NotReady);
        }

        let req = HostRequest::Transcribe {
            sample_count: audio.len(),
            language: language.to_owned(),
            beam_size,
            initial_prompt: initial_prompt.to_owned(),
        };

        if let Err(err) = protocol::write_request(&mut sess.stdin, &req, Some(audio)) {
            self.kill_child();
            return Err(ModelHostError::Protocol(err.to_string()));
        }

        match sess.rx.recv_timeout(self.config.transcribe_timeout) {
            Ok(Ok(HostResponse::Segments { segments })) => Ok(segments),
            Ok(Ok(HostResponse::Error { message, is_oom })) => {
                self.kill_child();
                if is_oom || looks_like_memory_error(&message) {
                    Err(ModelHostError::LowMemory(message))
                } else {
                    Err(ModelHostError::TranscribeFailed(message))
                }
            }
            Ok(Ok(other)) => {
                self.kill_child();
                Err(ModelHostError::Protocol(format!(
                    "unexpected response during transcribe: {other:?}"
                )))
            }
            Ok(Err(err_msg)) => {
                self.kill_child();
                Err(ModelHostError::TranscribeFailed(err_msg))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.kill_child();
                Err(ModelHostError::Timeout(
                    "transcribe timed out waiting for model host".into(),
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.kill_child();
                Err(ModelHostError::TranscribeFailed(
                    "model host reader disconnected".into(),
                ))
            }
        }
    }

    /// Gracefully ask the child to shut down, then kill and reap it.
    pub fn shutdown(&mut self) {
        if let Some(sess) = self.session.as_mut() {
            let _ = protocol::write_request(&mut sess.stdin, &HostRequest::Shutdown, None);
        }
        self.kill_child();
    }
}

fn classify_load_error(message: String, is_oom: bool) -> ModelHostError {
    if is_oom || looks_like_memory_error(&message) {
        ModelHostError::LowMemory(message)
    } else {
        ModelHostError::LoadFailed(message)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    const MOCK_CHILD_RS: &str = r#"
use std::io::{self, BufRead, BufReader, Read, Write};
use std::thread;
use std::time::Duration;

fn main() {
    let scenario = std::env::args().nth(1).unwrap_or_default();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).unwrap();
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.contains("\"shutdown\"") {
            break;
        } else if trimmed.contains("\"load\"") {
            match scenario.as_str() {
                "progress_then_ready" => {
                    for i in 1..=3 {
                        thread::sleep(Duration::from_millis(80));
                        let msg = format!(
                            "{{\"type\":\"progress\",\"message\":\"step {i}\",\"fraction\":0.{i}}}\n"
                        );
                        writer.write_all(msg.as_bytes()).unwrap();
                        writer.flush().unwrap();
                    }
                    writer.write_all(b"{\"type\":\"ready\",\"backend\":\"whisper.cpp\",\"model\":\"tiny\",\"device\":\"cpu\",\"whisper_version\":\"1.8.4\"}\n").unwrap();
                    writer.flush().unwrap();
                }
                "oom_on_load" => {
                    writer.write_all(b"{\"type\":\"error\",\"message\":\"std::bad_alloc\",\"is_oom\":true}\n").unwrap();
                    writer.flush().unwrap();
                }
                _ => {
                    writer.write_all(b"{\"type\":\"ready\",\"backend\":\"whisper.cpp\",\"model\":\"tiny\",\"device\":\"cpu\",\"whisper_version\":\"1.8.4\"}\n").unwrap();
                    writer.flush().unwrap();
                }
            }
        } else if trimmed.contains("\"transcribe\"") {
            if let Some(pos) = trimmed.find("\"sample_count\":") {
                let rest = &trimmed[pos + "\"sample_count\":".len()..];
                let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                let count: usize = num_str.parse().unwrap_or(0);
                let mut buf = vec![0u8; count * 4];
                reader.read_exact(&mut buf).unwrap();
            }
            match scenario.as_str() {
                "timeout_on_transcribe" => {
                    thread::sleep(Duration::from_millis(450));
                    let _ = writer.write_all(b"{\"type\":\"segments\",\"segments\":[{\"text\":\"stale\",\"no_speech_prob\":0.0,\"avg_logprob\":0.0}]}\n");
                    let _ = writer.flush();
                }
                "error_on_transcribe" => {
                    writer.write_all(b"{\"type\":\"error\",\"message\":\"decode failed\",\"is_oom\":false}\n").unwrap();
                    writer.flush().unwrap();
                }
                _ => {
                    writer.write_all(b"{\"type\":\"segments\",\"segments\":[{\"text\":\"fresh reply\",\"no_speech_prob\":0.02,\"avg_logprob\":-0.1}]}\n").unwrap();
                    writer.flush().unwrap();
                }
            }
        }
    }
}
"#;

    fn mock_child_exe() -> &'static PathBuf {
        static BIN: OnceLock<PathBuf> = OnceLock::new();
        BIN.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("eqho_host_mock_{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let src = dir.join("mock_child.rs");
            let exe = dir.join(if cfg!(windows) {
                "mock_child.exe"
            } else {
                "mock_child"
            });
            std::fs::write(&src, MOCK_CHILD_RS).unwrap();
            let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
            let status = Command::new(rustc)
                .arg("--edition")
                .arg("2024")
                .arg(&src)
                .arg("-o")
                .arg(&exe)
                .status()
                .expect("failed to invoke rustc for mock child");
            assert!(status.success(), "rustc failed building mock child");
            exe
        })
    }

    fn cfg_for(
        scenario: &str,
        load_inactivity_timeout: Duration,
        transcribe_timeout: Duration,
    ) -> ModelHostConfig {
        ModelHostConfig {
            child_exe: mock_child_exe().clone(),
            child_args: vec![scenario.to_owned()],
            dll_dir: PathBuf::from("."),
            load_inactivity_timeout,
            transcribe_timeout,
        }
    }

    #[test]
    fn subprocess_timeout_kills_child_and_prevents_stale_replies() {
        let mut host = ModelHost::new(cfg_for(
            "timeout_on_transcribe",
            Duration::from_secs(2),
            Duration::from_millis(150),
        ));

        host.load_model("tiny", Path::new("."), false, 4, &mut |_, _| {})
            .unwrap();
        assert!(host.is_ready("tiny", false));

        let err = host
            .transcribe(&[0.0f32; 800], "en", 1, "")
            .unwrap_err();
        assert!(matches!(err, ModelHostError::Timeout(_)));
        assert!(!host.is_ready("tiny", false));

        // Switch to healthy child; must not see any stale reply from timed-out child.
        host.config = cfg_for("healthy", Duration::from_secs(2), Duration::from_secs(2));
        host.load_model("tiny", Path::new("."), false, 4, &mut |_, _| {})
            .unwrap();
        let segs = host.transcribe(&[0.0f32; 800], "en", 1, "").unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "fresh reply");
    }

    #[test]
    fn subprocess_progress_resets_inactivity_timeout() {
        let mut host = ModelHost::new(cfg_for(
            "progress_then_ready",
            Duration::from_millis(180),
            Duration::from_secs(2),
        ));

        let mut progress_msgs = Vec::new();
        let info = host
            .load_model("tiny", Path::new("."), false, 4, &mut |msg, frac| {
                progress_msgs.push((msg.to_owned(), frac));
            })
            .unwrap();

        assert_eq!(info.model, "tiny");
        assert_eq!(progress_msgs.len(), 3);
        assert!(host.is_ready("tiny", false));
    }

    #[test]
    fn subprocess_oom_and_transcribe_error_propagation() {
        let mut oom_host = ModelHost::new(cfg_for(
            "oom_on_load",
            Duration::from_secs(2),
            Duration::from_secs(2),
        ));
        let load_err = oom_host
            .load_model("tiny", Path::new("."), false, 4, &mut |_, _| {})
            .unwrap_err();
        assert!(matches!(load_err, ModelHostError::LowMemory(_)));
        assert!(!oom_host.is_ready("tiny", false));
        assert_eq!(oom_host.last_load_error(), Some("std::bad_alloc"));

        let mut err_host = ModelHost::new(cfg_for(
            "error_on_transcribe",
            Duration::from_secs(2),
            Duration::from_secs(2),
        ));
        err_host
            .load_model("tiny", Path::new("."), false, 4, &mut |_, _| {})
            .unwrap();
        let tr_err = err_host
            .transcribe(&[0.0f32; 800], "en", 1, "")
            .unwrap_err();
        assert!(matches!(tr_err, ModelHostError::TranscribeFailed(_)));
        assert!(!err_host.is_ready("tiny", false));
    }
}
