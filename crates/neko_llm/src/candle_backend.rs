//! Candle + quantized Qwen2 GGUF backend.

use crate::{ChatMessage, GenerateOptions, LlmError, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_qwen2::ModelWeights as Qwen2;
use std::fs::File;
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;

pub struct CandleSession {
    model: Qwen2,
    tokenizer: Tokenizer,
    device: Device,
}

impl CandleSession {
    pub fn load(model_path: &Path, tokenizer_path: Option<String>, cpu: bool) -> Result<Self> {
        let device = if cpu {
            Device::Cpu
        } else {
            Device::cuda_if_available(0).unwrap_or(Device::Cpu)
        };

        let mut file = File::open(model_path)
            .map_err(|e| LlmError::Msg(format!("open {}: {e}", model_path.display())))?;
        let content = gguf_file::Content::read(&mut file)
            .map_err(|e| LlmError::Msg(format!("gguf read: {e}")))?;
        let model = Qwen2::from_gguf(content, &mut file, &device)
            .map_err(|e| LlmError::Msg(e.to_string()))?;

        let tokenizer_path = match tokenizer_path {
            Some(p) => PathBuf::from(p),
            None => download_tokenizer("Qwen/Qwen2.5-3B-Instruct")?,
        };
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| LlmError::Msg(format!("tokenizer: {e}")))?;

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    pub fn device_label(&self) -> &'static str {
        match self.device {
            Device::Cpu => "cpu",
            Device::Cuda(_) => "cuda",
            _ => "other",
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
        let prompt = format_chatml(messages);
        let encoding = self
            .tokenizer
            .encode(prompt.as_str(), true)
            .map_err(|e| LlmError::Msg(e.to_string()))?;
        let tokens: Vec<u32> = encoding.get_ids().to_vec();

        let sampling = if opts.temperature <= 0.0 {
            Sampling::ArgMax
        } else {
            Sampling::All {
                temperature: opts.temperature as f64,
            }
        };
        let mut logits_processor = LogitsProcessor::from_sampling(opts.seed, sampling);

        let input = Tensor::new(tokens.as_slice(), &self.device)?;
        let logits = self.model.forward(&input.unsqueeze(0)?, 0)?;
        let mut next_token = logits_processor.sample(&logits.squeeze(0)?)?;

        let eos = self.tokenizer.token_to_id("").unwrap_or(151645);

        let mut generated_ids = Vec::new();
        let prompt_len = tokens.len();
        let mut prev_len = 0usize;

        for i in 0..opts.max_tokens {
            if next_token == eos {
                break;
            }
            generated_ids.push(next_token);
            let partial = self
                .tokenizer
                .decode(&generated_ids, true)
                .map_err(|e| LlmError::Msg(e.to_string()))?;
            if partial.len() > prev_len {
                let delta = &partial[prev_len..];
                on_delta(delta)?;
                prev_len = partial.len();
            }

            let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, prompt_len + i as usize)?;
            let logits = logits.squeeze(0)?;
            let logits = if opts.repeat_penalty == 1.0 {
                logits
            } else {
                candle_transformers::utils::apply_repeat_penalty(
                    &logits,
                    opts.repeat_penalty,
                    &generated_ids,
                )?
            };
            next_token = logits_processor.sample(&logits)?;
        }

        let text = self
            .tokenizer
            .decode(&generated_ids, true)
            .map_err(|e| LlmError::Msg(e.to_string()))?;
        Ok(text.trim().to_string())
    }
}

fn format_chatml(messages: &[ChatMessage]) -> String {
    let mut s = String::new();
    for m in messages {
        let role = match m.role.as_str() {
            "system" => "system",
            "assistant" => "assistant",
            _ => "user",
        };
        s.push_str("<|im_start|>");
        s.push_str(role);
        s.push('\n');
        s.push_str(&m.content);
        s.push_str("\n\n");
    }
    s.push_str("<|im_start|>assistant\n");
    s
}

fn download_tokenizer(repo: &str) -> Result<PathBuf> {
    let api = hf_hub::api::sync::Api::new().map_err(|e| LlmError::Msg(e.to_string()))?;
    let path = api
        .model(repo.to_string())
        .get("tokenizer.json")
        .map_err(|e| LlmError::Msg(format!("hf-hub tokenizer: {e}")))?;
    Ok(path)
}
