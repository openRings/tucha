use serde::{Deserialize, Serialize};

use crate::{RoomId, UserId};

/// UDP аудио-пакет (header + Opus payload)
/// Сериализуется через bincode для минимального overhead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioPacket {
    pub user_id: UserId,
    pub room_id: RoomId,
    /// Монотонно растущий счётчик для обнаружения потерь и переупорядочивания
    pub seq: u32,
    /// Временная метка в семплах (48 000 Hz) для jitter-буфера
    pub timestamp: u32,
    /// Opus-закодированные данные
    pub payload: Vec<u8>,
}

impl AudioPacket {
    pub const MAX_SIZE: usize = 1400; // MTU-safe

    /// Сериализовать пакет в байты
    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| anyhow::anyhow!("encode error: {e}"))
    }

    /// Десериализовать пакет из байт
    pub fn decode(buf: &[u8]) -> anyhow::Result<Self> {
        let (pkt, _) = bincode::serde::decode_from_slice(buf, bincode::config::standard())
            .map_err(|e| anyhow::anyhow!("decode error: {e}"))?;
        Ok(pkt)
    }
}
