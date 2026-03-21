use crate::{cell::RemuxCell, render::surface::Surface};

#[derive(Clone, Debug, Default)]
pub struct SessionSwitcherOverlay {
    pub sessions: Vec<String>,
    pub selected: usize,
}

pub fn render_session_switcher_overlay(width: u16, height: u16, overlay: &SessionSwitcherOverlay) -> Surface {
    let popup_width = width.min(32).max(12);
    let popup_height = height.min((overlay.sessions.len() as u16).saturating_add(4)).max(5);
    let start_x = width.saturating_sub(popup_width) / 2;
    let start_y = height.saturating_sub(popup_height) / 2;

    let mut surface = Surface::new(width, height);
    fill_rect(&mut surface, start_x, start_y, popup_width, popup_height, b' ');
    draw_box(&mut surface, start_x, start_y, popup_width, popup_height);
    write_text(&mut surface, start_x + 2, start_y, "Sessions");

    for (index, session) in overlay.sessions.iter().enumerate() {
        let prefix = if index == overlay.selected { "> " } else { "  " };
        let row = start_y + 2 + index as u16;
        if row >= start_y + popup_height.saturating_sub(1) {
            break;
        }
        write_text(&mut surface, start_x + 2, row, &(prefix.to_owned() + session));
    }

    surface
}

fn fill_rect(surface: &mut Surface, x: u16, y: u16, width: u16, height: u16, byte: u8) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            let mut cell = RemuxCell::default();
            cell.set_content(&[byte]);
            surface.paint_cell(col, row, cell);
        }
    }
}

fn draw_box(surface: &mut Surface, x: u16, y: u16, width: u16, height: u16) {
    if width < 2 || height < 2 {
        return;
    }

    for col in x + 1..x + width - 1 {
        paint(surface, col, y, b'-');
        paint(surface, col, y + height - 1, b'-');
    }
    for row in y + 1..y + height - 1 {
        paint(surface, x, row, b'|');
        paint(surface, x + width - 1, row, b'|');
    }
    paint(surface, x, y, b'+');
    paint(surface, x + width - 1, y, b'+');
    paint(surface, x, y + height - 1, b'+');
    paint(surface, x + width - 1, y + height - 1, b'+');
}

fn write_text(surface: &mut Surface, x: u16, y: u16, text: &str) {
    for (offset, byte) in text.bytes().enumerate() {
        paint(surface, x + offset as u16, y, byte);
    }
}

fn paint(surface: &mut Surface, x: u16, y: u16, byte: u8) {
    let mut cell = RemuxCell::default();
    cell.set_content(&[byte]);
    surface.paint_cell(x, y, cell);
}
