use bytes::Bytes;

use crate::{
    cell::{set_cursor_position, RemuxCell},
    layout::Rect,
    render::surface::Surface,
};

pub fn render_surface_diff(prev: &Surface, curr: &Surface, force: bool) -> Bytes {
    let rect = Rect {
        x: 0,
        y: 0,
        width: curr.width(),
        height: curr.height(),
    };

    let mut output = RemuxCell::render_diff(rect, prev.cells(), curr.cells(), force);

    if curr.cursor_visible() {
        if let Some((x, y)) = curr.cursor() {
            set_cursor_position(&mut output, x + 1, y + 1);
        }
    }

    Bytes::from(output)
}
