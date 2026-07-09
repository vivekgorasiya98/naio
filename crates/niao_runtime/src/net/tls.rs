//! TLS client connections and socket wrapping.

use super::handles::{alloc_handle, with_handle_mut};
use super::socket::{NetHandle, ReadWriteStream};
use super::{net_error, ok_int, string_arg, NetResult};
use niao_ast::Span;
use niao_errors::codes;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use rustls_native_certs::load_native_certs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

struct TlsStream {
    inner: StreamOwned<ClientConnection, TcpStream>,
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl ReadWriteStream for TlsStream {
    fn set_timeout(&mut self, dur: Option<Duration>) -> Result<(), String> {
        self.inner
            .get_mut()
            .set_read_timeout(dur)
            .map_err(|e| e.to_string())?;
        self.inner
            .get_mut()
            .set_write_timeout(dur)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn default_tls_config() -> Result<ClientConfig, String> {
    let mut roots = RootCertStore::empty();
    let native = load_native_certs();
    for cert in native.certs {
        roots.add(cert).map_err(|e| format!("{e:?}"))?;
    }
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

fn tls_wrap_stream(stream: TcpStream, sni: &str) -> Result<TlsStream, String> {
    let config = default_tls_config()?;
    let server_name =
        ServerName::try_from(sni.to_string()).map_err(|_| "invalid sni host".to_string())?;
    let conn = ClientConnection::new(Arc::new(config), server_name).map_err(|e| e.to_string())?;
    let mut tls = StreamOwned::new(conn, stream);
    tls.flush().map_err(|e| e.to_string())?;
    Ok(TlsStream { inner: tls })
}

pub fn net_tls_connect(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 2, 3, "net_tls_connect", span)?;
    let host = string_arg(args, 0, "net_tls_connect", span)?;
    let port = super::port_arg(args, 1, "net_tls_connect", span)?;
    let sni = if args.len() == 3 {
        string_arg(args, 2, "net_tls_connect", span)?
    } else {
        host.clone()
    };
    let addr = format!("{host}:{port}");
    let tcp = match TcpStream::connect(&addr) {
        Ok(s) => s,
        Err(e) => {
            return Ok(net_error(
                span,
                codes::E1405_NET_TLS,
                "net_tls_error",
                e.to_string(),
            ))
        }
    };
    match tls_wrap_stream(tcp, &sni) {
        Ok(stream) => Ok(ok_int(
            alloc_handle(NetHandle::Tls {
                stream: Box::new(stream),
                timeout_ms: None,
            }) as i64,
        )),
        Err(e) => Ok(net_error(
            span,
            codes::E1405_NET_TLS,
            "net_tls_error",
            e,
        )),
    }
}

pub fn net_tls_wrap(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_tls_wrap", span)?;
    let id = super::handle_arg(args, 0, "net_tls_wrap", span)?;
    let sni = string_arg(args, 1, "net_tls_wrap", span)?;
    with_handle_mut(id, "net_tls_wrap", span, |handle| {
        let placeholder = NetHandle::Tcp {
            stream: TcpStream::connect("127.0.0.1:9").unwrap_or_else(|_| {
                std::net::TcpListener::bind("127.0.0.1:0")
                    .unwrap()
                    .accept()
                    .unwrap()
                    .0
            }),
            timeout_ms: None,
        };
        let NetHandle::Tcp { stream, timeout_ms } = std::mem::replace(handle, placeholder) else {
            return Err("net_tls_wrap() expects a tcp socket".into());
        };
        let tls = tls_wrap_stream(stream, &sni)?;
        *handle = NetHandle::Tls {
            stream: Box::new(tls),
            timeout_ms,
        };
        Ok(ok_int(id as i64))
    })
}

pub fn net_tls_config(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 0, 2, "net_tls_config", span)?;
    let verify = if args.is_empty() {
        true
    } else {
        super::bool_arg(args, 0, "net_tls_config", span)?
    };
    Ok(crate::Value::Bool(verify).ref_cell())
}
