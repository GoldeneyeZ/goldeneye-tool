use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::json;
use thiserror::Error;

use crate::assets;
use crate::backend::{ApiBackend, ApiRequest};

const DEFAULT_MAX_HEADER_BYTES: usize = 32 * 1_024;
const DEFAULT_MAX_BODY_BYTES: usize = 1_024 * 1_024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_POLL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind_address: SocketAddr,
    pub base_path: String,
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
}

impl ServerConfig {
    #[must_use]
    pub fn new(bind_address: SocketAddr) -> Self {
        Self {
            bind_address,
            base_path: String::new(),
            max_header_bytes: DEFAULT_MAX_HEADER_BYTES,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }

    /// Configures a URL prefix shared by static assets and API routes.
    ///
    /// # Errors
    ///
    /// Returns an error for absolute URLs, query fragments, control bytes, or traversal segments.
    pub fn with_base_path(mut self, base_path: impl AsRef<str>) -> Result<Self, HttpError> {
        self.base_path = normalize_base_path(base_path.as_ref())?;
        Ok(self)
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::new(SocketAddr::from(([127, 0, 0, 1], 7878)))
    }
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("HTTP I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("invalid HTTP base path: {0}")]
    InvalidBasePath(String),
}

#[derive(Debug, Clone, Default)]
pub struct ShutdownHandle(Arc<AtomicBool>);

impl ShutdownHandle {
    pub fn shutdown(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_shutdown(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub struct BoundServer<B> {
    listener: TcpListener,
    config: ServerConfig,
    backend: Arc<B>,
    shutdown: ShutdownHandle,
}

impl<B: ApiBackend> BoundServer<B> {
    /// Binds the HTTP listener without entering the accept loop.
    ///
    /// # Errors
    ///
    /// Returns a socket error when the listener cannot bind or enter non-blocking mode.
    pub fn bind(config: ServerConfig, backend: B) -> Result<Self, HttpError> {
        let listener = TcpListener::bind(config.bind_address)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            config,
            backend: Arc::new(backend),
            shutdown: ShutdownHandle::default(),
        })
    }

    /// Returns the effective listener address, including an ephemeral port.
    ///
    /// # Errors
    ///
    /// Returns a socket error when the local address cannot be read.
    pub fn local_addr(&self) -> Result<SocketAddr, HttpError> {
        Ok(self.listener.local_addr()?)
    }

    #[must_use]
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.shutdown.clone()
    }

    /// Serves requests until the shutdown handle is triggered.
    ///
    /// # Errors
    ///
    /// Returns an accept-loop socket error. Malformed individual requests receive HTTP errors.
    pub fn run(self) -> Result<(), HttpError> {
        while !self.shutdown.is_shutdown() {
            match self.listener.accept() {
                Ok((mut stream, _peer)) => {
                    stream.set_read_timeout(Some(READ_TIMEOUT))?;
                    stream.set_write_timeout(Some(READ_TIMEOUT))?;
                    handle_connection(&mut stream, &self.config, self.backend.as_ref());
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(IDLE_POLL);
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

fn handle_connection<B: ApiBackend>(stream: &mut TcpStream, config: &ServerConfig, backend: &B) {
    let response = match read_request(stream, config) {
        Ok(request) => dispatch(request, config, backend),
        Err(failure) => Payload::json(failure.status, json!({ "error": failure.message })),
    };
    let _ = write_response(stream, &response);
}

fn dispatch<B: ApiBackend>(request: ParsedRequest, config: &ServerConfig, backend: &B) -> Payload {
    let Some(scoped_path) = strip_base_path(&request.path, &config.base_path) else {
        return Payload::json(404, json!({ "error": "not found" }));
    };

    if request.method == "OPTIONS" && (scoped_path == "/rpc" || scoped_path.starts_with("/api/")) {
        return Payload::empty(204);
    }

    if scoped_path == "/rpc" || scoped_path.starts_with("/api/") {
        let api_request = ApiRequest {
            method: request.method,
            path: scoped_path,
            query: request.query,
            headers: request.headers,
            body: request.body,
        };
        return match backend.handle(&api_request) {
            Ok(response) => Payload::json(response.status, response.body),
            Err(error) => Payload::json(error.status, json!({ "error": error.message })),
        };
    }

    if request.method != "GET" && request.method != "HEAD" {
        return Payload::json(405, json!({ "error": "method not allowed" }));
    }
    static_payload(&scoped_path, &config.base_path, request.method == "HEAD")
}

fn static_payload(path: &str, base_path: &str, head: bool) -> Payload {
    if path == "/runtime-config.js" {
        let encoded = serde_json::to_string(base_path).expect("base path serializes");
        let body = format!("globalThis.__GOLDENEYE_UI_CONFIG__ = {{ apiBasePath: {encoded} }};\n")
            .into_bytes();
        return Payload::bytes(
            200,
            "text/javascript; charset=utf-8",
            body,
            "no-store",
            head,
        );
    }

    let requested = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if !safe_asset_path(requested) {
        return Payload::json(400, json!({ "error": "invalid asset path" }));
    }

    if requested == "index.html" || (!requested.contains('.') && assets::find(requested).is_none())
    {
        let Some(index) = assets::find("index.html") else {
            return Payload::json(500, json!({ "error": "UI index missing" }));
        };
        let html = rewrite_index(index.bytes, base_path);
        return Payload::bytes(
            200,
            assets::content_type("index.html"),
            html,
            "no-store",
            head,
        );
    }

    let Some(asset) = assets::find(requested) else {
        return Payload::json(404, json!({ "error": "not found" }));
    };
    let cache = if requested.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    Payload::bytes(
        200,
        assets::content_type(requested),
        asset.bytes.to_vec(),
        cache,
        head,
    )
}

fn rewrite_index(bytes: &[u8], base_path: &str) -> Vec<u8> {
    let html = String::from_utf8_lossy(bytes);
    let asset_prefix = format!("{base_path}/assets/");
    let runtime = format!("{base_path}/runtime-config.js");
    html.replace("/assets/", &asset_prefix)
        .replace("./runtime-config.js", &runtime)
        .into_bytes()
}

#[derive(Debug)]
struct ParsedRequest {
    method: String,
    path: String,
    query: BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct RequestFailure {
    status: u16,
    message: &'static str,
}

// The bounded parser stays linear so header/body ownership and size checks remain auditable.
#[allow(clippy::too_many_lines)]
fn read_request(
    stream: &mut TcpStream,
    config: &ServerConfig,
) -> Result<ParsedRequest, RequestFailure> {
    let mut bytes = Vec::with_capacity(4_096);
    let header_end = loop {
        let mut chunk = [0_u8; 4_096];
        let read = stream.read(&mut chunk).map_err(|_| RequestFailure {
            status: 400,
            message: "request read failed",
        })?;
        if read == 0 {
            return Err(RequestFailure {
                status: 400,
                message: "incomplete request",
            });
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&bytes) {
            break index;
        }
        if bytes.len() > config.max_header_bytes {
            return Err(RequestFailure {
                status: 431,
                message: "request headers too large",
            });
        }
    };

    if header_end > config.max_header_bytes {
        return Err(RequestFailure {
            status: 431,
            message: "request headers too large",
        });
    }
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| RequestFailure {
        status: 400,
        message: "invalid request headers",
    })?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or(RequestFailure {
        status: 400,
        message: "missing request line",
    })?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let version = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || method.is_empty()
        || !method.bytes().all(|byte| byte.is_ascii_uppercase())
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
    {
        return Err(RequestFailure {
            status: 400,
            message: "invalid request line",
        });
    }

    let mut headers = BTreeMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(RequestFailure {
                status: 400,
                message: "invalid request header",
            });
        };
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || headers.insert(name, value.trim().to_owned()).is_some() {
            return Err(RequestFailure {
                status: 400,
                message: "duplicate or invalid request header",
            });
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(RequestFailure {
            status: 400,
            message: "transfer encoding is not supported",
        });
    }
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| RequestFailure {
            status: 400,
            message: "invalid content length",
        })?
        .unwrap_or(0);
    if content_length > config.max_body_bytes {
        return Err(RequestFailure {
            status: 413,
            message: "request body too large",
        });
    }

    let body_start = header_end + 4;
    while bytes.len().saturating_sub(body_start) < content_length {
        let mut chunk = [0_u8; 8_192];
        let read = stream.read(&mut chunk).map_err(|_| RequestFailure {
            status: 400,
            message: "request body read failed",
        })?;
        if read == 0 {
            return Err(RequestFailure {
                status: 400,
                message: "incomplete request body",
            });
        }
        bytes.extend_from_slice(&chunk[..read]);
    }

    let (raw_path, raw_query) = target.split_once('?').unwrap_or((&target, ""));
    let path = percent_decode(raw_path, false)?;
    if !safe_request_path(&path) {
        return Err(RequestFailure {
            status: 400,
            message: "invalid request path",
        });
    }
    let query = parse_query(raw_query)?;
    Ok(ParsedRequest {
        method,
        path,
        query,
        headers,
        body: bytes[body_start..body_start + content_length].to_vec(),
    })
}

fn parse_query(raw: &str) -> Result<BTreeMap<String, String>, RequestFailure> {
    let mut query = BTreeMap::new();
    if raw.is_empty() {
        return Ok(query);
    }
    for pair in raw.split('&').take(256) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(key, true)?;
        let value = percent_decode(value, true)?;
        if key.is_empty() || query.insert(key, value).is_some() {
            return Err(RequestFailure {
                status: 400,
                message: "duplicate or invalid query parameter",
            });
        }
    }
    Ok(query)
}

fn percent_decode(raw: &str, plus_as_space: bool) -> Result<String, RequestFailure> {
    let source = raw.as_bytes();
    let mut decoded = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'%' if index + 2 < source.len() => {
                let high = hex(source[index + 1]);
                let low = hex(source[index + 2]);
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(RequestFailure {
                        status: 400,
                        message: "invalid percent encoding",
                    });
                };
                decoded.push(high * 16 + low);
                index += 3;
            }
            b'%' => {
                return Err(RequestFailure {
                    status: 400,
                    message: "invalid percent encoding",
                });
            }
            b'+' if plus_as_space => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| RequestFailure {
        status: 400,
        message: "request target is not UTF-8",
    })
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn normalize_base_path(value: &str) -> Result<String, HttpError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return Ok(String::new());
    }
    if trimmed.contains(['?', '#', '\\', '\0'])
        || trimmed.bytes().any(|byte| byte.is_ascii_control())
        || trimmed.contains("://")
    {
        return Err(HttpError::InvalidBasePath(value.to_owned()));
    }
    let normalized = format!("/{}", trimmed.trim_matches('/'));
    if normalized
        .split('/')
        .any(|segment| matches!(segment, "." | ".."))
    {
        return Err(HttpError::InvalidBasePath(value.to_owned()));
    }
    Ok(normalized)
}

fn strip_base_path(path: &str, base_path: &str) -> Option<String> {
    if base_path.is_empty() {
        return Some(path.to_owned());
    }
    if path == base_path {
        return Some("/".to_owned());
    }
    path.strip_prefix(base_path)
        .filter(|suffix| suffix.starts_with('/'))
        .map(ToOwned::to_owned)
}

fn safe_request_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.contains(['\\', '\0', '#'])
        && !path.bytes().any(|byte| byte.is_ascii_control())
        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
}

