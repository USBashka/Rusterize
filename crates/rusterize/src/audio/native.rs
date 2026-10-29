use super::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(target_os = "android"))]
use std::time::Instant;
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*},
        Weak,
    },
};

#[derive(Debug)]
pub(super) struct ClipData {
    pub sample_rate: u32,
    pub channels: u32,
    pub frames: usize,
    pub peaks: Vec<f32>,
    pcm: Vec<f32>,
}
impl ClipData {
    pub fn read_pcm(&self, start: usize, output: &mut [f32]) -> Result<(), Error> {
        output.fill(0.0);
        let start = start.saturating_mul(self.channels as usize);
        if start < self.pcm.len() {
            let n = output.len().min(self.pcm.len() - start);
            output[..n].copy_from_slice(&self.pcm[start..start + n]);
        }
        Ok(())
    }
}
pub(super) fn decode(bytes: Vec<u8>) -> Result<ClipData, Error> {
    use symphonia::core::{
        audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
        meta::MetadataOptions, probe::Hint,
    };
    validate_source(&bytes)?;
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let mut format = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(err)?
        .format;
    let track = format
        .default_track()
        .ok_or_else(|| Error("audio has no default track".into()))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(err)?;
    let (mut sample_rate, mut channels) = (0, 0);
    let mut pcm = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(e) => return Err(err(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(err)?;
        let rate = decoded.spec().rate;
        let count = decoded.spec().channels.count() as u32;
        if !(1..=2).contains(&count) || !(8000..=192000).contains(&rate) {
            return Err(Error("audio requires mono/stereo, 8–192 kHz".into()));
        }
        if sample_rate != 0 && (sample_rate != rate || channels != count) {
            return Err(Error("audio format changed within the stream".into()));
        }
        sample_rate = rate;
        channels = count;
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        samples.copy_interleaved_ref(decoded);
        if pcm.len().saturating_add(samples.len()) > MAX_PCM_BYTES / 4 {
            return Err(Error("decoded audio exceeds 256 MiB".into()));
        }
        pcm.extend_from_slice(samples.samples());
    }
    if pcm.is_empty() || sample_rate == 0 {
        return Err(Error("audio contains no samples".into()));
    }
    if pcm.iter().any(|s| !s.is_finite()) {
        return Err(Error("audio contains non-finite PCM".into()));
    }
    let frames = pcm.len() / channels as usize;
    let mut peaks = vec![0.0_f32; 512];
    for (i, frame) in pcm.chunks_exact(channels as usize).enumerate() {
        let bin = (i * 512 / frames).min(511);
        peaks[bin] = peaks[bin].max(frame.iter().fold(0.0_f32, |a, s| a.max(s.abs())));
    }
    Ok(ClipData {
        sample_rate,
        channels,
        frames,
        pcm,
        peaks,
    })
}
fn err(e: impl std::fmt::Display) -> Error {
    Error(format!("audio: {e}"))
}

// Controls are atomics; the callback does no allocation, file I/O, locking or FFT.
struct Shared {
    mode: AtomicU64,
    volume: AtomicU32,
    seek: AtomicU64,
    revision: AtomicU64,
    segments: [Segment; 64],
    latency: AtomicU64,
    failed: AtomicBool,
    #[cfg(not(target_os = "android"))]
    clock: Instant,
}
impl Shared {
    fn set_mode(&self, mode: u64) {
        self.mode
            .fetch_update(AcqRel, Acquire, |old| {
                Some(((old & !7).wrapping_add(8)) | mode)
            })
            .ok();
    }
}
#[derive(Default)]
struct Segment {
    sequence: AtomicU64,
    position: AtomicU64,
    at: AtomicU64,
    end: AtomicU64,
    mode: AtomicU32,
}
thread_local! {
    // Clippy 1.93 reports the macro's fallback initializer on Android even with const.
    #[cfg_attr(target_os="android", allow(clippy::missing_const_for_thread_local))]
    static VOICES: RefCell<Vec<Weak<Shared>>> = const { RefCell::new(Vec::new()) };
}
pub(super) fn suspend_all() {
    VOICES.with(|voices| {
        voices.borrow_mut().retain(|voice| {
            if let Some(voice) = voice.upgrade() {
                voice
                    .mode
                    .fetch_update(AcqRel, Acquire, |old| {
                        (old & 7 == 1).then_some(((old & !7).wrapping_add(8)) | 4)
                    })
                    .ok();
                true
            } else {
                false
            }
        })
    });
}
pub(super) struct Player {
    _stream: cpal::Stream,
    state: Arc<Shared>,
    duration: f64,
}
impl Player {
    pub fn new(clip: Arc<ClipData>) -> Result<Self, Error> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| Error("no audio output device".into()))?;
        let config = device.default_output_config().map_err(err)?;
        let state = Arc::new(Shared {
            mode: AtomicU64::new(0),
            volume: AtomicU32::new(1f32.to_bits()),
            seek: AtomicU64::new(0),
            revision: AtomicU64::new(0),
            segments: std::array::from_fn(|_| Segment::default()),
            latency: AtomicU64::new(0),
            failed: AtomicBool::new(false),
            #[cfg(not(target_os = "android"))]
            clock: Instant::now(),
        });
        let duration = clip.frames as f64 / clip.sample_rate as f64;
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => stream::<f32>(&device, &config.into(), clip, state.clone())?,
            cpal::SampleFormat::I16 => stream::<i16>(&device, &config.into(), clip, state.clone())?,
            cpal::SampleFormat::U16 => stream::<u16>(&device, &config.into(), clip, state.clone())?,
            other => {
                return Err(Error(format!(
                    "unsupported audio device sample format: {other}"
                )))
            }
        };
        stream.play().map_err(err)?;
        VOICES.with(|voices| {
            let mut v = voices.borrow_mut();
            v.retain(|s| s.strong_count() > 0);
            v.push(Arc::downgrade(&state));
        });
        Ok(Self {
            _stream: stream,
            state,
            duration,
        })
    }
    pub fn play(&mut self) -> Result<(), Error> {
        if self.state.failed.load(Acquire) {
            return Err(Error(
                "audio device was disconnected; recreate player".into(),
            ));
        }
        #[cfg(target_os = "android")]
        crate::native::query(&62u32.to_le_bytes())?;
        if self.snapshot()?.state == PlaybackState::Ended {
            self.seek(0.0)?;
        }
        self.state.set_mode(1);
        Ok(())
    }
    pub fn pause(&mut self) -> Result<(), Error> {
        self.state.set_mode(2);
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), Error> {
        self.state.set_mode(0);
        self.seek(0.0)
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), Error> {
        self.state.seek.store(seconds.to_bits(), Relaxed);
        self.state.revision.fetch_add(1, Release);
        self.state
            .mode
            .fetch_update(AcqRel, Acquire, |old| {
                Some(((old & !7).wrapping_add(8)) | if old & 7 == 3 { 2 } else { old & 7 })
            })
            .ok();
        Ok(())
    }
    pub fn set_volume(&mut self, volume: f32) -> Result<(), Error> {
        self.state.volume.store(volume.to_bits(), Release);
        Ok(())
    }
    pub fn snapshot(&self) -> Result<PlaybackSnapshot, Error> {
        #[cfg(target_os = "android")]
        {
            // CPAL 0.16 AAudio mixes relative callback time and absolute playback
            // time. Read AAudio's hardware frame timestamp in its own clock domain.
            let cpal::platform::StreamInner::AAudio(cpal::platform::AAudioStream::Output(stream)) =
                self._stream.as_inner()
            else {
                return Err(Error("expected an AAudio output stream".into()));
            };
            let rate = f64::from(stream.sample_rate());
            let written = stream.frames_written().max(0) as f64;
            let consumed = match stream.timestamp(ndk::audio::Clockid::Monotonic) {
                Ok(timestamp) => {
                    let elapsed = android_monotonic_seconds().map_or(0.0, |now| {
                        (now - timestamp.time_nanoseconds as f64 / 1e9).max(0.0)
                    });
                    timestamp.frame_position as f64 + elapsed * rate
                }
                Err(_) => stream.frames_read() as f64,
            }
            .clamp(0.0, written);
            let mut snapshot = self.state.snapshot(consumed / rate, self.duration);
            snapshot.output_latency = (written - consumed) / rate;
            Ok(snapshot)
        }
        #[cfg(not(target_os = "android"))]
        Ok(self
            .state
            .snapshot(self.state.clock.elapsed().as_secs_f64(), self.duration))
    }
}
#[cfg(target_os = "android")]
fn android_monotonic_seconds() -> Option<f64> {
    #[repr(C)]
    struct Timespec {
        seconds: std::ffi::c_long,
        nanos: std::ffi::c_long,
    }
    extern "C" {
        fn clock_gettime(clock: i32, output: *mut Timespec) -> i32;
    }
    let mut time = Timespec {
        seconds: 0,
        nanos: 0,
    };
    // CLOCK_MONOTONIC is the same clock requested from AAudioStream_getTimestamp.
    (unsafe { clock_gettime(1, &mut time) } == 0)
        .then_some(time.seconds as f64 + time.nanos as f64 / 1e9)
}
impl Shared {
    fn snapshot(&self, now: f64, duration: f64) -> PlaybackSnapshot {
        let mut selected = (0, 0.0, 0.0, 0.0, 0);
        for segment in &self.segments {
            let sequence = segment.sequence.load(SeqCst);
            if sequence == 0 || sequence & 1 != 0 {
                continue;
            }
            let (position, at, end, mode) = (
                f64::from_bits(segment.position.load(SeqCst)),
                f64::from_bits(segment.at.load(SeqCst)),
                f64::from_bits(segment.end.load(SeqCst)),
                segment.mode.load(SeqCst),
            );
            if segment.sequence.load(SeqCst) == sequence && at <= now && sequence > selected.0 {
                selected = (sequence, position, at, end, mode);
            }
        }
        let (_, anchor, at, limit, mode) = selected;
        let position = if mode == 1 {
            (anchor + (now - at).max(0.0)).min(limit)
        } else {
            anchor
        }
        .clamp(0.0, duration);
        let ended = mode == 3 || (mode == 1 && position >= duration);
        let requested = self.mode.load(Acquire) & 7;
        let state = if self.failed.load(Acquire) {
            PlaybackState::Failed
        } else if requested == 1 && mode != 1 {
            PlaybackState::Playing
        } else if mode == 4 {
            PlaybackState::Interrupted
        } else if mode == 0 {
            PlaybackState::Stopped
        } else if mode == 2 {
            PlaybackState::Paused
        } else if ended {
            PlaybackState::Ended
        } else {
            PlaybackState::Playing
        };
        PlaybackSnapshot {
            state,
            position,
            duration,
            output_latency: f64::from_bits(self.latency.load(Acquire)),
        }
    }
}
fn stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    clip: Arc<ClipData>,
    state: Arc<Shared>,
) -> Result<cpal::Stream, Error>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let rate = config.sample_rate.0 as f64;
    let step = clip.sample_rate as f64 / rate;
    let failure = state.clone();
    let (mut position, mut revision, mut sequence) = (0.0_f64, 0_u64, 0_u64);
    #[cfg(target_os = "android")]
    let mut written_frames = 0_u64;
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _info| {
                let control = state.mode.load(Acquire);
                let next = state.revision.load(Acquire);
                if next != revision {
                    position = f64::from_bits(state.seek.load(Relaxed)) * clip.sample_rate as f64;
                    revision = next;
                }
                let mode = (control & 7) as u32;
                let playing = mode == 1;
                let gain = f32::from_bits(state.volume.load(Relaxed));
                let start = position / clip.sample_rate as f64;
                #[cfg(not(target_os = "android"))]
                let timestamp = _info.timestamp();
                #[cfg(not(target_os = "android"))]
                let latency = timestamp
                    .playback
                    .duration_since(&timestamp.callback)
                    .map_or(0.0, |d| d.as_secs_f64());
                #[cfg(not(target_os = "android"))]
                let audible = state.clock.elapsed().as_secs_f64() + latency;
                #[cfg(target_os = "android")]
                let (audible, latency) = {
                    let at = written_frames as f64 / rate;
                    written_frames += (output.len() / channels) as u64;
                    (at, 0.0_f64)
                };
                for frame in output.chunks_mut(channels) {
                    for (channel, out) in frame.iter_mut().enumerate() {
                        let sample = if playing && position < clip.frames as f64 {
                            let index = position as usize;
                            let fraction = (position - index as f64) as f32;
                            let sample = |frame: usize| {
                                let offset = frame * clip.channels as usize;
                                if channels == 1 && clip.channels == 2 {
                                    (clip.pcm[offset] + clip.pcm[offset + 1]) * 0.5
                                } else if channel < 2 {
                                    clip.pcm[offset + channel.min(clip.channels as usize - 1)]
                                } else {
                                    0.0
                                }
                            };
                            let a = sample(index);
                            let b = sample((index + 1).min(clip.frames - 1));
                            (a + (b - a) * fraction) * gain
                        } else {
                            0.0
                        };
                        *out = T::from_sample(sample.clamp(-1.0, 1.0));
                    }
                    if playing {
                        position = (position + step).min(clip.frames as f64);
                    }
                }
                sequence += 1;
                let segment = &state.segments[sequence as usize % state.segments.len()];
                segment.sequence.store(sequence * 2 + 1, SeqCst);
                segment.position.store(start.to_bits(), SeqCst);
                segment
                    .end
                    .store((position / clip.sample_rate as f64).to_bits(), SeqCst);
                segment.at.store(audible.to_bits(), SeqCst);
                segment.mode.store(mode, SeqCst);
                segment.sequence.store(sequence * 2 + 2, SeqCst);
                state.latency.store(latency.to_bits(), Release);
                // Do not let completion of an old buffer overwrite a newer seek/play.
                if playing && position >= clip.frames as f64 {
                    state
                        .mode
                        .compare_exchange(control, (control & !7) | 3, AcqRel, Acquire)
                        .ok();
                }
            },
            move |_| {
                failure.failed.store(true, Release);
            },
            None,
        )
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audible_timeline_delays_seek_and_pause_until_their_output_buffer() {
        let state = Shared {
            mode: AtomicU64::new(2),
            volume: AtomicU32::new(1f32.to_bits()),
            seek: AtomicU64::new(0),
            revision: AtomicU64::new(0),
            segments: std::array::from_fn(|_| Segment::default()),
            latency: AtomicU64::new(0.02f64.to_bits()),
            failed: AtomicBool::new(false),
            #[cfg(not(target_os = "android"))]
            clock: Instant::now(),
        };
        for (i, (position, at, end, mode)) in [
            (4.0_f64, 1.0_f64, 4.1_f64, 1),
            (8.0, 1.1, 8.1, 1),
            (8.1, 1.2, 8.1, 2),
        ]
        .into_iter()
        .enumerate()
        {
            let s = &state.segments[i];
            s.position.store(position.to_bits(), SeqCst);
            s.at.store(at.to_bits(), SeqCst);
            s.end.store(end.to_bits(), SeqCst);
            s.mode.store(mode, SeqCst);
            s.sequence.store((i as u64 + 1) * 2, SeqCst);
        }
        assert!((state.snapshot(1.05, 12.0).position - 4.05).abs() < 1e-9);
        assert!((state.snapshot(1.15, 12.0).position - 8.05).abs() < 1e-9);
        assert_eq!(state.snapshot(1.25, 12.0).state, PlaybackState::Paused);
        assert_eq!(state.snapshot(100.0, 12.0).position, 8.1);
        state.segments[2].sequence.store(7, SeqCst); // A buffer being published is ignored.
        assert_eq!(state.snapshot(100.0, 12.0).state, PlaybackState::Playing);
        assert_eq!(state.snapshot(100.0, 12.0).position, 8.1); // No extrapolation past PCM.
        let old = state.mode.load(Acquire);
        state.set_mode(1);
        assert!(state
            .mode
            .compare_exchange(old, 3, AcqRel, Acquire)
            .is_err());
        state.failed.store(true, Release);
        assert_eq!(state.snapshot(1.25, 12.0).state, PlaybackState::Failed);
    }
}
