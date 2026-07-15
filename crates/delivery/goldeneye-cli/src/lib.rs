use goldeneye_bootstrap::BootstrapRuntime;
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
    let server = Server::from_env()?;
    run_session_with_server(reader, writer, &server)
}

/// Runs one MCP session against an injected production runtime until EOF.
///
/// # Errors
///
/// Returns an error when input framing, JSON serialization, or output writing fails.
pub fn run_session_with_runtime<R: Read, W: Write>(
    reader: R,
    writer: W,
    runtime: BootstrapRuntime,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = Server::with_runtime(runtime);
    run_session_with_server(reader, writer, &server)
}

/// Runs one MCP session against an injected server.
///
/// # Errors
///
/// Returns an error when input framing, JSON serialization, or output writing
/// fails.
pub fn run_session_with_server<R: Read, W: Write>(
    reader: R,
    writer: W,
    server: &Server,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut frames = FrameReader::new(BufReader::new(reader));
    let mut output = BufWriter::new(writer);

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
