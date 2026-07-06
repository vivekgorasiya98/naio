//! Native GGUF inference for Neko nllm (llama.cpp + Candle backends).

mod candle_backend;
#[cfg(feature = "llama")]
mod llama_backend;

use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("{0}")]
    Msg(String),
    #[error("candle: {0}")]
    Candle(#[from] candle_core::Error),
    #[cfg(not(feature = "llama"))]
    #[error("llama backend not compiled — rebuild with --features llama")]
    NoLlama,
}

pub type Result<T> = std::result::Result<T, LlmError>;

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Auto,
    Llama,
    Candle,
}

impl BackendKind {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "llama" | "llama.cpp" | "llamacpp" => Self::Llama,
            "candle" => Self::Candle,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub tokenizer_path: Option<String>,
    pub cpu: bool,
    pub backend: BackendKind,
    pub n_gpu_layers: Option<u32>,
    pub threads: Option<u32>,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            tokenizer_path: None,
            cpu: true,
            backend: BackendKind::Auto,
            n_gpu_layers: None,
            threads: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    pub repeat_penalty: f32,
    pub seed: u64,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            max_tokens: 600,
            temperature: 0.3,
            repeat_penalty: 1.1,
            seed: 42,
        }
    }
}

enum Engine {
    #[cfg(feature = "llama")]
    Llama(llama_backend::LlamaSession),
    Candle(candle_backend::CandleSession),
}

pub struct LlmSession {
    engine: Engine,
    backend_name: &'static str,
}

impl LlmSession {
    pub fn load(model_path: &Path, opts: LoadOptions) -> Result<Self> {
        let use_llama = match opts.backend {
            BackendKind::Llama => {
                #[cfg(not(feature = "llama"))]
                return Err(LlmError::NoLlama);
                #[cfg(feature = "llama")]
                true
            }
            BackendKind::Candle => false,
            BackendKind::Auto => {
                #[cfg(feature = "llama")]
                {
                    true
                }
                #[cfg(not(feature = "llama"))]
                {
                    false
                }
            }
        };

        if use_llama {
            #[cfg(feature = "llama")]
            {
                let sess = llama_backend::LlamaSession::load(
                    model_path,
                    opts.cpu,
                    opts.n_gpu_layers,
                    opts.threads,
                )?;
                let device = sess.device_label();
                return Ok(Self {
                    engine: Engine::Llama(sess),
                    backend_name: if device == "cuda" {
                        "llama+cuda"
                    } else {
                        "llama"
                    },
                });
            }
        }

        let sess = candle_backend::CandleSession::load(
            model_path,
            opts.tokenizer_path,
            opts.cpu,
        )?;
        let device = sess.device_label();
        Ok(Self {
            engine: Engine::Candle(sess),
            backend_name: if device == "cuda" {
                "candle+cuda"
            } else {
                "candle"
            },
        })
    }

    pub fn backend_name(&self) -> &str {
        self.backend_name
    }

    pub fn chat(&mut self, messages: &[ChatMessage], opts: GenerateOptions) -> Result<String> {
        self.chat_stream(messages, opts, |_| Ok(()))
    }

    pub fn chat_stream<F>(
        &mut self,
        messages: &[ChatMessage],
        opts: GenerateOptions,
        on_delta: F,
    ) -> Result<String>
    where
        F: FnMut(&str) -> Result<()>,
    {
        match &mut self.engine {
            #[cfg(feature = "llama")]
            Engine::Llama(sess) => sess.chat_stream(messages, opts, on_delta),
            Engine::Candle(sess) => sess.chat_stream(messages, opts, on_delta),
        }
    }
}
