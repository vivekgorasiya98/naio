//! Native GGUF inference for Niao nllm (llama.cpp + Candle backends).

mod candle_backend;
mod device;
#[cfg(feature = "llama")]
mod llama_backend;

use device::{resolve_load_options, DeviceInfo};
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

pub use device::DeviceInfo as LlmDeviceInfo;

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
    /// When false and CUDA is available, offload to GPU (auto-tuned).
    pub cpu: bool,
    pub backend: BackendKind,
    pub n_gpu_layers: Option<u32>,
    pub threads: Option<u32>,
    pub n_ctx: Option<u32>,
    /// Auto-tune threads, GPU layers, and context to device capacity.
    pub auto: bool,
    /// Internal: use llama.cpp fit_params for VRAM-aware layer split.
    pub(crate) use_fit: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            tokenizer_path: None,
            cpu: false,
            backend: BackendKind::Auto,
            n_gpu_layers: None,
            threads: None,
            n_ctx: None,
            auto: true,
            use_fit: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    pub repeat_penalty: f32,
    pub seed: u64,
    pub top_p: f32,
    pub top_k: u32,
    pub stop: Vec<String>,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            max_tokens: 1024,
            temperature: 0.25,
            repeat_penalty: 1.12,
            seed: 42,
            top_p: 0.9,
            top_k: 40,
            stop: vec![
                "".into(),
                "<|im_start|>".into(),
                "--- STUDENT QUESTION ---".into(),
                "--- SYLLABUS CONTEXT ---".into(),
                "[back to top]".into(),
            ],
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
    device: DeviceInfo,
}

impl LlmSession {
    pub fn load(model_path: &Path, opts: LoadOptions) -> Result<Self> {
        let opts = resolve_load_options(model_path, opts);
        let device = DeviceInfo::probe();

        let use_llama = match opts.backend {
            BackendKind::Llama => {
                #[cfg(not(feature = "llama"))]
                return Err(LlmError::NoLlama);
                #[cfg(feature = "llama")]
                true
            }
            BackendKind::Candle => false,
            BackendKind::Auto => cfg!(feature = "llama"),
        };

        if use_llama {
            #[cfg(feature = "llama")]
            {
                let sess = llama_backend::LlamaSession::load(
                    model_path,
                    opts.cpu,
                    opts.n_gpu_layers,
                    opts.threads,
                    opts.n_ctx,
                    opts.use_fit,
                )?;
                let device_label = sess.device_label();
                return Ok(Self {
                    engine: Engine::Llama(sess),
                    backend_name: match device_label {
                        "cuda" => "llama+cuda",
                        "vulkan" => "llama+vulkan",
                        "gpu" => "llama+gpu",
                        _ => "llama",
                    },
                    device,
                });
            }
        }

        let sess = candle_backend::CandleSession::load(
            model_path,
            opts.tokenizer_path,
            opts.cpu,
        )?;
        let device_label = sess.device_label();
        Ok(Self {
            engine: Engine::Candle(sess),
            backend_name: if device_label == "cuda" {
                "candle+cuda"
            } else {
                "candle"
            },
            device,
        })
    }

    pub fn device_info(&self) -> &DeviceInfo {
        &self.device
    }

    pub fn probe_device() -> DeviceInfo {
        DeviceInfo::probe()
    }

    pub fn backend_name(&self) -> &str {
        self.backend_name
    }

    pub fn reset(&mut self) {
        match &mut self.engine {
            #[cfg(feature = "llama")]
            Engine::Llama(sess) => sess.reset(),
            Engine::Candle(sess) => sess.reset(),
        }
    }

    pub fn complete(&mut self, prompt: &str, opts: GenerateOptions) -> Result<String> {
        self.chat(
            &[ChatMessage {
                role: "user".into(),
                content: prompt.into(),
            }],
            opts,
        )
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

    pub fn tokenize(&self, text: &str) -> Result<Vec<u32>> {
        match &self.engine {
            #[cfg(feature = "llama")]
            Engine::Llama(sess) => sess.tokenize(text),
            Engine::Candle(sess) => sess.tokenize(text),
        }
    }

    pub fn count_tokens(&self, text: &str) -> Result<usize> {
        Ok(self.tokenize(text)?.len())
    }

    pub fn apply_template(&self, messages: &[ChatMessage]) -> Result<String> {
        match &self.engine {
            #[cfg(feature = "llama")]
            Engine::Llama(sess) => sess.apply_template(messages),
            Engine::Candle(sess) => sess.apply_template(messages),
        }
    }

    pub fn list_backends() -> Vec<&'static str> {
        let mut out = Vec::new();
        #[cfg(feature = "llama")]
        {
            out.push("llama");
            #[cfg(feature = "cuda")]
            out.push("llama+cuda");
            #[cfg(feature = "vulkan")]
            out.push("llama+vulkan");
        }
        out.push("candle");
        #[cfg(feature = "cuda")]
        out.push("candle+cuda");
        out
    }
}
