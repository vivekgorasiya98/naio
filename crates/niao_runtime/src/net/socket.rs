//! TCP and UDP socket builtins.

use super::handles::{alloc_handle, remove_handle, with_handle_mut};
use super::{bytes_to_int_array, net_error, ok_int, ok_nil, string_arg, NetResult};
use niao_ast::Span;
use niao_errors::codes;
use socket2::{Domain, Socket, Type};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::time::Duration;

pub trait ReadWriteStream: Read + Write + Send {
    fn set_timeout(&mut self, dur: Option<Duration>) -> Result<(), String>;
}

impl ReadWriteStream for TcpStream {
    fn set_timeout(&mut self, dur: Option<Duration>) -> Result<(), String> {
        self.set_read_timeout(dur).map_err(|e| e.to_string())?;
        self.set_write_timeout(dur).map_err(|e| e.to_string())?;
        Ok(())
    }
}

pub enum NetHandle {
    Tcp {
        stream: TcpStream,
        timeout_ms: Option<u64>,
    },
    TcpListener {
        listener: TcpListener,
        timeout_ms: Option<u64>,
    },
    Udp {
        socket: UdpSocket,
        timeout_ms: Option<u64>,
    },
    Tls {
        stream: Box<dyn ReadWriteStream>,
        timeout_ms: Option<u64>,
    },
    WebSocket(super::websocket::WsConnection),
}

impl NetHandle {
    pub fn set_timeout_ms(&mut self, ms: Option<u64>) -> Result<(), String> {
        let dur = ms.map(Duration::from_millis);
        match self {
            Self::Tcp { stream, timeout_ms } => {
                stream.set_read_timeout(dur).map_err(|e| e.to_string())?;
                stream.set_write_timeout(dur).map_err(|e| e.to_string())?;
                *timeout_ms = ms;
            }
            Self::TcpListener { timeout_ms, .. } => {
                *timeout_ms = ms;
            }
            Self::Udp { socket, timeout_ms } => {
                socket.set_read_timeout(dur).map_err(|e| e.to_string())?;
                socket.set_write_timeout(dur).map_err(|e| e.to_string())?;
                *timeout_ms = ms;
            }
            Self::Tls { stream, timeout_ms } => {
                stream.set_timeout(dur)?;
                *timeout_ms = ms;
            }
            _ => return Err("handle does not support timeout".into()),
        }
        Ok(())
    }
}

pub fn tcp_handle(stream: TcpStream) -> NetHandle {
    NetHandle::Tcp {
        stream,
        timeout_ms: None,
    }
}

fn create_tcp_socket() -> Result<TcpStream, String> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).map_err(|e| e.to_string())?;
    socket.set_nonblocking(false).map_err(|e| e.to_string())?;
    socket
        .bind(&"0.0.0.0:0".parse::<SocketAddr>().unwrap().into())
        .map_err(|e| e.to_string())?;
    let stream: TcpStream = socket.into();
    Ok(stream)
}

pub fn net_tcp_socket(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 0, "net_tcp_socket", span)?;
    match create_tcp_socket() {
        Ok(stream) => Ok(ok_int(alloc_handle(tcp_handle(stream)) as i64)),
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e,
        )),
    }
}

