use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::{net::UdpSocket, sync::mpsc};
use tucha_proto::{AudioPacket, RoomId, UserId};

pub struct AudioStream {
    pub encoded_tx: mpsc::Sender<Vec<u8>>,
    pub decoded_rx: mpsc::Receiver<Vec<u8>>,
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

        let sock = std::sync::Arc::new(sock);

        let (encoded_tx, mut encoded_rx) = mpsc::channel::<Vec<u8>>(32);
        let (decoded_tx, decoded_rx) = mpsc::channel::<Vec<u8>>(32);

        // ─── Задача отправки ───────────────────────────────────────────────────
        let sock_send = sock.clone();
        tokio::spawn(async move {
            let mut seq: u32 = 0;
            let mut timestamp: u32 = 0;
            let mut skipped: u64 = 0;
            let mut sent: u64 = 0;

            while let Some(payload) = encoded_rx.recv().await {
                let room_id = match *room_id_ref.borrow() {
                    Some(id) => id,
                    None => {
                        skipped += 1;
                        // Каждые 100 пропущенных пакетов логируем — помогает диагностировать
                        if skipped % 100 == 1 {
                            tracing::debug!("audio send: no room yet, skipped {skipped} frames");
                        }
                        continue;
                    }
                };

                let pkt = AudioPacket { user_id, room_id, seq, timestamp, payload };
                seq = seq.wrapping_add(1);
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
        let sock_recv = sock.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            let mut received: u64 = 0;
            tracing::info!("audio recv task started, waiting for packets from {server_addr}");
            loop {
                let n = match sock_recv.recv(&mut buf).await {
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                        // macOS шлёт ECONNREFUSED когда сервер ещё не ответил —
                        // это не фатально, просто пробуем снова
                        tracing::debug!("udp recv: connection refused (transient), retrying");
                        continue;
                    }
                    Err(e) => { tracing::warn!("udp recv error: {e}"); break; }
                };

                received += 1;
                if received % 100 == 1 {
                    tracing::debug!("audio recv: got packet #{received}, {n} bytes");
                }

                match AudioPacket::decode(&buf[..n]) {
                    Ok(pkt) => {
                        if decoded_tx.send(pkt.payload).await.is_err() { break; }
                    }
                    Err(e) => tracing::warn!("pkt decode: {e}"),
                }
            }
            tracing::info!("audio recv task ended");
        });

        Ok(Self { encoded_tx, decoded_rx })
    }
}
