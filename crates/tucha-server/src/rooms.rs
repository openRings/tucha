use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicU32, Ordering}};

use dashmap::DashMap;
use tokio::sync::broadcast;
use tucha_proto::{RoomId, ServerMsg, UserId, RoomInfo, UserInfo};

static NEXT_USER_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_ROOM_ID: AtomicU32 = AtomicU32::new(1);

pub fn new_user_id() -> UserId { NEXT_USER_ID.fetch_add(1, Ordering::Relaxed) }
pub fn new_room_id() -> RoomId { NEXT_ROOM_ID.fetch_add(1, Ordering::Relaxed) }

/// Информация о подключённом клиенте (хранится на сервере)
#[derive(Debug, Clone)]
pub struct ConnectedUser {
    pub id: UserId,
    pub username: String,
    pub room_id: Option<RoomId>,
    /// UDP-адрес клиента (заполняется при первом аудио-пакете)
    pub udp_addr: Option<std::net::SocketAddr>,
}

/// Комната
#[derive(Debug)]
pub struct Room {
    pub id: RoomId,
    pub name: String,
    /// user_id -> UserInfo
    pub users: HashMap<UserId, UserInfo>,
}

impl Room {
    pub fn new(id: RoomId, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), users: HashMap::new() }
    }

    pub fn to_info(&self) -> RoomInfo {
        RoomInfo {
            id: self.id,
            name: self.name.clone(),
            users: self.users.values().cloned().collect(),
        }
    }
}

/// Разделяемое состояние сервера
#[derive(Clone)]
pub struct ServerState(Arc<Inner>);

struct Inner {
    /// user_id -> ConnectedUser
    pub users: DashMap<UserId, ConnectedUser>,
    /// room_id -> Room  (под Mutex для атомарных операций)
    pub rooms: DashMap<RoomId, Room>,
    /// Broadcast-канал для рассылки событий всем сессиям
    pub broadcast: broadcast::Sender<ServerMsg>,
}

impl ServerState {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        let state = Self(Arc::new(Inner {
            users: DashMap::new(),
            rooms: DashMap::new(),
            broadcast: tx,
        }));
        // Создаём дефолтные комнаты
        for name in ["general", "dev", "random"] {
            let id = new_room_id();
            state.0.rooms.insert(id, Room::new(id, name));
        }
        state
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerMsg> {
        self.0.broadcast.subscribe()
    }

    pub fn broadcast(&self, msg: ServerMsg) {
        let _ = self.0.broadcast.send(msg);
    }

    /// Зарегистрировать пользователя, вернуть его id и список комнат
    pub fn register_user(&self, username: String) -> (UserId, Vec<RoomInfo>) {
        let id = new_user_id();
        self.0.users.insert(id, ConnectedUser {
            id,
            username,
            room_id: None,
            udp_addr: None,
        });
        let rooms = self.room_list();
        (id, rooms)
    }

    pub fn remove_user(&self, user_id: UserId) {
        if let Some((_, user)) = self.0.users.remove(&user_id) {
            if let Some(room_id) = user.room_id {
                self.leave_room_inner(user_id, room_id);
            }
        }
    }

    pub fn room_list(&self) -> Vec<RoomInfo> {
        self.0.rooms.iter().map(|r| r.to_info()).collect()
    }

    /// Войти в комнату. Возвращает Ok(UserInfo) если успешно.
    pub fn join_room(&self, user_id: UserId, room_id: RoomId) -> Option<UserInfo> {
        let mut user_entry = self.0.users.get_mut(&user_id)?;

        // Выходим из текущей комнаты если в ней
        if let Some(old_room) = user_entry.room_id {
            if old_room == room_id { return None; }
            self.leave_room_inner(user_id, old_room);
        }

        let info = UserInfo {
            id: user_id,
            username: user_entry.username.clone(),
            muted: false,
        };

        user_entry.room_id = Some(room_id);
        drop(user_entry);

        if let Some(mut room) = self.0.rooms.get_mut(&room_id) {
            room.users.insert(user_id, info.clone());
        }

        Some(info)
    }

    pub fn leave_room(&self, user_id: UserId, room_id: RoomId) {
        if let Some(mut u) = self.0.users.get_mut(&user_id) {
            if u.room_id == Some(room_id) {
                u.room_id = None;
            }
        }
        self.leave_room_inner(user_id, room_id);
    }

    fn leave_room_inner(&self, user_id: UserId, room_id: RoomId) {
        if let Some(mut room) = self.0.rooms.get_mut(&room_id) {
            room.users.remove(&user_id);
        }
    }

    /// Создать комнату
    pub fn create_room(&self, name: String) -> RoomInfo {
        let id = new_room_id();
        let room = Room::new(id, name);
        let info = room.to_info();
        self.0.rooms.insert(id, room);
        info
    }

    /// Получить UDP-адреса всех участников комнаты (кроме отправителя)
    pub fn room_udp_peers(
        &self,
        room_id: RoomId,
        sender_id: UserId,
    ) -> Vec<std::net::SocketAddr> {
        let Some(room) = self.0.rooms.get(&room_id) else { return vec![] };
        room.users.keys()
            .filter(|&&uid| uid != sender_id)
            .filter_map(|uid| {
                self.0.users.get(uid)?.udp_addr
            })
            .collect()
    }

    /// Запомнить UDP-адрес пользователя
    pub fn set_udp_addr(&self, user_id: UserId, addr: std::net::SocketAddr) {
        if let Some(mut u) = self.0.users.get_mut(&user_id) {
            u.udp_addr = Some(addr);
        }
    }

    /// Комната пользователя
    pub fn user_room(&self, user_id: UserId) -> Option<RoomId> {
        self.0.users.get(&user_id)?.room_id
    }
}
