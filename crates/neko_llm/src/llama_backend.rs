//! llama.cpp backend (faster CPU inference than Candle).

use crate::{ChatMessage, GenerateOptions, LlmError, Result};
use encoding_rs::Decoder;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, LlamaModelParams};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::path::Path;

pub struct LlamaSession {
    backend: LlamaBackend,
    model: LlamaModel,
    n_ctx: NonZeroU32,
    n_gpu_layers: u32,
    threads: i32,
}

impl LlamaSession {
    pub fn load(
        model_path: &Path,
        cpu: bool,
        n_gpu_layers: Option<u32>,
        threads: Option<u32>,
    ) -> Result<Self> {
        let backend = LlamaBackend::init().map_err(|e| LlmError::Msg(e.to_string()))?;

        let gpu_layers = if cpu {
            0
        } else {
            n_gpu_layers.unwrap_or(99)
        };
        let model_params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| LlmError::Msg(format!("llama model load: {e}")))?;

        let thread_count = threads
            .map(|t| t as i32)
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4));

        Ok(Self {
            backend,
            model,
            n_ctx: NonZeroU32::new(4096).unwrap(),
            n_gpu_layers: gpu_layers,
            threads: thread_count,
        })
    }

    pub fn device_label(&self) -> &'static str {
        if self.n_gpu_layers > 0 {
            "cuda"
        } else {
            "cpu"
        }
    }

    pub fn chat(&mut self, messages: &[ChatMessage], opts: GenerateOptions) -> Result<String> {
        self.chat_stream(messages, opts, |_| Ok(()))
    }

    pub fn chat_stream<F>(
        &mut self,
        messages: &[ChatMessage],
        opts: GenerateOptions,
        mut on_delta: F,
    ) -> Result<String>
    where
        F: FnMut(&str) -> Result<()>,
    {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(self.n_ctx)
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads);
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| LlmError::Msg(format!("llama context: {e}")))?;

        let template = self
            .model
            .chat_template(None)
            .map_err(|e| LlmError::Msg(format!("chat template: {e}")))?;
        let chat: Vec<LlamaChatMessage> = messages
            .iter()
            .map(|m| {
                LlamaChatMessage::new(m.role.clone(), m.content.clone())
                    .map_err(|e| LlmError::Msg(e.to_string()))
            })
            .collect::<Result<Vec<_>>>()?;
        let prompt = self
            .model
            .apply_chat_template(&template, &chat, true)
            .map_err(|e| LlmError::Msg(format!("apply template: {e}")))?;

        let tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| LlmError::Msg(format!("tokenize: {e}")))?;

        let n_batch = ctx.n_batch() as usize;
        let mut batch = LlamaBatch::new(n_batch, 1);
        let mut pos: i32 = 0;

        for (i, token) in tokens.iter().enumerate() {
            let is_last = i + 1 == tokens.len();
            batch
                .add(*token, pos, &[0], is_last)
                .map_err(|e| LlmError::Msg(format!("batch add: {e}")))?;
            pos += 1;
            if batch.n_tokens() as usize >= n_batch || is_last {
                ctx.decode(&mut batch)
                    .map_err(|e| LlmError::Msg(format!("decode prompt: {e}")))?;
                batch.clear();
            }
        }

        let repeat = if opts.repeat_penalty == 1.0 {
            1.0
        } else {
            opts.repeat_penalty
        };
        let mut sampler = if opts.temperature <= 0.0 {
            LlamaSampler::chain_simple([LlamaSampler::penalties(-1, repeat, 0.0, 0.0), LlamaSampler::greedy()])
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::penalties(-1, repeat, 0.0, 0.0),
                LlamaSampler::temp(opts.temperature),
                LlamaSampler::dist(opts.seed as u32),
            ])
        };

        let mut decoder = Decoder::new();
        let mut full = String::new();

        for _ in 0..opts.max_tokens {
            let token = sampler.sample(&ctx, 0);
            if self.model.is_eog_token(token) {
                break;
            }

            let piece = self
                .model
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(|e| LlmError::Msg(format!("detokenize: {e}")))?;
            if !piece.is_empty() {
                on_delta(&piece)?;
                full.push_str(&piece);
            }

            batch.clear();
            batch
                .add(token, pos, &[0], true)
                .map_err(|e| LlmError::Msg(format!("batch add token: {e}")))?;
            ctx.decode(&mut batch)
                .map_err(|e| LlmError::Msg(format!("decode token: {e}")))?;
            pos += 1;
            sampler.accept(token);
        }

        Ok(full.trim().to_string())
    }
}
