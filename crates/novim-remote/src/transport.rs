//! Content-Length framed JSON transport for remote protocol.
//!
//! Reuses the same framing as the LSP transport layer:
//! `Content-Length: N\r\n\r\n<N bytes of JSON>`

use std::io::{self, BufRead, Write};
use serde::{Serialize, de::DeserializeOwned};

/// Write a message with Content-Length framing.
pub fn write_message<T: Serialize>(writer: &mut impl Write, msg: &T) -> io::Result<()> {
    let body = serde_json::to_string(msg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(body.as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Read a message with Content-Length framing. Returns None on EOF.
pub fn read_message<T: DeserializeOwned>(reader: &mut impl BufRead) -> Option<T> {
    // Read headers until empty line
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return None, // EOF
            Ok(_) => {}
            Err(_) => return None,
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }

        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            content_length = len_str.trim().parse().ok();
        }
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

/// Try to read a message non-blocking. Returns None if no data available or on error.
/// Uses a timeout approach: checks if data is available before blocking.
pub fn try_read_message<T: DeserializeOwned>(reader: &mut impl BufRead) -> Option<T> {
    // Check if there's data available in the buffer
    let buf = reader.fill_buf().ok()?;
    if buf.is_empty() {
        return None;
    }
    read_message(reader)
}
