use crate::{Error, Point};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Color {
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub const fn hex(rgb: u32) -> Self {
        Self::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    }
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    Solid(Color),
    /// Two-stop linear gradient, clamped outside its endpoints.
    Linear {
        from: Point,
        to: Point,
        start: Color,
        end: Color,
    },
}
impl From<Color> for Paint {
    fn from(c: Color) -> Self {
        Self::Solid(c)
    }
}
impl Paint {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Solid(_) => true,
            Self::Linear { from, to, .. } => from.finite() && to.finite() && from != to,
        }
    }
}

/// Portable generic families resolved to the system's installed fonts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontFamily {
    #[default]
    Sans,
    Serif,
    Monospace,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub color: Color,
    pub family: FontFamily,
    pub bold: bool,
}
impl TextStyle {
    pub const fn new(size: f32, color: Color) -> Self {
        Self {
            size,
            color,
            family: FontFamily::Sans,
            bold: false,
        }
    }
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub const fn family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }
}

/// Immutable, straight-alpha RGBA8 bitmap. Keep this object across frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    id: u64,
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}
impl Image {
    pub fn rgba(width: u32, height: u32, pixels: impl Into<Arc<[u8]>>) -> Result<Self, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let pixels = pixels.into();
        let expected = (width as u64)
            .checked_mul(height as u64)
            .and_then(|n| n.checked_mul(4))
            .unwrap_or(u64::MAX);
        if width == 0
            || height == 0
            || expected > 64 * 1024 * 1024
            || expected != pixels.len() as u64
        {
            return Err(Error("image requires non-zero dimensions and exactly width × height × 4 bytes (up to 64 MiB)".into()));
        }
        Ok(Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            pixels,
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
