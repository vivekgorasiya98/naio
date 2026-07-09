//! Host/device capability probing and load-option auto-tuning.
//!
//! Hybrid plan: GPU (CUDA/Vulkan) runs transformer layers; CPU runs overflow layers
//! and sampling. AMD XDNA NPUs are detected but cannot execute GGUF via llama.cpp —
//! they require ONNX Runtime GenAI + Ryzen AI quantized models.

use crate::LoadOptions;
use std::path::Path;

/// Snapshot of CPU / GPU / NPU capacity for `nllm_device_info` and auto-tuning.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub cpu_threads: u32,
    pub logical_cpus: u32,
    pub cuda_available: bool,
    pub cuda_device_count: u32,
    pub vulkan_available: bool,
    pub npu_available: bool,
    pub npu_name: String,
    pub nvidia_gpu: bool,
    pub llama_compiled: bool,
    pub cuda_compiled: bool,
    pub vulkan_compiled: bool,
    pub hybrid_strategy: String,
    pub npu_note: String,
}

impl DeviceInfo {
    pub fn probe() -> Self {
        let logical = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        let cpu_threads = logical.saturating_sub(1).max(1);

        #[cfg(feature = "cuda")]
        let (cuda_available, cuda_device_count) = cuda_status();
        #[cfg(not(feature = "cuda"))]
        let (cuda_available, cuda_device_count) = (false, 0u32);

        let vulkan_compiled = cfg!(feature = "vulkan");
        let vulkan_available = vulkan_compiled && probe_vulkan_runtime();

        let (npu_available, npu_name) = probe_npu();
        let nvidia_gpu = probe_nvidia_gpu();

        let cuda_compiled = cfg!(feature = "cuda");
        let llama_compiled = cfg!(feature = "llama");

        let hybrid_strategy = pick_hybrid_strategy(
            cuda_compiled,
            cuda_available,
            vulkan_compiled,
            vulkan_available,
            nvidia_gpu,
            npu_available,
        );

        let npu_note = if npu_available {
            "NPU used for RAG via VitisAI sidecar (batched query embed). LLM on RTX Vulkan.".into()
        } else {
            String::new()
        };

        Self {
            cpu_threads,
            logical_cpus: logical,
            cuda_available,
            cuda_device_count,
            vulkan_available,
            npu_available,
            npu_name,
            nvidia_gpu,
            llama_compiled,
            cuda_compiled,
            vulkan_compiled,
            hybrid_strategy,
            npu_note,
        }
    }

    pub fn to_map(&self) -> Vec<(&'static str, String)> {
        vec![
            ("cpu_threads", self.cpu_threads.to_string()),
            ("logical_cpus", self.logical_cpus.to_string()),
            ("cuda_available", self.cuda_available.to_string()),
            ("cuda_devices", self.cuda_device_count.to_string()),
            ("vulkan_available", self.vulkan_available.to_string()),
            ("vulkan_build", self.vulkan_compiled.to_string()),
            ("npu_available", self.npu_available.to_string()),
            ("npu_name", self.npu_name.clone()),
            ("nvidia_gpu", self.nvidia_gpu.to_string()),
            ("llama", self.llama_compiled.to_string()),
            ("cuda_build", self.cuda_compiled.to_string()),
            ("hybrid_strategy", self.hybrid_strategy.clone()),
            ("npu_note", self.npu_note.clone()),
        ]
    }

    pub fn prefer_gpu(&self) -> bool {
        (self.cuda_compiled && self.cuda_available)
            || (self.vulkan_compiled && self.vulkan_available)
    }
}

fn pick_hybrid_strategy(
    cuda_compiled: bool,
    cuda_available: bool,
    vulkan_compiled: bool,
    vulkan_available: bool,
    nvidia_gpu: bool,
    npu_available: bool,
) -> String {
    if cuda_compiled && cuda_available {
        return "gpu_cuda+cpu_layers".into();
    }
    if vulkan_compiled && vulkan_available {
        if npu_available {
            return "gpu_vulkan+npu_standby+cpu_layers".into();
        }
        return "gpu_vulkan+cpu_layers".into();
    }
    if nvidia_gpu && (!cuda_compiled || !cuda_available) && (!vulkan_compiled || !vulkan_available) {
        return "cpu_only_gpu_present_install_cuda_or_vulkan_sdk".into();
    }
    if npu_available {
        return "cpu_only_npu_standby_for_onnx".into();
    }
    "cpu_only".into()
}

