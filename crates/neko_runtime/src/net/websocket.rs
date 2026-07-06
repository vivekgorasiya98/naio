//! WebSocket client via `tungstenite`.

use super::handles::{alloc_handle, remove_handle, with_handle_mut};
use super::socket::NetHandle;
use super::{bytes_to_int_array, net_error, ok_nil, ok_string, string_arg, NetResult};
use neko_ast::Span;
use neko_errors::codes;
use tungstenite::{connect, Message};
use tungstenite::stream::MaybeTlsStream;

pub struct WsConnection {
    pub socket: tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
}

pub fn net_ws_connect(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 1, 2, "net_ws_connect", span)?;
    let url = string_arg(args, 0, "net_ws_connect", span)?;
    match connect(&url) {
        Ok((socket, _resp)) => Ok(crate::Value::Int(
            alloc_handle(NetHandle::WebSocket(WsConnection { socket })) as i64,
        )
        .ref_cell()),
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

pub fn net_ws_send(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_ws_send", span)?;
    let id = super::handle_arg(args, 0, "net_ws_send", span)?;
    let msg = super::payload_arg(args, 1, "net_ws_send", span)?;
    with_handle_mut(id, "net_ws_send", span, |handle| {
        if let NetHandle::WebSocket(ws) = handle {
            let text = String::from_utf8_lossy(&msg).into_owned();
            ws.socket
                .send(Message::Text(text.into()))
                .map_err(|e| e.to_string())?;
            Ok(ok_nil())
        } else {
            Err("not a websocket".into())
        }
    })
}

pub fn net_ws_recv(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_ws_recv", span)?;
    let id = super::handle_arg(args, 0, "net_ws_recv", span)?;
    with_handle_mut(id, "net_ws_recv", span, |handle| {
        if let NetHandle::WebSocket(ws) = handle {
            match ws.socket.read() {
                Ok(Message::Text(s)) => Ok(ok_string(s.to_string())),
                Ok(Message::Binary(b)) => Ok(bytes_to_int_array(b.to_vec())),
                Ok(Message::Close(_)) => Ok(ok_nil()),
                Ok(_) => Ok(ok_string(String::new())),
                Err(e) => Err(e.to_string()),
            }
        } else {
            Err("not a websocket".into())
        }
    })
}

pub fn net_ws_close(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_ws_close", span)?;
    let id = super::handle_arg(args, 0, "net_ws_close", span)?;
    if let Some(NetHandle::WebSocket(mut ws)) = remove_handle(id) {
        let _ = ws.socket.close(None);
        Ok(ok_nil())
    } else {
        Err(crate::RuntimeError::at(
            span,
            codes::E1402_NET_INVALID_HANDLE,
            format!("net_ws_close(): invalid handle {id}"),
        ))
    }
}
