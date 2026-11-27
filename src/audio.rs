//! Record lectures alongside notes and play them back (16 kHz mono WAV files).
//! - `Recorder` captures on cpal's audio thread into a bounded recycled-buffer channel and flushes
//!   to disk on a writer thread, syncing and atomically renaming on stop.
//! - `Player` streams PCM samples from disk through a bounded ~256 KiB chunk channel so multi-hour
//!   lectures open instantaneously in <1 MB RAM, with race-free generation-tagged seeking.

use crate::tf;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::time::Duration;

pub const RATE: u32 = 16_000;
const STREAM_CHUNK_SAMPLES: usize = 8_192; // ~0.51s of 16 kHz mono = 16 KiB

/// May the app use the microphone right now? (Desktop systems don't ask apps; Android does.)
pub fn mic_granted() -> bool {
    #[cfg(target_os = "android")]
    { android::granted() }
    #[cfg(not(target_os = "android"))]
    { true }
}

/// Show the system's "allow microphone?" dialog (Android); the answer is picked up by polling `mic_granted`.
pub fn mic_request() -> Result<(), String> {
    #[cfg(target_os = "android")]
    { android::request() }
    #[cfg(not(target_os = "android"))]
    { Ok(()) }
}

#[cfg(target_os = "android")]
pub(crate) use android::{clear_activity, set_activity, with_activity};

#[cfg(target_os = "android")]
mod android {
    use jni::{objects::JObject, strings::JNIString, sys::jobject, Env, JavaVM};
    const MIC: &str = "android.permission.RECORD_AUDIO";

