use std::io::BufRead;

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

const CONTENT_LENGTH_PREFIX: &[u8] = b"Content-Length:";

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Content-Length header")]
    InvalidHeader,
    #[error("frame contains {size} bytes; limit is {limit}")]
    FrameTooLarge { size: usize, limit: usize },
    #[error("frame ended before declared Content-Length")]
    UnexpectedEof,
}

pub struct FrameReader<R> {
    reader: R,
}

impl<R: BufRead> FrameReader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Reads the next newline-delimited or `Content-Length`-framed message.
    ///
    /// # Errors
    ///
    /// Returns [`FrameError`] when input fails, a header is invalid, a frame exceeds
    /// [`MAX_FRAME_BYTES`], or a declared body ends early.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        let Some(first_line) = self.read_bounded_line()? else {
            return Ok(None);
        };
        let line = trim_line_ending(&first_line);

        if !starts_with_ignore_ascii_case(line, CONTENT_LENGTH_PREFIX) {
            return Ok(Some(line.to_vec()));
        }

        let length = parse_content_length(&line[CONTENT_LENGTH_PREFIX.len()..])?;
        if length > MAX_FRAME_BYTES {
            return Err(FrameError::FrameTooLarge {
                size: length,
                limit: MAX_FRAME_BYTES,
            });
        }

        self.consume_headers()?;

        let mut body = vec![0; length];
        match self.reader.read_exact(&mut body) {
            Ok(()) => Ok(Some(body)),
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                Err(FrameError::UnexpectedEof)
            }
            Err(error) => Err(FrameError::Io(error)),
        }
    }

    fn consume_headers(&mut self) -> Result<(), FrameError> {
        loop {
            let header = self.read_bounded_line()?.ok_or(FrameError::InvalidHeader)?;
            if trim_line_ending(&header).is_empty() {
                return Ok(());
            }
        }
    }

    fn read_bounded_line(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        let mut line = Vec::new();

        loop {
            let available = self.reader.fill_buf()?;
            if available.is_empty() {
                return if line.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(line))
                };
            }

            if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
                let chunk_len = newline + 1;
                let raw_size = line.len().saturating_add(chunk_len);
                let has_cr = if newline > 0 {
                    available[newline - 1] == b'\r'
                } else {
                    line.last() == Some(&b'\r')
                };
                let content_size = raw_size - 1 - usize::from(has_cr);
                if content_size > MAX_FRAME_BYTES {
                    return Err(FrameError::FrameTooLarge {
                        size: content_size,
                        limit: MAX_FRAME_BYTES,
                    });
                }

                line.extend_from_slice(&available[..chunk_len]);
                self.reader.consume(chunk_len);
                return Ok(Some(line));
            }

            let observed_size = line.len().saturating_add(available.len());
            if observed_size > MAX_FRAME_BYTES {
                return Err(FrameError::FrameTooLarge {
                    size: observed_size,
                    limit: MAX_FRAME_BYTES,
                });
            }

            let consumed = available.len();
            line.extend_from_slice(available);
            self.reader.consume(consumed);
        }
    }
}

fn starts_with_ignore_ascii_case(value: &[u8], prefix: &[u8]) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

fn parse_content_length(value: &[u8]) -> Result<usize, FrameError> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .ok_or(FrameError::InvalidHeader)
}

