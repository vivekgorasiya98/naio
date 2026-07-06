//! SMTP client via `lettre`.

use super::{net_error, ok_nil, string_arg, NetResult};
use lettre::message::header::ContentType;
use lettre::message::SinglePart;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use neko_ast::Span;
use neko_errors::codes;

pub fn net_smtp_send(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 6, 7, "net_smtp_send", span)?;
    let host = string_arg(args, 0, "net_smtp_send", span)?;
    let port = super::port_arg(args, 1, "net_smtp_send", span)?;
    let from = string_arg(args, 2, "net_smtp_send", span)?;
    let to = string_arg(args, 3, "net_smtp_send", span)?;
    let subject = string_arg(args, 4, "net_smtp_send", span)?;
    let body = string_arg(args, 5, "net_smtp_send", span)?;

    let email = Message::builder()
        .from(from.parse().map_err(|e: lettre::address::AddressError| {
            super::RuntimeError::at(span, codes::E1401_NET_ERROR, e.to_string())
        })?)
        .to(to.parse().map_err(|e: lettre::address::AddressError| {
            super::RuntimeError::at(span, codes::E1401_NET_ERROR, e.to_string())
        })?)
        .subject(subject)
        .singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_PLAIN)
                .body(body),
        )
        .map_err(|e| super::RuntimeError::at(span, codes::E1401_NET_ERROR, e.to_string()))?;

    let mut builder = SmtpTransport::relay(&host)
        .map_err(|e| super::RuntimeError::at(span, codes::E1401_NET_ERROR, e.to_string()))?
        .port(port);

    if args.len() == 7 {
        let opts = super::object_arg(args, 6, "net_smtp_send", span)?;
        if let Ok(user) = super::object_string_field(&opts, "user", span) {
            if let Ok(pass) = super::object_string_field(&opts, "password", span) {
                builder = builder.credentials(Credentials::new(user, pass));
            }
        }
    }

    let mailer = builder.build();
    match mailer.send(&email) {
        Ok(_) => Ok(ok_nil()),
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}
