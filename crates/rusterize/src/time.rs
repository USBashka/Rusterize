//! Monotonic host time for input timestamps and deadlines, independent of frames.
use crate::Error;
/// Wall-clock Unix time for saved history, not for animation or audio timing.
pub fn unix_seconds() -> Result<f64, Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .map_err(|e| Error(e.to_string()))
    }
    #[cfg(target_arch = "wasm32")]
    {
        host_clock(48)
    }
}
pub fn monotonic_seconds() -> Result<f64, Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        Ok(ORIGIN
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_secs_f64())
    }
    #[cfg(target_arch = "wasm32")]
    {
        host_clock(49)
    }
}
#[cfg(target_arch = "wasm32")]
fn host_clock(operation: u32) -> Result<f64, Error> {
    let bytes = crate::native::query(&operation.to_le_bytes())?;
    let raw: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error("invalid host clock response".into()))?;
    let seconds = f64::from_le_bytes(raw);
    if seconds.is_finite() && seconds >= 0.0 {
        Ok(seconds)
    } else {
        Err(Error("invalid host clock".into()))
    }
}
