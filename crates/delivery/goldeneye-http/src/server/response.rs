use super::{TcpStream, Write, io};

pub(super) struct Payload {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    cache_control: &'static str,
    head: bool,
}

impl Payload {
    #[allow(clippy::needless_pass_by_value)]
    pub(super) fn json(status: u16, value: serde_json::Value) -> Self {
        Self::bytes(
            status,
            "application/json; charset=utf-8",
            serde_json::to_vec(&value).expect("JSON value serializes"),
            "no-store",
            false,
        )
    }

    pub(super) fn empty(status: u16) -> Self {
        Self::bytes(
            status,
            "text/plain; charset=utf-8",
            Vec::new(),
            "no-store",
            false,
        )
    }

    pub(super) fn bytes(
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

pub(super) fn write_response(stream: &mut TcpStream, response: &Payload) -> io::Result<()> {
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
