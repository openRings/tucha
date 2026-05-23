use anyhow::{Context, Result};
use audiopus::{coder::Encoder, coder::Decoder, Application, Channels, SampleRate};

/// 20ms фрейм при 48kHz моно = 960 семплов
pub const FRAME_SIZE: usize = 960;
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u8 = 1;

pub struct OpusEncoder {
    enc: Encoder,
}

impl OpusEncoder {
    pub fn new() -> Result<Self> {
        let enc = Encoder::new(
            SampleRate::Hz48000,
            Channels::Mono,
            Application::Voip,
        )
        .context("create opus encoder")?;
        Ok(Self { enc })
    }

    /// Кодирует срез f32 семплов (длина FRAME_SIZE) в Opus байты
    pub fn encode(&mut self, pcm: &[f32]) -> Result<Vec<u8>> {
        let mut out = vec![0u8; 4000];
        let len = self.enc
            .encode_float(pcm, &mut out)
            .context("opus encode")?;
        out.truncate(len);
        Ok(out)
    }
}

pub struct OpusDecoder {
    dec: Decoder,
}

impl OpusDecoder {
    pub fn new() -> Result<Self> {
        let dec = Decoder::new(SampleRate::Hz48000, Channels::Mono)
            .context("create opus decoder")?;
        Ok(Self { dec })
    }

    /// Декодирует Opus байты в f32 семплы
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<f32>> {
        let mut out = vec![0f32; FRAME_SIZE * 2];
        let n = self.dec
            .decode_float(Some(data), &mut out, false)
            .context("opus decode")?;
        out.truncate(n);
        Ok(out)
    }

    /// Декодирует потерянный пакет (PLC)
    pub fn decode_lost(&mut self) -> Result<Vec<f32>> {
        let mut out = vec![0f32; FRAME_SIZE];
        let none: Option<&[u8]> = None;
        let n = self.dec
            .decode_float(none, &mut out, false)
            .context("opus plc")?;
        out.truncate(n);
        Ok(out)
    }
}
