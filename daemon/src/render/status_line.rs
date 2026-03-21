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

        let a = self.resolve_items(&self.template.a, active_session_name);
        let b = self.resolve_items(&self.template.b, active_session_name);
        let c = self.resolve_items(&self.template.c, active_session_name);

        self.paint_text(&mut surface, 0, &a);

        let center_start = width.saturating_sub(b.len() as u16) / 2;
        self.paint_text(&mut surface, center_start, &b);

        let right_start = width.saturating_sub(c.len() as u16);
        self.paint_text(&mut surface, right_start, &c);

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

    fn paint_text(&self, surface: &mut Surface, start_x: u16, text: &str) {
        for (offset, byte) in text.bytes().enumerate() {
            let x = start_x + offset as u16;
            if x >= surface.width() {
                break;
            }

            let mut cell = RemuxCell::default();
            cell.set_content(&[byte]);
            surface.paint_cell(x, 0, cell);
        }
    }
}
