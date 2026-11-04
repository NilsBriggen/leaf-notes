//! Record lectures alongside notes and play them back (16 kHz mono WAV files).
//! The microphone is read on cpal's audio thread, turned into 16 kHz mono, and written to disk on a
//! writer thread as it arrives, so a long recording doesn't fill memory.

use crate::tf;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

pub const RATE: u32 = 16_000;

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
pub(crate) use android::{set_activity, with_activity};

#[cfg(target_os = "android")]
mod android {
    use jni::{objects::JObject, strings::JNIString, sys::jobject, Env, JavaVM};
    const MIC: &str = "android.permission.RECORD_AUDIO";

    /// Run Java calls against the activity. A Java exception is printed to the system log and cleared
    /// (a pending one would break every later call); the error comes back as text.
    /// The running Activity (from android-activity). ndk_context only holds the Application, which
    /// can check permissions but not ask for them, and has no window.
    static ACTIVITY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    pub(crate) fn set_activity(ptr: *mut std::ffi::c_void) { ACTIVITY.store(ptr as usize, std::sync::atomic::Ordering::Relaxed); }

    pub(crate) fn with_activity<R>(f: impl for<'j> FnOnce(&mut Env<'j>, JObject<'j>) -> jni::errors::Result<R>) -> Result<R, String> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
        let act = ACTIVITY.load(std::sync::atomic::Ordering::Relaxed);
        let raw = if act != 0 { act as jobject } else { ctx.context() as jobject };
        vm.attach_current_thread(|env: &mut Env<'_>| {
            let activity = unsafe { JObject::from_raw(env, raw) };
            let r = f(env, activity);
            if r.is_err() && env.exception_check() { let _ = env.exception_describe(); let _ = env.exception_clear(); }
            r
        }).map_err(|e| e.to_string())
    }

    pub fn granted() -> bool {
        with_activity(|env, act| {
            let p = env.new_string(MIC)?;
            env.call_method(&act, JNIString::new("checkSelfPermission"), jni::jni_sig!("(Ljava/lang/String;)I"), &[(&p).into()])?.i()
        }) == Ok(0) // PackageManager.PERMISSION_GRANTED
    }

    pub fn request() -> Result<(), String> {
        with_activity(|env, act| {
            let p = env.new_string(MIC)?;
            let list = env.new_object_array(1, jni::jni_str!("java/lang/String"), &p)?;
            env.call_method(&act, JNIString::new("requestPermissions"), jni::jni_sig!("([Ljava/lang/String;I)V"), &[(&list).into(), 7i32.into()])?;
            Ok(())
        })
    }
}

pub struct Recorder {
    stream: Option<cpal::Stream>,
    writer: Option<std::thread::JoinHandle<Result<u64, String>>>,
    pub path: PathBuf,
}

/// A whole 16-bit mono WAV file in memory.
pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let mut b = wav_header(samples.len() as u64);
    for s in samples { b.extend_from_slice(&s.to_le_bytes()); }
    b
}

/// 44-byte WAV header for 16-bit mono at RATE.
fn wav_header(samples: u64) -> Vec<u8> {
    let data = (samples * 2) as u32;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&1u16.to_le_bytes()); // mono
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&(RATE * 2).to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    h
}

/// Turns device audio (any rate, any channel count) into 16 kHz mono by averaging.
struct Downmix { channels: usize, step: f64, phase: f64, sum: f32, count: u32 }

impl Downmix {
    fn push(&mut self, frame_avg: f32, out: &mut Vec<i16>) {
        self.sum += frame_avg;
        self.count += 1;
        self.phase += 1.0;
        while self.phase >= self.step {
            out.push(((self.sum / self.count.max(1) as f32).clamp(-1.0, 1.0) * 32767.0) as i16);
            self.sum = 0.0;
            self.count = 0;
            self.phase -= self.step;
        }
    }
    fn feed<T: SizedSample>(&mut self, data: &[T], out: &mut Vec<i16>) where f32: FromSample<T> {
        for frame in data.chunks(self.channels.max(1)) {
            let avg = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / frame.len() as f32;
            self.push(avg, out);
        }
    }
}

