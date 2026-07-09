//! .nml binary checkpoint format.

use crate::layer::{Layer, LayerKind};
use crate::model::Sequential;
use niao_tensor::{Device, Tensor, TensorResult};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

const MAGIC: &[u8; 4] = b"NML\0";
const VERSION: u32 = 1;

pub fn save_model(path: &Path, model: &Sequential) -> TensorResult<()> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    let n_layers = model.layers.len() as u32;
    buf.extend_from_slice(&n_layers.to_le_bytes());
    for layer in &model.layers {
        write_layer(&mut buf, layer)?;
    }
    let mut f = File::create(path).map_err(|e| niao_tensor::TensorError::Io(e.to_string()))?;
    f.write_all(&buf)
        .map_err(|e| niao_tensor::TensorError::Io(e.to_string()))?;
    Ok(())
}

pub fn load_model(path: &Path, device: Device) -> TensorResult<Sequential> {
    let mut f = File::open(path).map_err(|e| niao_tensor::TensorError::Io(e.to_string()))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| niao_tensor::TensorError::Io(e.to_string()))?;
    let mut pos = 0;
    if buf.len() < 12 || &buf[0..4] != MAGIC {
        return Err(niao_tensor::TensorError::Io("invalid .nml file".into()));
    }
    pos += 4;
    let _version = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap());
    pos += 4;
    let n_layers = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    let mut layers = Vec::with_capacity(n_layers);
    for _ in 0..n_layers {
        let (layer, new_pos) = read_layer(&buf, pos, device)?;
        layers.push(layer);
        pos = new_pos;
    }
    Ok(Sequential::new(layers, device))
}

fn write_layer(buf: &mut Vec<u8>, layer: &Layer) -> TensorResult<()> {
    let kind_tag: u8 = match &layer.kind {
        LayerKind::Linear { .. } => 1,
        LayerKind::ReLU => 2,
        LayerKind::Conv2d { .. } => 3,
        LayerKind::BatchNorm2d { .. } => 4,
        _ => 0,
    };
    buf.push(kind_tag);
    if let Some(w) = &layer.weight {
        write_tensor(buf, w)?;
    } else {
        buf.push(0);
    }
    if let Some(b) = &layer.bias {
        write_tensor(buf, b)?;
    } else {
        buf.push(0);
    }
    Ok(())
}

fn write_tensor(buf: &mut Vec<u8>, t: &Tensor) -> TensorResult<()> {
    buf.push(1);
    let shape = &t.shape;
    buf.extend_from_slice(&(shape.len() as u32).to_le_bytes());
    for &d in shape {
        buf.extend_from_slice(&(d as u32).to_le_bytes());
    }
    let data = t.to_cpu()?;
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    for v in data {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}

fn read_layer(buf: &[u8], mut pos: usize, device: Device) -> TensorResult<(Layer, usize)> {
    let tag = buf[pos];
    pos += 1;
    let (weight, pos) = read_optional_tensor(buf, pos, device)?;
    let (bias, pos) = read_optional_tensor(buf, pos, device)?;
    let kind = match tag {
        1 => {
            let w = weight.as_ref().unwrap();
            LayerKind::Linear {
                in_features: w.shape[1],
                out_features: w.shape[0],
            }
        }
        2 => LayerKind::ReLU,
        3 => {
            let w = weight.as_ref().unwrap();
            LayerKind::Conv2d {
                in_channels: w.shape[1],
                out_channels: w.shape[0],
                kernel: (w.shape[2], w.shape[3]),
                stride: 1,
                padding: 0,
            }
        }
        4 => {
            let g = weight.as_ref().unwrap();
            LayerKind::BatchNorm2d {
                channels: g.shape[0],
            }
        }
        _ => LayerKind::ReLU,
    };
    let mut layer = Layer {
        kind,
        weight,
        bias,
        gamma: None,
        beta: None,
        training: true,
        cache: None,
    };
    if matches!(layer.kind, LayerKind::BatchNorm2d { .. }) {
        layer.gamma = layer.weight.take();
        layer.beta = layer.bias.take();
        layer.weight = None;
        layer.bias = None;
    }
    Ok((layer, pos))
}

fn read_optional_tensor(
    buf: &[u8],
    pos: usize,
    device: Device,
) -> TensorResult<(Option<Tensor>, usize)> {
    if buf[pos] == 0 {
        return Ok((None, pos + 1));
    }
    let (t, new_pos) = read_tensor(buf, pos + 1, device)?;
    Ok((Some(t), new_pos))
}

fn read_tensor(buf: &[u8], mut pos: usize, device: Device) -> TensorResult<(Tensor, usize)> {
    let ndim = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    let mut shape = Vec::with_capacity(ndim);
    for _ in 0..ndim {
        shape.push(u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize);
        pos += 4;
    }
    let n = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    let mut data = Vec::with_capacity(n);
    for _ in 0..n {
        data.push(f32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()));
        pos += 4;
    }
    Ok((Tensor::from_cpu_data(&shape, data, device)?, pos))
}
