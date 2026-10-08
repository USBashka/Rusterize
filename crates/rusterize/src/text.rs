use crate::*;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrap {
    None,
    #[default]
    Word,
    Character,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextDirection {
    #[default]
    LeftToRight,
    RightToLeft,
}

/// Layout dimensions are logical pixels. Height is determined by the native engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextOptions {
    pub width: f32,
    pub wrap: TextWrap,
    pub align: TextAlign,
    pub direction: TextDirection,
}
impl Default for TextOptions {
    fn default() -> Self {
        Self {
            width: 1_000_000.0,
            wrap: TextWrap::None,
            align: TextAlign::Left,
            direction: TextDirection::LeftToRight,
        }
    }
}
impl TextOptions {
    pub fn wrap(width: f32) -> Self {
        Self {
            width,
            wrap: TextWrap::Word,
            ..Self::default()
        }
    }
    pub(crate) fn valid(self) -> bool {
        self.width.is_finite() && self.width > 0.0 && self.width <= 1_000_000.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub width_including_trailing_whitespace: f32,
    pub height: f32,
    pub baseline: f32,
}

/// Byte range in the original UTF-8 string; includes a terminating newline if present.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    pub range: std::ops::Range<usize>,
    pub top: f32,
    pub height: f32,
    pub baseline: f32,
}

/// Native shaping and line breaking. Keep a layout until its text/style/width changes.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    pub(crate) text: Arc<str>,
    pub(crate) style: TextStyle,
    pub(crate) options: TextOptions,
    metrics: TextMetrics,
    lines: Arc<[TextLine]>,
}
impl TextLayout {
    pub fn new(
        text: impl Into<Arc<str>>,
        style: TextStyle,
        options: TextOptions,
    ) -> Result<Self, Error> {
        let text = text.into();
        validate_text(&text, &style, options)?;
        let mut request = crate::protocol::Writer(Vec::new());
        request.u32(1);
        request.text(&text, &style, options);
        let result = native::query(&request.0)?;
        let mut r = native::Reader::new(&result);
        let metrics = TextMetrics {
            width: r.f32()?,
            width_including_trailing_whitespace: r.f32()?,
            height: r.f32()?,
            baseline: r.f32()?,
        };
        let count = r.u32()? as usize;
        if count == 0 || count > text.len() + 1 {
            return Err(Error("invalid native line count".into()));
        }
        let mut lines = Vec::with_capacity(count);
        let mut end = 0;
        for _ in 0..count {
            let start = r.u32()? as usize;
            let next = r.u32()? as usize;
            if start != end
                || next < start
                || !text.is_char_boundary(start)
                || !text.is_char_boundary(next)
            {
                return Err(Error("invalid native UTF-8 line range".into()));
            }
            let line = TextLine {
                range: start..next,
                top: r.f32()?,
                height: r.f32()?,
                baseline: r.f32()?,
            };
            if line.top < 0.0 || line.height < 0.0 || line.baseline < 0.0 {
                return Err(Error("invalid native line metrics".into()));
            }
            end = next;
            lines.push(line);
        }
        if end != text.len()
            || !r.done()
            || metrics.width < 0.0
            || metrics.width_including_trailing_whitespace < 0.0
            || metrics.height < 0.0
            || metrics.baseline < 0.0
        {
            return Err(Error("invalid native text metrics".into()));
        }
        Ok(Self {
            text,
            style,
            options,
            metrics,
            lines: lines.into(),
        })
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn style(&self) -> &TextStyle {
        &self.style
    }
    pub fn options(&self) -> TextOptions {
        self.options
    }
    pub fn metrics(&self) -> TextMetrics {
        self.metrics
    }
    pub fn lines(&self) -> &[TextLine] {
        &self.lines
    }
    /// End byte of the last complete line fitting the supplied height.
    pub fn fitting_prefix(&self, height: f32) -> usize {
        self.lines
            .iter()
            .take_while(|l| l.top + l.height <= height)
            .last()
            .map_or(0, |l| l.range.end)
    }
}

pub fn measure_text(text: &str, style: &TextStyle) -> Result<TextMetrics, Error> {
    Ok(TextLayout::new(text, style.clone(), TextOptions::default())?.metrics())
}
pub(crate) fn validate_text(
    text: &str,
    style: &TextStyle,
    options: TextOptions,
) -> Result<(), Error> {
    if text.len() > 1024 * 1024 || text.contains('\0') || !style.valid() || !options.valid() {
        Err(Error("invalid text, font or layout dimensions".into()))
    } else {
        Ok(())
    }
}
