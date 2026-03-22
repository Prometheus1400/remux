use crate::{cell::RemuxCell, render::surface::Surface};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarStyle {
    pub background_fg: u8,
    pub background_bg: u8,
    pub left_fg: u8,
    pub left_bg: u8,
    pub center_fg: u8,
    pub center_bg: u8,
    pub right_fg: u8,
    pub right_bg: u8,
}

impl Default for BarStyle {
    fn default() -> Self {
        Self {
            background_fg: 252,
            background_bg: 236,
            left_fg: 231,
            left_bg: 24,
            center_fg: 153,
            center_bg: 236,
            right_fg: 187,
            right_bg: 236,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarSpec {
    pub enabled: bool,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    pub style: BarStyle,
}

#[derive(Clone, Debug)]
pub struct BarRenderer {
    spec: BarSpec,
}

impl BarRenderer {
    pub fn from_spec(spec: BarSpec) -> Self {
        Self { spec }
    }

    pub fn enabled(&self) -> bool {
        self.spec.enabled
    }

    pub fn render(&self, width: u16) -> Surface {
        let mut surface = Surface::new(width, 1);
        if !self.spec.enabled || width == 0 {
            return surface;
        }

        self.paint_background(&mut surface);

        let left = self.spec.left.join(" | ");
        let center = self.spec.center.join(" | ");
        let right = self.spec.right.join(" | ");

        let left = truncate_text(&left, usize::from(width.saturating_sub(2)));
        let left_width = text_width(&left) as u16;
        self.paint_text(
            &mut surface,
            0,
            &left,
            vt100::Color::Idx(self.spec.style.left_fg),
            vt100::Color::Idx(self.spec.style.left_bg),
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
                vt100::Color::Idx(self.spec.style.right_fg),
                vt100::Color::Idx(self.spec.style.right_bg),
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
                    vt100::Color::Idx(self.spec.style.center_fg),
                    vt100::Color::Idx(self.spec.style.center_bg),
                );
            }
        }

        surface
    }

    fn paint_background(&self, surface: &mut Surface) {
        for x in 0..surface.width() {
            let mut cell = RemuxCell::default();
            cell.set_fg_color(vt100::Color::Idx(self.spec.style.background_fg));
            cell.set_bg_color(vt100::Color::Idx(self.spec.style.background_bg));
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
