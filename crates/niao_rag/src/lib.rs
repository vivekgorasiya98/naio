//! Vector RAG: embedding + cosine search for Niao nrag.

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use ort::ep::directml::{DeviceFilter, PerformancePreference};
use ort::ep::DirectML;
use ort::execution_providers::ExecutionProviderDispatch;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;

const MAGIC: &[u8; 5] = b"NRAG1";
pub const DEFAULT_DIM: usize = 384;
pub const MODEL_NAME: &str = "all-MiniLM-L6-v2";

#[derive(Debug, Error)]
pub enum RagError {
    #[error("{0}")]
    Msg(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("embed: {0}")]
    Embed(String),
}

pub type Result<T> = std::result::Result<T, RagError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub content: String,
    pub source: String,
    pub chapter: Option<String>,
    pub section: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub chunk_index: usize,
    pub score: f32,
}

#[derive(Debug)]
pub struct RagIndex {
    pub dim: usize,
    pub chunks: Vec<Chunk>,
    /// L2-normalized embedding vectors (unit length) for fast dot-product search.
    embeddings: Vec<f32>,
}

pub struct Embedder {
    backend: EmbedBackend,
    device: &'static str,
}

enum EmbedBackend {
    Local(TextEmbedding),
    Sidecar(String),
}

impl Embedder {
    fn try_new_with_providers(
        providers: Vec<ExecutionProviderDispatch>,
        device: &'static str,
    ) -> Result<Self> {
        let opts = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_show_download_progress(false)
            .with_execution_providers(providers);
        let model = TextEmbedding::try_new(opts).map_err(|e| RagError::Embed(e.to_string()))?;
        Ok(Self {
            backend: EmbedBackend::Local(model),
            device,
        })
    }