pub fn net_tcp_connect(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_tcp_connect", span)?;
    let host = string_arg(args, 0, "net_tcp_connect", span)?;
    let port = super::port_arg(args, 1, "net_tcp_connect", span)?;
    let addr = format!("{host}:{port}");
    match TcpStream::connect(&addr) {
        Ok(stream) => Ok(ok_int(alloc_handle(tcp_handle(stream)) as i64)),
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

pub fn net_tcp_bind(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_tcp_bind", span)?;
    let host = string_arg(args, 0, "net_tcp_bind", span)?;
    let port = super::port_arg(args, 1, "net_tcp_bind", span)?;
    let addr = format!("{host}:{port}");
    match TcpListener::bind(&addr) {
        Ok(listener) => Ok(ok_int(
            alloc_handle(NetHandle::TcpListener {
                listener,
                timeout_ms: None,
            }) as i64,
        )),
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

pub fn net_tcp_listen(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_tcp_listen", span)?;
    let id = super::handle_arg(args, 0, "net_tcp_listen", span)?;
    let _backlog = super::int_arg(args, 1, "net_tcp_listen", span)?;
    with_handle_mut(id, "net_tcp_listen", span, |handle| match handle {
        NetHandle::TcpListener { .. } => Ok(ok_nil()),
        _ => Err("not a tcp listener".into()),
    })
}

pub fn net_tcp_accept(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_tcp_accept", span)?;
    let id = super::handle_arg(args, 0, "net_tcp_accept", span)?;
    let stream = with_handle_mut(id, "net_tcp_accept", span, |handle| match handle {
        NetHandle::TcpListener { listener, .. } => listener
            .accept()
            .map(|(stream, _)| stream)
            .map_err(|e| e.to_string()),
        _ => Err("not a tcp listener".into()),
    })?;
    Ok(ok_int(alloc_handle(tcp_handle(stream)) as i64))
}

pub fn net_tcp_send(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_tcp_send", span)?;
    let id = super::handle_arg(args, 0, "net_tcp_send", span)?;
    let data = super::payload_arg(args, 1, "net_tcp_send", span)?;
    with_handle_mut(id, "net_tcp_send", span, |handle| {
        let written = match handle {
            NetHandle::Tcp { stream, .. } => stream.write(&data).map_err(|e| e.to_string())?,
            NetHandle::Tls { stream, .. } => stream.write(&data).map_err(|e| e.to_string())?,
            _ => return Err("not a connected socket".into()),
        };
        Ok(ok_int(written as i64))
    })
}

pub fn net_tcp_recv(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_tcp_recv", span)?;
    let id = super::handle_arg(args, 0, "net_tcp_recv", span)?;
    let n = super::size_arg(args, 1, "net_tcp_recv", span)?;
    with_handle_mut(id, "net_tcp_recv", span, |handle| {
        let mut buf = vec![0u8; n];
        let read = match handle {
            NetHandle::Tcp { stream, .. } => stream.read(&mut buf).map_err(|e| e.to_string())?,
            NetHandle::Tls { stream, .. } => stream.read(&mut buf).map_err(|e| e.to_string())?,
            _ => return Err("not a connected socket".into()),
        };
        buf.truncate(read);
        Ok(bytes_to_int_array(buf))
    })
}

pub fn net_tcp_close(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_tcp_close", span)?;
    let id = super::handle_arg(args, 0, "net_tcp_close", span)?;
    if remove_handle(id).is_some() {
        Ok(ok_nil())
    } else {
        Err(super::RuntimeError::at(
            span,
            codes::E1402_NET_INVALID_HANDLE,
            format!("net_tcp_close(): invalid handle {id}"),
        ))
    }
}

pub fn net_udp_socket(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 0, "net_udp_socket", span)?;
    match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => Ok(ok_int(
            alloc_handle(NetHandle::Udp {
                socket,
                timeout_ms: None,
            }) as i64,
        )),
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

pub fn net_udp_bind(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_udp_bind", span)?;
    let host = string_arg(args, 0, "net_udp_bind", span)?;
    let port = super::port_arg(args, 1, "net_udp_bind", span)?;
    let addr = format!("{host}:{port}");
    match UdpSocket::bind(&addr) {
        Ok(socket) => Ok(ok_int(
            alloc_handle(NetHandle::Udp {
                socket,
                timeout_ms: None,
            }) as i64,
        )),
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

pub fn net_udp_send(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 4, "net_udp_send", span)?;
    let id = super::handle_arg(args, 0, "net_udp_send", span)?;
    let host = string_arg(args, 1, "net_udp_send", span)?;
    let port = super::port_arg(args, 2, "net_udp_send", span)?;
    let data = super::payload_arg(args, 3, "net_udp_send", span)?;
    let target: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e: std::net::AddrParseError| {
            super::RuntimeError::at(span, codes::E1401_NET_ERROR, e.to_string())
        })?;
    with_handle_mut(id, "net_udp_send", span, |handle| match handle {
        NetHandle::Udp { socket, .. } => {
            let n = socket.send_to(&data, target).map_err(|e| e.to_string())?;
            Ok(ok_int(n as i64))
        }
        _ => Err("not a udp socket".into()),
    })
}

pub fn net_udp_recv(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_udp_recv", span)?;
    let id = super::handle_arg(args, 0, "net_udp_recv", span)?;
    let n = super::size_arg(args, 1, "net_udp_recv", span)?;
    with_handle_mut(id, "net_udp_recv", span, |handle| match handle {
        NetHandle::Udp { socket, .. } => {
            let mut buf = vec![0u8; n];
            let (read, _addr) = socket.recv_from(&mut buf).map_err(|e| e.to_string())?;
            buf.truncate(read);
            Ok(bytes_to_int_array(buf))
        }
        _ => Err("not a udp socket".into()),
    })
}

pub fn net_set_timeout(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_set_timeout", span)?;
    let id = super::handle_arg(args, 0, "net_set_timeout", span)?;
    let ms = super::int_arg(args, 1, "net_set_timeout", span)?;
    let timeout = if ms < 0 { None } else { Some(ms as u64) };
    with_handle_mut(id, "net_set_timeout", span, |handle| {
        handle.set_timeout_ms(timeout)?;
        Ok(ok_nil())
    })
}
