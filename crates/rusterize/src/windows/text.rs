use super::*;

/// Full DirectWrite layout interface, including range formatting, hit testing,
/// typography, inline objects and cluster metrics. Positions here are UTF-16 units.
pub fn create_text_layout(
    text: &str,
    style: &TextStyle,
    options: TextOptions,
) -> Result<IDWriteTextLayout, Error> {
    crate::text::validate_text(text, style, options)?;
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let name = style.font_name.as_deref().unwrap_or(match style.family {
            FontFamily::Sans => "Segoe UI",
            FontFamily::Serif => "Georgia",
            FontFamily::Monospace => "Consolas",
        });
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let format = factory.CreateTextFormat(
            PCWSTR(name.as_ptr()),
            None,
            if style.bold {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            },
            if style.italic {
                DWRITE_FONT_STYLE_ITALIC
            } else {
                DWRITE_FONT_STYLE_NORMAL
            },
            DWRITE_FONT_STRETCH_NORMAL,
            style.size,
            w!(""),
        )?;
        format.SetWordWrapping(match options.wrap {
            TextWrap::None => DWRITE_WORD_WRAPPING_NO_WRAP,
            TextWrap::Word => DWRITE_WORD_WRAPPING_WRAP,
            TextWrap::Character => DWRITE_WORD_WRAPPING_CHARACTER,
        })?;
        let rtl = options.direction == TextDirection::RightToLeft;
        format.SetReadingDirection(if rtl {
            DWRITE_READING_DIRECTION_RIGHT_TO_LEFT
        } else {
            DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
        })?;
        format.SetTextAlignment(match options.align {
            TextAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            TextAlign::Left if rtl => DWRITE_TEXT_ALIGNMENT_TRAILING,
            TextAlign::Right if !rtl => DWRITE_TEXT_ALIGNMENT_TRAILING,
            _ => DWRITE_TEXT_ALIGNMENT_LEADING,
        })?;
        let utf16: Vec<u16> = text.encode_utf16().collect();
        let layout = factory.CreateTextLayout(&utf16, &format, options.width, 1_000_000.0)?;
        let range = DWRITE_TEXT_RANGE {
            startPosition: 0,
            length: utf16.len() as u32,
        };
        layout.SetUnderline(style.underline, range)?;
        layout.SetStrikethrough(style.strikethrough, range)?;
        Ok(layout)
    }
}
pub(super) fn line_metrics(layout: &IDWriteTextLayout) -> Result<Vec<DWRITE_LINE_METRICS>, Error> {
    unsafe {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics)?;
        let mut lines = vec![DWRITE_LINE_METRICS::default(); metrics.lineCount as usize];
        let mut count = 0;
        layout.GetLineMetrics(Some(&mut lines), &mut count)?;
        lines.truncate(count as usize);
        Ok(lines)
    }
}
pub(super) fn measure(
    text: &str,
    style: &TextStyle,
    options: TextOptions,
) -> Result<Vec<u8>, Error> {
    let layout = create_text_layout(text, style, options)?;
    let lines = line_metrics(&layout)?;
    let mut m = DWRITE_TEXT_METRICS::default();
    unsafe {
        layout.GetMetrics(&mut m)?;
    }
    let mut w = crate::protocol::Writer(Vec::new());
    for v in [
        m.width,
        m.widthIncludingTrailingWhitespace,
        m.height,
        lines.first().map_or(0.0, |l| l.baseline),
    ] {
        w.f32(v);
    }
    w.u32(lines.len() as u32);
    let mut byte = 0;
    let mut top = 0.0;
    let mut chars = text.char_indices();
    for line in lines {
        let start = byte;
        let mut units = 0;
        while units < line.length {
            let (index, ch) = chars
                .next()
                .ok_or_else(|| Error("invalid DirectWrite line range".into()))?;
            units += ch.len_utf16() as u32;
            byte = index + ch.len_utf8();
        }
        w.u32(start as u32);
        w.u32(byte as u32);
        w.f32(top);
        w.f32(line.height);
        w.f32(top + line.baseline);
        top += line.height;
    }
    Ok(w.0)
}
pub(super) fn read(r: &mut native::Reader<'_>) -> Result<(String, TextStyle, TextOptions), Error> {
    let size = r.f32()?;
    let c = r.take(4)?;
    let color = Color::rgba(c[0], c[1], c[2], c[3]);
    let family = match r.u32()? {
        0 => FontFamily::Sans,
        1 => FontFamily::Serif,
        2 => FontFamily::Monospace,
        _ => return Err(Error("unknown font family".into())),
    };
    let flags = r.u32()?;
    let font = r.string()?;
    let width = r.f32()?;
    let wrap = match r.u32()? {
        0 => TextWrap::None,
        1 => TextWrap::Word,
        2 => TextWrap::Character,
        _ => return Err(Error("unknown wrap mode".into())),
    };
    let align = match r.u32()? {
        0 => TextAlign::Left,
        1 => TextAlign::Center,
        2 => TextAlign::Right,
        _ => return Err(Error("unknown alignment".into())),
    };
    let direction = match r.u32()? {
        0 => TextDirection::LeftToRight,
        1 => TextDirection::RightToLeft,
        _ => return Err(Error("unknown direction".into())),
    };
    let text = r.string()?;
    Ok((
        text,
        TextStyle {
            size,
            color,
            family,
            bold: flags & 1 != 0,
            italic: flags & 2 != 0,
            underline: flags & 4 != 0,
            strikethrough: flags & 8 != 0,
            font_name: (!font.is_empty()).then_some(font),
        },
        TextOptions {
            width,
            wrap,
            align,
            direction,
        },
    ))
}
