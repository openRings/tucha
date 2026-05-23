use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::{net::UdpSocket, sync::mpsc};
use tucha_proto::{AudioPacket, RoomId, UserId};

use crate::audio::codec::OpusDecoder;
use crate::audio::rms_level;

/// Уровни входящего звука от собеседников: user_id → (rms 0..1, время последнего пакета)
pub type PeerLevels = Arc<Mutex<HashMap<UserId, (f32, Instant)>>>;

pub struct AudioStream {
    pub encoded_tx: mpsc::Sender<Vec<u8>>,
    pub decoded_rx:  mpsc::Receiver<Vec<u8>>,
    /// Обновляется в recv-задаче — читается из main loop для VU-метров
    pub peer_levels: PeerLevels,
}

impl AudioStream {
    pub async fn connect(
        server_addr: SocketAddr,
        user_id: UserId,
        room_id_ref: tokio::sync::watch::Receiver<Option<RoomId>>,
    ) -> Result<Self> {
        let sock = UdpSocket::bind("0.0.0.0:0")
            .await
            .context("bind UDP socket")?;
        sock.connect(server_addr)
            .await
            .context("connect UDP to server")?;

        let local_addr = sock.local_addr().ok();
        tracing::info!("UDP socket: local={local_addr:?} → remote={server_addr}");

        let sock = Arc::new(sock);

        let (encoded_tx, mut encoded_rx) = mpsc::channel::<Vec<u8>>(32);
        let (decoded_tx, decoded_rx)     = mpsc::channel::<Vec<u8>>(32);
        let peer_levels: PeerLevels      = Arc::new(Mutex::new(HashMap::new()));

        // ─── Задача отправки ───────────────────────────────────────────────────
        let sock_send = sock.clone();
        tokio::spawn(async move {
            let mut seq: u32       = 0;
            let mut timestamp: u32 = 0;
            let mut skipped: u64   = 0;
            let mut sent: u64      = 0;

            while let Some(payload) = encoded_rx.recv().await {
                let room_id = match *room_id_ref.borrow() {
                    Some(id) => id,
                    None => {
                        skipped += 1;
                        if skipped % 100 == 1 {
                            tracing::debug!("audio send: no room, skipped {skipped} frames");
                        }
                        continue;
                    }
                };

                let pkt = AudioPacket { user_id, room_id, seq, timestamp, payload };
                seq       = seq.wrapping_add(1);
                timestamp = timestamp.wrapping_add(960);

                match pkt.encode() {
                    Ok(bytes) => {
                        sent += 1;
                        if sent % 200 == 1 {
                            tracing::debug!("audio send: seq={seq} room={room_id} sent={sent}");
                        }
                        if let Err(e) = sock_send.send(&bytes).await {
                            tracing::warn!("udp send error: {e}");
                        }
                    }
                    Err(e) => tracing::warn!("pkt encode: {e}"),
                }
            }
            tracing::info!("audio send task ended");
        });

        // ─── Задача приёма ────────────────────────────────────────────────────
        let sock_recv   = sock.clone();
        let levels_recv = peer_levels.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            // Отдельный декодер на каждого собеседника (Opus stateful per stream)
            let mut decoders: HashMap<UserId, OpusDecoder> = HashMap::new();
            let mut received: u64 = 0;

            tracing::info!("audio recv task started, server={server_addr}");

            loop {
                let n = match sock_recv.recv(&mut buf).await {
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                        tracing::debug!("udp recv: ECONNREFUSED (transient), retrying");
                        continue;
                    }
                    Err(e) => { tracing::warn!("udp recv error: {e}"); break; }
                };

                let pkt = match AudioPacket::decode(&buf[..n]) {
                    Ok(p)  => p,
                    Err(e) => { tracing::warn!("pkt decode: {e}"); continue; }
                };

                received += 1;
                if received % 100 == 1 {
                    tracing::debug!("audio recv: #{received} from uid={}", pkt.user_id);
                }

                // Декодируем Opus → PCM → считаем RMS для VU-метра
                let decoder = decoders
                    .entry(pkt.user_id)
                    .or_insert_with(|| OpusDecoder::new().expect("create decoder"));

                if let Ok(pcm) = decoder.decode(&pkt.payload) {
                    let level = rms_level(&pcm);
                    levels_recv
                        .lock()
                        .unwrap()
                        .insert(pkt.user_id, (level, Instant::now()));
                }

                // Forwarding: raw Opus bytes → PlaybackStream (он декодирует сам)
                if decoded_tx.send(pkt.payload).await.is_err() {
                    break;
                }
            }
            tracing::info!("audio recv task ended");
        });

        Ok(Self { encoded_tx, decoded_rx, peer_levels })
    }
}