    fn try_sidecar(base: &str) -> Option<Self> {
        let base = base.trim_end_matches('/');
        let url = format!("{base}/health");
        let resp = ureq::get(&url).call().ok()?;
        if resp.status() != 200 {
            return None;
        }
        let body: serde_json::Value = resp
            .into_string()
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())?;
        let dev = body.get("device")?.as_str()?;
        if dev != "npu" {
            return None;
        }
        eprintln!("nrag: using NPU embed sidecar at {base}");
        Some(Self {
            backend: EmbedBackend::Sidecar(base.to_string()),
            device: "npu",
        })
    }

    pub fn new() -> Result<Self> {
        let mode = std::env::var("NIAO_RAG_DEVICE")
            .unwrap_or_else(|_| "auto".into())
            .to_ascii_lowercase();
        let hybrid = std::env::var("NIAO_HYBRID_ACCEL")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let want_npu = mode == "npu" || mode == "xdna" || (mode == "auto" && hybrid);
        if want_npu {
            let sidecar_url = std::env::var("NIAO_RAG_NPU_SIDECAR_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:18765".into());
            if let Some(emb) = Self::try_sidecar(&sidecar_url) {
                return Ok(emb);
            }
            let npu_providers = vec![DirectML::default()
                .with_device_filter(DeviceFilter::Npu)
                .with_performance_preference(PerformancePreference::HighPerformance)
                .build()];
            match Self::try_new_with_providers(npu_providers, "npu") {
                Ok(mut emb) => {
                    if emb.npu_probe_works() {
                        return Ok(emb);
                    }
                    eprintln!(
                        "nrag: NPU filter accepted but runs like CPU — start scripts/npu_embed_sidecar.py or install Ryzen AI"
                    );
                }
                Err(e) => {
                    eprintln!("nrag: NPU embedder init failed ({e}), falling back to CPU");
                }
            }
        }

        if mode == "dml" || mode == "igpu" || mode == "directml" || mode == "gpu" {
            let dml = vec![DirectML::default()
                .with_device_filter(DeviceFilter::Gpu)
                .with_performance_preference(PerformancePreference::HighPerformance)
                .build()];
            if let Ok(emb) = Self::try_new_with_providers(dml, "directml") {
                return Ok(emb);
            }
        }

        Self::try_new_with_providers(Vec::new(), "cpu")
    }

    /// NPU vs CPU embed timing — if nearly identical, ORT fell back to CPU internally.
    fn npu_probe_works(&mut self) -> bool {
        let text = "ayurveda vata pitta kapha dosha";
        let t0 = std::time::Instant::now();
        for _ in 0..8 {
            let _ = self.embed_one(text);
        }
        let npu_ms = t0.elapsed().as_millis();

        let cpu_emb = match Self::try_new_with_providers(Vec::new(), "cpu") {
            Ok(e) => e,
            Err(_) => return false,
        };
        let mut cpu_emb = cpu_emb;
        let t1 = std::time::Instant::now();
        for _ in 0..8 {
            let _ = cpu_emb.embed_one(text);
        }
        let cpu_ms = t1.elapsed().as_millis().max(1);
        // Must be materially faster than CPU; equal timing means ORT fell back internally.
        npu_ms * 10 < cpu_ms * 7
    }

    pub fn device(&self) -> &'static str {
        self.device
    }

    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>> {
        if let EmbedBackend::Sidecar(base) = &self.backend {
            let base = base.clone();
            return sidecar_embed(&base, &[text])
                .and_then(|v| v.into_iter().next().ok_or_else(|| RagError::Msg("empty embedding".into())));
        }
        let EmbedBackend::Local(model) = &mut self.backend else {
            unreachable!();
        };
        let out = model
            .embed(vec![text], None)
            .map_err(|e| RagError::Embed(e.to_string()))?;
        out.into_iter()
            .next()
            .ok_or_else(|| RagError::Msg("empty embedding".into()))
    }

    pub fn embed_batch(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if let EmbedBackend::Sidecar(base) = &self.backend {
            let base = base.clone();
            let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            return sidecar_embed(&base, &refs);
        }
        let EmbedBackend::Local(model) = &mut self.backend else {
            unreachable!();
        };
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        model
            .embed(refs, None)
            .map_err(|e| RagError::Embed(e.to_string()))
    }

    pub fn dim(&self) -> usize {
        DEFAULT_DIM
    }

    pub fn model_name(&self) -> &'static str {
        MODEL_NAME
    }
}

fn sidecar_embed(base: &str, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let url = format!("{}/embed", base.trim_end_matches('/'));
    let payload = serde_json::json!({ "texts": texts });
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|e| RagError::Embed(format!("sidecar request failed: {e}")))?;
    if resp.status() != 200 {
        return Err(RagError::Embed(format!("sidecar HTTP {}", resp.status())));
    }
    let body: serde_json::Value = resp
        .into_string()
        .map_err(|e| RagError::Embed(format!("sidecar body: {e}")))
        .and_then(|s| serde_json::from_str(&s).map_err(|e| RagError::Embed(format!("sidecar json: {e}"))))?;
    let arr = body
        .get("embeddings")
        .and_then(|v| v.as_array())
        .ok_or_else(|| RagError::Msg("sidecar missing embeddings".into()))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let row = item
            .as_array()
            .ok_or_else(|| RagError::Msg("bad embedding row".into()))?;
        let vec: Result<Vec<f32>> = row
            .iter()
            .map(|v| {
                v.as_f64()
                    .map(|f| f as f32)
                    .ok_or_else(|| RagError::Msg("bad embedding value".into()))
            })
            .collect();
        out.push(vec?);
    }
    Ok(out)
}

impl RagIndex {
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn get_chunk(&self, index: usize) -> Option<&Chunk> {
        self.chunks.get(index)
    }

    pub fn get_chunk_by_id(&self, id: &str) -> Option<&Chunk> {
        self.chunks.iter().find(|c| c.id == id)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let mut index = if path.extension().and_then(|e| e.to_str()) == Some("nrag") {
            Self::load_binary(path)?
        } else {
            Self::load_json(path)?
        };
        index.normalize_embeddings();
        Ok(index)
    }

