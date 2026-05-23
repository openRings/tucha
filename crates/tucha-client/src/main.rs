mod app;
mod audio;
mod network;
mod ui;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::{mpsc, watch};

use cpal::traits::DeviceTrait;

use app::{AppState, ConnectField, ConnStatus, DeviceFocus, Focus, Screen};
use audio::{
    capture::CaptureStream,
    devices::AudioDevices,
    playback::PlaybackStream, AudioMetrics, MetricsRef,
};
use network::SignalingClient;
use tucha_proto::{ClientMsg, RoomId};

#[tokio::main]
async fn main() -> Result<()> {
    // Логи в файл чтобы не мешали TUI
    let file = std::fs::File::create("/tmp/tucha.log")?;
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter("tucha=debug")
        .init();

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal).await;

    // Восстановить терминал
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = &result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

async fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut state = AppState::new();

    // Загружаем аудиоустройства
    let audio_devs = AudioDevices::new();
    state.input_devices = audio_devs
        .input_devices()
        .into_iter()
        .map(|d| d.name)
        .collect();
    state.output_devices = audio_devs
        .output_devices()
        .into_iter()
        .map(|d| d.name)
        .collect();

    // Ставим дефолтные
    if let Some(dev) = audio_devs.default_input() {
        if let Ok(name) = dev.name() {
            if let Some(idx) = state.input_devices.iter().position(|d| d == &name) {
                state.selected_input = idx;
            }
        }
    }
    if let Some(dev) = audio_devs.default_output() {
        if let Ok(name) = dev.name() {
            if let Some(idx) = state.output_devices.iter().position(|d| d == &name) {
                state.selected_output = idx;
            }
        }
    }

    // Аудио метрики
    let metrics: MetricsRef = Arc::new(Mutex::new(AudioMetrics::default()));
    let muted = Arc::new(Mutex::new(false));
    let deafened = Arc::new(Mutex::new(false));

    // Каналы для аудио-движка
    let (encoded_tx, _encoded_rx) = mpsc::channel::<Vec<u8>>(32);
    let (_decoded_tx, mut decoded_rx) = mpsc::channel::<Vec<u8>>(32);

    // Watch-канал для текущей комнаты (для UDP стрима)
    let (room_tx, room_rx) = watch::channel::<Option<RoomId>>(None);

    // Signaling + audio stream handles (создаются при подключении)
    let mut signaling: Option<SignalingClient> = None;
    let mut _capture_stream: Option<CaptureStream> = None;
    let mut _playback_stream: Option<PlaybackStream> = None;
    let mut playback_tx: Option<mpsc::Sender<Vec<u8>>> = None;

    // Задача: перекладывает декодированные пакеты в playback
    let playback_fwd_tx = Arc::new(Mutex::new(Option::<mpsc::Sender<Vec<u8>>>::None));
    {
        let pfwd = playback_fwd_tx.clone();
        tokio::spawn(async move {
            while let Some(data) = decoded_rx.recv().await {
                if let Some(tx) = pfwd.lock().unwrap().as_ref() {
                    let _ = tx.try_send(data);
                }
            }
        });
    }

    let tick_rate = Duration::from_millis(50); // 20 fps

    loop {
        // Render
        terminal.draw(|f| ui::render(f, &state))?;

        // Обновить метрики из аудио
        {
            let m = metrics.lock().unwrap();
            state.my_input_level = m.input_level;
            state.is_muted = m.is_muted;
        }
        state.is_deafened = *deafened.lock().unwrap();
        state.clear_expired_notification();

        // Обработать ServerMsg если подключены
        if let Some(sig) = signaling.as_mut() {
            // Неблокирующий recv
            while let Ok(msg) = sig.server_rx.try_recv() {
                state.apply(msg);
            }
        }

        // Обработка событий клавиатуры
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                match state.screen {
                    Screen::Connect => {
                        handle_connect_keys(
                            &mut state,
                            key,
                            &mut signaling,
                            &mut _capture_stream,
                            &mut _playback_stream,
                            &mut playback_tx,
                            &playback_fwd_tx,
                            &encoded_tx,
                            &metrics,
                            &muted,
                            &deafened,
                            &audio_devs,
                            &room_rx,
                        ).await?;
                    }
                    Screen::Main => {
                        handle_main_keys(
                            &mut state,
                            key,
                            &mut signaling,
                            &muted,
                            &deafened,
                            &room_tx,
                        ).await?;
                    }
                    Screen::DeviceSelect => {
                        handle_device_keys(
                            &mut state,
                            key,
                            &mut _capture_stream,
                            &mut _playback_stream,
                            &mut playback_tx,
                            &playback_fwd_tx,
                            &encoded_tx,
                            &metrics,
                            &muted,
                            &deafened,
                            &audio_devs,
                        )?;
                    }
                }
            }
        }

        if state.should_quit { break; }
    }

    Ok(())
}

