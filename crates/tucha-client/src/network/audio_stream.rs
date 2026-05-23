use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::{net::UdpSocket, sync::mpsc};
use tucha_proto::{AudioPacket, RoomId, UserId};

/// UDP аудио-стрим.
/// send_task: encoded_rx → упаковывает в AudioPacket → UDP send
/// recv_task: UDP recv → распаковывает → decoded_tx
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

        let sock = std::sync::Arc::new(sock);

        let (encoded_tx, mut encoded_rx) = mpsc::channel::<Vec<u8>>(32);
        let (decoded_tx, decoded_rx) = mpsc::channel::<Vec<u8>>(32);

        // ─── Задача отправки ───────────────────────────────────────────────────
        let sock_send = sock.clone();
        tokio::spawn(async move {
            let mut seq: u32 = 0;
            let mut timestamp: u32 = 0;

            while let Some(payload) = encoded_rx.recv().await {
                let room_id = match *room_id_ref.borrow() {
                    Some(id) => id,
                    None => continue, // не в комнате — не шлём
                };

                let pkt = AudioPacket {
                    user_id,
                    room_id,
                    seq,
                    timestamp,
                    payload,
                };

                seq = seq.wrapping_add(1);
                timestamp = timestamp.wrapping_add(960); // 20ms @ 48kHz

                match pkt.encode() {
                    Ok(bytes) => {
                        if let Err(e) = sock_send.send(&bytes).await {
                            tracing::debug!("udp send: {e}");
                        }
                    }
                    Err(e) => tracing::warn!("pkt encode: {e}"),
                }
            }
        });

        // ─── Задача приёма ────────────────────────────────────────────────────
        let sock_recv = sock.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            loop {
                let n = match sock_recv.recv(&mut buf).await {
                    Ok(n) => n,
                    Err(e) => { tracing::debug!("udp recv: {e}"); break; }
                };

                match AudioPacket::decode(&buf[..n]) {
                    Ok(pkt) => {
                        if decoded_tx.send(pkt.payload).await.is_err() { break; }
                    }
                    Err(e) => tracing::warn!("pkt decode: {e}"),
                }
            }
        });

        Ok(Self { encoded_tx, decoded_rx })
    }
}
