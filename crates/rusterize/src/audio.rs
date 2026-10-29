//! Optional MP3/WAV playback. See docs/MEDIA.md for clock and latency contracts.
use crate::Error;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
#[path = "audio/native.rs"]
mod backend;
#[cfg(target_arch = "wasm32")]
#[path = "audio/browser.rs"]
mod backend;

pub const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PCM_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Clip(Arc<backend::ClipData>);
impl PartialEq for Clip {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Clip {}
impl Clip {
    pub fn sample_rate(&self) -> u32 {
        self.0.sample_rate
    }
    pub fn channels(&self) -> u32 {
        self.0.channels
    }
    pub fn frames(&self) -> usize {
        self.0.frames
    }
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.sample_rate() as f64
    }
    /// Cached mono peak envelope, computed once while loading.
    pub fn waveform(&self) -> &[f32] {
        &self.0.peaks
    }
    /// Interleaved PCM. The bounded copy is also available in browsers; samples
    /// beyond the end are zero. No decoder runs during this call.
    pub fn read_pcm(&self, start_frame: usize, output: &mut [f32]) -> Result<(), Error> {
        if output.len() > 32768 {
            return Err(Error("PCM reads are limited to 32768 samples".into()));
        }
        self.0.read_pcm(start_frame, output)
    }
    /// 32 logarithmic Goertzel bands over a 1024-frame Hann window. This is a
    /// small UI-side analysis, never part of the audio callback.
    pub fn spectrum(&self, position: f64) -> Result<[f32; 32], Error> {
        if !position.is_finite() || position < 0.0 {
            return Err(Error("invalid spectrum position".into()));
        }
        let channels = self.channels() as usize;
        let mut samples = vec![0.0; 1024 * channels];
        self.read_pcm(
            (position * self.sample_rate() as f64) as usize,
            &mut samples,
        )?;
        let mut mono = [0.0; 1024];
        for (i, frame) in samples.chunks_exact(channels).enumerate() {
            mono[i] = frame.iter().sum::<f32>() / channels as f32
                * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / 1023.0).cos());
        }
        let mut bands = [0.0; 32];
        let high = 16000.0_f32.min(self.sample_rate() as f32 * 0.45);
        for (i, band) in bands.iter_mut().enumerate() {
            let frequency = 40.0 * (high / 40.0).powf(i as f32 / 31.0);
            let coefficient =
                2.0 * (std::f32::consts::TAU * frequency / self.sample_rate() as f32).cos();
            let (mut a, mut b) = (0.0, 0.0);
            for sample in mono {
                let next = sample + coefficient * a - b;
                b = a;
                a = next;
            }
            *band = ((a * a + b * b - coefficient * a * b).max(0.0).sqrt() / 256.0).min(1.0);
        }
        Ok(bands)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn decode(bytes: Vec<u8>) -> Result<Self, Error> {
        backend::decode(bytes).map(|d| Self(Arc::new(d)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
    Ended,
    Interrupted,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackSnapshot {
    pub state: PlaybackState,
    pub position: f64,
    pub duration: f64,
    /// Host's estimated output queue latency in seconds, not acoustic calibration.
    pub output_latency: f64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioEvent {
    Ended,
    Interrupted,
    Error(Error),
}

/// One output voice; keep a music player and several reusable effect players.
/// Dropping a player releases its stream/nodes. Cloned clips reuse decoded PCM.
pub struct Player {
    backend: backend::Player,
    clip: Clip,
    reported: Option<PlaybackState>,
}
impl Player {
    pub fn new(clip: Clip) -> Result<Self, Error> {
        Ok(Self {
            backend: backend::Player::new(clip.0.clone())?,
            clip,
            reported: None,
        })
    }
    pub fn clip(&self) -> &Clip {
        &self.clip
    }
    /// Browsers require a trusted click/key event. Retry after an unlock error.
    pub fn play(&mut self) -> Result<(), Error> {
        self.reported = None;
        self.backend.play()
    }
    pub fn pause(&mut self) -> Result<(), Error> {
        self.backend.pause()
    }
    pub fn stop(&mut self) -> Result<(), Error> {
        self.reported = None;
        self.backend.stop()
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), Error> {
        if !seconds.is_finite() || seconds < 0.0 || seconds > self.clip.duration() {
            return Err(Error("seek is outside the audio duration".into()));
        }
        self.reported = None;
        self.backend.seek(seconds)
    }
    pub fn set_volume(&mut self, volume: f32) -> Result<(), Error> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(Error("volume must be between 0 and 1".into()));
        }
        self.backend.set_volume(volume)
    }
    pub fn snapshot(&self) -> Result<PlaybackSnapshot, Error> {
        self.backend.snapshot()
    }
    /// Drain from `Event::Frame`; events are delivered once per transition.
    pub fn poll_event(&mut self) -> Option<AudioEvent> {
        match self.snapshot() {
            Ok(s) => {
                if self.reported == Some(s.state) {
                    return None;
                }
                self.reported = Some(s.state);
                match s.state {
                    PlaybackState::Ended => Some(AudioEvent::Ended),
                    PlaybackState::Interrupted => Some(AudioEvent::Interrupted),
                    PlaybackState::Failed => Some(AudioEvent::Error(Error(
                        "audio output device failed; recreate the player".into(),
                    ))),
                    _ => None,
                }
            }
            Err(e) if self.reported != Some(PlaybackState::Failed) => {
                self.reported = Some(PlaybackState::Failed);
                Some(AudioEvent::Error(e))
            }
            _ => None,
        }
    }
}
pub(crate) fn suspend_all() {
    backend::suspend_all();
}
pub(crate) fn validate_source(bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES {
        return Err(Error("audio source must contain 1 byte to 32 MiB".into()));
    }
    let wav = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE");
    if wav {
        let declared = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as u64 + 8;
        if declared > bytes.len() as u64 || declared < 44 {
            return Err(Error("truncated or invalid WAV container".into()));
        }
    }
    let mp3 = bytes.starts_with(b"ID3")
        || bytes
            .windows(2)
            .take(4096)
            .any(|b| b[0] == 0xff && b[1] & 0xe0 == 0xe0);
    if !wav && !mp3 {
        return Err(Error("expected MP3 or WAV data".into()));
    }
    Ok(())
}
pub(crate) fn decode_browser_response(bytes: &[u8]) -> Result<Clip, Error> {
    #[cfg(target_arch = "wasm32")]
    {
        backend::decode_response(bytes).map(|d| Clip(Arc::new(d)))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = bytes;
        Err(Error("browser audio response on a native host".into()))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn wav_float_pcm_bounds_and_nonfinite_data() {
        let mut wav = b"RIFF\x2c\0\0\0WAVEfmt \x10\0\0\0\x03\0\x01\0\x40\x1f\0\0\0\x7d\0\0\x04\0\x20\0data\x08\0\0\0".to_vec();
        wav.extend_from_slice(&0.25f32.to_le_bytes());
        wav.extend_from_slice(&(-0.5f32).to_le_bytes());
        let clip = Clip::decode(wav.clone()).unwrap();
        assert_eq!(clip.frames(), 2);
        assert_eq!(clip.sample_rate(), 8000);
        let mut pcm = [0.0; 4];
        clip.read_pcm(0, &mut pcm).unwrap();
        assert_eq!(pcm, [0.25, -0.5, 0.0, 0.0]);
        assert!(clip.read_pcm(0, &mut vec![0.0; 32769]).is_err());
        assert!(Clip::decode(wav[..wav.len() - 1].to_vec()).is_err());
        wav[44..48].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Clip::decode(wav).is_err());
        assert!(validate_source(&vec![0; MAX_SOURCE_BYTES + 1]).is_err());
    }
    #[test]
    fn mp3_duration_pcm_analysis_and_corruption() {
        let clip = Clip::decode(include_bytes!("../../../examples/media/assets/demo.mp3").to_vec())
            .unwrap();
        assert!((clip.duration() - 12.0).abs() < 0.1);
        assert_eq!(clip.channels(), 1);
        assert_eq!(clip.waveform().len(), 512);
        let mut pcm = [0.0; 1024];
        clip.read_pcm(22050, &mut pcm).unwrap();
        assert!(pcm.iter().any(|s| s.abs() > 0.02));
        clip.read_pcm(usize::MAX, &mut pcm).unwrap();
        assert!(pcm.iter().all(|s| *s == 0.0));
        assert!(clip.spectrum(1.0).unwrap().iter().any(|s| *s > 0.01));
        assert!(clip.spectrum(f64::NAN).is_err());
        assert!(Clip::decode(b"RIFF\0\0\0\0WAVEbad".to_vec()).is_err());
        assert!(Clip::decode(b"ID3bad".to_vec()).is_err());
    }
    #[test]
    #[ignore = "requires a real output device; plays a short quiet test tone"]
    fn output_clock_pause_seek_effect_and_completion() {
        use std::{
            thread::sleep,
            time::{Duration, Instant},
        };
        let clip = Clip::decode(include_bytes!("../../../examples/media/assets/demo.mp3").to_vec())
            .unwrap();
        let mut music = Player::new(clip.clone()).unwrap();
        let mut effect = Player::new(clip).unwrap();
        music.set_volume(0.05).unwrap();
        effect.set_volume(0.02).unwrap();
        assert!(music.seek(f64::NAN).is_err());
        assert!(music.set_volume(-1.0).is_err());
        assert!(music.seek(13.0).is_err());
        music.play().unwrap();
        effect.seek(11.9).unwrap();
        effect.play().unwrap();
        sleep(Duration::from_millis(200));
        assert!(music.snapshot().unwrap().position > 0.03);
        music.pause().unwrap();
        sleep(Duration::from_millis(150));
        let paused = music.snapshot().unwrap();
        assert_eq!(paused.state, PlaybackState::Paused);
        sleep(Duration::from_millis(80));
        assert!((music.snapshot().unwrap().position - paused.position).abs() < 0.003);
        music.seek(11.8).unwrap();
        sleep(Duration::from_millis(150));
        assert!((music.snapshot().unwrap().position - 11.8).abs() < 0.02);
        music.play().unwrap();
        let deadline = Instant::now();
        let mut ended = false;
        while deadline.elapsed() < Duration::from_secs(2) {
            if music.poll_event() == Some(AudioEvent::Ended) {
                ended = true;
                break;
            }
            sleep(Duration::from_millis(10));
        }
        assert!(ended, "{:?}", music.snapshot());
        assert_eq!(music.poll_event(), None);
        music.stop().unwrap();
        sleep(Duration::from_millis(150));
        assert_eq!(music.snapshot().unwrap().state, PlaybackState::Stopped);
        assert!(music.snapshot().unwrap().position < 0.001);
        music.play().unwrap();
        sleep(Duration::from_millis(80));
        suspend_all();
        sleep(Duration::from_millis(100));
        assert_eq!(music.snapshot().unwrap().state, PlaybackState::Interrupted);
    }
}
