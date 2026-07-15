use super::{BTreeMap, HttpError, Read, ServerConfig, TcpStream};

#[derive(Debug)]
pub(super) struct ParsedRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) query: BTreeMap<String, String>,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Vec<u8>,
}

#[derive(Debug)]
pub(super) struct RequestFailure {
    pub(super) status: u16,
    pub(super) message: &'static str,
}

// The bounded parser stays linear so header/body ownership and size checks remain auditable.
#[allow(clippy::too_many_lines)]
pub(super) fn read_request(
    stream: &mut TcpStream,
    config: &ServerConfig,
) -> Result<ParsedRequest, RequestFailure> {
    let (mut bytes, header_end) = read_request_head(stream, config.max_header_bytes)?;
    let (method, target, headers) = parse_request_head(&bytes[..header_end])?;
    let content_length = parse_content_length(&headers, config.max_body_bytes)?;
    let body_start = header_end + 4;
    read_request_body(stream, &mut bytes, body_start, content_length)?;

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

fn read_request_head(
    stream: &mut TcpStream,
    max_header_bytes: usize,
) -> Result<(Vec<u8>, usize), RequestFailure> {
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
        if bytes.len() > max_header_bytes {
            return Err(RequestFailure {
                status: 431,
                message: "request headers too large",
            });
        }
    };
    if header_end > max_header_bytes {
        return Err(RequestFailure {
            status: 431,
            message: "request headers too large",
        });
    }
    Ok((bytes, header_end))
}

fn parse_request_head(
    bytes: &[u8],
) -> Result<(String, String, BTreeMap<String, String>), RequestFailure> {
    let header = std::str::from_utf8(bytes).map_err(|_| RequestFailure {
        status: 400,
        message: "invalid request headers",
    })?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or(RequestFailure {
        status: 400,
        message: "missing request line",
    })?;
    let (method, target) = parse_request_line(request_line)?;
    let headers = parse_headers(lines)?;
    if headers.contains_key("transfer-encoding") {
        return Err(RequestFailure {
            status: 400,
            message: "transfer encoding is not supported",
        });
    }
    Ok((method, target, headers))
}

fn parse_request_line(request_line: &str) -> Result<(String, String), RequestFailure> {
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
    Ok((method, target))
}

fn parse_headers<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, String>, RequestFailure> {
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
    Ok(headers)
}

fn parse_content_length(
    headers: &BTreeMap<String, String>,
    max_body_bytes: usize,
) -> Result<usize, RequestFailure> {
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| RequestFailure {
            status: 400,
            message: "invalid content length",
        })?
        .unwrap_or(0);
    if content_length > max_body_bytes {
        return Err(RequestFailure {
            status: 413,
            message: "request body too large",
        });
    }
    Ok(content_length)
}

fn read_request_body(
    stream: &mut TcpStream,
    bytes: &mut Vec<u8>,
    body_start: usize,
    content_length: usize,
) -> Result<(), RequestFailure> {
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
    Ok(())
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

pub(super) fn normalize_base_path(value: &str) -> Result<String, HttpError> {
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

pub(super) fn strip_base_path(path: &str, base_path: &str) -> Option<String> {
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

pub(super) fn safe_asset_path(path: &str) -> bool {
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