fn input_stream<T: SizedSample>(dev: &cpal::Device, cfg: cpal::StreamConfig, tx: Sender<Vec<i16>>) -> Result<cpal::Stream, String> where f32: FromSample<T> {
    let mut dm = Downmix { channels: cfg.channels as usize, step: cfg.sample_rate as f64 / RATE as f64, phase: 0.0, sum: 0.0, count: 0 };
    dev.build_input_stream(cfg, move |data: &[T], _: &cpal::InputCallbackInfo| {
        let mut out = Vec::with_capacity(data.len() / 2);
        dm.feed(data, &mut out);
        let _ = tx.send(out);
    }, |e| eprintln!("microphone error: {e}"), None).map_err(|e| e.to_string())
}

impl Recorder {
    /// Start recording the default microphone into `path`.
    pub fn start(path: &Path) -> Result<Recorder, String> {
        let host = cpal::default_host();
        let dev = host.default_input_device().ok_or("No microphone found.")?;
        let sup = dev.default_input_config().map_err(|e| tf!("The microphone can't be opened ({e}). Check that no other app is using it, and that microphone access is allowed in your system's privacy settings.", e = e))?;
        let (tx, rx) = channel::<Vec<i16>>();
        let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        file.write_all(&wav_header(0)).map_err(|e| e.to_string())?;
        // the writer keeps going until the stream (and with it the sender) is dropped
        let writer = std::thread::spawn(move || -> Result<u64, String> {
            let mut n = 0u64;
            let mut buf = std::io::BufWriter::new(file);
            for chunk in rx {
                for s in &chunk { buf.write_all(&s.to_le_bytes()).map_err(|e| e.to_string())?; }
                n += chunk.len() as u64;
            }
            let mut file = buf.into_inner().map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            file.write_all(&wav_header(n)).map_err(|e| e.to_string())?;
            Ok(n)
        });
        let cfg: cpal::StreamConfig = sup.clone().into();
        let stream = match sup.sample_format() {
            SampleFormat::F32 => input_stream::<f32>(&dev, cfg, tx),
            SampleFormat::I16 => input_stream::<i16>(&dev, cfg, tx),
            SampleFormat::U16 => input_stream::<u16>(&dev, cfg, tx),
            SampleFormat::I32 => input_stream::<i32>(&dev, cfg, tx),
            f => Err(tf!("Unsupported microphone format {f}", f = format!("{:?}", f))),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Recorder { stream: Some(stream), writer: Some(writer), path: path.to_path_buf() })
    }

    /// Stop and finish the file; returns its length in seconds.
    pub fn stop(mut self) -> Result<f32, String> {
        drop(self.stream.take());
        let n = self.writer.take().ok_or("already stopped")?.join().map_err(|_| "the recording writer crashed".to_string())??;
        Ok(n as f32 / RATE as f32)
    }
}

/// Read one of our WAV files back as samples.
pub fn load_wav(path: &Path) -> Result<Vec<i16>, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    if b.len() < 44 || &b[..4] != b"RIFF" { return Err(crate::tr("Not a recording file").into()); }
    Ok(b[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
}

/// Plays one recording; position and pause are shared with the audio thread.
pub struct Player {
    _stream: cpal::Stream,
    pos: Arc<AtomicU64>, // position in 16 kHz samples, as f64 bits
    playing: Arc<AtomicBool>,
    pub len: f32,
    pub id: u64,
}

fn output_stream<T: SizedSample + FromSample<f32>>(dev: &cpal::Device, cfg: cpal::StreamConfig, data: Arc<Vec<i16>>, pos: Arc<AtomicU64>, playing: Arc<AtomicBool>) -> Result<cpal::Stream, String> {
    let ch = cfg.channels as usize;
    let step = RATE as f64 / cfg.sample_rate as f64;
    dev.build_output_stream(cfg, move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        let mut p = f64::from_bits(pos.load(Ordering::Relaxed));
        let on = playing.load(Ordering::Relaxed);
        for frame in out.chunks_mut(ch.max(1)) {
            let v = if on && (p as usize) + 1 < data.len() {
                let i = p as usize;
                let f = (p - i as f64) as f32;
                let s = data[i] as f32 * (1.0 - f) + data[i + 1] as f32 * f;
                p += step;
                s / 32768.0
            } else { 0.0 };
            for x in frame.iter_mut() { *x = T::from_sample(v); }
        }
        if on && p as usize + 1 >= data.len() { playing.store(false, Ordering::Relaxed); }
        pos.store(p.to_bits(), Ordering::Relaxed);
    }, |e| eprintln!("speaker error: {e}"), None).map_err(|e| e.to_string())
}

