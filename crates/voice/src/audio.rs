//! Microphone in and speakers out through cpal. Streams live on their own
//! threads (a cpal stream may not move between threads) and stop when the
//! handle is dropped.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::{MIC_RATE, Result, VoiceError};

/// Linear resampler that keeps its position across chunks.
#[derive(Debug)]
pub struct Resampler {
    step: f64,
    pos: f64,
    last: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self {
            step: f64::from(from) / f64::from(to),
            pos: 0.0,
            last: 0.0,
        }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if (self.step - 1.0).abs() < f64::EPSILON {
            return input.to_vec();
        }
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        // Positions are relative to `input`, with -1 meaning `self.last`.
        while self.pos < input.len() as f64 - 1.0 {
            let i = self.pos.floor();
            let frac = (self.pos - i) as f32;
            let a = if i < 0.0 {
                self.last
            } else {
                input[i as usize]
            };
            let b = input[(i + 1.0) as usize];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        if let Some(&l) = input.last() {
            self.last = l;
        }
        out
    }
}

fn to_mono<T: Copy>(data: &[T], channels: usize, conv: impl Fn(T) -> f32) -> Vec<f32> {
    data.chunks(channels.max(1))
        .map(|frame| frame.iter().map(|&s| conv(s)).sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Captures the default microphone as 16 kHz mono chunks.
pub struct Mic {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Mic {
    pub fn start(chunks: SyncSender<Vec<f32>>) -> Result<Self> {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let thread = std::thread::Builder::new()
            .name("sidekick-mic".into())
            .spawn(move || {
                let stream = match open_mic(chunks) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                let _ = stop_rx.recv();
                drop(stream);
            })?;
        ready_rx
            .recv()
            .map_err(|_| VoiceError::Audio("microphone thread ended".into()))??;
        Ok(Self {
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open_mic(chunks: SyncSender<Vec<f32>>) -> Result<cpal::Stream> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or(VoiceError::NoMicrophone)?;
    let supported = device
        .default_input_config()
        .map_err(|e| VoiceError::Audio(e.to_string()))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    log::info!("microphone: {} Hz, {channels} channels, {format:?}", rate);
    let mut resampler = Resampler::new(rate, MIC_RATE);
    let mut send = move |mono: Vec<f32>| {
        // A full queue means the listener is behind; dropping audio beats
        // growing without bound.
        let _ = chunks.try_send(resampler.process(&mono));
    };
    let err = |e| log::warn!("microphone: {e}");
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |d: &[f32], _: &_| send(to_mono(d, channels, |s| s)),
            err,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |d: &[i16], _: &_| send(to_mono(d, channels, |s| f32::from(s) / 32_768.0)),
            err,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &config,
            move |d: &[u16], _: &_| {
                send(to_mono(d, channels, |s| {
                    (f32::from(s) - 32_768.0) / 32_768.0
                }))
            },
            err,
            None,
        ),
        other => {
            return Err(VoiceError::Audio(format!(
                "unsupported microphone format {other:?}"
            )));
        }
    }
    .map_err(|e| VoiceError::Audio(e.to_string()))?;
    stream
        .play()
        .map_err(|e| VoiceError::Audio(e.to_string()))?;
    Ok(stream)
}

/// Plays mono audio on the default output device.
pub struct Output {
    queue: Arc<Mutex<VecDeque<f32>>>,
    rate: u32,
    playing: Arc<AtomicBool>,
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Output {
    pub fn start() -> Result<Self> {
        let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let playing = Arc::new(AtomicBool::new(false));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32>>();
        let (q, p) = (queue.clone(), playing.clone());
        let thread = std::thread::Builder::new()
            .name("sidekick-speaker".into())
            .spawn(move || match open_output(q, p) {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })?;
        let rate = ready_rx
            .recv()
            .map_err(|_| VoiceError::Audio("speaker thread ended".into()))??;
        Ok(Self {
            queue,
            rate,
            playing,
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }

    /// Queues mono samples recorded at `rate`.
    pub fn play(&self, samples: &[f32], rate: u32) {
        let resampled = Resampler::new(rate, self.rate).process(samples);
        if let Ok(mut q) = self.queue.lock() {
            q.extend(resampled);
        }
    }

    /// Stops at once and forgets what was queued.
    pub fn clear(&self) {
        if let Ok(mut q) = self.queue.lock() {
            q.clear();
        }
    }

    /// Something is queued or still sounding.
    pub fn busy(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
            || self.queue.lock().map(|q| !q.is_empty()).unwrap_or(false)
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open_output(
    queue: Arc<Mutex<VecDeque<f32>>>,
    playing: Arc<AtomicBool>,
) -> Result<(cpal::Stream, u32)> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(VoiceError::NoSpeaker)?;
    let supported = device
        .default_output_config()
        .map_err(|e| VoiceError::Audio(e.to_string()))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let rate = config.sample_rate.0;
    let fill = move |out: &mut dyn FnMut(usize, f32), frames: usize| {
        let mut q = match queue.lock() {
            Ok(q) => q,
            Err(_) => return,
        };
        let mut any = false;
        for f in 0..frames {
            let s = q.pop_front();
            any |= s.is_some();
            let v = s.unwrap_or(0.0).clamp(-1.0, 1.0);
            for c in 0..channels {
                out(f * channels + c, v);
            }
        }
        playing.store(any, Ordering::Relaxed);
    };
    let err = |e| log::warn!("speaker: {e}");
    let stream = match format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |d: &mut [f32], _: &_| {
                let frames = d.len() / channels;
                fill(&mut |i, v| d[i] = v, frames);
            },
            err,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            &config,
            move |d: &mut [i16], _: &_| {
                let frames = d.len() / channels;
                fill(&mut |i, v| d[i] = (v * 32_767.0) as i16, frames);
            },
            err,
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            &config,
            move |d: &mut [u16], _: &_| {
                let frames = d.len() / channels;
                fill(&mut |i, v| d[i] = (v * 32_767.0 + 32_768.0) as u16, frames);
            },
            err,
            None,
        ),
        other => {
            return Err(VoiceError::Audio(format!(
                "unsupported speaker format {other:?}"
            )));
        }
    }
    .map_err(|e| VoiceError::Audio(e.to_string()))?;
    stream
        .play()
        .map_err(|e| VoiceError::Audio(e.to_string()))?;
    Ok((stream, rate))
}

/// Receives microphone chunks; kept here so callers do not need mpsc types.
pub type Chunks = Receiver<Vec<f32>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_across_chunks() {
        let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut r = Resampler::new(48_000, 16_000);
        let out: Vec<f32> = tone.chunks(480).flat_map(|c| r.process(c)).collect();
        assert!((out.len() as i64 - 16_000).abs() <= 2, "{}", out.len());
        // Every third input sample, give or take interpolation.
        assert!((out[100] - tone[300]).abs() < 0.02);
        let mut same = Resampler::new(16_000, 16_000);
        assert_eq!(same.process(&[0.1, 0.2]), vec![0.1, 0.2]);
    }

    #[test]
    fn downmixes_to_mono() {
        assert_eq!(to_mono(&[1.0f32, 0.0, 0.5, 0.5], 2, |s| s), vec![0.5, 0.5]);
    }
}
