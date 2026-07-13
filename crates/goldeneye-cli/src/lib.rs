use goldeneye_mcp::server::Server;
use goldeneye_mcp::transport::FrameReader;
use std::io::{BufReader, BufWriter, Read, Write};

/// Runs one MCP session until the input reaches EOF.
///
/// # Errors
///
/// Returns an error when input framing, JSON serialization, or output writing
/// fails.
pub fn run_session<R: Read, W: Write>(
    reader: R,
    writer: W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut frames = FrameReader::new(BufReader::new(reader));
    let mut output = BufWriter::new(writer);
    let server = Server::default();

    while let Some(frame) = frames.next_frame()? {
        let response = match String::from_utf8(frame) {
            Ok(line) => server.handle_line(&line),
            Err(_) => Some(goldeneye_mcp::protocol::Response::parse_error()),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }

    Ok(())
}
