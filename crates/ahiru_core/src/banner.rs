use crate::router::RouteInfo;

pub fn local_ip_hint() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

pub fn print_startup_banner(host: &str, port: u16, routes: &[RouteInfo], dev: bool, network: bool) {
    eprintln!();
    eprintln!("ahiru-server");
    eprintln!("  local    http://127.0.0.1:{port}");
    if network || host == "0.0.0.0" {
        if let Some(ip) = local_ip_hint() {
            eprintln!("  network  http://{ip}:{port}");
        }
        eprintln!("  bind     0.0.0.0:{port}");
    } else {
        eprintln!("  bind     http://{host}:{port}");
    }
    if dev {
        eprintln!("  mode     dev (auto-reload on file changes)");
    }
    eprintln!("  routes");
    if routes.is_empty() {
        eprintln!("    (none)");
    } else {
        for r in routes {
            let ws = if r.websocket { " [ws]" } else { "" };
            eprintln!("    {} {}{}", r.method, r.path, ws);
        }
    }
    eprintln!("  ctrl+c to stop");
    eprintln!();
}

pub fn log_request(request_id: &str, method: &str, path: &str, status: u16, ms: f64) {
    eprintln!("[{request_id}] {method} {path} → {status} ({ms:.1}ms)");
}
