use rusterize::{
    audio::{AudioEvent, Clip, PlaybackSnapshot, PlaybackState, Player},
    native::{FileDialog, NativeResponse, RequestId, WindowState},
    *,
};
use std::collections::BTreeMap;

const APP: &str = "dev.rusterize.media";
const BG: Color = Color::hex(0x10151e);
const CARD: Color = Color::hex(0x1b2330);
const INK: Color = Color::hex(0xeaf1fc);
const MUTED: Color = Color::hex(0x96a6bd);
const ACCENT: Color = Color::hex(0x8de2c0);

#[derive(Clone, Copy)]
enum Pending {
    PickAudio,
    PickImage,
    AudioBytes,
    ImageBytes,
    DecodeAudio,
    DecodeEffect,
    DecodeImage,
    Restore,
    Save,
    Export,
    Window(bool),
}
pub struct App {
    requests: BTreeMap<u64, Pending>,
    initialized: bool,
    volume: f32,
    music: Option<Player>,
    effect: Option<Player>,
    image: Option<Image>,
    clip: Option<Clip>,
    snapshot: Option<PlaybackSnapshot>,
    spectrum: [f32; 32],
    status: String,
    size: Size,
    pressed: Option<usize>,
    dragging: Option<usize>,
    fullscreen: bool,
}
impl Default for App {
    fn default() -> Self {
        Self {
            requests: BTreeMap::new(),
            initialized: false,
            volume: 0.6,
            music: None,
            effect: None,
            image: None,
            clip: None,
            snapshot: None,
            spectrum: [0.0; 32],
            status: String::new(),
            size: Size::new(840.0, 680.0),
            pressed: None,
            dragging: None,
            fullscreen: false,
        }
    }
}
impl App {
    fn remember(&mut self, id: RequestId, pending: Pending) {
        self.requests.insert(id.0, pending);
    }
    fn bounds(&self) -> Vec<Rect> {
        let w = self.size.width;
        let gap = 12.0;
        let half = (w - 48.0 - gap) / 2.0;
        vec![
            Rect::new(24.0, 80.0, half, 44.0),
            Rect::new(24.0 + half + gap, 80.0, half, 44.0),
            Rect::new(24.0, 392.0, (w - 72.0) / 3.0, 48.0),
            Rect::new(36.0 + (w - 72.0) / 3.0, 392.0, (w - 72.0) / 3.0, 48.0),
            Rect::new(48.0 + 2.0 * (w - 72.0) / 3.0, 392.0, (w - 72.0) / 3.0, 48.0),
            Rect::new(24.0, 332.0, w - 48.0, 38.0),
            Rect::new(130.0, 458.0, w - 154.0, 32.0),
            Rect::new(24.0, 516.0, half, 42.0),
            Rect::new(36.0 + half, 516.0, half, 42.0),
        ]
    }
    fn save(&mut self, c: &mut Context) {
        let id = c.store_data(APP, "volume", self.volume.to_string().into_bytes());
        self.remember(id, Pending::Save);
    }
    fn refresh(&mut self) {
        if let Some(player) = &mut self.effect {
            if let Some(AudioEvent::Error(error)) = player.poll_event() {
                self.status = error.to_string();
            }
        }
        if let Some(player) = &mut self.music {
            match player.snapshot() {
                Ok(snapshot) => {
                    self.snapshot = Some(snapshot);
                    if snapshot.state == PlaybackState::Playing {
                        self.spectrum = player
                            .clip()
                            .spectrum(snapshot.position)
                            .unwrap_or([0.0; 32]);
                    }
                }
                Err(e) => self.status = e.to_string(),
            }
            if let Some(event) = player.poll_event() {
                self.status = match event {
                    AudioEvent::Ended => "Воспроизведение завершено".into(),
                    AudioEvent::Interrupted => "Звук прерван. Нажмите «Воспроизвести»".into(),
                    AudioEvent::Error(e) => e.to_string(),
                };
            }
        }
    }
    fn activate(&mut self, index: usize, c: &mut Context) -> Result<(), Error> {
        match index {
            0 | 1 => {
                let id = c.select_file(FileDialog {
                    title: if index == 0 {
                        "Музыка: MP3 или WAV"
                    } else {
                        "Изображение: PNG или JPEG"
                    }
                    .into(),
                    ..Default::default()
                });
                self.remember(
                    id,
                    if index == 0 {
                        Pending::PickAudio
                    } else {
                        Pending::PickImage
                    },
                );
            }
            2 => {
                if let Some(player) = &mut self.music {
                    if player.snapshot()?.state == PlaybackState::Playing {
                        player.pause()?;
                    } else {
                        player.play()?;
                    }
                }
            }
            3 => {
                if let Some(player) = &mut self.music {
                    player.stop()?;
                }
            }
            4 => {
                if let Some(player) = &mut self.effect {
                    player.stop()?;
                    player.play()?;
                }
            }
            7 => {
                let id = c.select_file(FileDialog {
                    title: "Экспорт настроек".into(),
                    save: true,
                    suggested_name: "volume.txt".into(),
                    ..Default::default()
                });
                self.remember(id, Pending::Export);
            }
            8 => {
                let fullscreen = !self.fullscreen;
                let id = c.set_window_state(if fullscreen {
                    WindowState::Fullscreen
                } else {
                    WindowState::Normal
                });
                self.remember(id, Pending::Window(fullscreen));
            }
            _ => {}
        }
        Ok(())
    }
    fn slider(&mut self, index: usize, x: f32) -> Result<(), Error> {
        let rect = self.bounds()[index];
        let fraction = ((x - rect.x) / rect.width).clamp(0.0, 1.0);
        if index == 5 {
            if let Some(player) = &mut self.music {
                player.seek(f64::from(fraction) * player.clip().duration())?;
            }
        } else {
            self.volume = fraction;
            if let Some(player) = &mut self.music {
                player.set_volume(self.volume)?;
            }
            if let Some(player) = &mut self.effect {
                player.set_volume(self.volume)?;
            }
        }
        Ok(())
    }
    fn result(
        &mut self,
        pending: Pending,
        result: NativeResponse,
        c: &mut Context,
    ) -> Result<(), Error> {
        match (pending, result) {
            (_, NativeResponse::Cancelled) => self.status = "Выбор отменён".into(),
            (Pending::Restore, NativeResponse::Missing) => {}
            (Pending::Restore, NativeResponse::Bytes(bytes)) => {
                self.volume = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|s| s.parse::<f32>().ok())
                    .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                    .ok_or_else(|| Error("Повреждены сохранённые настройки".into()))?;
                if let Some(player) = &mut self.music {
                    player.set_volume(self.volume)?;
                }
                if let Some(player) = &mut self.effect {
                    player.set_volume(self.volume)?;
                }
                self.status = "Настройки восстановлены".into();
            }
            (Pending::PickAudio | Pending::PickImage, NativeResponse::Files(locations)) => {
                if let Some(location) = locations.into_iter().next() {
                    let id = c.read_file(location.clone());
                    self.remember(
                        id,
                        if matches!(pending, Pending::PickAudio) {
                            Pending::AudioBytes
                        } else {
                            Pending::ImageBytes
                        },
                    );
                    c.release_file(location);
                    self.status = "Загрузка…".into();
                }
            }
            (Pending::AudioBytes, NativeResponse::Bytes(bytes)) => {
                let id = c.decode_audio(bytes);
                self.remember(id, Pending::DecodeAudio);
            }
            (Pending::ImageBytes, NativeResponse::Bytes(bytes)) => {
                let id = c.decode_image(bytes);
                self.remember(id, Pending::DecodeImage);
            }
            (Pending::DecodeImage, NativeResponse::Image(image)) => {
                self.image = Some(image);
                self.status = "Изображение загружено".into();
            }
            (Pending::DecodeAudio, NativeResponse::Audio(clip)) => {
                let mut player = Player::new(clip.clone())?;
                player.set_volume(self.volume)?;
                self.clip = Some(clip);
                self.music = Some(player);
                self.status = "Музыка готова".into();
            }
            (Pending::DecodeEffect, NativeResponse::Audio(clip)) => {
                let mut player = Player::new(clip)?;
                player.set_volume(self.volume)?;
                self.effect = Some(player);
            }
            (Pending::Export, NativeResponse::Files(locations)) => {
                if let Some(location) = locations.into_iter().next() {
                    let id = c.write_file(location.clone(), self.volume.to_string().into_bytes());
                    self.remember(id, Pending::Save);
                    c.release_file(location);
                }
            }
            (Pending::Save, NativeResponse::Done) => self.status = "Сохранено".into(),
            (Pending::Window(fullscreen), NativeResponse::Done) => self.fullscreen = fullscreen,
            _ => {}
        }
        Ok(())
    }
}
impl Application for App {
    fn window_options(&self) -> WindowOptions {
        WindowOptions {
            min_size: Size::new(360.0, 640.0),
            ..WindowOptions::default()
                .with_title("Rusterize · Медиа")
                .with_size(840.0, 680.0)
                .with_title_bar(TitleBarStyle::colors(BG, INK))
        }
    }
    fn event(&mut self, event: Event, c: &mut Context) {
        match event {
            Event::Resume => {
                if !self.initialized {
                    self.initialized = true;
                    let id = c.load_data(APP, "volume");
                    self.remember(id, Pending::Restore);
                    #[cfg(feature = "bundled-media")]
                    {
                        let id = c.decode_audio(include_bytes!("../assets/demo.mp3").to_vec());
                        self.remember(id, Pending::DecodeAudio);
                        let id = c.decode_image(include_bytes!("../assets/cover.png").to_vec());
                        self.remember(id, Pending::DecodeImage);
                    }
                    let id = c.decode_audio(effect_wav());
                    self.remember(id, Pending::DecodeEffect);
                }
            }
            Event::Resize(v) => self.size = v.size,
            Event::NativeResult { id, result } => {
                if let Some(pending) = self.requests.remove(&id.0) {
                    if let Err(e) = result.and_then(|r| self.result(pending, r, c)) {
                        self.status = e.to_string();
                    }
                } else if let Err(e) = result {
                    self.status = e.to_string();
                }
            }
            Event::Pointer {
                phase: PointerPhase::Down,
                position,
                ..
            } => {
                self.pressed = self.bounds().iter().position(|r| r.contains(position));
                if let Some(i @ (5 | 6)) = self.pressed {
                    self.dragging = Some(i);
                    if let Err(e) = self.slider(i, position.x) {
                        self.status = e.to_string();
                    }
                }
            }
            Event::Pointer {
                phase: PointerPhase::Move,
                position,
                ..
            } => {
                if let Some(i) = self.dragging {
                    if let Err(e) = self.slider(i, position.x) {
                        self.status = e.to_string();
                    }
                }
            }
            Event::Pointer {
                phase: PointerPhase::Up,
                position,
                ..
            } => {
                if let Some(i) = self.pressed.take() {
                    if self.bounds()[i].contains(position) {
                        if let Err(e) = self.activate(i, c) {
                            self.status = e.to_string();
                        }
                    }
                }
                if self.dragging.take() == Some(6) {
                    self.save(c);
                }
            }
            Event::Pointer {
                phase: PointerPhase::Cancel,
                ..
            }
            | Event::Focus(false)
            | Event::Suspend => {
                self.pressed = None;
                self.dragging = None;
            }
            Event::Key {
                key: Key::Space,
                pressed: true,
                repeat: false,
                ..
            } => {
                if let Err(e) = self.activate(2, c) {
                    self.status = e.to_string();
                }
            }
            Event::Frame { .. } => self.refresh(),
            _ => {}
        }
        self.refresh();
        let playing = self
            .snapshot
            .is_some_and(|s| s.state == PlaybackState::Playing);
        let effect = self
            .effect
            .as_ref()
            .and_then(|p| p.snapshot().ok())
            .is_some_and(|s| s.state == PlaybackState::Playing);
        c.set_animation(playing || effect || !self.requests.is_empty());
        c.request_redraw();
    }
    fn draw(&mut self, c: &mut Canvas<'_>, v: Viewport) {
        self.size = v.size;
        c.clear(BG);
        let w = self.size.width;
        c.text("Медиатека", (24.0, 48.0), TextStyle::new(28.0, INK).bold());
        let bounds = self.bounds();
        let playing = self
            .snapshot
            .is_some_and(|s| s.state == PlaybackState::Playing);
        for (i, label) in [
            (0, "Открыть музыку"),
            (1, "Изображение"),
            (
                2,
                if playing {
                    "Пауза"
                } else {
                    "Воспроизвести"
                },
            ),
            (3, "Стоп"),
            (4, "Эффект"),
            (7, "Экспорт настроек"),
            (
                8,
                if self.fullscreen {
                    "Обычное окно"
                } else {
                    "Полный экран"
                },
            ),
        ] {
            let r = bounds[i];
            c.rounded_rect(r, 12.0, if i == 2 { ACCENT } else { CARD });
            c.text(
                label,
                (r.x + 14.0, r.y + r.height * 0.5 + 5.0),
                TextStyle::new(
                    if w < 480.0 { 12.0 } else { 15.0 },
                    if i == 2 { BG } else { INK },
                ),
            );
        }
        let cover = Rect::new(24.0, 148.0, 156.0, 156.0);
        c.rounded_rect(cover, 16.0, CARD);
        if let Some(image) = &self.image {
            c.with_save(|c| {
                c.clip(Shape::RoundedRect(cover, 16.0));
                let scale =
                    (cover.width / image.width() as f32).max(cover.height / image.height() as f32);
                let iw = image.width() as f32 * scale;
                let ih = image.height() as f32 * scale;
                c.image(
                    image,
                    Rect::new(
                        cover.x + (cover.width - iw) / 2.0,
                        cover.y + (cover.height - ih) / 2.0,
                        iw,
                        ih,
                    ),
                    1.0,
                );
            });
        }
        let left = 204.0;
        let available = (w - left - 24.0).max(100.0);
        c.text("Спектр", (left, 166.0), TextStyle::new(14.0, MUTED));
        for (i, value) in self.spectrum.iter().enumerate() {
            let height = (value.sqrt() * 110.0).max(3.0);
            c.rounded_rect(
                Rect::new(
                    left + i as f32 * available / 32.0,
                    304.0 - height,
                    (available / 32.0 - 3.0).max(1.0),
                    height,
                ),
                2.0,
                ACCENT,
            );
        }
        let r = bounds[5];
        let progress = self.snapshot.map_or(0.0, |s| {
            if s.duration > 0.0 {
                (s.position / s.duration) as f32
            } else {
                0.0
            }
        });
        if let Some(clip) = &self.clip {
            for (i, value) in clip.waveform().iter().enumerate().step_by(4) {
                let x = r.x + r.width * i as f32 / 512.0;
                let h = (*value * 28.0).max(2.0);
                c.fill_rect(
                    Rect::new(x, r.y + (r.height - h) * 0.5, 2.0, h),
                    if x < r.x + r.width * progress {
                        ACCENT
                    } else {
                        MUTED
                    },
                );
            }
        }
        c.fill_rect(Rect::new(r.x + r.width * progress, r.y, 2.0, r.height), INK);
        let time = self.snapshot.map_or("0:00 / 0:00".into(), |s| {
            format!("{} / {}", timestamp(s.position), timestamp(s.duration))
        });
        c.text(time, (24.0, 386.0), TextStyle::new(12.0, MUTED));
        c.text("Громкость", (24.0, 480.0), TextStyle::new(14.0, MUTED));
        let r = bounds[6];
        c.rounded_rect(Rect::new(r.x, r.y + 13.0, r.width, 6.0), 3.0, CARD);
        c.rounded_rect(
            Rect::new(r.x, r.y + 13.0, r.width * self.volume, 6.0),
            3.0,
            ACCENT,
        );
        c.circle((r.x + r.width * self.volume, r.y + 16.0), 7.0, INK);
        if let Ok(layout) = TextLayout::new(
            self.status.as_str(),
            TextStyle::new(13.0, MUTED),
            TextOptions::wrap(w - 48.0),
        ) {
            c.text_layout(&layout, (24.0, 584.0));
        }
    }
}
fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
fn effect_wav() -> Vec<u8> {
    let count = 4410u32;
    let mut bytes = Vec::with_capacity(count as usize * 2 + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + count * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&22050u32.to_le_bytes());
    bytes.extend_from_slice(&44100u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(count * 2).to_le_bytes());
    for i in 0..count {
        let sample = (std::f32::consts::TAU * 880.0 * i as f32 / 22050.0).sin()
            * 0.25
            * (1.0 - i as f32 / count as f32).powi(2);
        bytes.extend_from_slice(&((sample * 32767.0) as i16).to_le_bytes());
    }
    bytes
}
rusterize::export_app!(App);
