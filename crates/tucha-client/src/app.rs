use std::collections::HashMap;
use tucha_proto::{RoomId, RoomInfo, ServerMsg, UserInfo, UserId};

/// Экраны/режимы приложения
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    /// Экран подключения
    Connect,
    /// Главный экран (комнаты + голос)
    Main,
    /// Overlay выбора аудиоустройства
    DeviceSelect,
}

/// Какое поле активно на экране подключения
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectField {
    Server,
    Username,
}

/// Какой список в фокусе на главном экране
#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    Rooms,
    Users,
}

/// Статус соединения
#[derive(Debug, Clone, PartialEq)]
pub enum ConnStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// Главное состояние приложения
#[derive(Debug)]
pub struct AppState {
    pub screen: Screen,
    pub conn_status: ConnStatus,

    // ─── Экран подключения ────────────────────────────────────────────
    pub server_input: String,
    pub username_input: String,
    pub connect_field: ConnectField,

    // ─── Основной экран ───────────────────────────────────────────────
    pub my_user_id: Option<UserId>,
    pub rooms: Vec<RoomInfo>,
    pub current_room: Option<RoomId>,
    pub focus: Focus,
    pub room_list_idx: usize,
    pub user_list_idx: usize,

    // ─── Аудио ───────────────────────────────────────────────────────
    pub is_muted: bool,
    pub is_deafened: bool,
    /// user_id -> уровень 0.0–1.0
    pub volume_levels: HashMap<UserId, f32>,
    /// user_id -> флаг "говорит"
    pub speaking: HashMap<UserId, bool>,
    pub my_input_level: f32,

    // ─── Выбор устройств ─────────────────────────────────────────────
    pub input_devices: Vec<String>,
    pub output_devices: Vec<String>,
    pub selected_input: usize,
    pub selected_output: usize,
    pub device_focus: DeviceFocus,

    // ─── Уведомление ────────────────────────────────────────────────
    pub notification: Option<(String, std::time::Instant)>,

    pub should_quit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceFocus {
    Input,
    Output,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            screen: Screen::Connect,
            conn_status: ConnStatus::Disconnected,
            server_input: "81.31.246.215:7878".into(),
            username_input: String::new(),
            connect_field: ConnectField::Server,
            my_user_id: None,
            rooms: Vec::new(),
            current_room: None,
            focus: Focus::Rooms,
            room_list_idx: 0,
            user_list_idx: 0,
            is_muted: false,
            is_deafened: false,
            volume_levels: HashMap::new(),
            speaking: HashMap::new(),
            my_input_level: 0.0,
            input_devices: Vec::new(),
            output_devices: Vec::new(),
            selected_input: 0,
            selected_output: 0,
            device_focus: DeviceFocus::Input,
            notification: None,
            should_quit: false,
        }
    }

    /// Применить ServerMsg к состоянию
    pub fn apply(&mut self, msg: ServerMsg) {
        match msg {
            ServerMsg::Connected { user_id, rooms } => {
                self.my_user_id = Some(user_id);
                self.rooms = rooms;
                self.screen = Screen::Main;
                self.conn_status = ConnStatus::Connected;
            }
            ServerMsg::RoomList { rooms } => {
                self.rooms = rooms;
            }
            ServerMsg::RoomCreated { room } => {
                if !self.rooms.iter().any(|r| r.id == room.id) {
                    self.rooms.push(room);
                }
            }
            ServerMsg::UserJoined { user, room_id } => {
                if let Some(room) = self.rooms.iter_mut().find(|r| r.id == room_id) {
                    if !room.users.iter().any(|u| u.id == user.id) {
                        room.users.push(user);
                    }
                }
            }
            ServerMsg::UserLeft { user_id, room_id } => {
                if room_id == 0 {
                    // Вышел из всех комнат
                    for room in &mut self.rooms {
                        room.users.retain(|u| u.id != user_id);
                    }
                } else if let Some(room) = self.rooms.iter_mut().find(|r| r.id == room_id) {
                    room.users.retain(|u| u.id != user_id);
                }
            }
            ServerMsg::VolumeUpdate { user_id, level, .. } => {
                self.volume_levels.insert(user_id, level);
                self.speaking.insert(user_id, level > 0.05);
            }
            ServerMsg::Error { message } => {
                self.notify(format!("⚠ {message}"));
            }
            ServerMsg::Pong => {}
        }
    }

    /// Текущая комната
    pub fn current_room_info(&self) -> Option<&RoomInfo> {
        let id = self.current_room?;
        self.rooms.iter().find(|r| r.id == id)
    }

    /// Пользователи в текущей комнате
    pub fn current_users(&self) -> Vec<&UserInfo> {
        self.current_room_info()
            .map(|r| r.users.iter().collect())
            .unwrap_or_default()
    }

    pub fn notify(&mut self, msg: String) {
        self.notification = Some((msg, std::time::Instant::now()));
    }

    pub fn clear_expired_notification(&mut self) {
        if let Some((_, t)) = &self.notification {
            if t.elapsed().as_secs() > 3 {
                self.notification = None;
            }
        }
    }
}
