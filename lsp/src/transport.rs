use bytes::BytesMut;
use std::io;
use tokio::io::AsyncReadExt;
use tracing::trace;

/// Read and decode one Content-Length framed message from a reader.
pub async fn read_message<R: AsyncReadExt + Unpin>(reader: &mut R) -> io::Result<Option<String>> {
    let mut header_buf = BytesMut::with_capacity(4096);
    let mut body_buf = Vec::new();

    loop {
        let mut byte = [0u8; 1];
        match reader.read(&mut byte).await {
            Ok(0) => {
                if header_buf.is_empty() {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed during header",
                ));
            }
            Ok(_) => {
                header_buf.extend_from_slice(&byte);
                if header_buf.len() >= 4
                    && header_buf[header_buf.len() - 4..] == [b'\r', b'\n', b'\r', b'\n']
                {
                    break;
                }
                if header_buf.len() > 4096 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "header too large",
                    ));
                }
            }
            Err(e) => return Err(e),
        }
    }

    let header_str = std::str::from_utf8(&header_buf[..header_buf.len() - 4])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let content_length = header_str
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim().eq_ignore_ascii_case("Content-Length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;

    body_buf.resize(content_length, 0);
    let mut offset = 0;
    while offset < content_length {
        let n = reader.read(&mut body_buf[offset..]).await?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed during body",
            ));
        }
        offset += n;
    }

    let body =
        String::from_utf8(body_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    trace!("LSP recv: {} bytes", body.len());
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn test_read_message() {
        let data = "Content-Length: 12\r\n\r\n{\"hello\": 1}";
        let mut reader = BufReader::new(data.as_bytes());
        let msg = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(msg, "{\"hello\": 1}");
    }

    #[tokio::test]
    async fn test_read_message_multiple_headers() {
        let data = "Content-Length: 12\r\nContent-Type: text/plain\r\n\r\n{\"hello\": 1}";
        let mut reader = BufReader::new(data.as_bytes());
        let msg = read_message(&mut reader).await.unwrap().unwrap();
        assert_eq!(msg, "{\"hello\": 1}");
    }

    #[tokio::test]
    async fn test_empty_stream_returns_none() {
        let mut reader = BufReader::new(&b""[..]);
        let msg = read_message(&mut reader).await.unwrap();
        assert!(msg.is_none());
    }

    #[tokio::test]
    async fn test_missing_content_length() {
        let data = "Content-Type: text/plain\r\n\r\n{}";
        let mut reader = BufReader::new(data.as_bytes());
        let result = read_message(&mut reader).await;
        assert!(result.is_err());
    }
}
