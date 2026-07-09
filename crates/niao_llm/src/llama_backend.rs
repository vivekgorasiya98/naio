//! llama.cpp backend — mmap, auto GPU fit, flash-attn, persistent context.

use crate::{ChatMessage, GenerateOptions, LlmError, Result};
use encoding_rs::UTF_8;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::Path;
use std::pin::Pin;

pub struct LlamaSession {
    backend: LlamaBackend,
    ctx: Option<LlamaContext<'static>>,
    model: LlamaModel,
    n_ctx: NonZeroU32,
    n_gpu_layers: u32,
    threads: i32,
    n_batch: u32,
    pos: i32,
}

impl LlamaSession {
    pub fn load(
        model_path: &Path,
        cpu: bool,
        n_gpu_layers: Option<u32>,
        threads: Option<u32>,
        n_ctx: Option<u32>,
        use_fit: bool,
    ) -> Result<Self> {
        let backend = LlamaBackend::init().map_err(|e| LlmError::Msg(e.to_string()))?;

        let thread_count = threads
            .map(|t| t as i32)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(|n| n.get() as i32)
                    .unwrap_or(4)
            });

        let ctx_size = n_ctx.unwrap_or(2048).max(512);
        let n_ctx_nz = NonZeroU32::new(ctx_size).unwrap_or(NonZeroU32::new(2048).unwrap());

        let mut ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(n_ctx_nz))
            .with_n_threads(thread_count)
            .with_n_threads_batch(thread_count)
            .with_n_batch(2048)
            .with_n_ubatch(512);

        #[cfg(feature = "cuda")]
        {
            ctx_params = ctx_params.with_flash_attention_policy(
                llama_cpp_sys_2::llama_flash_attn_type::LLAMA_FLASH_ATTN_TYPE_AUTO,
            );
        }

        let mut model_params = LlamaModelParams::default().with_use_mmap(true);
        let gpu_layers = if cpu {
            model_params = model_params.with_n_gpu_layers(0);
            0u32
        } else if use_fit && n_gpu_layers.is_none() {
            let mut pinned = Pin::new(&mut model_params);
            fit_gpu_layers(model_path, &mut pinned, &mut ctx_params)?
        } else {
            let layers = n_gpu_layers.unwrap_or(u32::MAX);
            model_params = model_params.with_n_gpu_layers(layers);
            layers
        };

        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| LlmError::Msg(format!("llama model load: {e}")))?;

        let n_ctx = ctx_params.n_ctx().unwrap_or(n_ctx_nz);

        Ok(Self {
            backend,
            ctx: None,
            model,
            n_ctx,
            n_gpu_layers: gpu_layers,
            threads: thread_count,
            n_batch: ctx_params.n_batch(),
            pos: 0,
        })
    }

    pub fn device_label(&self) -> &'static str {
        if self.n_gpu_layers > 0 {
            #[cfg(feature = "cuda")]
            {
                return "cuda";
            }
            #[cfg(feature = "vulkan")]
            {
                return "vulkan";
            }
            return "gpu";
        }
        "cpu"
    }

    pub fn reset(&mut self) {
        if let Some(ctx) = self.ctx.as_mut() {
            ctx.clear_kv_cache();
        }
        self.pos = 0;
    }

    pub fn tokenize(&self, text: &str) -> Result<Vec<u32>> {
        let tokens = self
            .model
            .str_to_token(text, AddBos::Never)
            .map_err(|e| LlmError::Msg(format!("tokenize: {e}")))?;
        Ok(tokens.iter().map(token_id).collect())
    }

    pub fn apply_template(&self, messages: &[ChatMessage]) -> Result<String> {
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
        self.model
            .apply_chat_template(&template, &chat, true)
            .map_err(|e| LlmError::Msg(format!("apply template: {e}")))
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
        self.reset();

        let prompt = self.apply_template(messages)?;
        let tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| LlmError::Msg(format!("tokenize: {e}")))?;

        let n_batch = self.n_batch as usize;
        let mut ctx = self.take_context()?;
        let mut batch = LlamaBatch::new(n_batch, 1);
        batch.clear();
        self.pos = 0;
        let mut last_logits_idx = 0i32;

        for (i, token) in tokens.iter().enumerate() {
            let is_last_prompt = i + 1 == tokens.len();
            batch
                .add(*token, self.pos, &[0], is_last_prompt)
                .map_err(|e| LlmError::Msg(format!("batch add: {e}")))?;
            self.pos += 1;
            if batch.n_tokens() as usize >= n_batch || is_last_prompt {
                last_logits_idx = batch.n_tokens() - 1;
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
            LlamaSampler::chain_simple([
                LlamaSampler::penalties(-1, repeat, 0.0, 0.0),
                LlamaSampler::greedy(),
            ])
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::penalties(-1, repeat, 0.0, 0.0),
                LlamaSampler::top_k(opts.top_k.max(1) as i32),
                LlamaSampler::top_p(opts.top_p, 1),
                LlamaSampler::temp(opts.temperature),
                LlamaSampler::dist(opts.seed as u32),
            ])
        };

        let mut decoder = UTF_8.new_decoder();
        let mut full = String::new();

        for _ in 0..opts.max_tokens {
            let token = if full.is_empty() {
                sampler.sample(&ctx, last_logits_idx)
            } else {
                sampler.sample(&ctx, 0)
            };
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
            if hits_stop(&full, &opts.stop) {
                full = trim_at_stop(full, &opts.stop);
                break;
            }

            batch.clear();
            batch
                .add(token, self.pos, &[0], true)
                .map_err(|e| LlmError::Msg(format!("batch add token: {e}")))?;
            ctx.decode(&mut batch)
                .map_err(|e| LlmError::Msg(format!("decode token: {e}")))?;
            self.pos += 1;
            sampler.accept(token);
        }

        self.ctx = Some(ctx);
        Ok(full.trim().to_string())
    }

    fn take_context(&mut self) -> Result<LlamaContext<'static>> {
        if let Some(ctx) = self.ctx.take() {
            return Ok(ctx);
        }
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(self.n_ctx))
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads)
            .with_n_batch(self.n_batch)
            .with_n_ubatch(512.min(self.n_batch));
        let ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| LlmError::Msg(format!("llama context: {e}")))?;
        Ok(unsafe { std::mem::transmute::<LlamaContext<'_>, LlamaContext<'static>>(ctx) })
    }

    #[allow(dead_code)]
    fn ensure_context(&mut self) -> Result<&mut LlamaContext<'static>> {
        if self.ctx.is_none() {
            self.ctx = Some(self.take_context()?);
        }
        Ok(self.ctx.as_mut().unwrap())
    }
}

