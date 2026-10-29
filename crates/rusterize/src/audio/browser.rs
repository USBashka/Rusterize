use super::*;
use crate::{
    native::{query, Reader},
    protocol::Writer,
};

#[derive(Debug)]
pub(super) struct ClipData {
    id: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub frames: usize,
    pub peaks: Vec<f32>,
}
pub(super) fn decode_response(bytes: &[u8]) -> Result<ClipData, Error> {
    let mut r = Reader::new(bytes);
    let id = r.u32()?;
    let sample_rate = r.u32()?;
    let channels = r.u32()?;
    let frames = r.u32()?;
    if id == 0
        || !(8000..=192000).contains(&sample_rate)
        || !(1..=2).contains(&channels)
        || frames == 0
        || frames as u64 * channels as u64 * 4 > MAX_PCM_BYTES as u64
    {
        return Err(Error("invalid browser audio metadata".into()));
    }
    let mut peaks = Vec::with_capacity(512);
    for _ in 0..512 {
        peaks.push(r.f32()?);
    }
    if !r.done() {
        return Err(Error("trailing browser audio metadata".into()));
    }
    Ok(ClipData {
        id,
        sample_rate,
        channels,
        frames: frames as usize,
        peaks,
    })
}
fn command(op: u32, id: u32) -> Writer {
    let mut w = Writer(Vec::new());
    w.u32(op);
    w.u32(id);
    w
}
impl ClipData {
    pub fn read_pcm(&self, start: usize, output: &mut [f32]) -> Result<(), Error> {
        let mut w = command(78, self.id);
        w.u32(start.min(u32::MAX as usize) as u32);
        w.u32(output.len() as u32);
        let response = query(&w.0)?;
        let mut r = Reader::new(&response);
        for sample in output {
            *sample = r.f32()?;
        }
        if !r.done() {
            return Err(Error("invalid browser PCM length".into()));
        }
        Ok(())
    }
}
impl Drop for ClipData {
    fn drop(&mut self) {
        let _ = query(&command(79, self.id).0);
    }
}
pub(super) struct Player {
    id: u32,
}
impl Player {
    pub fn new(clip: Arc<ClipData>) -> Result<Self, Error> {
        let result = query(&command(70, clip.id).0)?;
        let id = Reader::new(&result).u32()?;
        Ok(Self { id })
    }
    pub fn play(&mut self) -> Result<(), Error> {
        query(&command(71, self.id).0).map(|_| ())
    }
    pub fn pause(&mut self) -> Result<(), Error> {
        query(&command(72, self.id).0).map(|_| ())
    }
    pub fn stop(&mut self) -> Result<(), Error> {
        query(&command(73, self.id).0).map(|_| ())
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), Error> {
        let mut w = command(74, self.id);
        w.0.extend_from_slice(&seconds.to_le_bytes());
        query(&w.0).map(|_| ())
    }
    pub fn set_volume(&mut self, volume: f32) -> Result<(), Error> {
        let mut w = command(75, self.id);
        w.f32(volume);
        query(&w.0).map(|_| ())
    }
    pub fn snapshot(&self) -> Result<PlaybackSnapshot, Error> {
        let response = query(&command(76, self.id).0)?;
        let mut r = Reader::new(&response);
        let state = match r.u32()? {
            0 => PlaybackState::Stopped,
            1 => PlaybackState::Playing,
            2 => PlaybackState::Paused,
            3 => PlaybackState::Ended,
            4 => PlaybackState::Interrupted,
            _ => PlaybackState::Failed,
        };
        let position = f64::from_le_bytes(r.take(8)?.try_into().unwrap());
        let duration = f64::from_le_bytes(r.take(8)?.try_into().unwrap());
        let output_latency = f64::from_le_bytes(r.take(8)?.try_into().unwrap());
        if !r.done()
            || ![position, duration, output_latency]
                .iter()
                .all(|s| s.is_finite() && *s >= 0.0)
        {
            return Err(Error("invalid browser audio clock".into()));
        }
        Ok(PlaybackSnapshot {
            state,
            position,
            duration,
            output_latency,
        })
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        let _ = query(&command(77, self.id).0);
    }
}
pub(super) fn suspend_all() {
    let _ = query(&80u32.to_le_bytes());
}
