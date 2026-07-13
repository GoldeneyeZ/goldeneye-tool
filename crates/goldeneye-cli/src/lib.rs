use goldeneye_mcp::server::Server;
use goldeneye_mcp::transport::FrameReader;
use std::io::{BufReader, BufWriter, Read, Write};

/// Runs one MCP session until the input reaches EOF.
///
/// # Errors
///
/// Returns an error when input framing, UTF-8 decoding, JSON serialization, or
/// output writing fails.
pub fn run_session<R: Read, W: Write>(
    reader: R,
    writer: W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut frames = FrameReader::new(BufReader::new(reader));
    let mut output = BufWriter::new(writer);
    let server = Server::default();

    while let Some(frame) = frames.next_frame()? {
        let line = String::from_utf8(frame)?;
        if let Some(response) = server.handle_line(&line) {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }

    Ok(())
}
