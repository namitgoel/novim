//! JSON-RPC transport layer for LSP communication.
//!
//! Handles the Content-Length framing used by the Language Server Protocol.

use std::io::{self, BufRead, Write};

/// Write a JSON-RPC message with Content-Length header.
pub fn write_message(writer: &mut impl Write, msg: &serde_json::Value) -> io::Result<()> {
    let body = serde_json::to_string(msg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(body.as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Read a JSON-RPC message by parsing the Content-Length header.
/// Returns None on EOF or parse failure.
pub fn read_message(reader: &mut impl BufRead) -> Option<serde_json::Value> {
    // Read headers until empty line
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return None, // EOF
            Ok(_) => {}
            Err(_) => return None,
        }

        let line = line.trim();
        if line.is_empty() {
            break; // End of headers
        }

        if let Some(len_str) = line.strip_prefix("Content-Length: ") {
            content_length = len_str.trim().parse().ok();
        }
        // Ignore other headers (Content-Type, etc.)
    }

    let length = content_length?;

    // Read exactly `length` bytes
    let mut body = vec![0u8; length];
    match reader.read_exact(&mut body) {
        Ok(()) => {}
        Err(_) => return None,
    }

    serde_json::from_slice(&body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_write_read_roundtrip() {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "test",
            "params": { "hello": "world" }
        });

        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();

        let mut reader = io::BufReader::new(Cursor::new(buf));
        let result = read_message(&mut reader).unwrap();

        assert_eq!(result["method"], "test");
        assert_eq!(result["params"]["hello"], "world");
    }

    #[test]
    fn test_read_eof() {
        let mut reader = io::BufReader::new(Cursor::new(Vec::<u8>::new()));
        assert!(read_message(&mut reader).is_none());
    }
}
