use std::io::{self, Write};
use tokio::net::TcpListener;

const MAX_AUTO_SCAN: u16 = 100;

#[derive(Debug, Clone, Copy)]
pub enum PortBindPolicy {
    /// Try `preferred`, then `preferred+1`, … without prompting.
    AutoNext,
    /// Ask on stdin when `preferred` is busy (user explicitly chose the port).
    Prompt,
}

#[derive(Debug)]
pub enum PortBindError {
    Io(std::io::Error),
    Declined,
    Exhausted { preferred: u16 },
}

impl std::fmt::Display for PortBindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortBindError::Io(e) => write!(f, "{e}"),
            PortBindError::Declined => write!(f, "server not started — port unavailable"),
            PortBindError::Exhausted { preferred } => {
                write!(
                    f,
                    "no free port found in range {preferred}..{}",
                    preferred.saturating_add(MAX_AUTO_SCAN)
                )
            }
        }
    }
}

impl std::error::Error for PortBindError {}

pub fn is_addr_in_use(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::AddrInUse || err.raw_os_error() == Some(10048)
}

pub async fn bind_listener(
    host: &str,
    preferred: u16,
    policy: PortBindPolicy,
) -> Result<(TcpListener, u16), PortBindError> {
    match policy {
        PortBindPolicy::AutoNext => bind_auto_next(host, preferred).await,
        PortBindPolicy::Prompt => bind_with_prompt(host, preferred).await,
    }
}

async fn bind_auto_next(host: &str, preferred: u16) -> Result<(TcpListener, u16), PortBindError> {
    for offset in 0..=MAX_AUTO_SCAN {
        let port = preferred.saturating_add(offset);
        match try_bind(host, port).await {
            Ok(listener) => {
                if offset > 0 {
                    eprintln!("  [!] Port {preferred} in use — using {port}");
                }
                return Ok((listener, port));
            }
            Err(e) if is_addr_in_use(&e) => continue,
            Err(e) => return Err(PortBindError::Io(e)),
        }
    }
    Err(PortBindError::Exhausted { preferred })
}

async fn bind_with_prompt(host: &str, preferred: u16) -> Result<(TcpListener, u16), PortBindError> {
    loop {
        match try_bind(host, preferred).await {
            Ok(listener) => return Ok((listener, preferred)),
            Err(e) if is_addr_in_use(&e) => {
                let choice = prompt_port_busy(host, preferred)?;
                match choice {
                    PortChoice::Use(port) => {
                        return bind_auto_next(host, port).await;
                    }
                    PortChoice::Quit => return Err(PortBindError::Declined),
                }
            }
            Err(e) => return Err(PortBindError::Io(e)),
        }
    }
}

enum PortChoice {
    Use(u16),
    Quit,
}

fn prompt_port_busy(host: &str, port: u16) -> Result<PortChoice, PortBindError> {
    let next = port.saturating_add(1);
    eprintln!();
    eprintln!("Port {port} is already in use on {host}.");
    eprintln!("  [Y] Use next free port (from {next})");
    eprintln!("  [C] Enter a custom port");
    eprintln!("  [N] Quit");
    eprint!("Choice [Y/c/n]: ");
    io::stderr().flush().map_err(PortBindError::Io)?;
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(PortBindError::Io)?;
    match line.trim().to_lowercase().as_str() {
        "" | "y" | "yes" => Ok(PortChoice::Use(next)),
        "c" | "custom" => {
            eprint!("  Port: ");
            io::stderr().flush().map_err(PortBindError::Io)?;
            line.clear();
            io::stdin().read_line(&mut line).map_err(PortBindError::Io)?;
            let custom: u16 = line.trim().parse().map_err(|_| {
                PortBindError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid port number",
                ))
            })?;
            if custom == 0 {
                return Err(PortBindError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "port must be > 0",
                )));
            }
            Ok(PortChoice::Use(custom))
        }
        _ => Ok(PortChoice::Quit),
    }
}

async fn try_bind(host: &str, port: u16) -> Result<TcpListener, std::io::Error> {
    let addr = format!("{host}:{port}");
    TcpListener::bind(addr).await
}