    static ACTIVITY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    pub(crate) fn set_activity(ptr: *mut std::ffi::c_void) {
        ACTIVITY.store(ptr as usize, std::sync::atomic::Ordering::Release);
    }
    pub(crate) fn clear_activity() {
        ACTIVITY.store(0, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn with_activity<R>(f: impl for<'j> FnOnce(&mut Env<'j>, JObject<'j>) -> jni::errors::Result<R>) -> Result<R, String> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
        let act = ACTIVITY.load(std::sync::atomic::Ordering::Acquire);
        let raw = if act != 0 { act as jobject } else { ctx.context() as jobject };
        vm.attach_current_thread(|env: &mut Env<'_>| {
            let activity = unsafe { JObject::from_raw(env, raw) };
            let r = f(env, activity);
            if r.is_err() && env.exception_check() {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            r
        })
        .map_err(|e| e.to_string())
    }

    pub fn granted() -> bool {
        with_activity(|env, act| {
            let p = env.new_string(MIC)?;
            env.call_method(&act, JNIString::new("checkSelfPermission"), jni::jni_sig!("(Ljava/lang/String;)I"), &[(&p).into()])?.i()
        }) == Ok(0)
    }

    pub fn request() -> Result<(), String> {
        with_activity(|env, act| {
            env.call_method(&act, JNIString::new("requestMic"), jni::jni_sig!("()V"), &[])?;
            Ok(())
        })
    }
}

pub struct Recorder {
    stream: Option<cpal::Stream>,
    writer: Option<std::thread::JoinHandle<Result<u64, String>>>,
    /// The finished file; while recording it is `<path>.part`, which sync leaves alone.
    pub path: PathBuf,
    /// The microphone's last error, for the window to show.
    pub error: Arc<std::sync::Mutex<Option<String>>>,
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Ensure audio callback and writer thread shut down cleanly even if dropped without `.stop()`.
        drop(self.stream.take());
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

/// Where a recording is written while it's going on.
pub fn part_path(path: &Path) -> PathBuf {
    path.with_extension("wav.part")
}

fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    crate::storage::set_private_perms(from);
    #[cfg(windows)]
    if to.exists() {
        let _ = std::fs::remove_file(to);
    }
    std::fs::rename(from, to)
}

/// Finish a recording that was still going when the app stopped (crash, battery, the system
/// closing it): write its real length into the header and give it its final name. Its length in seconds.
pub fn finish_unfinished(path: &Path) -> Option<f32> {
    let part = part_path(path);
    if part.exists() {
        let len = std::fs::metadata(&part).ok()?.len();
        if len < 44 {
            return None;
        }
        let mut f = std::fs::OpenOptions::new().write(true).open(&part).ok()?;
        f.write_all(&wav_header_rate((len - 44) / 2, RATE)).ok()?;
        let _ = f.sync_all();
        drop(f);
        replace_file(&part, path).ok()?;
    }
    let len = std::fs::metadata(path).ok()?.len();
    Some(len.saturating_sub(44) as f32 / 2.0 / RATE as f32)
}

/// A whole 16-bit mono WAV file in memory at the given sample rate.
pub fn wav_bytes(samples: &[i16], rate: u32) -> Vec<u8> {
    let mut b = wav_header_rate(samples.len() as u64, rate);
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// 44-byte WAV header for 16-bit mono at `rate` Hz.
fn wav_header_rate(samples: u64, rate: u32) -> Vec<u8> {
    let data = (samples.saturating_mul(2)).min(u32::MAX as u64 - 36) as u32;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&1u16.to_le_bytes()); // mono
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&(rate.saturating_mul(2)).to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    h
}

/// Turns device audio (any rate, any channel count) into 16 kHz mono by averaging.
struct Downmix {
    channels: usize,
    step: f64,
    phase: f64,
    sum: f32,
    count: u32,
}

impl Downmix {
    fn push(&mut self, frame_avg: f32, out: &mut Vec<i16>) {
        self.sum += frame_avg;
        self.count += 1;
        self.phase += 1.0;
        let v = ((self.sum / self.count as f32).clamp(-1.0, 1.0) * 32767.0) as i16;
        let mut pushed = false;
        while self.phase >= self.step {
            out.push(v);
            pushed = true;
            self.phase -= self.step;
        }
        if pushed {
            self.sum = 0.0;
            self.count = 0;
        }
    }
    fn feed<T: SizedSample>(&mut self, data: &[T], out: &mut Vec<i16>)
    where
        f32: FromSample<T>,
    {
        for frame in data.chunks(self.channels.max(1)) {
            let avg = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / frame.len() as f32;
            self.push(avg, out);
        }
    }
}

fn input_stream<T: SizedSample>(
    dev: &cpal::Device,
    cfg: cpal::StreamConfig,
    tx: SyncSender<Vec<i16>>,
    pool_rx: Receiver<Vec<i16>>,
    error: Arc<std::sync::Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let mut dm = Downmix {
        channels: cfg.channels as usize,
        step: cfg.sample_rate as f64 / RATE as f64,
        phase: 0.0,
        sum: 0.0,
        count: 0,
    };
    dev.build_input_stream(
        cfg,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            // Reuse a preallocated buffer from the writer pool to avoid heap allocations on the audio thread.
            let mut out = pool_rx.try_recv().unwrap_or_else(|_| Vec::with_capacity(2048));
            out.clear();
            dm.feed(data, &mut out);
            let _ = tx.try_send(out);
        },
        move |e| {
            if let Ok(mut g) = error.lock() {
                *g = Some(e.to_string());
            }
        },
        None,
    )
    .map_err(|e| e.to_string())
}

impl Recorder {
    /// Start recording the default microphone into `path`.
    pub fn start(path: &Path) -> Result<Recorder, String> {
        let host = cpal::default_host();
        let dev = host.default_input_device().ok_or("No microphone found.")?;
        let sup = dev.default_input_config().map_err(|e| {
            tf!(
                "The microphone can't be opened ({e}). Check that no other app is using it, and that microphone access is allowed in your system's privacy settings.",
                e = e
            )
        })?;
        // Bounded channel + buffer recycling pool (no unbounded growth if disk stalls).
        let (tx, rx) = sync_channel::<Vec<i16>>(64);
        let (pool_tx, pool_rx) = sync_channel::<Vec<i16>>(64);
        for _ in 0..16 {
            let _ = pool_tx.try_send(Vec::with_capacity(2048));
        }
        let part = part_path(path);
        let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        crate::storage::set_private_perms(&part);
        file.write_all(&wav_header_rate(0, RATE)).map_err(|e| e.to_string())?;
        let writer = std::thread::spawn(move || -> Result<u64, String> {
            let mut n = 0u64;
            let mut buf = std::io::BufWriter::new(file);
            for chunk in rx {
                for s in &chunk {
                    buf.write_all(&s.to_le_bytes()).map_err(|e| e.to_string())?;
                }
                n += chunk.len() as u64;
                let _ = pool_tx.try_send(chunk);
            }
            let mut file = buf.into_inner().map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            file.write_all(&wav_header_rate(n, RATE)).map_err(|e| e.to_string())?;
            let _ = file.sync_all();
            Ok(n)
        });
        let cfg: cpal::StreamConfig = sup.into();
        let error = Arc::new(std::sync::Mutex::new(None));
        let stream = match sup.sample_format() {
            SampleFormat::F32 => input_stream::<f32>(&dev, cfg, tx, pool_rx, error.clone()),
            SampleFormat::I16 => input_stream::<i16>(&dev, cfg, tx, pool_rx, error.clone()),
            SampleFormat::U16 => input_stream::<u16>(&dev, cfg, tx, pool_rx, error.clone()),
            SampleFormat::I24 => input_stream::<cpal::I24>(&dev, cfg, tx, pool_rx, error.clone()),
            SampleFormat::I32 => input_stream::<i32>(&dev, cfg, tx, pool_rx, error.clone()),
            f => Err(tf!("Unsupported microphone format {f}", f = format!("{:?}", f))),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Recorder { stream: Some(stream), writer: Some(writer), path: path.to_path_buf(), error })
    }

