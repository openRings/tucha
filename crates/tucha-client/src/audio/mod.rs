pub mod capture;
pub mod playback;
pub mod codec;
pub mod devices;


use std::sync::{Arc, Mutex};

/// Уровень громкости (0.0 – 1.0), вычисляется как RMS
pub fn rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() { return 0.0; }
    let mean_sq = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    mean_sq.sqrt()
}

/// Простой VAD: возвращает true если сигнал выше порога тишины
pub fn is_active(samples: &[f32]) -> bool {
    rms_level(samples) > 0.01
}

/// Команды управления аудиодвижком
#[derive(Debug)]
pub enum AudioCmd {
    SetInputDevice(String),
    SetOutputDevice(String),
    SetMuted(bool),
    SetDeafened(bool),
    Stop,
}

/// Метрики аудио (уровни, флаги)
#[derive(Debug, Clone, Default)]
pub struct AudioMetrics {
    pub input_level: f32,
    pub is_muted: bool,
    pub is_deafened: bool,
    pub is_speaking: bool,
}

pub type MetricsRef = Arc<Mutex<AudioMetrics>>;
