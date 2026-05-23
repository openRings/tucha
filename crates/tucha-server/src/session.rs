use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{debug, error, info, warn};
use tucha_proto::{
    codec::{decode_msg, encode_msg},
    ClientMsg, ServerMsg, UserId,
};

use crate::rooms::ServerState;

pub async fn run_tcp(state: ServerState, port: u16) -> Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    info!("TCP listener ready on :{port}");

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                info!("new TCP connection from {addr}");
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(state, stream).await {
                        warn!("session error: {e}");
                    }
                });
            }
            Err(e) => error!("accept error: {e}"),
        }
    }
}

async fn handle_client(state: ServerState, mut stream: TcpStream) -> Result<()> {
    // Ждём первое сообщение Connect
    let user_id = match read_msg::<ClientMsg>(&mut stream).await? {
        ClientMsg::Connect { username } => {
            let (id, rooms) = state.register_user(username.clone());
            info!("user '{username}' connected, id={id}");
            let reply = ServerMsg::Connected { user_id: id, rooms };
            write_msg(&mut stream, &reply).await?;
            id
        }
        _ => {
            write_msg(&mut stream, &ServerMsg::Error {
                message: "expected Connect as first message".into(),
            }).await?;
            return Ok(());
        }
    };

    // Подписываемся на broadcast
    let mut rx = state.subscribe();

    loop {
        tokio::select! {
            // Входящие от клиента
            result = read_msg::<ClientMsg>(&mut stream) => {
                match result {
                    Ok(msg) => handle_msg(&state, &mut stream, user_id, msg).await?,
                    Err(e) => {
                        debug!("client {user_id} disconnected: {e}");
                        break;
                    }
                }
            }
            // Broadcast события от других сессий
            Ok(event) = rx.recv() => {
                // Фильтрация: не отправляем клиенту его же события
                // (сервер сам уже ответил на его запрос)
                if should_forward(user_id, &event) {
                    if write_msg(&mut stream, &event).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    state.remove_user(user_id);
    info!("user {user_id} disconnected");
    state.broadcast(ServerMsg::UserLeft { user_id, room_id: 0 }); // room_id=0 = из всех
    Ok(())
}

async fn handle_msg(
    state: &ServerState,
    stream: &mut TcpStream,
    user_id: UserId,
    msg: ClientMsg,
) -> Result<()> {
    match msg {
        ClientMsg::Connect { .. } => {
            write_msg(stream, &ServerMsg::Error {
                message: "already connected".into(),
            }).await?;
        }

        ClientMsg::JoinRoom { room_id } => {
            if let Some(user_info) = state.join_room(user_id, room_id) {
                let event = ServerMsg::UserJoined { user: user_info, room_id };
                state.broadcast(event.clone());
                // Отправляем клиенту обновлённый список комнат
                write_msg(stream, &ServerMsg::RoomList { rooms: state.room_list() }).await?;
            } else {
                write_msg(stream, &ServerMsg::Error {
                    message: format!("room {room_id} not found or already joined"),
                }).await?;
            }
        }

        ClientMsg::LeaveRoom { room_id } => {
            state.leave_room(user_id, room_id);
            let event = ServerMsg::UserLeft { user_id, room_id };
            state.broadcast(event);
            write_msg(stream, &ServerMsg::RoomList { rooms: state.room_list() }).await?;
        }

        ClientMsg::CreateRoom { name } => {
            let room = state.create_room(name);
            let event = ServerMsg::RoomCreated { room: room.clone() };
            state.broadcast(event);
            write_msg(stream, &ServerMsg::RoomList { rooms: state.room_list() }).await?;
        }

        ClientMsg::Ping => {
            write_msg(stream, &ServerMsg::Pong).await?;
        }
    }
    Ok(())
}

/// Определяем нужно ли форвардить broadcast конкретному клиенту
fn should_forward(user_id: UserId, msg: &ServerMsg) -> bool {
    match msg {
        // Не форвардим клиенту его собственное UserLeft
        ServerMsg::UserLeft { user_id: uid, .. } if *uid == user_id => false,
        ServerMsg::UserJoined { user, .. } if user.id == user_id => false,
        _ => true,
    }
}

// ─── Framing helpers ─────────────────────────────────────────────────────────

/// Читать length-prefixed сообщение
async fn read_msg<T: for<'de> serde::Deserialize<'de>>(
    stream: &mut TcpStream,
) -> Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 1024 * 64 {
        anyhow::bail!("message too large: {len} bytes");
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    decode_msg(&buf)
}

/// Записать length-prefixed сообщение
async fn write_msg<T: serde::Serialize>(stream: &mut TcpStream, msg: &T) -> Result<()> {
    let buf = encode_msg(msg)?;
    stream.write_all(&buf).await?;
    Ok(())
}
