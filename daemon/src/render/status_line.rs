use crate::{cell::RemuxCell, render::surface::Surface};

#[derive(Clone, Debug, Default)]
pub struct StatusLineTemplate {
    pub enabled: bool,
    pub a: Vec<String>,
    pub b: Vec<String>,
    pub c: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct StatusLineRenderer {
    template: StatusLineTemplate,
}

impl StatusLineRenderer {
    pub fn from_template(template: StatusLineTemplate) -> Self {
        Self { template }
    }

    pub fn enabled(&self) -> bool {
        self.template.enabled
    }

    pub fn render(&self, width: u16, active_session_name: Option<&str>) -> Surface {
        let mut surface = Surface::new(width, 1);
        if !self.template.enabled || width == 0 {
            return surface;
        }

        self.paint_background(&mut surface);

        let left = self.resolve_items(&self.template.a, active_session_name);
        let center = self.resolve_items(&self.template.b, active_session_name);
        let right = self.resolve_items(&self.template.c, active_session_name);

        let left = truncate_text(&left, usize::from(width.saturating_sub(2)));
        let left_width = text_width(&left) as u16;
        self.paint_text(
            &mut surface,
            0,
            &left,
            vt100::Color::Idx(231),
            vt100::Color::Idx(24),
        );

        let mut right_start = width;
        let right_max = width.saturating_sub(left_width.saturating_add(2));
        let right = truncate_text(&right, usize::from(right_max));
        let right_width = text_width(&right) as u16;
        if right_width > 0 && right_width.saturating_add(left_width).saturating_add(1) <= width {
            right_start = width.saturating_sub(right_width);
            self.paint_text(
                &mut surface,
                right_start,
                &right,
                vt100::Color::Idx(187),
                vt100::Color::Idx(236),
            );
        }

        let center_left_bound = left_width.saturating_add(2);
        let center_right_bound = right_start.saturating_sub(2);
        if center_right_bound > center_left_bound {
            let center_slot_width = center_right_bound - center_left_bound + 1;
            let center = truncate_text(&center, usize::from(center_slot_width));
            let center_width = text_width(&center) as u16;
            let minimum_clearance = 6;

            if center_width > 0
                && center_width <= center_slot_width
                && center_slot_width >= center_width.saturating_add(minimum_clearance)
            {
                let center_start = center_left_bound + (center_slot_width - center_width) / 2;
                self.paint_text(
                    &mut surface,
                    center_start,
                    &center,
                    vt100::Color::Idx(153),
                    vt100::Color::Idx(236),
                );
            }
        }

        surface
    }

    fn resolve_items(&self, items: &[String], active_session_name: Option<&str>) -> String {
        items
            .iter()
            .map(|item| {
                if item == "active-session" {
                    active_session_name.unwrap_or_default().to_owned()
                } else {
                    item.clone()
                }
            })
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn paint_background(&self, surface: &mut Surface) {
        for x in 0..surface.width() {
            let mut cell = RemuxCell::default();
            cell.set_fg_color(vt100::Color::Idx(252));
            cell.set_bg_color(vt100::Color::Idx(236));
            surface.paint_cell(x, 0, cell);
        }
    }

    fn paint_text(&self, surface: &mut Surface, start_x: u16, text: &str, fg: vt100::Color, bg: vt100::Color) {
        for (offset, byte) in text.bytes().enumerate() {
            let x = start_x + offset as u16;
            if x >= surface.width() {
                break;
            }

            let mut cell = RemuxCell::default();
            cell.set_content(&[byte]);
            cell.set_fg_color(fg);
            cell.set_bg_color(bg);
            surface.paint_cell(x, 0, cell);
        }
    }
}

fn text_width(text: &str) -> usize {
    text.chars().count()
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text_width(text) <= max_chars {
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
