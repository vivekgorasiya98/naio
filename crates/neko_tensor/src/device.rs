use std::fmt;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Device {
    Cpu,
    Cuda(usize),
    Wgpu,
}

impl Device {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_lowercase();
        if s == "cpu" {
            return Some(Device::Cpu);
        }
        if s == "wgpu" {
            return Some(Device::Wgpu);
        }
        if let Some(rest) = s.strip_prefix("cuda") {
            if rest.is_empty() || rest == ":0" {
                return Some(Device::Cuda(0));
            }
            if let Some(idx) = rest.strip_prefix(':') {
                if let Ok(n) = idx.parse::<usize>() {
                    return Some(Device::Cuda(n));
                }
            }
        }
        None
    }

    pub fn name(self) -> String {
        match self {
            Device::Cpu => "cpu".into(),
            Device::Cuda(i) => format!("cuda:{i}"),
            Device::Wgpu => "wgpu".into(),
        }
    }

    pub fn is_cpu(self) -> bool {
        matches!(self, Device::Cpu)
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

static GLOBAL_DEVICE: Mutex<Device> = Mutex::new(Device::Cpu);

pub fn set_global_device(device: Device) {
    *GLOBAL_DEVICE.lock().unwrap() = device;
}

pub fn global_device() -> Device {
    *GLOBAL_DEVICE.lock().unwrap()
}

pub fn cuda_device_count() -> usize {
    #[cfg(feature = "cuda")]
    {
        cuda::device_count()
    }
    #[cfg(not(feature = "cuda"))]
    {
        0
    }
}
