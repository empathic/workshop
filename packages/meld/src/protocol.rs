use anyhow::Result;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Tags for viewer -> host messages.
pub mod viewer {
    pub const VIEWPORT: u8 = 0x01;
    pub const REQUEST_TURN: u8 = 0x02;
    pub const INPUT: u8 = 0x03;
    pub const RELEASE_TURN: u8 = 0x04;
    pub const HELLO: u8 = 0x05;
    pub const GOODBYE: u8 = 0x06;
}

/// Tags for host -> viewer messages.
pub mod host {
    pub const OUTPUT: u8 = 0x01;
    pub const TURN_GRANTED: u8 = 0x02;
    pub const TURN_REVOKED: u8 = 0x03;
    pub const TURN_DENIED: u8 = 0x04;
    pub const DIMS_CHANGED: u8 = 0x05;
}

/// Frame layout: `[tag: u8][len: u32 BE][payload]`.
pub async fn write_msg<W: AsyncWrite + Unpin>(w: &mut W, tag: u8, payload: &[u8]) -> Result<()> {
    let len = payload.len() as u32;
    let mut header = [0u8; 5];
    header[0] = tag;
    header[1..5].copy_from_slice(&len.to_be_bytes());
    w.write_all(&header).await?;
    if !payload.is_empty() {
        w.write_all(payload).await?;
    }
    Ok(())
}

pub async fn read_msg<R: AsyncRead + Unpin>(r: &mut R) -> Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 5];
    r.read_exact(&mut header).await?;
    let tag = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        r.read_exact(&mut payload).await?;
    }
    Ok((tag, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    async fn roundtrip(tag: u8, payload: Vec<u8>) {
        let (mut a, mut b) = duplex(1 << 20);
        let expected = payload.clone();
        let writer = tokio::spawn(async move {
            write_msg(&mut a, tag, &payload).await.unwrap();
        });
        let (got_tag, got_payload) = read_msg(&mut b).await.unwrap();
        writer.await.unwrap();
        assert_eq!(got_tag, tag);
        assert_eq!(got_payload, expected);
    }

    #[tokio::test]
    async fn empty_payload() {
        roundtrip(0x42, vec![]).await;
    }

    #[tokio::test]
    async fn small_payload() {
        roundtrip(0x01, b"hello world".to_vec()).await;
    }

    #[tokio::test]
    async fn payload_above_u16_limit() {
        // Regression: would have truncated under the old u16 length header.
        let payload = vec![0xABu8; 128 * 1024];
        roundtrip(0x01, payload).await;
    }

    #[tokio::test]
    async fn preserves_all_tag_values() {
        for tag in [0u8, 1, 127, 128, 255] {
            roundtrip(tag, vec![tag; 7]).await;
        }
    }
}