fn trim_line_ending(line: &[u8]) -> &[u8] {
    let Some(without_lf) = line.strip_suffix(b"\n") else {
        return line;
    };
    without_lf.strip_suffix(b"\r").unwrap_or(without_lf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_newline_delimited_json() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
        let mut reader = FrameReader::new(&input[..]);

        assert_eq!(
            reader.next_frame().expect("read").expect("frame"),
            &input[..input.len() - 1]
        );
    }

    #[test]
    fn reads_content_length_frame() {
        let body = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}";
        let input = format!(
            "Content-Length: {}\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        let mut reader = FrameReader::new(input.as_bytes());

        assert_eq!(reader.next_frame().expect("read").expect("frame"), body);
    }

    #[test]
    fn reads_crlf_delimited_json() {
        let input = b"{\"jsonrpc\":\"2.0\"}\r\n";
        let mut reader = FrameReader::new(&input[..]);

        assert_eq!(
            reader.next_frame().expect("read").expect("frame"),
            &input[..input.len() - 2]
        );
    }

    #[test]
    fn preserves_unterminated_trailing_carriage_return() {
        let input = b"{\"jsonrpc\":\"2.0\"}\r";
        let mut reader = FrameReader::new(&input[..]);

        assert_eq!(reader.next_frame().expect("read").expect("frame"), input);
    }

    #[test]
    fn returns_none_on_clean_eof() {
        let mut reader = FrameReader::new(&b""[..]);

        assert!(reader.next_frame().expect("read").is_none());
    }

    #[test]
    fn reads_case_insensitive_content_length_with_additional_headers() {
        let input = b"content-length: 2\nContent-Type: application/json\n\n{}";
        let mut reader = FrameReader::new(&input[..]);

        assert_eq!(reader.next_frame().expect("read").expect("frame"), b"{}");
    }

    #[test]
    fn reads_empty_content_length_frame() {
        let input = b"Content-Length: 0\r\n\r\n";
        let mut reader = FrameReader::new(&input[..]);

        assert_eq!(reader.next_frame().expect("read").expect("frame"), b"");
    }

    #[test]
    fn rejects_invalid_content_length_values() {
        for value in ["", "-1", "1x", "184467440737095516160"] {
            let input = format!("Content-Length: {value}\r\n\r\n");
            let mut reader = FrameReader::new(input.as_bytes());

            assert!(
                matches!(reader.next_frame(), Err(FrameError::InvalidHeader)),
                "value {value:?} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_content_length_headers_without_blank_line() {
        let input = b"Content-Length: 1\r\nContent-Type: application/json\r\n";
        let mut reader = FrameReader::new(&input[..]);

        assert!(matches!(
            reader.next_frame(),
            Err(FrameError::InvalidHeader)
        ));
    }

    #[test]
    fn rejects_declared_frame_over_limit() {
        let input = format!("Content-Length: {}\r\n\r\n", MAX_FRAME_BYTES + 1);
        let mut reader = FrameReader::new(input.as_bytes());

        assert!(matches!(
            reader.next_frame(),
            Err(FrameError::FrameTooLarge {
                size,
                limit: MAX_FRAME_BYTES
            }) if size == MAX_FRAME_BYTES + 1
        ));
    }

    #[test]
    fn rejects_accumulated_newline_frame_over_limit() {
        let mut input = vec![b'x'; MAX_FRAME_BYTES + 1];
        input.push(b'\n');
        let mut reader = FrameReader::new(input.as_slice());

        assert!(matches!(
            reader.next_frame(),
            Err(FrameError::FrameTooLarge {
                size,
                limit: MAX_FRAME_BYTES
            }) if size == MAX_FRAME_BYTES + 1
        ));
    }

    #[test]
    fn accepts_newline_frame_at_limit() {
        let mut input = vec![b'x'; MAX_FRAME_BYTES];
        input.extend_from_slice(b"\r\n");
        let mut reader = FrameReader::new(input.as_slice());

        assert_eq!(
            reader.next_frame().expect("read").expect("frame").len(),
            MAX_FRAME_BYTES
        );
    }

    #[test]
    fn reports_truncated_content_length_body() {
        let input = b"Content-Length: 3\r\n\r\n{}";
        let mut reader = FrameReader::new(&input[..]);

        assert!(matches!(
            reader.next_frame(),
            Err(FrameError::UnexpectedEof)
        ));
    }

    #[test]
    fn reads_multiple_frames_without_losing_buffered_bytes() {
        let input = b"Content-Length: 2\r\n\r\n{}[]\n";
        let mut reader = FrameReader::new(&input[..]);

        assert_eq!(reader.next_frame().expect("read").expect("frame"), b"{}");
        assert_eq!(reader.next_frame().expect("read").expect("frame"), b"[]");
        assert!(reader.next_frame().expect("read").is_none());
    }
}
