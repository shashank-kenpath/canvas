//! HTTPS requests to the Inkboard server, with the session token. Desktop
//! and Android speak HTTP over their own TLS sockets (blocking, on worker
//! threads); in a browser the page's `fetch` does the work.

use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
const MAX_RESPONSE: usize = 40 << 20;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait Stream: Read + Write + Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Read + Write + Send> Stream for T {}

/// A failed request: the HTTP status (0 when the server was not reached),
/// a message to show and the server's error code.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    pub code: String,
}

impl ApiError {
    pub fn offline(message: impl Into<String>) -> Self {
        ApiError { status: 0, message: message.into(), code: "offline".into() }
    }

    pub fn signed_out(&self) -> bool {
        self.status == 401
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: std::sync::OnceLock<Arc<rustls::ClientConfig>> = std::sync::OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            Arc::new(
                rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .expect("tls versions")
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

/// Where requests go: "https://host[:port]" or, for local testing, "http://host:port".
#[derive(Clone, Debug)]
pub struct Origin {
    pub tls: bool,
    pub host: String,
    pub port: u16,
}

impl Origin {
    pub fn parse(base: &str) -> Result<Origin, String> {
        let (tls, rest) = if let Some(r) = base.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = base.strip_prefix("http://") {
            (false, r)
        } else {
            return Err("The server address must start with https://".into());
        };
        let authority = rest.split('/').next().unwrap_or_default();
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h, p.parse().map_err(|_| "Bad server address")?),
            _ => (authority, if tls { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err("Bad server address".into());
        }
        Ok(Origin { tls, host: host.to_string(), port })
    }

    pub fn authority(&self) -> String {
        if self.port == if self.tls { 443 } else { 80 } {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// Opens a connection. The second value is a handle for changing timeouts.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn connect(&self, timeout: Duration) -> Result<(Box<dyn Stream>, TcpStream), String> {
        let addrs: Vec<SocketAddr> = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|_| "No internet connection".to_string())?
            .collect();
        let mut last = "No internet connection".to_string();
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(tcp) => {
                    let _ = tcp.set_nodelay(true);
                    let _ = tcp.set_read_timeout(Some(Duration::from_secs(30)));
                    let _ = tcp.set_write_timeout(Some(Duration::from_secs(20)));
                    let handle = tcp.try_clone().map_err(|e| e.to_string())?;
                    if !self.tls {
                        return Ok((Box::new(tcp), handle));
                    }
                    let name = rustls::pki_types::ServerName::try_from(self.host.clone()).map_err(|e| e.to_string())?;
                    let conn = rustls::ClientConnection::new(tls_config(), name).map_err(|e| e.to_string())?;
                    return Ok((Box::new(rustls::StreamOwned::new(conn, tcp)), handle));
                }
                Err(e) => last = e.to_string(),
            }
        }
        Err(format!("Could not reach the server ({last})"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Decodes a chunked body; `None` while incomplete.
#[cfg(not(target_arch = "wasm32"))]
fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line = find(data, b"\r\n")?;
        let size_text = std::str::from_utf8(&data[..line]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        data = &data[line + 2..];
        if size == 0 {
            return Some(out);
        }
        if data.len() < size + 2 {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

/// One request. `body` is (content type, bytes).
#[cfg(not(target_arch = "wasm32"))]
pub fn call(
    base: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<(&str, &[u8])>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), ApiError> {
    let origin = Origin::parse(base).map_err(ApiError::offline)?;
    let (mut s, _) = origin.connect(timeout).map_err(ApiError::offline)?;
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Inkboard\r\nAccept: application/json\r\nConnection: close\r\n", origin.authority());
    if let Some(token) = token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    let (ctype, bytes) = body.unwrap_or(("application/json", &[]));
    if body.is_some() || method != "GET" {
        head.push_str(&format!("Content-Type: {ctype}\r\nContent-Length: {}\r\n", bytes.len()));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(bytes);
    let offline = |e: std::io::Error| ApiError::offline(format!("Connection lost ({e})"));
    s.write_all(&out).map_err(offline)?;
    s.flush().map_err(offline)?;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        if let Some(end) = find(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
            let chunked = head.lines().any(|l| l.starts_with("transfer-encoding:") && l.contains("chunked"));
            let length = head.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok()));
            let status = head.split(' ').nth(1).and_then(|s| s.parse::<u16>().ok()).ok_or_else(|| ApiError::offline("Bad response"))?;
            let body = &buf[end + 4..];
            let done = if chunked {
                dechunk(body)
            } else if let Some(length) = length {
                (body.len() >= length).then(|| body[..length].to_vec())
            } else if matches!(status, 101 | 204 | 304) {
                Some(Vec::new())
            } else {
                None
            };
            if let Some(body) = done {
                return Ok((status, body));
            }
        }
        if buf.len() > MAX_RESPONSE {
            return Err(ApiError::offline("Response too large"));
        }
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(offline(e)),
        }
    }
    // Closed without a length: everything after the head is the body.
    let end = find(&buf, b"\r\n\r\n").ok_or_else(|| ApiError::offline("Bad response"))?;
    let status = String::from_utf8_lossy(&buf[..end]).split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| ApiError::offline("Bad response"))?;
    Ok((status, buf[end + 4..].to_vec()))
}

/// Turns a response into the JSON value, or an error carrying the server's message.
fn json_result(status: u16, bytes: &[u8]) -> Result<Value, ApiError> {
    let value: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(value);
    }
    Err(ApiError {
        status,
        message: value["error"].as_str().map(str::to_string).unwrap_or_else(|| format!("The server answered {status}")),
        code: value["code"].as_str().unwrap_or("error").to_string(),
    })
}

/// A JSON request, blocking until the answer arrives.
#[cfg(not(target_arch = "wasm32"))]
pub fn api_blocking(base: &str, method: &str, path: &str, token: Option<&str>, body: Option<&Value>) -> Result<Value, ApiError> {
    let text = body.map(|b| b.to_string());
    let (status, bytes) = call(base, method, path, token, text.as_ref().map(|t| ("application/json", t.as_bytes())), Duration::from_secs(15))?;
    json_result(status, &bytes)
}

/// A JSON request; errors carry the server's message.
#[cfg(not(target_arch = "wasm32"))]
pub async fn api(base: &str, method: &str, path: &str, token: Option<&str>, body: Option<&Value>) -> Result<Value, ApiError> {
    api_blocking(base, method, path, token, body)
}

/// One request through the page's `fetch`. `body` is (content type, bytes).
#[cfg(target_arch = "wasm32")]
pub async fn call(base: &str, method: &str, path: &str, token: Option<&str>, body: Option<(&str, &[u8])>) -> Result<(u16, Vec<u8>), ApiError> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let offline = |e: wasm_bindgen::JsValue| ApiError::offline(format!("No connection to the server ({})", e.as_string().unwrap_or_else(|| "network error".into())));
    let init = web_sys::RequestInit::new();
    init.set_method(method);
    let headers = web_sys::Headers::new().map_err(offline)?;
    headers.set("Accept", "application/json").map_err(offline)?;
    if let Some(token) = token {
        headers.set("Authorization", &format!("Bearer {token}")).map_err(offline)?;
    }
    if let Some((ctype, bytes)) = body {
        headers.set("Content-Type", ctype).map_err(offline)?;
        init.set_body(&js_sys::Uint8Array::from(bytes).into());
    }
    init.set_headers(&headers);
    let url = format!("{}{path}", base.trim_end_matches('/'));
    let request = web_sys::Request::new_with_str_and_init(&url, &init).map_err(offline)?;
    let window = web_sys::window().ok_or_else(|| ApiError::offline("No window"))?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_request(&request)).await.map_err(offline)?.dyn_into().map_err(offline)?;
    let buffer = JsFuture::from(response.array_buffer().map_err(offline)?).await.map_err(offline)?;
    Ok((response.status(), js_sys::Uint8Array::new(&buffer).to_vec()))
}

/// A JSON request; errors carry the server's message.
#[cfg(target_arch = "wasm32")]
pub async fn api(base: &str, method: &str, path: &str, token: Option<&str>, body: Option<&Value>) -> Result<Value, ApiError> {
    let text = body.map(|b| b.to_string());
    let (status, bytes) = call(base, method, path, token, text.as_ref().map(|t| ("application/json", t.as_bytes()))).await?;
    json_result(status, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_origins() {
        let o = Origin::parse("https://ink.example.com/").unwrap();
        assert_eq!((o.tls, o.host.as_str(), o.port), (true, "ink.example.com", 443));
        assert_eq!(o.authority(), "ink.example.com");
        let o = Origin::parse("http://127.0.0.1:8787").unwrap();
        assert_eq!((o.tls, o.port, o.authority().as_str()), (false, 8787, "127.0.0.1:8787"));
        assert!(Origin::parse("ftp://x").is_err());
    }

    #[test]
    fn decodes_chunks() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
        assert_eq!(dechunk(b"4\r\nWi"), None);
    }
}
