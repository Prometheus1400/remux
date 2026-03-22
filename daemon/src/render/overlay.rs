use crate::{cell::RemuxCell, render::surface::Surface};

#[derive(Clone, Debug, Default)]
pub struct SessionSwitcherOverlay {
    pub sessions: Vec<String>,
    pub selected: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSwitcherStyle {
    pub title: String,
    pub footer: String,
    pub border_fg: u8,
    pub background_fg: u8,
    pub background_bg: u8,
    pub title_fg: u8,
    pub text_fg: u8,
    pub selected_fg: u8,
    pub selected_bg: u8,
    pub footer_fg: u8,
}

impl Default for SessionSwitcherStyle {
    fn default() -> Self {
        Self {
            title: "Sessions".to_owned(),
            footer: "arrows move  enter select  esc cancel".to_owned(),
            border_fg: 110,
            background_fg: 252,
            background_bg: 236,
            title_fg: 229,
            text_fg: 252,
            selected_fg: 231,
            selected_bg: 31,
            footer_fg: 245,
        }
    }
}

pub fn render_session_switcher_overlay(
    width: u16,
    height: u16,
    overlay: &SessionSwitcherOverlay,
    style: &SessionSwitcherStyle,
) -> Surface {
    if width < 12 || height < 6 {
        return Surface::new(width, height);
    }

    let title = &style.title;
    let footer = &style.footer;
    let longest_session = overlay
        .sessions
        .iter()
        .map(|session| session.chars().count())
        .max()
        .unwrap_or(0);
    let desired_width = (longest_session.max(title.len()).max(footer.len().min(36)) + 6) as u16;
    let popup_width = desired_width.min(width.saturating_sub(4).max(18));

    let chrome_rows = 5u16;
    let desired_height = overlay.sessions.len() as u16 + chrome_rows;
    let popup_height = desired_height.min(height.saturating_sub(2).max(6)).max(6);
    let start_x = width.saturating_sub(popup_width) / 2;
    let start_y = height.saturating_sub(popup_height) / 2;
    let list_rows = popup_height.saturating_sub(chrome_rows) as usize;
    let max_start = overlay.sessions.len().saturating_sub(list_rows);
    let first_visible = overlay.selected.saturating_sub(list_rows / 2).min(max_start);

    let mut surface = Surface::new(width, height);
    fill_rect(
        &mut surface,
        start_x,
        start_y,
        popup_width,
        popup_height,
        b' ',
        vt100::Color::Idx(style.background_fg),
        vt100::Color::Idx(style.background_bg),
    );
    draw_box(
        &mut surface,
        start_x,
        start_y,
        popup_width,
        popup_height,
        vt100::Color::Idx(style.border_fg),
        vt100::Color::Idx(style.background_bg),
    );
    write_text(
        &mut surface,
        start_x + 2,
        start_y + 1,
        title,
        vt100::Color::Idx(style.title_fg),
        vt100::Color::Idx(style.background_bg),
    );
    draw_horizontal_rule(
        &mut surface,
        start_x + 1,
        start_y + 2,
        popup_width.saturating_sub(2),
        vt100::Color::Idx(style.border_fg),
        vt100::Color::Idx(style.background_bg),
    );

    for row_index in 0..list_rows {
        let session_index = first_visible + row_index;
        let row = start_y + 3 + row_index as u16;
        if row >= start_y + popup_height.saturating_sub(2) {
            break;
        }

        let is_selected = session_index == overlay.selected;
        let (fg, bg) = if is_selected {
            (
                vt100::Color::Idx(style.selected_fg),
                vt100::Color::Idx(style.selected_bg),
            )
        } else {
            (vt100::Color::Idx(style.text_fg), vt100::Color::Idx(style.background_bg))
        };

        fill_rect(
            &mut surface,
            start_x + 1,
            row,
            popup_width.saturating_sub(2),
            1,
            b' ',
            fg,
            bg,
        );

        let Some(session) = overlay.sessions.get(session_index) else {
            continue;
        };

        let prefix = if is_selected { "> " } else { "  " };
        let label_width = popup_width.saturating_sub(6) as usize;
        let label = truncate_text(session, label_width);
        write_text(&mut surface, start_x + 2, row, &(prefix.to_owned() + &label), fg, bg);
    }

    write_text(
        &mut surface,
        start_x + 2,
        start_y + popup_height.saturating_sub(2),
        &truncate_text(footer, popup_width.saturating_sub(4) as usize),
        vt100::Color::Idx(style.footer_fg),
        vt100::Color::Idx(style.background_bg),
    );

    surface
}

fn fill_rect(
    surface: &mut Surface,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    byte: u8,
    fg: vt100::Color,
    bg: vt100::Color,
) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            surface.paint_cell(col, row, styled_cell(byte, fg, bg));
        }
    }
}

fn draw_box(surface: &mut Surface, x: u16, y: u16, width: u16, height: u16, fg: vt100::Color, bg: vt100::Color) {
    if width < 2 || height < 2 {
        return;
    }

    for col in x + 1..x + width - 1 {
        paint(surface, col, y, "─", fg, bg);
        paint(surface, col, y + height - 1, "─", fg, bg);
    }
    for row in y + 1..y + height - 1 {
        paint(surface, x, row, "│", fg, bg);
        paint(surface, x + width - 1, row, "│", fg, bg);
    }
    paint(surface, x, y, "╭", fg, bg);
    paint(surface, x + width - 1, y, "╮", fg, bg);
    paint(surface, x, y + height - 1, "╰", fg, bg);
    paint(surface, x + width - 1, y + height - 1, "╯", fg, bg);
}

fn draw_horizontal_rule(surface: &mut Surface, x: u16, y: u16, width: u16, fg: vt100::Color, bg: vt100::Color) {
    for col in x..x.saturating_add(width) {
        paint(surface, col, y, "─", fg, bg);
    }
}

fn write_text(surface: &mut Surface, x: u16, y: u16, text: &str, fg: vt100::Color, bg: vt100::Color) {
    for (offset, byte) in text.bytes().enumerate() {
        let mut cell = RemuxCell::default();
        cell.set_content(&[byte]);
        cell.set_fg_color(fg);
        cell.set_bg_color(bg);
        surface.paint_cell(x + offset as u16, y, cell);
    }
}

fn paint(surface: &mut Surface, x: u16, y: u16, text: &str, fg: vt100::Color, bg: vt100::Color) {
    let mut cell = RemuxCell::default();
    cell.set_content(text.as_bytes());
    cell.set_fg_color(fg);
    cell.set_bg_color(bg);
    surface.paint_cell(x, y, cell);
}

fn styled_cell(byte: u8, fg: vt100::Color, bg: vt100::Color) -> RemuxCell {
    let mut cell = RemuxCell::default();
    cell.set_content(&[byte]);
    cell.set_fg_color(fg);
    cell.set_bg_color(bg);
    cell
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let width = text.chars().count();
    if width <= max_chars {
        return text.to_owned();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let mut out = String::new();
    for ch in text.chars().take(max_chars - 3) {
        out.push(ch);
    }
    out.push_str("...");
    out
}
