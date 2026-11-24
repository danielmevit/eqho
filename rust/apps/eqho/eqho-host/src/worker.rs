//! Child-process stdin/stdout loop for `Eqho.exe --model-host`.
//!
//! Stdout is reserved strictly for framed IPC [`HostResponse`] lines; all
//! diagnostics go to stderr.

use std::io::{BufReader, BufWriter};
use std::path::Path;

use eqho_core::modelstore::looks_like_memory_error;

use crate::downloader;
use crate::protocol::{self, HostRequest, HostResponse};
use crate::whisper_ffi::WhisperEngine;

/// Run the child model-host IPC loop on stdin/stdout. Returns process exit code.
pub fn run_child_stdio_loop() -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    let mut engine: Option<WhisperEngine> = None;
    let mut loaded_threads: u32 = 4;

    loop {
        let (req, audio) = match protocol::read_request(&mut reader) {
            Ok(Some(pair)) => pair,
            Ok(None) => return 0,
            Err(err) => {
                let msg = err.to_string();
                eprintln!("eqho-host worker read error: {msg}");
                let _ = protocol::write_response(
                    &mut writer,
                    &HostResponse::Error {
                        message: msg,
                        is_oom: false,
                    },
                );
                return 1;
            }
        };

        match req {
            HostRequest::Shutdown => return 0,
            HostRequest::Load {
                model_key,
                model_dir,
                dll_dir,
                use_gpu,
                threads,
            } => {
                loaded_threads = threads.max(1);
                let mut progress_write_failed = false;
                let model_path = match downloader::download_ggml_model(
                    &model_key,
                    Path::new(&model_dir),
                    &mut |message, fraction| {
                        if protocol::write_response(
                            &mut writer,
                            &HostResponse::Progress {
                                message: message.to_owned(),
                                fraction,
                            },
                        )
                        .is_err()
                        {
                            progress_write_failed = true;
                        }
                    },
                ) {
                    Ok(path) => path,
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = protocol::write_response(
                            &mut writer,
                            &HostResponse::Error {
                                is_oom: looks_like_memory_error(&msg),
                                message: msg,
                            },
                        );
                        if progress_write_failed {
                            return 1;
                        }
                        continue;
                    }
                };

                if progress_write_failed {
                    return 1;
                }

                if protocol::write_response(
                    &mut writer,
                    &HostResponse::Progress {
                        message: format!(
                            "loading whisper.cpp model ({})",
                            if use_gpu { "gpu" } else { "cpu" }
                        ),
                        fraction: None,
                    },
                )
                .is_err()
                {
                    return 1;
                }

                match WhisperEngine::load(Path::new(&dll_dir), &model_path, use_gpu) {
                    Ok(eng) => {
                        let resp = HostResponse::Ready {
                            backend: "whisper.cpp".into(),
                            model: model_key,
                            device: if use_gpu { "gpu" } else { "cpu" }.into(),
                            whisper_version: eng.version().to_owned(),
                        };
                        engine = Some(eng);
                        if protocol::write_response(&mut writer, &resp).is_err() {
                            return 1;
                        }
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        if protocol::write_response(
                            &mut writer,
                            &HostResponse::Error {
                                is_oom: looks_like_memory_error(&msg),
                                message: msg,
                            },
                        )
                        .is_err()
                        {
                            return 1;
                        }
                    }
                }
            }
            HostRequest::Transcribe {
                sample_count: _,
                language,
                beam_size,
                initial_prompt,
            } => {
                let Some(eng) = engine.as_mut() else {
                    if protocol::write_response(
                        &mut writer,
                        &HostResponse::Error {
                            message: "no model loaded".into(),
                            is_oom: false,
                        },
                    )
                    .is_err()
                    {
                        return 1;
                    }
                    continue;
                };

                match eng.transcribe(
                    &audio,
                    &language,
                    beam_size,
                    &initial_prompt,
                    loaded_threads,
                ) {
                    Ok(segments) => {
                        if protocol::write_response(
                            &mut writer,
                            &HostResponse::Segments { segments },
                        )
                        .is_err()
                        {
                            return 1;
                        }
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        if protocol::write_response(
                            &mut writer,
                            &HostResponse::Error {
                                is_oom: looks_like_memory_error(&msg),
                                message: msg,
                            },
                        )
                        .is_err()
                        {
                            return 1;
                        }
                    }
                }
            }
        }
    }
}