    /// Stop and finish the file; returns its length in seconds.
    pub fn stop(mut self) -> Result<f32, String> {
        drop(self.stream.take());
        let n = self
            .writer
            .take()
            .ok_or("already stopped")?
            .join()
            .map_err(|_| "the recording writer crashed".to_string())??;
        replace_file(&part_path(&self.path), &self.path).map_err(|e| e.to_string())?;
        Ok(n as f32 / RATE as f32)
    }
}

/// Read one of our WAV files back as samples (used for transcription and tests).
pub fn load_wav(path: &Path) -> Result<Vec<i16>, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    if b.len() < 44 || &b[..4] != b"RIFF" {
        return Err(crate::tr("Not a recording file").into());
    }
    Ok(b[44..].as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect())
}

/// Inspect a WAV file header without reading its PCM payload into RAM.
/// Returns `(total_samples, sample_rate)`.
fn inspect_wav(path: &Path) -> Result<(usize, u32), String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = f.metadata().map_err(|e| e.to_string())?;
    if meta.len() < 44 {
        return Err(crate::tr("Not a recording file").into());
    }
    let mut hdr = [0u8; 44];
    f.read_exact(&mut hdr).map_err(|e| e.to_string())?;
    if &hdr[..4] != b"RIFF" || &hdr[8..12] != b"WAVE" {
        return Err(crate::tr("Not a recording file").into());
    }
    let rate = u32::from_le_bytes([hdr[24], hdr[25], hdr[26], hdr[27]]).max(1);
    let total_samples = ((meta.len() - 44) / 2) as usize;
    Ok((total_samples, rate))
}

struct AudioChunk {
    gen: u64,
    start: usize,
    samples: Vec<i16>,
}

