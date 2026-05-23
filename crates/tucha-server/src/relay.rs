use anyhow::Result;
use tokio::net::UdpSocket;
use tracing::{debug, error, trace, warn};
use tucha_proto::AudioPacket;

use crate::rooms::ServerState;

/// UDP relay: получает аудио-пакеты и рассылает их участникам комнаты
pub async fn run_udp(state: ServerState, port: u16) -> Result<()> {
    let sock = UdpSocket::bind(("0.0.0.0", port)).await?;
    tracing::info!("UDP relay ready on :{port}");

    let mut buf = vec![0u8; 2048];

    loop {
        let (n, addr) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => { error!("udp recv: {e}"); continue; }
        };

        let pkt = match AudioPacket::decode(&buf[..n]) {
            Ok(p) => p,
            Err(e) => { warn!("bad audio packet from {addr}: {e}"); continue; }
        };

        trace!("audio pkt from uid={} room={} seq={} len={}",
            pkt.user_id, pkt.room_id, pkt.seq, pkt.payload.len());

        // Запоминаем UDP-адрес отправителя
        state.set_udp_addr(pkt.user_id, addr);

        // Пересылаем всем остальным участникам комнаты
        let peers = state.room_udp_peers(pkt.room_id, pkt.user_id);
        if peers.is_empty() {
            continue;
        }

        let raw = match pkt.encode() {
            Ok(b) => b,
            Err(e) => { error!("re-encode: {e}"); continue; }
        };

        for peer_addr in peers {
            if let Err(e) = sock.send_to(&raw, peer_addr).await {
                debug!("send to {peer_addr}: {e}");
            }
        }
    }
}
