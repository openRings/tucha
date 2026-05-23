use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, StreamTrait},
    Device, Stream, StreamConfig,
};
use ringbuf::{
    traits::{Consumer, Producer, Split},
    HeapRb,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use super::codec::{OpusDecoder, FRAME_SIZE, SAMPLE_RATE};

/// Размер jitter-буфера: ~10 фреймов × 960 семплов = ~200ms буферизации
const JITTER_CAPACITY: usize = FRAME_SIZE * 10;

/// Поток воспроизведения.
/// Принимает закодированные Opus пакеты, декодирует и отправляет в динамики.
pub struct PlaybackStream {
    _stream: Stream,
    /// Канал для подачи новых Opus-пакетов в декодер
    pub packet_tx: mpsc::Sender<Vec<u8>>,
}

impl PlaybackStream {
    pub fn new(device: &Device, deafened: Arc<Mutex<bool>>) -> Result<Self> {
        let config = StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        let (packet_tx, mut packet_rx) = mpsc::channel::<Vec<u8>>(64);

        // Ring buffer между декодером и cpal output callback
        let rb = HeapRb::<f32>::new(JITTER_CAPACITY);
        let (mut prod, mut cons) = rb.split();

        // Задача декодирования: читает пакеты → пишет PCM в ringbuf
        let deafened_dec = deafened.clone();
        tokio::spawn(async move {
            let mut decoder = match OpusDecoder::new() {
                Ok(d) => d,
                Err(e) => { tracing::error!("decoder init: {e}"); return; }
            };

            while let Some(opus_data) = packet_rx.recv().await {
                if *deafened_dec.lock().unwrap() { continue; }

                let pcm = match decoder.decode(&opus_data) {
                    Ok(p) => p,
                    Err(_) => decoder.decode_lost().unwrap_or_default(),
                };

                // Пушим в ringbuf — push_slice сам дропает лишнее при переполнении
                prod.push_slice(&pcm);
            }
        });

        // cpal output callback: читает из ringbuf и заполняет аудиобуфер
        let stream = device.build_output_stream(
            &config,
            move |output: &mut [f32], _| {
                let n = cons.pop_slice(output);
                // Тишина если в буфере пусто
                output[n..].fill(0.0);
            },
            |e| tracing::error!("playback stream error: {e}"),
            None,
        )
        .context("build output stream")?;

        stream.play().context("play output stream")?;

        Ok(Self {
            _stream: stream,
            packet_tx,
        })
    }
}
