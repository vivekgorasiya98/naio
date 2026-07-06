//! Neko Windows installer — extracts embedded payload to %USERPROFILE%\.neko
//! and adds the bin directory to the user PATH.

use rust_embed::RustEmbed;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(RustEmbed)]
#[folder = "../payload/"]
struct Payload;

fn main() {
    if let Err(e) = run() {
        eprintln!("\nInstall failed: {e}");
        pause();
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    println!("Neko {VERSION} Setup");
    println!("====================\n");

    let install_root = default_install_dir();
    let bin_dir = install_root.join("bin");
    fs::create_dir_all(&bin_dir)?;

    let file_count = extract_all(&install_root)?;
    patch_install_json(&install_root)?;
    add_to_user_path(&bin_dir)?;

    println!("\nInstalled to: {}", install_root.display());
    println!("  Files:      {file_count}");
    println!("  neko.exe:   {}", bin_dir.join("neko.exe").display());
    println!("  nm.exe:     {}", bin_dir.join("nm.exe").display());
    println!("  Libraries:  15 standard libs (pre-installed)");

    if let Ok(out) = Command::new(bin_dir.join("neko.exe"))
        .arg("version")
        .output()
    {
        if out.status.success() {
            print!("  Version:    ");
            io::stdout().write_all(&out.stdout)?;
        }
    }

    println!("\nOpen a NEW Command Prompt or PowerShell window, then run:");
    println!("  neko version");
    println!("  neko run examples\\hello.neko");
    println!("\nDone.");
    pause();
    Ok(())
}

fn default_install_dir() -> PathBuf {
    if let Ok(dir) = env::var("NEKO_INSTALL_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(profile) = env::var("USERPROFILE") {
        return PathBuf::from(profile).join(".neko");
    }
    PathBuf::from(".neko")
}

fn extract_all(root: &Path) -> io::Result<usize> {
    let mut count = 0usize;
    for file in Payload::iter() {
        let rel = file.as_ref();
        if rel.is_empty() {
            continue;
        }
        let dest = root.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = Payload::get(rel).expect("embedded file");
        fs::write(&dest, data.data.as_ref())?;
        count += 1;
        if count % 10 == 0 || rel.ends_with(".exe") {
            println!("  {}", rel.replace('/', "\\"));
        }
    }
    Ok(count)
}

fn patch_install_json(root: &Path) -> io::Result<()> {
    let path = root.join("install.json");
    if !path.is_file() {
        return Ok(());
    }
    let text = fs::read_to_string(&path)?;
    let root_str = root.to_string_lossy().replace('\\', "\\\\");
    let patched = text.replace("%USERPROFILE%\\\\.neko", &root_str);
    let patched = patched.replace("%USERPROFILE%\\.neko", &root.display().to_string());
    fs::write(path, patched)
}

fn add_to_user_path(bin_dir: &Path) -> io::Result<()> {
    let bin = bin_dir.to_string_lossy().to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env, _) = hkcu.create_subkey("Environment")?;
    let path: String = env.get_value("Path").unwrap_or_default();

    let already = env::split_paths(&path).any(|p| p == bin_dir);
    if already {
        println!("\nPATH already contains {}", bin);
        return Ok(());
    }

    let new_path = if path.is_empty() {
        bin.clone()
    } else {
        format!("{path};{bin}")
    };
    env.set_value("Path", &new_path)?;
    broadcast_env_change();
    println!("\nAdded to user PATH: {bin}");
    Ok(())
}

fn broadcast_env_change() {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        extern "system" {
            fn SendMessageTimeoutW(
                hwnd: isize,
                msg: u32,
                wparam: usize,
                lparam: *const u16,
                flags: u32,
                timeout: u32,
                pdw_result: *mut usize,
            ) -> isize;
        }
        let wide: Vec<u16> = OsStr::new("Environment")
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            SendMessageTimeoutW(
                0xffff,
                0x001A,
                0,
                wide.as_ptr(),
                0x0002,
                5000,
                std::ptr::null_mut(),
            );
        }
    }
}

fn pause() {
    print!("\nPress Enter to close...");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
}