#[cfg(feature = "cuda")]
fn cuda_status() -> (bool, u32) {
    match candle_core::Device::cuda_if_available(0) {
        Ok(candle_core::Device::Cuda(_)) => (true, 1),
        _ => (false, 0),
    }
}

fn probe_vulkan_runtime() -> bool {
    #[cfg(windows)]
    {
        if std::path::Path::new(r"C:\Windows\System32\vulkan-1.dll").exists() {
            return true;
        }
        if std::env::var("VULKAN_SDK").is_ok() {
            return true;
        }
        if let Ok(entries) = std::fs::read_dir(r"C:\VulkanSDK") {
            return entries.filter_map(Result::ok).any(|e| e.path().is_dir());
        }
        false
    }
    #[cfg(not(windows))]
    {
        std::env::var("VULKAN_SDK").is_ok()
            || std::path::Path::new("/usr/lib/x86_64-linux-gnu/libvulkan.so.1").exists()
    }
}

fn probe_nvidia_gpu() -> bool {
    std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn probe_npu() -> (bool, String) {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -match 'NPU Compute' -and $_.Status -eq 'OK' } | Select-Object -First 1 -ExpandProperty FriendlyName)",
            ])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !name.is_empty() {
                    return (true, name);
                }
            }
        }
        // AMD Phoenix/Hawk Point NPU PCI id
        let reg = std::process::Command::new("reg")
            .args(["query", r"HKLM\SYSTEM\CurrentControlSet\Enum\PCI\VEN_1022&DEV_1502"])
            .output();
        if let Ok(o) = reg {
            if o.status.success() {
                return (true, "AMD XDNA NPU (VEN_1022&DEV_1502)".into());
            }
        }
        (false, String::new())
    }
    #[cfg(target_os = "linux")]
    {
        let path = std::path::Path::new("/dev/accel/accel0");
        if path.exists() {
            return (true, "Linux accel device".into());
        }
        (false, String::new())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        (false, String::new())
    }
}

/// Apply device-aware defaults when fields are unset (`auto` mode).
pub fn resolve_load_options(model_path: &Path, mut opts: LoadOptions) -> LoadOptions {
    let dev = DeviceInfo::probe();

    if opts.threads.is_none() {
        opts.threads = Some(dev.cpu_threads);
    }

    if opts.auto {
        if !opts.cpu && dev.prefer_gpu() {
            opts.cpu = false;
            if opts.n_gpu_layers.is_none() {
                opts.use_fit = true;
            }
        } else if !opts.cpu && cfg!(feature = "llama") {
            opts.cpu = true;
            opts.n_gpu_layers = Some(0);
        } else if !opts.cpu {
            opts.cpu = false;
        }
    } else if !opts.cpu && opts.n_gpu_layers.is_none() {
        opts.n_gpu_layers = Some(u32::MAX);
    }

    if opts.n_gpu_layers.is_some() {
        opts.use_fit = false;
    }

    if opts.n_ctx.is_none() {
        opts.n_ctx = Some(suggest_ctx(model_path, &dev));
    }

    opts
}

fn suggest_ctx(model_path: &Path, dev: &DeviceInfo) -> u32 {
    let file_gb = std::fs::metadata(model_path)
        .map(|m| m.len())
        .unwrap_or(0) as f64
        / (1024.0 * 1024.0 * 1024.0);

    let base = if dev.prefer_gpu() {
        if file_gb > 12.0 {
            4096
        } else if file_gb > 6.0 {
            3072
        } else {
            4096
        }
    } else if file_gb > 8.0 {
        2048
    } else {
        3072
    };
    base.max(512)
}