/// Spawns a disk reader thread streaming bounded 16 KiB PCM chunks from `path`.
fn spawn_wav_reader(
    path: PathBuf,
    total_samples: usize,
    seek_gen: Arc<AtomicU64>,
    seek_target: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) -> (Receiver<AudioChunk>, std::thread::JoinHandle<()>) {
    let (tx, rx) = sync_channel::<AudioChunk>(16); // max ~256 KiB buffered
    let handle = std::thread::spawn(move || {
        let Ok(mut file) = std::fs::File::open(&path) else { return };
        let mut cur_gen = 0u64;
        let mut cur_sample = 0usize;
        let _ = file.seek(SeekFrom::Start(44));
        let mut raw = vec![0u8; STREAM_CHUNK_SAMPLES * 2];
        while !stop.load(Ordering::Acquire) {
            let g = seek_gen.load(Ordering::Acquire);
            if g != cur_gen {
                cur_gen = g;
                cur_sample = (seek_target.load(Ordering::Acquire) as usize).min(total_samples);
                let _ = file.seek(SeekFrom::Start(44 + cur_sample as u64 * 2));
            }
            if cur_sample >= total_samples {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            let want_samples = (total_samples - cur_sample).min(STREAM_CHUNK_SAMPLES);
            let want_bytes = want_samples * 2;
            let mut got = 0usize;
            while got < want_bytes {
                match file.read(&mut raw[got..want_bytes]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(_) => break,
                }
            }
            let n_samples = got / 2;
            if n_samples == 0 {
                cur_sample = total_samples;
                continue;
            }
            let samples: Vec<i16> = raw[..n_samples * 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_le_bytes(*c))
                .collect();
            let mut chunk = AudioChunk { gen: cur_gen, start: cur_sample, samples };
            cur_sample += n_samples;
            loop {
                if stop.load(Ordering::Acquire) || seek_gen.load(Ordering::Acquire) != cur_gen {
                    break;
                }
                match tx.try_send(chunk) {
                    Ok(()) => break,
                    Err(std::sync::mpsc::TrySendError::Full(c)) => {
                        chunk = c;
                        std::thread::sleep(Duration::from_millis(12));
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return,
                }
            }
        }
    });
    (rx, handle)
}

/// Pulls interpolated samples from streamed disk chunks inside the real-time audio callback without blocking.
struct StreamCursor {
    rx: Receiver<AudioChunk>,
    cur: Option<AudioChunk>,
    next: Option<AudioChunk>,
    gen: u64,
    pos: f64,
    total_samples: usize,
}

impl StreamCursor {
    fn new(rx: Receiver<AudioChunk>, total_samples: usize) -> Self {
        Self { rx, cur: None, next: None, gen: 0, pos: 0.0, total_samples }
    }

    fn sync_seek(&mut self, seek_gen: &AtomicU64, seek_target: &AtomicU64) {
        let g = seek_gen.load(Ordering::Acquire);
        if g != self.gen {
            self.gen = g;
            self.pos = (seek_target.load(Ordering::Acquire) as usize).min(self.total_samples) as f64;
            self.cur = None;
            self.next = None;
            while let Ok(c) = self.rx.try_recv() {
                if c.gen == self.gen {
                    self.cur = Some(c);
                    break;
                }
            }
        }
    }

    fn pump(&mut self) {
        while self.cur.is_none() || self.next.is_none() {
            match self.rx.try_recv() {
                Ok(c) if c.gen == self.gen => {
                    if self.cur.is_none() {
                        self.cur = Some(c);
                    } else {
                        self.next = Some(c);
                    }
                }
                Ok(_) => {} // discard chunks from an older seek generation
                Err(_) => break,
            }
        }
    }

    fn sample_at(&mut self, idx: usize) -> Option<i16> {
        loop {
            self.pump();
            if let Some(c) = &self.cur {
                let end = c.start + c.samples.len();
                if idx >= c.start && idx < end {
                    return Some(c.samples[idx - c.start]);
                }
                if idx >= end {
                    self.cur = self.next.take();
                    continue;
                }
            }
            return None;
        }
    }

    fn next_sample(&mut self, step: f64) -> Option<f32> {
        let i = self.pos as usize;
        if i + 1 >= self.total_samples {
            return None;
        }
        let s0 = self.sample_at(i)? as f32;
        let s1 = self.sample_at(i + 1).unwrap_or(s0 as i16) as f32;
        let frac = (self.pos - i as f64) as f32;
        self.pos += step;
        Some((s0 * (1.0 - frac) + s1 * frac) / 32768.0)
    }
}

/// Plays one recording streamed from disk; position, pause, and generation-tagged seek are lock-free.
pub struct Player {
    stream: Option<cpal::Stream>,
    reader: Option<std::thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pos: Arc<AtomicU64>,         // current position in source samples (f64 bits)
    seek_gen: Arc<AtomicU64>,    // incremented on each seek so callbacks never clobber a seek
    seek_target: Arc<AtomicU64>, // target sample index for the latest seek_gen
    playing: Arc<AtomicBool>,
    speed: Arc<AtomicU64>, // playback speed (f64 bits)
    rate: u32,
    total_samples: usize,
    pub len: f32,
    pub id: u64,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        drop(self.stream.take());
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn output_stream<T: SizedSample + FromSample<f32>>(
    dev: &cpal::Device,
    cfg: cpal::StreamConfig,
    mut cursor: StreamCursor,
    src_rate: u32,
    pos: Arc<AtomicU64>,
    seek_gen: Arc<AtomicU64>,
    seek_target: Arc<AtomicU64>,
    playing: Arc<AtomicBool>,
    speed: Arc<AtomicU64>,
) -> Result<cpal::Stream, String> {
    let ch = cfg.channels as usize;
    let base = src_rate as f64 / cfg.sample_rate as f64;
    dev.build_output_stream(
        cfg,
        move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
            cursor.sync_seek(&seek_gen, &seek_target);
            let step = base * f64::from_bits(speed.load(Ordering::Relaxed)).clamp(0.5, 3.0);
            let on = playing.load(Ordering::Relaxed);
            for frame in out.chunks_mut(ch.max(1)) {
                let v = if on { cursor.next_sample(step).unwrap_or(0.0) } else { 0.0 };
                for x in frame.iter_mut() {
                    *x = T::from_sample(v);
                }
            }
            if on && (cursor.pos as usize) + 1 >= cursor.total_samples {
                playing.store(false, Ordering::Relaxed);
            }
            // Only publish our position if no newer seek arrived concurrently.
            if seek_gen.load(Ordering::Acquire) == cursor.gen {
                pos.store(cursor.pos.to_bits(), Ordering::Relaxed);
            }
        },
        |e| eprintln!("speaker error: {e}"),
        None,
    )
    .map_err(|e| e.to_string())
}

impl Player {
    pub fn open(path: &Path, id: u64) -> Result<Player, String> {
        let (total_samples, rate) = inspect_wav(path)?;
        let len = total_samples as f32 / rate as f32;
        let dev = cpal::default_host().default_output_device().ok_or("No speaker found.")?;
        let sup = dev.default_output_config().map_err(|e| e.to_string())?;
        let pos = Arc::new(AtomicU64::new(0f64.to_bits()));
        let seek_gen = Arc::new(AtomicU64::new(0));
        let seek_target = Arc::new(AtomicU64::new(0));
        let playing = Arc::new(AtomicBool::new(false));
        let speed = Arc::new(AtomicU64::new(1f64.to_bits()));
        let stop = Arc::new(AtomicBool::new(false));
        let (rx, reader) = spawn_wav_reader(
            path.to_path_buf(),
            total_samples,
            seek_gen.clone(),
            seek_target.clone(),
            stop.clone(),
        );
        let cursor = StreamCursor::new(rx, total_samples);
        let cfg: cpal::StreamConfig = sup.into();
        let stream = match sup.sample_format() {
            SampleFormat::F32 => output_stream::<f32>(&dev, cfg, cursor, rate, pos.clone(), seek_gen.clone(), seek_target.clone(), playing.clone(), speed.clone()),
            SampleFormat::I16 => output_stream::<i16>(&dev, cfg, cursor, rate, pos.clone(), seek_gen.clone(), seek_target.clone(), playing.clone(), speed.clone()),
            SampleFormat::U16 => output_stream::<u16>(&dev, cfg, cursor, rate, pos.clone(), seek_gen.clone(), seek_target.clone(), playing.clone(), speed.clone()),
            SampleFormat::I24 => output_stream::<cpal::I24>(&dev, cfg, cursor, rate, pos.clone(), seek_gen.clone(), seek_target.clone(), playing.clone(), speed.clone()),
            SampleFormat::I32 => output_stream::<i32>(&dev, cfg, cursor, rate, pos.clone(), seek_gen.clone(), seek_target.clone(), playing.clone(), speed.clone()),
            f => Err(tf!("Unsupported speaker format {f}", f = format!("{:?}", f))),
        };
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                stop.store(true, Ordering::Release);
                let _ = reader.join();
                return Err(e);
            }
        };
        if let Err(e) = stream.play() {
            stop.store(true, Ordering::Release);
            drop(stream);
            let _ = reader.join();
            return Err(e.to_string());
        }
        Ok(Player {
            stream: Some(stream),
            reader: Some(reader),
            stop,
            pos,
            seek_gen,
            seek_target,
            playing,
            speed,
            rate,
            total_samples,
            len,
            id,
        })
    }

    pub fn time(&self) -> f32 {
        (f64::from_bits(self.pos.load(Ordering::Relaxed)) / self.rate as f64) as f32
    }

    pub fn seek(&self, secs: f32) {
        let clamped = secs.clamp(0.0, self.len) as f64;
        let sample = ((clamped * self.rate as f64) as usize).min(self.total_samples);
        self.seek_target.store(sample as u64, Ordering::Release);
        self.seek_gen.fetch_add(1, Ordering::AcqRel);
        self.pos.store((sample as f64).to_bits(), Ordering::Relaxed);
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn set_speed(&self, x: f32) {
        self.speed.store((x as f64).to_bits(), Ordering::Relaxed);
    }

    pub fn set_playing(&self, on: bool) {
        if on && self.time() >= self.len - 0.05 {
            self.seek(0.0);
        }
        self.playing.store(on, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finishes_a_recording_cut_off_by_a_crash() {
        let p = std::env::temp_dir().join(format!("ln-rec-{}.wav", crate::model::new_id()));
        let mut bytes = wav_header_rate(0, RATE);
        bytes.resize(44 + 2 * RATE as usize * 3, 0);
        std::fs::write(part_path(&p), bytes).unwrap();
        let secs = finish_unfinished(&p).unwrap();
        assert!((secs - 3.0).abs() < 0.01 && p.exists() && !part_path(&p).exists());
        assert_eq!(std::fs::read(&p).unwrap()[..44], wav_header_rate(RATE as u64 * 3, RATE)[..]);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn streams_wav_and_seeks_accurately() {
        let p = std::env::temp_dir().join(format!("ln-stream-{}.wav", crate::model::new_id()));
        let samples: Vec<i16> = (0..25_000i32).map(|i| (i % 30_000) as i16).collect();
        std::fs::write(&p, wav_bytes(&samples, RATE)).unwrap();
        let (total, rate) = inspect_wav(&p).unwrap();
        assert_eq!((total, rate), (25_000, RATE));
        let seek_gen = Arc::new(AtomicU64::new(0));
        let seek_target = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (rx, handle) = spawn_wav_reader(p.clone(), total, seek_gen.clone(), seek_target.clone(), stop.clone());
        let mut cur = StreamCursor::new(rx, total);
        // Wait briefly for initial chunk and verify sample 0
        std::thread::sleep(Duration::from_millis(30));
        cur.sync_seek(&seek_gen, &seek_target);
        assert_eq!(cur.sample_at(0), Some(0));
        assert_eq!(cur.sample_at(100), Some(100));
        // Seek across chunk boundary to sample 18_000
        seek_target.store(18_000, Ordering::Release);
        seek_gen.fetch_add(1, Ordering::AcqRel);
        std::thread::sleep(Duration::from_millis(40));
        cur.sync_seek(&seek_gen, &seek_target);
        assert_eq!(cur.sample_at(18_000), Some(18_000));
        stop.store(true, Ordering::Release);
        let _ = handle.join();
        let _ = std::fs::remove_file(p);
    }

    #[test]
    #[ignore]
    fn plays_silence() {
        let p = std::env::temp_dir().join("leafnotes-silence.wav");
        let mut bytes = wav_header_rate(RATE as u64, RATE);
        bytes.resize(44 + 2 * RATE as usize, 0);
        std::fs::write(&p, bytes).unwrap();
        let pl = Player::open(&p, 1).expect("speaker opens");
        pl.set_playing(true);
        std::thread::sleep(Duration::from_millis(400));
        assert!(pl.time() > 0.1, "playback advances: {}", pl.time());
        println!("played {:.2}s of {:.2}s", pl.time(), pl.len);
    }

    #[test]
    fn a_slow_microphone_is_not_padded_with_silence() {
        let mut dm = Downmix { channels: 1, step: 8_000.0 / RATE as f64, phase: 0.0, sum: 0.0, count: 0 };
        let mut out = vec![];
        dm.feed(&vec![0.5f32; 8_000], &mut out);
        assert!((out.len() as i64 - RATE as i64).abs() <= 1, "{}", out.len());
        assert!(out.iter().all(|s| *s > 16_000), "no gaps of silence");
    }

    #[test]
    fn downmixes_to_16k_and_reads_back() {
        let mut dm = Downmix { channels: 2, step: 48_000.0 / RATE as f64, phase: 0.0, sum: 0.0, count: 0 };
        let input: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let v = (i as f32 * 0.01).sin() * 0.5;
                [v, v]
            })
            .collect();
        let mut out = vec![];
        dm.feed(&input, &mut out);
        assert!((out.len() as i64 - RATE as i64).abs() <= 1, "{}", out.len());
        let dir = std::env::temp_dir().join(format!("leafnotes-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        std::fs::write(&p, wav_bytes(&out, RATE)).unwrap();
        assert_eq!(load_wav(&p).unwrap(), out);
        let _ = std::fs::remove_dir_all(dir);
    }
}
