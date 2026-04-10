use anyhow::Result;
use iroh::endpoint::{RecvStream, SendStream};

/// Tags for viewer -> host messages.
pub mod viewer {
    pub const VIEWPORT: u8 = 0x01;
    pub const REQUEST_TURN: u8 = 0x02;
    pub const INPUT: u8 = 0x03;
    pub const RELEASE_TURN: u8 = 0x04;
}

/// Tags for host -> viewer messages.
pub mod host {
    pub const OUTPUT: u8 = 0x01;
    pub const TURN_GRANTED: u8 = 0x02;
    pub const TURN_REVOKED: u8 = 0x03;
    pub const TURN_DENIED: u8 = 0x04;
}

/// Write a framed message: `[tag: u8][len: u16 BE][payload]`.
pub async fn write_msg(send: &mut SendStream, tag: u8, payload: &[u8]) -> Result<()> {
    let len = payload.len() as u16;
    let header = [tag, (len >> 8) as u8, len as u8];
    send.write_all(&header).await?;
    if !payload.is_empty() {
        send.write_all(payload).await?;
    }
    Ok(())
}

/// Read a framed message. Returns `(tag, payload)`.
pub async fn read_msg(recv: &mut RecvStream) -> Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 3];
    recv.read_exact(&mut header).await?;
    let tag = header[0];
    let len = u16::from_be_bytes([header[1], header[2]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        recv.read_exact(&mut payload).await?;
    }
    Ok((tag, payload))
}