    pub fn load_binary(path: &Path) -> Result<Self> {
        let mut f = File::open(path)?;
        let mut magic = [0u8; 5];
        f.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(RagError::Msg("invalid nrag magic".into()));
        }
        let mut buf4 = [0u8; 4];
        f.read_exact(&mut buf4)?;
        let _version = u32::from_le_bytes(buf4);
        f.read_exact(&mut buf4)?;
        let dim = u32::from_le_bytes(buf4) as usize;
        f.read_exact(&mut buf4)?;
        let n_chunks = u32::from_le_bytes(buf4) as usize;
        let mut buf8 = [0u8; 8];
        f.read_exact(&mut buf8)?;
        let meta_len = u64::from_le_bytes(buf8) as usize;

        let emb_bytes = n_chunks
            .checked_mul(dim)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| RagError::Msg("index too large".into()))?;
        let mut emb_raw = vec![0u8; emb_bytes];
        f.read_exact(&mut emb_raw)?;
        let embeddings: Vec<f32> = emb_raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        let mut meta_raw = vec![0u8; meta_len];
        f.read_exact(&mut meta_raw)?;
        let chunks: Vec<Chunk> = serde_json::from_slice(&meta_raw)?;

        if chunks.len() != n_chunks || embeddings.len() != n_chunks * dim {
            return Err(RagError::Msg("corrupt nrag index dimensions".into()));
        }
        Ok(Self {
            dim,
            chunks,
            embeddings,
        })
    }

    pub fn load_json(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let root: JsonIndexRoot = serde_json::from_reader(reader)?;
        let dim = root.dim.unwrap_or(DEFAULT_DIM);
        let mut embeddings = Vec::with_capacity(root.chunks.len() * dim);
        for ch in &root.chunks {
            let emb = ch
                .embedding
                .as_ref()
                .ok_or_else(|| RagError::Msg(format!("chunk {} missing embedding", ch.id)))?;
            if emb.len() != dim {
                return Err(RagError::Msg(format!(
                    "chunk {} embedding dim {} != {}",
                    ch.id,
                    emb.len(),
                    dim
                )));
            }
            embeddings.extend_from_slice(emb);
        }
        let chunks: Vec<Chunk> = root
            .chunks
            .into_iter()
            .map(|c| Chunk {
                id: c.id,
                content: c.content,
                source: c.source,
                chapter: c.chapter,
                section: c.section,
            })
            .collect();
        Ok(Self {
            dim,
            chunks,
            embeddings,
        })
    }

    fn normalize_embeddings(&mut self) {
        for i in 0..self.chunks.len() {
            let start = i * self.dim;
            let end = start + self.dim;
            normalize_in_place(&mut self.embeddings[start..end]);
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        Self::write_binary(path, &self.chunks, &self.embeddings, self.dim)
    }

    /// Cosine search using pre-normalized vectors (parallel over chunks).
    pub fn search(&self, query: &[f32], top_k: usize, threshold: f32) -> Vec<SearchHit> {
        if query.len() != self.dim || self.chunks.is_empty() || top_k == 0 {
            return Vec::new();
        }
        let mut q = query.to_vec();
        normalize_in_place(&mut q);

        let mut scores: Vec<(usize, f32)> = (0..self.chunks.len())
            .into_par_iter()
            .filter_map(|i| {
                let start = i * self.dim;
                let score = dot(&q, &self.embeddings[start..start + self.dim]);
                if score >= threshold {
                    Some((i, score))
                } else {
                    None
                }
            })
            .collect();

        let k = top_k.min(scores.len());
        if k == 0 {
            return Vec::new();
        }
        if scores.len() > k {
            scores.select_nth_unstable_by(k - 1, |a, b| {
                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
            });
            scores.truncate(k);
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
            .into_iter()
            .map(|(i, s)| SearchHit {
                chunk_index: i,
                score: s,
            })
            .collect()
    }

    pub fn write_binary(path: &Path, chunks: &[Chunk], embeddings: &[f32], dim: usize) -> Result<()> {
        if chunks.is_empty() {
            return Err(RagError::Msg("no chunks".into()));
        }
        if embeddings.len() != chunks.len() * dim {
            return Err(RagError::Msg("embedding length mismatch".into()));
        }
        let meta = serde_json::to_vec(chunks)?;
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(dim as u32).to_le_bytes());
        out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
        out.extend_from_slice(&(meta.len() as u64).to_le_bytes());
        for &v in embeddings {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&meta);
        std::fs::write(path, out)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct JsonIndexRoot {
    dim: Option<usize>,
    chunks: Vec<JsonChunk>,
}

#[derive(Debug, Deserialize)]
struct JsonChunk {
    id: String,
    content: String,
    source: String,
    chapter: Option<String>,
    section: Option<String>,
    embedding: Option<Vec<f32>>,
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalize_in_place(v: &mut [f32]) {
    let mut norm = 0.0f32;
    for x in v.iter() {
        norm += x * x;
    }
    let denom = norm.sqrt();
    if denom > 1e-9 {
        for x in v.iter_mut() {
            *x /= denom;
        }
    }
}

/// Build a `.nrag` binary from a JSON export that may lack embeddings (embeds via fastembed).
pub fn build_index_from_json(json_path: &Path, out_path: &Path) -> Result<usize> {
    let file = File::open(json_path)?;
    let root: JsonIndexRoot = serde_json::from_reader(BufReader::new(file))?;
    let mut embedder = Embedder::new()?;
    let dim = embedder.dim();
    let mut chunks = Vec::with_capacity(root.chunks.len());
    let mut flat = Vec::with_capacity(root.chunks.len() * dim);

    let mut pending: Vec<(Chunk, Option<Vec<f32>>)> = Vec::with_capacity(root.chunks.len());
    let mut to_embed: Vec<String> = Vec::new();
    let mut embed_indices: Vec<usize> = Vec::new();

    for ch in root.chunks {
        let chunk = Chunk {
            id: ch.id,
            content: ch.content.clone(),
            source: ch.source,
            chapter: ch.chapter,
            section: ch.section,
        };
        if let Some(e) = ch.embedding {
            if e.len() != dim {
                return Err(RagError::Msg(format!("bad embedding dim for {}", chunk.id)));
            }
            pending.push((chunk, Some(e)));
        } else {
            embed_indices.push(pending.len());
            to_embed.push(ch.content);
            pending.push((chunk, None));
        }
    }

    if !to_embed.is_empty() {
        let batch = embedder.embed_batch(&to_embed)?;
        for (i, emb) in embed_indices.into_iter().zip(batch) {
            pending[i].1 = Some(emb);
        }
    }

    for (chunk, emb) in pending {
        let emb = emb.ok_or_else(|| RagError::Msg(format!("missing embedding for {}", chunk.id)))?;
        flat.extend_from_slice(&emb);
        chunks.push(chunk);
    }

    RagIndex::write_binary(out_path, &chunks, &flat, dim)?;
    Ok(chunks.len())
}

/// Build index from in-memory chunks (embeds all content).
pub fn build_index_from_chunks(chunks: &[Chunk], out_path: &Path) -> Result<usize> {
    if chunks.is_empty() {
        return Err(RagError::Msg("no chunks".into()));
    }
    let mut embedder = Embedder::new()?;
    let dim = embedder.dim();
    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let embs = embedder.embed_batch(&texts)?;
    let mut flat = Vec::with_capacity(chunks.len() * dim);
    for emb in embs {
        flat.extend_from_slice(&emb);
    }
    RagIndex::write_binary(out_path, chunks, &flat, dim)?;
    Ok(chunks.len())
}
