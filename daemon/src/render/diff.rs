use bytes::Bytes;

use crate::{
    cell::{RemuxCell, set_cursor_position},
    control_signals::CLEAR,
    layout::Rect,
    render::surface::Surface,
};

const SHOW_CURSOR: &[u8] = b"\x1b[?25h";
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";

pub fn render_surface_diff(prev: &Surface, curr: &Surface, force: bool) -> Bytes {
    let rect = Rect {
        x: 0,
        y: 0,
        width: curr.width(),
        height: curr.height(),
    };

    let mut output = Vec::new();
    if force {
        output.extend_from_slice(CLEAR);
    }

    output.extend(RemuxCell::render_diff(rect, prev.cells(), curr.cells(), force));

    if curr.cursor_visible() {
        output.extend_from_slice(SHOW_CURSOR);
        if let Some((x, y)) = curr.cursor() {
            set_cursor_position(&mut output, x + 1, y + 1);
        }
    } else {
        output.extend_from_slice(HIDE_CURSOR);
    }

    Bytes::from(output)
}
