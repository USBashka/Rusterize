use rusterize::*;
use std::{hint::black_box, time::Instant};
fn main() {
    let mut scene = Scene::default();
    let mut bytes = Vec::new();
    let frames = 2000;
    let objects = 1000;
    let start = Instant::now();
    for _ in 0..frames {
        let mut c = Canvas::new(&mut scene);
        c.clear(Color::WHITE);
        for i in 0..objects {
            c.rounded_rect(
                Rect::new((i % 40) as f32 * 20.0, (i / 40) as f32 * 20.0, 16.0, 16.0),
                4.0,
                Color::hex(0x406de0),
            );
        }
        c.finish().unwrap();
        protocol::encode(black_box(&scene), &mut bytes);
        black_box(&bytes);
    }
    println!("{} shapes/frame, {} frames, {:.1} us/frame (record + validate + encode), {} wire bytes/frame",objects,frames,start.elapsed().as_secs_f64()*1e6/frames as f64,bytes.len());
}
