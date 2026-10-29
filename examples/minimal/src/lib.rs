use rusterize::*;
#[derive(Default)]
pub struct App;
impl Application for App {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: Viewport) {
        canvas.clear(Color::hex(0x121923));
    }
}
rusterize::export_app!(App);