// ─── Обработка клавиш: экран подключения ─────────────────────────────────────

async fn handle_connect_keys(
    state: &mut AppState,
    key: crossterm::event::KeyEvent,
    signaling: &mut Option<SignalingClient>,
    capture: &mut Option<CaptureStream>,
    playback: &mut Option<PlaybackStream>,
    playback_tx: &mut Option<mpsc::Sender<Vec<u8>>>,
    playback_fwd: &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    encoded_tx: &mpsc::Sender<Vec<u8>>,
    metrics: &MetricsRef,
    muted: &Arc<Mutex<bool>>,
    deafened: &Arc<Mutex<bool>>,
    devs: &AudioDevices,
    _room_rx: &watch::Receiver<Option<RoomId>>,
) -> Result<()> {
    use KeyCode::*;
    match key.code {
        Esc => { state.should_quit = true; }
        Tab => {
            state.connect_field = match state.connect_field {
                ConnectField::Server   => ConnectField::Username,
                ConnectField::Username => ConnectField::Server,
            };
        }
        Char(c) => {
            match state.connect_field {
                ConnectField::Server   => state.server_input.push(c),
                ConnectField::Username => state.username_input.push(c),
            }
        }
        Backspace => {
            match state.connect_field {
                ConnectField::Server   => { state.server_input.pop(); }
                ConnectField::Username => { state.username_input.pop(); }
            }
        }
        Enter => {
            if state.username_input.is_empty() {
                state.notify("Введите имя пользователя".into());
                return Ok(());
            }
            state.conn_status = ConnStatus::Connecting;

            // Подключаемся к серверу
            let server = state.server_input.clone();
            let username = state.username_input.clone();

            match SignalingClient::connect(&server).await {
                Ok(client) => {
                    client.send(ClientMsg::Connect { username }).await?;
                    *signaling = Some(client);
                    state.conn_status = ConnStatus::Connected;
                    state.screen = Screen::Main;

                    // Запускаем аудиоустройства
                    start_audio(state, capture, playback, playback_tx, playback_fwd,
                                encoded_tx, metrics, muted, deafened, devs)?;

                    // Запускаем UDP стрим
                    let _udp_addr: std::net::SocketAddr = {
                        let mut addr = server.clone();
                        // Заменяем TCP порт (7878) на UDP (7879)
                        if let Some(pos) = addr.rfind(':') {
                            addr = format!("{}:7879", &addr[..pos]);
                        }
                        addr.parse().unwrap_or_else(|_| "127.0.0.1:7879".parse().unwrap())
                    };

                    // UDP стрим запустится в фоне
                    // user_id придёт в Connected msg, запускаем после apply
                }
                Err(e) => {
                    state.conn_status = ConnStatus::Error(e.to_string());
                    state.notify(format!("Ошибка: {e}"));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

// ─── Обработка клавиш: главный экран ─────────────────────────────────────────

async fn handle_main_keys(
    state: &mut AppState,
    key: crossterm::event::KeyEvent,
    signaling: &mut Option<SignalingClient>,
    muted: &Arc<Mutex<bool>>,
    deafened: &Arc<Mutex<bool>>,
    room_tx: &watch::Sender<Option<RoomId>>,
) -> Result<()> {
    use KeyCode::*;
    match key.code {
        Char('q') | Esc => {
            state.should_quit = true;
        }
        Char('m') => {
            let mut m = muted.lock().unwrap();
            *m = !*m;
            state.notify(if *m { "🔇 Микрофон выключен".into() } else { "🎙 Микрофон включён".into() });
        }
        Char('d') => {
            let mut d = deafened.lock().unwrap();
            *d = !*d;
            state.notify(if *d { "🎧 Звук выключен".into() } else { "🔊 Звук включён".into() });
        }
        Char('o') => {
            state.screen = Screen::DeviceSelect;
        }
        Tab => {
            state.focus = match state.focus {
                Focus::Rooms => Focus::Users,
                Focus::Users => Focus::Rooms,
            };
        }
        Char('j') | KeyCode::Down => {
            match state.focus {
                Focus::Rooms => {
                    if state.room_list_idx + 1 < state.rooms.len() {
                        state.room_list_idx += 1;
                    }
                }
                Focus::Users => {
                    let count = state.rooms.iter().map(|r| r.users.len()).sum::<usize>();
                    if state.user_list_idx + 1 < count {
                        state.user_list_idx += 1;
                    }
                }
            }
        }
        Char('k') | KeyCode::Up => {
            match state.focus {
                Focus::Rooms => {
                    state.room_list_idx = state.room_list_idx.saturating_sub(1);
                }
                Focus::Users => {
                    state.user_list_idx = state.user_list_idx.saturating_sub(1);
                }
            }
        }
        Enter => {
            if state.focus == Focus::Rooms {
                if let Some(room) = state.rooms.get(state.room_list_idx) {
                    let room_id = room.id;
                    // Покидаем предыдущую
                    if let Some(old) = state.current_room {
                        if let Some(sig) = signaling.as_ref() {
                            let _ = sig.client_tx.try_send(ClientMsg::LeaveRoom { room_id: old });
                        }
                    }
                    state.current_room = Some(room_id);
                    let _ = room_tx.send(Some(room_id));

                    if let Some(sig) = signaling.as_ref() {
                        let _ = sig.client_tx.try_send(ClientMsg::JoinRoom { room_id });
                    }
                    state.notify(format!("Вошли в #{}", state.rooms[state.room_list_idx].name));
                }
            }
        }
        Char('n') => {
            // Создать комнату (упрощённо — по фиксированному имени)
            // В полной версии нужен inline input
            state.notify("Функция создания комнаты: введите имя (TODO)".into());
        }
        _ => {}
    }
    Ok(())
}

// ─── Обработка клавиш: выбор устройств ───────────────────────────────────────

fn handle_device_keys(
    state: &mut AppState,
    key: crossterm::event::KeyEvent,
    capture: &mut Option<CaptureStream>,
    playback: &mut Option<PlaybackStream>,
    playback_tx: &mut Option<mpsc::Sender<Vec<u8>>>,
    playback_fwd: &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    encoded_tx: &mpsc::Sender<Vec<u8>>,
    metrics: &MetricsRef,
    muted: &Arc<Mutex<bool>>,
    deafened: &Arc<Mutex<bool>>,
    devs: &AudioDevices,
) -> Result<()> {
    use KeyCode::*;
    match key.code {
        Esc => {
            state.screen = Screen::Main;
        }
        Tab => {
            state.device_focus = match state.device_focus {
                DeviceFocus::Input  => DeviceFocus::Output,
                DeviceFocus::Output => DeviceFocus::Input,
            };
        }
        Char('j') | Down => {
            match state.device_focus {
                DeviceFocus::Input => {
                    if state.selected_input + 1 < state.input_devices.len() {
                        state.selected_input += 1;
                    }
                }
                DeviceFocus::Output => {
                    if state.selected_output + 1 < state.output_devices.len() {
                        state.selected_output += 1;
                    }
                }
            }
        }
        Char('k') | Up => {
            match state.device_focus {
                DeviceFocus::Input => {
                    state.selected_input = state.selected_input.saturating_sub(1);
                }
                DeviceFocus::Output => {
                    state.selected_output = state.selected_output.saturating_sub(1);
                }
            }
        }
        Enter => {
            // Перезапускаем аудио с выбранными устройствами
            start_audio(state, capture, playback, playback_tx, playback_fwd,
                        encoded_tx, metrics, muted, deafened, devs)?;
            state.screen = Screen::Main;
            state.notify("Аудиоустройства обновлены".into());
        }
        _ => {}
    }
    Ok(())
}

// ─── Запуск аудио ─────────────────────────────────────────────────────────────

fn start_audio(
    state: &AppState,
    capture: &mut Option<CaptureStream>,
    playback: &mut Option<PlaybackStream>,
    playback_tx: &mut Option<mpsc::Sender<Vec<u8>>>,
    playback_fwd: &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    encoded_tx: &mpsc::Sender<Vec<u8>>,
    metrics: &MetricsRef,
    muted: &Arc<Mutex<bool>>,
    deafened: &Arc<Mutex<bool>>,
    devs: &AudioDevices,
) -> Result<()> {
    // Остановить текущие стримы
    *capture = None;
    *playback = None;

    // Найти выбранные устройства
    let input_name = state.input_devices.get(state.selected_input);
    let output_name = state.output_devices.get(state.selected_output);

    let input_dev = input_name
        .and_then(|n| devs.find_input(n))
        .or_else(|| devs.default_input());

    let output_dev = output_name
        .and_then(|n| devs.find_output(n))
        .or_else(|| devs.default_output());

    // Запустить playback
    if let Some(dev) = output_dev {
        match PlaybackStream::new(&dev, deafened.clone()) {
            Ok(pb) => {
                let tx = pb.packet_tx.clone();
                *playback_tx = Some(tx.clone());
                *playback_fwd.lock().unwrap() = Some(tx);
                *playback = Some(pb);
            }
            Err(e) => tracing::warn!("playback init: {e}"),
        }
    }

    // Запустить capture
    if let Some(dev) = input_dev {
        match CaptureStream::new(&dev, encoded_tx.clone(), metrics.clone(), muted.clone()) {
            Ok(cap) => { *capture = Some(cap); }
            Err(e) => tracing::warn!("capture init: {e}"),
        }
    }

    Ok(())
}