impl Player {
    pub fn open(path: &Path, id: u64) -> Result<Player, String> {
        let data = Arc::new(load_wav(path)?);
        let len = data.len() as f32 / RATE as f32;
        let dev = cpal::default_host().default_output_device().ok_or("No speaker found.")?;
        let sup = dev.default_output_config().map_err(|e| e.to_string())?;
        let (pos, playing) = (Arc::new(AtomicU64::new(0f64.to_bits())), Arc::new(AtomicBool::new(false)));
        let cfg: cpal::StreamConfig = sup.clone().into();
        let stream = match sup.sample_format() {
            SampleFormat::F32 => output_stream::<f32>(&dev, cfg, data, pos.clone(), playing.clone()),
            SampleFormat::I16 => output_stream::<i16>(&dev, cfg, data, pos.clone(), playing.clone()),
            SampleFormat::U16 => output_stream::<u16>(&dev, cfg, data, pos.clone(), playing.clone()),
            SampleFormat::I32 => output_stream::<i32>(&dev, cfg, data, pos.clone(), playing.clone()),
            f => Err(tf!("Unsupported speaker format {f}", f = format!("{:?}", f))),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Player { _stream: stream, pos, playing, len, id })
    }
    pub fn time(&self) -> f32 { (f64::from_bits(self.pos.load(Ordering::Relaxed)) / RATE as f64) as f32 }
    pub fn seek(&self, secs: f32) { self.pos.store(((secs.clamp(0.0, self.len) as f64) * RATE as f64).to_bits(), Ordering::Relaxed); }
    pub fn playing(&self) -> bool { self.playing.load(Ordering::Relaxed) }
    pub fn set_playing(&self, on: bool) {
        if on && self.time() >= self.len - 0.05 { self.seek(0.0); }
        self.playing.store(on, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Opens the speaker with one second of silence (not run by default: needs a sound device).
    #[test]
    #[ignore]
    fn plays_silence() {
        let p = std::env::temp_dir().join("leafnotes-silence.wav");
        let mut bytes = wav_header(RATE as u64);
        bytes.resize(44 + 2 * RATE as usize, 0);
        std::fs::write(&p, bytes).unwrap();
        let pl = Player::open(&p, 1).expect("speaker opens");
        pl.set_playing(true);
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(pl.time() > 0.1, "playback advances: {}", pl.time());
        println!("played {:.2}s of {:.2}s", pl.time(), pl.len);
    }
    #[test]
    fn downmixes_to_16k_and_reads_back() {
        // one second of 48 kHz stereo becomes one second of 16 kHz mono
        let mut dm = Downmix { channels: 2, step: 48_000.0 / RATE as f64, phase: 0.0, sum: 0.0, count: 0 };
        let input: Vec<f32> = (0..48_000).flat_map(|i| { let v = (i as f32 * 0.01).sin() * 0.5; [v, v] }).collect();
        let mut out = vec![];
        dm.feed(&input, &mut out);
        assert!((out.len() as i64 - RATE as i64).abs() <= 1, "{}", out.len());
        let dir = std::env::temp_dir().join(format!("leafnotes-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.wav");
        let mut bytes = wav_header(out.len() as u64);
        for s in &out { bytes.extend_from_slice(&s.to_le_bytes()); }
        std::fs::write(&p, bytes).unwrap();
        assert_eq!(load_wav(&p).unwrap(), out);
        let _ = std::fs::remove_dir_all(dir);
    }
}
