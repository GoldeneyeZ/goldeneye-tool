use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::json;
use thiserror::Error;

use crate::backend::{ApiBackend, ApiRequest};

mod request;
mod response;
#[path = "server/assets.rs"]
mod static_assets;
#[cfg(test)]
mod tests;

use request::{ParsedRequest, normalize_base_path, read_request, strip_base_path};
use response::{Payload, write_response};
use static_assets::static_payload;

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
