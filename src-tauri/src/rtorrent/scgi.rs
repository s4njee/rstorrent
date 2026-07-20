//! SCGI transport for rtorrent's XML-RPC endpoint.
//!
//! rtorrent exposes XML-RPC over SCGI, not HTTP, on either a unix socket
//! (`scgi_local`, preferred) or a TCP port (`scgi_port`). One request/response
//! per connection: we frame the XML body as an SCGI netstring, write it, and
//! read the CGI-style reply until the daemon closes the socket.
//!
//! Netstring header block (NUL-separated, `CONTENT_LENGTH` first, then `SCGI 1`):
//!   `"<hdrlen>:" CONTENT_LENGTH \0 <bodylen> \0 SCGI \0 1 \0 "," <body>`

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixStream};
use tokio::time::timeout;

use super::xmlrpc::{self, Value};
use super::{Result, RtorrentError};
use crate::ipc::Transport;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Encode a method call, send it over SCGI, and parse the XML-RPC response.
pub async fn call(transport: &Transport, method: &str, params: &[Value]) -> Result<Value> {
    let body = xmlrpc::method_call(method, params);
    let raw = request(transport, &body).await?;
    let xml = split_cgi_body(&raw)?;
    xmlrpc::parse_response(xml)
}

/// Send a raw XML body and return the full response bytes (headers + body).
async fn request(transport: &Transport, body: &str) -> Result<Vec<u8>> {
    let frame = build_frame(body);
    match transport {
        Transport::UnixSocket { path } => {
            let stream = timeout(CONNECT_TIMEOUT, UnixStream::connect(path))
                .await
                .map_err(|_| RtorrentError::Timeout)?
                .map_err(|e| RtorrentError::Unreachable(format!("{path}: {e}")))?;
            exchange(stream, &frame).await
        }
        Transport::Tcp { host, port } => {
            let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), *port)))
                .await
                .map_err(|_| RtorrentError::Timeout)?
                .map_err(|e| RtorrentError::Unreachable(format!("{host}:{port}: {e}")))?;
            exchange(stream, &frame).await
        }
        // Unreachable in practice: `transport::call` routes HTTP to `http.rs`.
        // Surfaced as an error rather than a panic in case a future caller
        // bypasses the dispatcher.
        Transport::Http { .. } => Err(RtorrentError::Protocol(
            "HTTP transport routed into the SCGI path".into(),
        )),
    }
}

/// Write the framed request and read the response to EOF (daemon closes it).
async fn exchange<S>(mut stream: S, frame: &[u8]) -> Result<Vec<u8>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let io = async {
        stream.write_all(frame).await?;
        stream.flush().await?;
        let mut out = Vec::with_capacity(8 * 1024);
        stream.read_to_end(&mut out).await?;
        Ok::<_, std::io::Error>(out)
    };
    timeout(READ_TIMEOUT, io)
        .await
        .map_err(|_| RtorrentError::Timeout)?
        .map_err(|e| RtorrentError::Protocol(e.to_string()))
}

/// Build the SCGI netstring frame around an XML body.
fn build_frame(body: &str) -> Vec<u8> {
    let mut headers = Vec::new();
    let clen = body.len().to_string();
    // CONTENT_LENGTH must be the first header per the SCGI spec.
    headers.extend_from_slice(b"CONTENT_LENGTH");
    headers.push(0);
    headers.extend_from_slice(clen.as_bytes());
    headers.push(0);
    headers.extend_from_slice(b"SCGI");
    headers.push(0);
    headers.extend_from_slice(b"1");
    headers.push(0);

    let mut frame = Vec::with_capacity(headers.len() + body.len() + 16);
    frame.extend_from_slice(headers.len().to_string().as_bytes());
    frame.push(b':');
    frame.extend_from_slice(&headers);
    frame.push(b','); // netstring terminator
    frame.extend_from_slice(body.as_bytes());
    frame
}

/// Strip the CGI response headers, returning the XML body slice.
fn split_cgi_body(raw: &[u8]) -> Result<&[u8]> {
    if let Some(pos) = find(raw, b"\r\n\r\n") {
        return Ok(&raw[pos + 4..]);
    }
    if let Some(pos) = find(raw, b"\n\n") {
        return Ok(&raw[pos + 2..]);
    }
    Err(RtorrentError::Protocol(
        "missing CGI header separator in SCGI response".into(),
    ))
}

/// First index of `needle` within `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_has_content_length_first_and_terminator() {
        let frame = build_frame("<x/>");
        let s = String::from_utf8_lossy(&frame);
        // "<hdrlen>:CONTENT_LENGTH\0..."
        let colon = frame.iter().position(|&b| b == b':').unwrap();
        assert_eq!(&frame[colon + 1..colon + 15], b"CONTENT_LENGTH");
        // body appears after the netstring comma.
        assert!(s.ends_with(",<x/>"));
    }

    #[test]
    fn splits_cgi_body_crlf() {
        let raw = b"Content-Type: text/xml\r\n\r\n<hi/>";
        assert_eq!(split_cgi_body(raw).unwrap(), b"<hi/>");
    }

    #[test]
    fn splits_cgi_body_lf() {
        let raw = b"Status: 200 OK\n\n<hi/>";
        assert_eq!(split_cgi_body(raw).unwrap(), b"<hi/>");
    }

    #[test]
    fn missing_separator_errors() {
        assert!(split_cgi_body(b"no-separator-here").is_err());
    }
}