fn token_id(t: &LlamaToken) -> u32 {
    t.0.max(0) as u32
}

fn hits_stop(text: &str, stops: &[String]) -> bool {
    stops.iter().any(|s| !s.is_empty() && text.contains(s))
}

fn trim_at_stop(mut text: String, stops: &[String]) -> String {
    for stop in stops {
        if let Some(idx) = text.find(stop) {
            text.truncate(idx);
        }
    }
    text.trim().to_string()
}

#[cfg(feature = "llama")]
fn fit_gpu_layers(
    model_path: &Path,
    model_params: &mut Pin<&mut LlamaModelParams>,
    ctx_params: &mut LlamaContextParams,
) -> Result<u32> {
    use llama_cpp_2::model::params::FitError;

    let path = model_path
        .to_str()
        .ok_or_else(|| LlmError::Msg("model path is not valid UTF-8".into()))?;
    let c_path = CString::new(path).map_err(|e| LlmError::Msg(e.to_string()))?;

    let max_devices = unsafe { llama_cpp_sys_2::llama_max_devices() }.max(1) as usize;
    let margin = 256 * 1024 * 1024;
    let mut margins = vec![margin; max_devices];

    match model_params.as_mut().fit_params(
        &c_path,
        ctx_params,
        &mut margins,
        512,
        llama_cpp_sys_2::GGML_LOG_LEVEL_INFO,
    ) {
        Ok(_) => Ok(model_params.n_gpu_layers().max(0) as u32),
        Err(FitError::Failure) => {
            **model_params = LlamaModelParams::default()
                .with_use_mmap(true)
                .with_n_gpu_layers(0);
            Ok(0)
        }
        Err(FitError::Error) => Err(LlmError::Msg("llama fit_params failed".into())),
    }
}
