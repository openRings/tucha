use serde::{Deserialize, Serialize};

pub type UserId = u32;
pub type RoomId = u32;

/// Сообщения от клиента к серверу (TCP)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMsg {
    /// Первичное подключение
    Connect { username: String },
    /// Войти в комнату
    JoinRoom { room_id: RoomId },
    /// Покинуть комнату
    LeaveRoom { room_id: RoomId },
    /// Создать новую комнату
    CreateRoom { name: String },
    /// Heartbeat
    Ping,
}

/// Сообщения от сервера к клиенту (TCP)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMsg {
    /// Успешное подключение
    Connected {
        user_id: UserId,
        rooms: Vec<RoomInfo>,
    },
    /// Обновлённый список комнат
    RoomList { rooms: Vec<RoomInfo> },
    /// Новая комната создана
    RoomCreated { room: RoomInfo },
    /// Пользователь зашёл в комнату
    UserJoined {
        user: UserInfo,
        room_id: RoomId,
    },
    /// Пользователь покинул комнату
    UserLeft {
        user_id: UserId,
        room_id: RoomId,
    },
    /// Обновление уровня громкости пользователя
    VolumeUpdate {
        user_id: UserId,
        room_id: RoomId,
        /// 0.0 – 1.0
        level: f32,
    },
    /// Ошибка
    Error { message: String },
    /// Pong
    Pong,
}

/// Мета-данные комнаты
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub id: RoomId,
    pub name: String,
    pub users: Vec<UserInfo>,
}

/// Мета-данные пользователя
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: UserId,
    pub username: String,
    pub muted: bool,
}
