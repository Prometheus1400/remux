use crate::{cell::RemuxCell, render::surface::Surface};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectorItem {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorStyle {
    pub title: String,
    pub footer: String,
    pub scrim_fg: u8,
    pub scrim_bg: u8,
    pub border_fg: u8,
    pub background_fg: u8,
    pub background_bg: u8,
    pub title_fg: u8,
    pub text_fg: u8,
    pub selected_fg: u8,
    pub selected_bg: u8,
    pub footer_fg: u8,
    pub empty_fg: u8,
}

impl Default for SelectorStyle {
    fn default() -> Self {
        Self {
            title: "Sessions".to_owned(),
            footer: "Up/Down Move  Enter Open  Esc Cancel".to_owned(),
            scrim_fg: 236,
            scrim_bg: 234,
            border_fg: 110,
            background_fg: 252,
            background_bg: 236,
            title_fg: 229,
            text_fg: 252,
            selected_fg: 231,
            selected_bg: 31,
            footer_fg: 245,
            empty_fg: 245,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzySelectorStyle {
    pub title: String,
    pub footer: String,
    pub scrim_fg: u8,
    pub scrim_bg: u8,
    pub border_fg: u8,
    pub background_fg: u8,
    pub background_bg: u8,
    pub title_fg: u8,
    pub text_fg: u8,
    pub selected_fg: u8,
    pub selected_bg: u8,
    pub footer_fg: u8,
    pub query_fg: u8,
    pub query_bg: u8,
    pub placeholder_fg: u8,
    pub empty_fg: u8,
}

impl Default for FuzzySelectorStyle {
    fn default() -> Self {
        Self {
            title: "Search".to_owned(),
            footer: "Type Filter  Up/Down Move  Enter Open  Esc Cancel".to_owned(),
            scrim_fg: 236,
            scrim_bg: 234,
            border_fg: 110,
            background_fg: 252,
            background_bg: 236,
            title_fg: 229,
            text_fg: 252,
            selected_fg: 231,
            selected_bg: 31,
            footer_fg: 245,
            query_fg: 231,
            query_bg: 24,
            placeholder_fg: 245,
            empty_fg: 245,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorOverlay {
    pub title: String,
    pub footer: String,
    pub items: Vec<SelectorItem>,
    pub selected: usize,
    pub style: SelectorStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzySelectorOverlay {
    pub title: String,
    pub footer: String,
    pub placeholder: String,
    pub query: String,
    pub items: Vec<SelectorItem>,
    pub selected: usize,
    pub style: FuzzySelectorStyle,
}

pub fn render_selector_overlay(width: u16, height: u16, overlay: &SelectorOverlay) -> Surface {
    let spec = PopupSpec {
        width,
        height,
        title: title_or_default(&overlay.title, &overlay.style.title),
        footer: title_or_default(&overlay.footer, &overlay.style.footer),
        items: &overlay.items,
        selected: overlay.selected,
        colors: PopupColors {
            border_fg: overlay.style.border_fg,
            background_fg: overlay.style.background_fg,
            background_bg: overlay.style.background_bg,
            title_fg: overlay.style.title_fg,
            text_fg: overlay.style.text_fg,
            selected_fg: overlay.style.selected_fg,
            selected_bg: overlay.style.selected_bg,
            footer_fg: overlay.style.footer_fg,
            empty_fg: overlay.style.empty_fg,
        },
        query: None,
        empty_message: "No items",
    };
    render_popup(spec)
}

pub fn render_fuzzy_selector_overlay(width: u16, height: u16, overlay: &FuzzySelectorOverlay) -> Surface {
    let query = if overlay.query.is_empty() {
        QueryRow {
            text: overlay.placeholder.as_str(),
            fg: overlay.style.placeholder_fg,
            bg: overlay.style.query_bg,
            prefix: "/",
        }
    } else {
        QueryRow {
            text: overlay.query.as_str(),
            fg: overlay.style.query_fg,
            bg: overlay.style.query_bg,
            prefix: "/",
        }
    };

    let spec = PopupSpec {
        width,
        height,
        title: title_or_default(&overlay.title, &overlay.style.title),
        footer: title_or_default(&overlay.footer, &overlay.style.footer),
        items: &overlay.items,
        selected: overlay.selected,
        colors: PopupColors {
            border_fg: overlay.style.border_fg,
            background_fg: overlay.style.background_fg,
            background_bg: overlay.style.background_bg,
            title_fg: overlay.style.title_fg,
            text_fg: overlay.style.text_fg,
            selected_fg: overlay.style.selected_fg,
            selected_bg: overlay.style.selected_bg,
            footer_fg: overlay.style.footer_fg,
            empty_fg: overlay.style.empty_fg,
        },
        query: Some(query),
        empty_message: if overlay.query.is_empty() {
            "No items"
        } else {
            "No matches"
        },
    };
    render_popup(spec)
}

#[derive(Clone, Copy)]
struct PopupColors {
    border_fg: u8,
    background_fg: u8,
    background_bg: u8,
    title_fg: u8,
    text_fg: u8,
    selected_fg: u8,
    selected_bg: u8,
    footer_fg: u8,
    empty_fg: u8,
}

#[derive(Clone, Copy)]
struct QueryRow<'a> {
    text: &'a str,
    fg: u8,
    bg: u8,
    prefix: &'a str,
}

struct PopupSpec<'a> {
    width: u16,
    height: u16,
    title: &'a str,
    footer: &'a str,
    items: &'a [SelectorItem],
    selected: usize,
    colors: PopupColors,
    query: Option<QueryRow<'a>>,
    empty_message: &'a str,
}

fn render_popup(spec: PopupSpec<'_>) -> Surface {
    if spec.width < 12 || spec.height < 6 {
        return Surface::new(spec.width, spec.height);
    }

    let longest_item = spec
        .items
        .iter()
        .map(|item| item.label.chars().count())
        .max()
        .unwrap_or(0);
    let query_width = spec
        .query
        .map(|query| query.text.chars().count() + query.prefix.chars().count() + 1)
        .unwrap_or(0);
    let desired_width = (longest_item
        .max(spec.title.len())
        .max(spec.footer.len().min(36))
        .max(query_width)
        + 6) as u16;
    let popup_width = desired_width.min(spec.width.saturating_sub(4).max(18));

    let query_rows = u16::from(spec.query.is_some());
    let chrome_rows = 5u16 + query_rows;
    let desired_height = spec.items.len() as u16 + chrome_rows;
    let min_height = 6 + query_rows;
    let popup_height = desired_height
        .min(spec.height.saturating_sub(2).max(min_height))
        .max(min_height);
    let start_x = spec.width.saturating_sub(popup_width) / 2;
    let start_y = spec.height.saturating_sub(popup_height) / 2;
    let list_start_y = start_y + 3 + query_rows;
    let list_rows = popup_height.saturating_sub(chrome_rows) as usize;
    let max_start = spec.items.len().saturating_sub(list_rows);
    let first_visible = spec.selected.saturating_sub(list_rows / 2).min(max_start);

    let mut surface = Surface::new(spec.width, spec.height);
    fill_rect(
        &mut surface,
        start_x,
        start_y,
        popup_width,
        popup_height,
        b' ',
        vt100::Color::Idx(spec.colors.background_fg),
        vt100::Color::Idx(spec.colors.background_bg),
    );
    draw_box(
        &mut surface,
        start_x,
        start_y,
        popup_width,
        popup_height,
        vt100::Color::Idx(spec.colors.border_fg),
        vt100::Color::Idx(spec.colors.background_bg),
    );
    write_text(
        &mut surface,
        start_x + 2,
        start_y + 1,
        spec.title,
        vt100::Color::Idx(spec.colors.title_fg),
        vt100::Color::Idx(spec.colors.background_bg),
    );
    draw_horizontal_rule(
        &mut surface,
        start_x + 1,
        start_y + 2,
        popup_width.saturating_sub(2),
        vt100::Color::Idx(spec.colors.border_fg),
        vt100::Color::Idx(spec.colors.background_bg),
    );

    if let Some(query) = spec.query {
        fill_rect(
            &mut surface,
            start_x + 1,
            start_y + 3,
            popup_width.saturating_sub(2),
            1,
            b' ',
            vt100::Color::Idx(query.fg),
            vt100::Color::Idx(query.bg),
        );
        write_text(
            &mut surface,
            start_x + 2,
            start_y + 3,
            &(query.prefix.to_owned() + " " + &truncate_text(query.text, popup_width.saturating_sub(6) as usize)),
            vt100::Color::Idx(query.fg),
            vt100::Color::Idx(query.bg),
        );
    }

    let list_width = popup_width.saturating_sub(2);
    let list_bottom = start_y + popup_height.saturating_sub(2);
    if spec.items.is_empty() {
        let row = list_start_y.min(list_bottom.saturating_sub(1));
        fill_rect(
            &mut surface,
            start_x + 1,
            row,
            list_width,
            1,
            b' ',
            vt100::Color::Idx(spec.colors.empty_fg),
            vt100::Color::Idx(spec.colors.background_bg),
        );
        let message = truncate_text(spec.empty_message, popup_width.saturating_sub(6) as usize);
        let message_width = message.chars().count() as u16;
        let message_x = start_x + 1 + list_width.saturating_sub(message_width) / 2;
        write_text(
            &mut surface,
            message_x,
            row,
            &message,
            vt100::Color::Idx(spec.colors.empty_fg),
            vt100::Color::Idx(spec.colors.background_bg),
        );
    } else {
        for row_index in 0..list_rows {
            let item_index = first_visible + row_index;
            let row = list_start_y + row_index as u16;
            if row >= list_bottom {
                break;
            }

            let is_selected = item_index == spec.selected;
            let (fg, bg) = if is_selected {
                (
                    vt100::Color::Idx(spec.colors.selected_fg),
                    vt100::Color::Idx(spec.colors.selected_bg),
                )
            } else {
                (
                    vt100::Color::Idx(spec.colors.text_fg),
                    vt100::Color::Idx(spec.colors.background_bg),
                )
            };

            fill_rect(&mut surface, start_x + 1, row, list_width, 1, b' ', fg, bg);

            let Some(item) = spec.items.get(item_index) else {
                continue;
            };

            let prefix = if is_selected { "> " } else { "  " };
            let label_width = popup_width.saturating_sub(6) as usize;
            let label = truncate_text(&item.label, label_width);
            write_text(&mut surface, start_x + 2, row, &(prefix.to_owned() + &label), fg, bg);
        }
    }

    write_text(
        &mut surface,
        start_x + 2,
        start_y + popup_height.saturating_sub(2),
        &truncate_text(spec.footer, popup_width.saturating_sub(4) as usize),
        vt100::Color::Idx(spec.colors.footer_fg),
        vt100::Color::Idx(spec.colors.background_bg),
    );

    surface
}

pub fn render_scrim(width: u16, height: u16, fg: u8, bg: u8) -> Surface {
    let mut surface = Surface::new(width, height);
    fill_rect(
        &mut surface,
        0,
        0,
        width,
        height,
        b' ',
        vt100::Color::Idx(fg),
        vt100::Color::Idx(bg),
    );
    surface
}

fn title_or_default<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
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
