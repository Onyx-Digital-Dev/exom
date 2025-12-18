//! Length-prefixed frame encoding/decoding
//!
//! Wire format: [4-byte big-endian length][JSON payload]
//! Maximum frame size: 1MB (sanity limit)

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};
use crate::protocol::Message;

/// Maximum allowed frame size (1MB)
const MAX_FRAME_SIZE: u32 = 1024 * 1024;

/// Read a length-prefixed frame from a stream
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Message> {
    // Read 4-byte length prefix
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::ConnectionClosed
        } else {
            Error::Io(e)
        }
    })?;

    let len = u32::from_be_bytes(len_buf);

    // Sanity check
    if len == 0 {
        return Err(Error::Protocol("Empty frame".into()));
    }
    if len > MAX_FRAME_SIZE {
        return Err(Error::Protocol(format!(
            "Frame too large: {} bytes (max {})",
            len, MAX_FRAME_SIZE
        )));
    }

    // Read payload
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::ConnectionClosed
        } else {
            Error::Io(e)
        }
    })?;

    // Deserialize
    Message::from_bytes(&payload).map_err(|e| Error::Protocol(format!("Invalid JSON: {}", e)))
}

/// Write a length-prefixed frame to a stream
pub async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, msg: &Message) -> Result<()> {
    let payload = msg
        .to_bytes()
        .map_err(|e| Error::Protocol(format!("Serialization failed: {}", e)))?;

    let len = payload.len() as u32;
    if len > MAX_FRAME_SIZE {
        return Err(Error::Protocol(format!(
            "Message too large: {} bytes (max {})",
            len, MAX_FRAME_SIZE
        )));
    }

    // Write length prefix
    writer.write_all(&len.to_be_bytes()).await?;

    // Write payload
    writer.write_all(&payload).await?;

    // Flush to ensure delivery
    writer.flush().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_frame_roundtrip() {
        let msg = Message::Ping;

        // Write to buffer
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).await.unwrap();

        // Read back
        let mut cursor = Cursor::new(buf);
        let decoded = read_frame(&mut cursor).await.unwrap();

        assert!(matches!(decoded, Message::Ping));
    }

    #[tokio::test]
    async fn test_empty_frame_rejected() {
        // 4 zero bytes = length 0
        let mut cursor = Cursor::new(vec![0, 0, 0, 0]);
        let result = read_frame(&mut cursor).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_oversized_frame_rejected() {
        // Length = MAX_FRAME_SIZE + 1
        let len = (MAX_FRAME_SIZE + 1).to_be_bytes();
        let mut cursor = Cursor::new(len.to_vec());
        let result = read_frame(&mut cursor).await;
        assert!(result.is_err());
    }
}
