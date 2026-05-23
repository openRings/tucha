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
    playback::PlaybackStream,
    AudioMetrics, MetricsRef,
};
use network::{AudioStream, SignalingClient};
use tucha_proto::{ClientMsg, RoomId, ServerMsg};

#[tokio::main]
async fn main() -> Result<()> {
    let file = std::fs::File::create("/tmp/tucha.log")?;
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter("debug")
        .init();

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(e) = &result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

async fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut state = AppState::new();

    // ─── Аудиоустройства ──────────────────────────────────────────────────────
    let audio_devs = AudioDevices::new();
    state.input_devices = audio_devs.input_devices().into_iter().map(|d| d.name).collect();
    state.output_devices = audio_devs.output_devices().into_iter().map(|d| d.name).collect();

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

    // ─── Состояние аудио ──────────────────────────────────────────────────────
    let metrics: MetricsRef = Arc::new(Mutex::new(AudioMetrics::default()));
    let muted    = Arc::new(Mutex::new(false));
    let deafened = Arc::new(Mutex::new(false));

    // ─── Watch-канал текущей комнаты (нужен AudioStream для UDP пакетов) ──────
    let (room_tx, room_rx) = watch::channel::<Option<RoomId>>(None);

    // ─── Сетевые и аудио хэндлы ───────────────────────────────────────────────
    let mut signaling: Option<SignalingClient> = None;

    // Канал: encoded audio → UDP send
    // Создаётся внутри AudioStream, сюда кладём его Sender
    let audio_enc_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>> =
        Arc::new(Mutex::new(None));

    // Канал: UDP recv decoded → Playback
    // playback_fwd_tx обновляется при каждой смене устройства вывода
    let playback_fwd_tx: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>> =
        Arc::new(Mutex::new(None));

    let mut _capture_stream: Option<CaptureStream>   = None;
    let mut _playback_stream: Option<PlaybackStream> = None;

    let tick_rate = Duration::from_millis(50);

    loop {
        terminal.draw(|f| ui::render(f, &state))?;

        // Обновляем UI-метрики из аудио потока
        {
            let m = metrics.lock().unwrap();
            state.my_input_level = m.input_level;
            state.is_muted       = m.is_muted;
        }
        state.is_deafened = *deafened.lock().unwrap();
        state.clear_expired_notification();

        // Читаем входящие TCP сообщения от сервера
        if let Some(sig) = signaling.as_mut() {
            while let Ok(msg) = sig.server_rx.try_recv() {
                state.apply(msg);
            }
        }

        // Обработка клавиатуры
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
                            &audio_enc_tx,
                            &playback_fwd_tx,
                            &metrics,
                            &muted,
                            &deafened,
                            &audio_devs,
                            room_rx.clone(),
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
                            &audio_enc_tx,
                            &playback_fwd_tx,
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

// ─── Подключение ─────────────────────────────────────────────────────────────

async fn handle_connect_keys(
    state: &mut AppState,
    key: crossterm::event::KeyEvent,
    signaling: &mut Option<SignalingClient>,
    capture:  &mut Option<CaptureStream>,
    playback: &mut Option<PlaybackStream>,
    audio_enc_tx:    &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    playback_fwd_tx: &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    metrics:  &MetricsRef,
    muted:    &Arc<Mutex<bool>>,
    deafened: &Arc<Mutex<bool>>,
    devs:     &AudioDevices,
    room_rx:  watch::Receiver<Option<RoomId>>,
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
        Char(c) => match state.connect_field {
            ConnectField::Server   => state.server_input.push(c),
            ConnectField::Username => state.username_input.push(c),
        },
        Backspace => match state.connect_field {
            ConnectField::Server   => { state.server_input.pop(); }
            ConnectField::Username => { state.username_input.pop(); }
        },
        Enter => {
            if state.username_input.is_empty() {
                state.notify("Введите имя пользователя".into());
                return Ok(());
            }
            state.conn_status = ConnStatus::Connecting;

            let server   = state.server_input.clone();
            let username = state.username_input.clone();

            // 1. TCP signaling
            let mut client = match SignalingClient::connect(&server).await {
                Ok(c)  => c,
                Err(e) => {
                    state.conn_status = ConnStatus::Error(e.to_string());
                    state.notify(format!("TCP ошибка: {e}"));
                    return Ok(());
                }
            };

            client.send(ClientMsg::Connect { username }).await?;

            // 2. Ждём Connected чтобы получить user_id
            let user_id = match client.server_rx.recv().await {
                Some(ServerMsg::Connected { user_id, rooms }) => {
                    state.my_user_id = Some(user_id);
                    state.rooms      = rooms;
                    user_id
                }
                Some(ServerMsg::Error { message }) => {
                    state.conn_status = ConnStatus::Error(message.clone());
                    state.notify(format!("Сервер: {message}"));
                    return Ok(());
                }
                _ => {
                    state.conn_status = ConnStatus::Error("неожиданный ответ".into());
                    state.notify("Неожиданный ответ от сервера".into());
                    return Ok(());
                }
            };

            // 3. UDP AudioStream — строим адрес (заменяем TCP порт → UDP порт)
            let udp_addr: std::net::SocketAddr = {
                let base = if let Some(pos) = server.rfind(':') {
                    format!("{}:7879", &server[..pos])
                } else {
                    format!("{server}:7879")
                };
                base.parse().unwrap_or_else(|_| "127.0.0.1:7879".parse().unwrap())
            };

            let audio_stream = match AudioStream::connect(udp_addr, user_id, room_rx).await {
                Ok(s)  => s,
                Err(e) => {
                    state.conn_status = ConnStatus::Error(e.to_string());
                    state.notify(format!("UDP ошибка: {e}"));
                    return Ok(());
                }
            };

            // 4. Сохраняем Sender encoded audio → UDP send task
            *audio_enc_tx.lock().unwrap() = Some(audio_stream.encoded_tx.clone());

            // 5. Запускаем аудио устройства (capture → audio_stream.encoded_tx)
            start_audio(
                state, capture, playback,
                &audio_stream.encoded_tx,
                playback_fwd_tx,
                metrics, muted, deafened, devs,
            )?;

            // 6. Бридж: UDP decoded_rx → playback
            //    audio_stream.decoded_rx → playback_fwd_tx
            {
                let pfwd = playback_fwd_tx.clone();
                let mut decoded_rx = audio_stream.decoded_rx;
                tokio::spawn(async move {
                    while let Some(pkt) = decoded_rx.recv().await {
                        if let Some(tx) = pfwd.lock().unwrap().as_ref() {
                            let _ = tx.try_send(pkt);
                        }
                    }
                    tracing::info!("audio bridge task ended");
                });
            }

            *signaling = Some(client);
            state.conn_status = ConnStatus::Connected;
            state.screen      = Screen::Main;
            state.notify(format!("Подключено! user_id={user_id}"));
        }
        _ => {}
    }
    Ok(())
}

// ─── Главный экран ────────────────────────────────────────────────────────────

async fn handle_main_keys(
    state: &mut AppState,
    key: crossterm::event::KeyEvent,
    signaling: &mut Option<SignalingClient>,
    muted:    &Arc<Mutex<bool>>,
    deafened: &Arc<Mutex<bool>>,
    room_tx:  &watch::Sender<Option<RoomId>>,
) -> Result<()> {
    use KeyCode::*;
    match key.code {
        Char('q') | Esc => { state.should_quit = true; }
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
        Char('o') => { state.screen = Screen::DeviceSelect; }
        Tab => {
            state.focus = match state.focus {
                Focus::Rooms => Focus::Users,
                Focus::Users => Focus::Rooms,
            };
        }
        Char('j') | Down => match state.focus {
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
        },
        Char('k') | Up => match state.focus {
            Focus::Rooms => { state.room_list_idx = state.room_list_idx.saturating_sub(1); }
            Focus::Users => { state.user_list_idx = state.user_list_idx.saturating_sub(1); }
        },
        Enter => {
            if state.focus == Focus::Rooms {
                if let Some(room) = state.rooms.get(state.room_list_idx) {
                    let room_id   = room.id;
                    let room_name = room.name.clone();

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
                    state.notify(format!("Вошли в #{room_name}"));
                }
            }
        }
        Char('n') => {
            state.notify("TODO: создание комнат (введите имя)".into());
        }
        _ => {}
    }
    Ok(())
}

// ─── Выбор устройств ─────────────────────────────────────────────────────────

fn handle_device_keys(
    state: &mut AppState,
    key: crossterm::event::KeyEvent,
    capture:  &mut Option<CaptureStream>,
    playback: &mut Option<PlaybackStream>,
    audio_enc_tx:    &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    playback_fwd_tx: &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    metrics:  &MetricsRef,
    muted:    &Arc<Mutex<bool>>,
    deafened: &Arc<Mutex<bool>>,
    devs:     &AudioDevices,
) -> Result<()> {
    use KeyCode::*;
    match key.code {
        Esc => { state.screen = Screen::Main; }
        Tab => {
            state.device_focus = match state.device_focus {
                DeviceFocus::Input  => DeviceFocus::Output,
                DeviceFocus::Output => DeviceFocus::Input,
            };
        }
        Char('j') | Down => match state.device_focus {
            DeviceFocus::Input => {
                if state.selected_input + 1 < state.input_devices.len() { state.selected_input += 1; }
            }
            DeviceFocus::Output => {
                if state.selected_output + 1 < state.output_devices.len() { state.selected_output += 1; }
            }
        },
        Char('k') | Up => match state.device_focus {
            DeviceFocus::Input  => { state.selected_input  = state.selected_input.saturating_sub(1); }
            DeviceFocus::Output => { state.selected_output = state.selected_output.saturating_sub(1); }
        },
        Enter => {
            if let Some(enc_tx) = audio_enc_tx.lock().unwrap().clone() {
                start_audio(state, capture, playback, &enc_tx,
                            playback_fwd_tx, metrics, muted, deafened, devs)?;
                state.notify("Аудиоустройства обновлены".into());
            } else {
                state.notify("Подключитесь к серверу сначала".into());
            }
            state.screen = Screen::Main;
        }
        _ => {}
    }
    Ok(())
}

// ─── Запуск аудио устройств ───────────────────────────────────────────────────
//
// Правильная схема:
//   Mic → CaptureStream → enc_tx ─────────────────────► AudioStream.encoded_tx → UDP
//                                                        UDP → AudioStream.decoded_rx
//   Playback ← PlaybackStream.packet_tx ← playback_fwd_tx ◄──────────────────────────
//
// AudioStream создаётся один раз при подключении и не пересоздаётся при смене устройств.
// При смене устройств пересоздаются только CaptureStream и PlaybackStream.
// enc_tx — это AudioStream.encoded_tx, передаётся снаружи.

fn start_audio(
    state:           &AppState,
    capture:         &mut Option<CaptureStream>,
    playback:        &mut Option<PlaybackStream>,
    enc_tx:          &mpsc::Sender<Vec<u8>>,
    playback_fwd_tx: &Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    metrics:         &MetricsRef,
    muted:           &Arc<Mutex<bool>>,
    deafened:        &Arc<Mutex<bool>>,
    devs:            &AudioDevices,
) -> Result<()> {
    // Остановить старые потоки (drop)
    *capture  = None;
    *playback = None;

    let input_dev = state.input_devices
        .get(state.selected_input)
        .and_then(|n| devs.find_input(n))
        .or_else(|| devs.default_input());

    let output_dev = state.output_devices
        .get(state.selected_output)
        .and_then(|n| devs.find_output(n))
        .or_else(|| devs.default_output());

    // Playback: cpal output → speaker
    if let Some(dev) = output_dev {
        match PlaybackStream::new(&dev, deafened.clone()) {
            Ok(pb) => {
                // Регистрируем новый packet_tx в общем форвардере
                *playback_fwd_tx.lock().unwrap() = Some(pb.packet_tx.clone());
                *playback = Some(pb);
            }
            Err(e) => tracing::warn!("playback init: {e}"),
        }
    }

    // Capture: mic → opus encode → enc_tx → [AudioStream → UDP]
    if let Some(dev) = input_dev {
        match CaptureStream::new(&dev, enc_tx.clone(), metrics.clone(), muted.clone()) {
            Ok(cap) => { *capture = Some(cap); }
            Err(e)  => tracing::warn!("capture init: {e}"),
        }
    }

    Ok(())
}
