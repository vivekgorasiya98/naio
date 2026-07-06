//! Vector RAG: embedding + cosine search for Neko nrag.

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;

const MAGIC: &[u8; 5] = b"NRAG1";

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
    embeddings: Vec<f32>,
}

pub struct Embedder {
    model: TextEmbedding,
}

impl Embedder {
    pub fn new() -> Result<Self> {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )
        .map_err(|e| RagError::Embed(e.to_string()))?;
        Ok(Self { model })
    }

    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>> {
        let out = self
            .model
            .embed(vec![text], None)
            .map_err(|e| RagError::Embed(e.to_string()))?;
        out.into_iter()
            .next()
            .ok_or_else(|| RagError::Msg("empty embedding".into()))
    }

    pub fn dim(&self) -> usize {
        384
    }
}

impl RagIndex {
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn load(path: &Path) -> Result<Self> {
        if path.extension().and_then(|e| e.to_str()) == Some("nrag") {
            return Self::load_binary(path);
        }
        Self::load_json(path)
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
        let dim = root.dim.unwrap_or(384);
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

    pub fn search(&self, query: &[f32], top_k: usize, threshold: f32) -> Vec<SearchHit> {
        if query.len() != self.dim || self.chunks.is_empty() {
            return Vec::new();
        }
        let mut scores: Vec<(usize, f32)> = (0..self.chunks.len())
            .map(|i| {
                let start = i * self.dim;
                let slice = &self.embeddings[start..start + self.dim];
                (i, cosine(query, slice))
            })
            .filter(|(_, s)| *s >= threshold)
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
            .into_iter()
            .take(top_k)
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

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom < 1e-9 {
        0.0
    } else {
        dot / denom
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
    for ch in root.chunks {
        let emb = if let Some(e) = ch.embedding {
            if e.len() != dim {
                return Err(RagError::Msg(format!("bad embedding dim for {}", ch.id)));
            }
            e
        } else {
            embedder.embed_one(&ch.content)?
        };
        flat.extend_from_slice(&emb);
        chunks.push(Chunk {
            id: ch.id,
            content: ch.content,
            source: ch.source,
            chapter: ch.chapter,
            section: ch.section,
        });
    }
    RagIndex::write_binary(out_path, &chunks, &flat, dim)?;
    Ok(chunks.len())
}
