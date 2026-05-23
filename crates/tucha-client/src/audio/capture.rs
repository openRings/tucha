use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    Device, Stream, StreamConfig,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::{codec::{OpusEncoder, FRAME_SIZE, SAMPLE_RATE}, is_active, rms_level, MetricsRef};

/// Поток захвата с микрофона.
/// Читает PCM, кодирует в Opus и отправляет в канал.
pub struct CaptureStream {
    _stream: Stream,
}

impl CaptureStream {
    pub fn new(
        device: &Device,
        encoded_tx: mpsc::Sender<Vec<u8>>,
        metrics: MetricsRef,
        muted: Arc<Mutex<bool>>,
    ) -> Result<Self> {
        let config = StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        let encoder = Arc::new(Mutex::new(
            OpusEncoder::new().context("create encoder")?,
        ));

        // Накопительный буфер (может прийти меньше FRAME_SIZE семплов за раз)
        let accumulator: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(FRAME_SIZE * 2)));

        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                let is_muted = *muted.lock().unwrap();

                // Обновляем метрики уровня
                {
                    let mut m = metrics.lock().unwrap();
                    m.input_level = rms_level(data);
                    m.is_speaking = is_active(data) && !is_muted;
                    m.is_muted = is_muted;
                }

                if is_muted { return; }

                let mut acc = accumulator.lock().unwrap();
                acc.extend_from_slice(data);

                while acc.len() >= FRAME_SIZE {
                    let frame: Vec<f32> = acc.drain(..FRAME_SIZE).collect();
                    let mut enc = encoder.lock().unwrap();
                    match enc.encode(&frame) {
                        Ok(opus_data) => {
                            let _ = encoded_tx.try_send(opus_data);
                        }
                        Err(e) => tracing::warn!("encode error: {e}"),
                    }
                }
            },
            |e| tracing::error!("capture stream error: {e}"),
            None,
        )
        .context("build input stream")?;

        stream.play().context("play input stream")?;
        Ok(Self { _stream: stream })
    }
}
