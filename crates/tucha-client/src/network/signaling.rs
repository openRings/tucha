use anyhow::{Context, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};
use tucha_proto::{
    codec::{decode_msg, encode_msg},
    ClientMsg, ServerMsg,
};

/// Клиент TCP-signaling.
/// Запускает две задачи: чтение → server_rx, запись ← client_tx.
pub struct SignalingClient {
    pub client_tx: mpsc::Sender<ClientMsg>,
    pub server_rx: mpsc::Receiver<ServerMsg>,
}

impl SignalingClient {
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connect to {addr}"))?;

        let (reader, writer) = stream.into_split();

        let (client_tx, mut client_rx) = mpsc::channel::<ClientMsg>(64);
        let (server_tx, server_rx) = mpsc::channel::<ServerMsg>(64);

        // Задача записи: забирает ClientMsg и шлёт на сервер
        tokio::spawn(async move {
            let mut writer = writer;
            while let Some(msg) = client_rx.recv().await {
                match encode_msg(&msg) {
                    Ok(buf) => {
                        if writer.write_all(&buf).await.is_err() { break; }
                    }
                    Err(e) => tracing::error!("encode client msg: {e}"),
                }
            }
        });

        // Задача чтения: читает ServerMsg и кладёт в server_tx
        tokio::spawn(async move {
            let mut reader = reader;
            loop {
                let mut len_buf = [0u8; 4];
                if reader.read_exact(&mut len_buf).await.is_err() { break; }
                let len = u32::from_be_bytes(len_buf) as usize;
                if len > 64 * 1024 { break; }
                let mut buf = vec![0u8; len];
                if reader.read_exact(&mut buf).await.is_err() { break; }
                match decode_msg::<ServerMsg>(&buf) {
                    Ok(msg) => { if server_tx.send(msg).await.is_err() { break; } }
                    Err(e) => tracing::warn!("decode server msg: {e}"),
                }
            }
            tracing::info!("signaling read task ended");
        });

        Ok(Self { client_tx, server_rx })
    }

    pub async fn send(&self, msg: ClientMsg) -> Result<()> {
        self.client_tx.send(msg).await.context("send client msg")
    }

    pub async fn recv(&mut self) -> Option<ServerMsg> {
        self.server_rx.recv().await
    }
}
