//! DNS resolution and hostname helpers.

use super::{net_error, ok_string, string_arg, NetResult};
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};

pub fn net_resolve(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_resolve", span)?;
    let host = string_arg(args, 0, "net_resolve", span)?;
    let port = super::int_arg(args, 1, "net_resolve", span)?;
    if port < 0 || port > 65535 {
        return Err(super::type_err(
            span,
            "net_resolve() port must be 0..=65535",
        ));
    }
    let target = format!("{host}:{port}");
    match target.to_socket_addrs() {
        Ok(addrs) => {
            let mut out = Vec::new();
            for addr in addrs {
                let mut map = HashMap::new();
                map.insert("ip".into(), ok_string(addr.ip().to_string()));
                map.insert("port".into(), crate::Value::Int(addr.port() as i64).ref_cell());
                let family = if addr.is_ipv4() { "ipv4" } else { "ipv6" };
                map.insert("family".into(), ok_string(family.into()));
                out.push(crate::Value::Object(map).ref_cell());
            }
            Ok(crate::Value::Array(out).ref_cell())
        }
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

pub fn net_hostname(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 0, "net_hostname", span)?;
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".into());
    Ok(ok_string(name))
}

/// Quick connectivity probe used in tests.
#[allow(dead_code)]
pub fn tcp_probe(host: &str, port: u16) -> bool {
    TcpStream::connect((host, port)).is_ok()
}
