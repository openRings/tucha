use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Вспомогательный тип: длина-префикс для framing TCP сообщений
/// Формат: [u32 len BE][bytes...]
pub fn encode_msg<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    let payload = bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .map_err(|e| anyhow::anyhow!("msg encode: {e}"))?;
    let len = payload.len() as u32;
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

pub fn decode_msg<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> Result<T> {
    let (msg, _) = bincode::serde::decode_from_slice(payload, bincode::config::standard())
        .map_err(|e| anyhow::anyhow!("msg decode: {e}"))?;
    Ok(msg)
}
