//! 录麦克风：系统默认的输入设备，转成 16 kHz、16 位、单声道 PCM，每 100 毫秒交出一块。

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use duolo_core::UserError;

/// 输出的采样率。
pub const RATE: u32 = 16_000;
/// 每块多少个采样（100 毫秒）。
const CHUNK: usize = RATE as usize / 10;
/// 等系统打开麦克风最长多久。
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// 一块声音和它的音量（0~1）。
pub struct Chunk {
    pub pcm: Vec<i16>,
    pub level: f32,
}

/// 正在录音。丢掉就停止。
pub struct Mic {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Mic {
    /// 开始录音，声音从返回的通道里取。
    pub fn start() -> Result<(Self, Receiver<Chunk>)> {
        let (chunks, rx) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let (opened, open_result) = mpsc::channel();
        // 有些系统上音频流不能跨线程，从打开到关闭都放在这个线程里。
        let thread = thread::spawn(move || match open(chunks) {
            Ok(stream) => {
                let _ = opened.send(Ok(()));
                let _ = stopped.recv();
                drop(stream);
            }
            Err(err) => {
                let _ = opened.send(Err(err));
            }
        });
        let mic = Self { stop: Some(stop), thread: Some(thread) };
        open_result.recv_timeout(OPEN_TIMEOUT).map_err(|_| anyhow!("microphone did not open"))??;
        Ok((mic, rx))
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn open(chunks: Sender<Chunk>) -> Result<cpal::Stream> {
    let device = cpal::default_host().default_input_device().ok_or(UserError::VoicePermission)?;
    let supported = device.default_input_config().context("cannot read the microphone format")?;
    let config = supported.config();
    let name = device.description().map(|d| d.to_string()).unwrap_or_default();
    eprintln!("microphone: {name}, {} Hz, {} ch, {:?}", config.sample_rate, config.channels, supported.sample_format());
    let pcm = Resampler::new(config.sample_rate, usize::from(config.channels), chunks);
    let on_error = |err| eprintln!("microphone: {err}");
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, pcm, on_error),
        SampleFormat::I16 => build::<i16>(&device, &config, pcm, on_error),
        SampleFormat::I32 => build::<i32>(&device, &config, pcm, on_error),
        SampleFormat::U16 => build::<u16>(&device, &config, pcm, on_error),
        other => return Err(anyhow!("unsupported microphone format {other:?}")),
    }?;
    stream.play().context("cannot start the microphone")?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut pcm: Resampler,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = device
        .build_input_stream::<T, _, _>(
            *config,
            move |data: &[T], _: &_| pcm.push(data.iter().map(|s| s.to_sample::<f32>())),
            on_error,
            None,
        )
        .map_err(|err| match err.kind() {
            cpal::ErrorKind::PermissionDenied => anyhow::Error::from(UserError::VoicePermission),
            _ => anyhow::Error::from(err).context("cannot open the microphone"),
        })?;
    Ok(stream)
}

/// 把任意采样率、声道数的声音转成 16 kHz 单声道：声道取平均，降采样时取区间内的平均值（顺带滤掉一些高频）。
struct Resampler {
    /// 输入采样率 / 16000。
    step: f64,
    channels: usize,
    /// 当前帧已经读到第几个声道、这几个声道的和。
    channel: usize,
    frame_sum: f32,
    /// 距离下一个输出采样还差多少个输入帧，以及期间输入的和与个数。
    until_next: f64,
    sum: f32,
    count: u32,
    out: Vec<i16>,
    chunks: Sender<Chunk>,
}

impl Resampler {
    fn new(rate: u32, channels: usize, chunks: Sender<Chunk>) -> Self {
        let step = f64::from(rate) / f64::from(RATE);
        Self {
            step,
            channels: channels.max(1),
            channel: 0,
            frame_sum: 0.0,
            until_next: step,
            sum: 0.0,
            count: 0,
            out: Vec::with_capacity(CHUNK),
            chunks,
        }
    }

    /// 交错排列的采样（-1~1）。
    fn push(&mut self, samples: impl Iterator<Item = f32>) {
        for s in samples {
            self.frame_sum += s;
            self.channel += 1;
            if self.channel < self.channels {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let frame = self.frame_sum / self.channels as f32;
            self.channel = 0;
            self.frame_sum = 0.0;
            self.sum += frame;
            self.count += 1;
            self.until_next -= 1.0;
            // 升采样（输入低于 16 kHz）时一个输入帧对应好几个输出采样。
            while self.until_next <= 0.0 {
                #[allow(clippy::cast_precision_loss)]
                let value = if self.count > 0 { self.sum / self.count as f32 } else { frame };
                #[allow(clippy::cast_possible_truncation)]
                self.out.push((value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16);
                self.sum = 0.0;
                self.count = 0;
                self.until_next += self.step;
                if self.out.len() >= CHUNK {
                    self.flush();
                }
            }
        }
    }

    fn flush(&mut self) {
        if self.out.is_empty() {
            return;
        }
        let pcm = std::mem::replace(&mut self.out, Vec::with_capacity(CHUNK));
        let level = level(&pcm);
        let _ = self.chunks.send(Chunk { pcm, level });
    }
}

/// 音量（0~1）：均方根放大后截断，正常说话接近 1，安静的房间在 0.1 以下。
fn level(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return 0.0;
    }
    let sum: f64 = pcm.iter().map(|&s| (f64::from(s) / f64::from(i16::MAX)).powi(2)).sum();
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let rms = (sum / pcm.len() as f64).sqrt() as f32;
    (rms * 10.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resample(rate: u32, channels: usize, samples: &[f32]) -> Vec<i16> {
        let (tx, rx) = mpsc::channel();
        let mut r = Resampler::new(rate, channels, tx);
        r.push(samples.iter().copied());
        r.flush();
        rx.try_iter().flat_map(|c| c.pcm).collect()
    }

    #[test]
    fn downsamples_and_mixes_channels() {
        // 48 kHz 立体声一秒：左 0.5、右 0（平均 0.25）。
        let input: Vec<f32> = (0..48_000).flat_map(|_| [0.5, 0.0]).collect();
        let out = resample(48_000, 2, &input);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|&s| (s - 8191).abs() <= 1), "{:?}", &out[..4]);
    }

    #[test]
    fn upsamples_and_chunks() {
        let (tx, rx) = mpsc::channel();
        let mut r = Resampler::new(8_000, 1, tx);
        r.push(std::iter::repeat_n(0.1, 8_000));
        let chunks: Vec<Chunk> = rx.try_iter().collect();
        assert_eq!(chunks.len(), 10);
        assert!(chunks.iter().all(|c| c.pcm.len() == CHUNK && c.level > 0.9));
    }

    #[test]
    fn silence_is_quiet() {
        assert!(level(&[0; 100]) < 0.01);
        assert!(level(&[300; 100]) < 0.12);
    }
}