fn safe_asset_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', '\0'])
        && !path
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".."))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

struct Payload {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    cache_control: &'static str,
    head: bool,
}

impl Payload {
    #[allow(clippy::needless_pass_by_value)]
    fn json(status: u16, value: serde_json::Value) -> Self {
        Self::bytes(
            status,
            "application/json; charset=utf-8",
            serde_json::to_vec(&value).expect("JSON value serializes"),
            "no-store",
            false,
        )
    }

    fn empty(status: u16) -> Self {
        Self::bytes(
            status,
            "text/plain; charset=utf-8",
            Vec::new(),
            "no-store",
            false,
        )
    }

    fn bytes(
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
        cache_control: &'static str,
        head: bool,
    ) -> Self {
        Self {
            status,
            content_type,
            body,
            cache_control,
            head,
        }
    }
}

fn write_response(stream: &mut TcpStream, response: &Payload) -> io::Result<()> {
    let status_text = match response.status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; worker-src 'self' blob:\r\n\r\n",
        response.status,
        status_text,
        response.content_type,
        response.body.len(),
        response.cache_control,
    );
    stream.write_all(header.as_bytes())?;
    if !response.head {
        stream.write_all(&response.body)?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::thread;

    use serde_json::json;

    use crate::{ApiBackend, ApiError, ApiRequest, ApiResponse};

    use super::{BoundServer, ServerConfig};

    struct NoopBackend;

    impl ApiBackend for NoopBackend {
        fn handle(&self, _request: &ApiRequest) -> Result<ApiResponse, ApiError> {
            Ok(ApiResponse::ok(json!({})))
        }
    }

    #[test]
    fn shutdown_handle_stops_the_blocking_server_loop() {
        let server = BoundServer::bind(
            ServerConfig::new(SocketAddr::from(([127, 0, 0, 1], 0))),
            NoopBackend,
        )
        .expect("bind server");
        let shutdown = server.shutdown_handle();
        let clone = shutdown.clone();
        let join = thread::spawn(move || server.run());

        clone.shutdown();
        join.join()
            .expect("server thread")
            .expect("server shutdown");

        assert!(shutdown.is_shutdown());
    }
}
