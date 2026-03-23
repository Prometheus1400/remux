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
    pub window_active_fg: u8,
    pub window_active_bg: u8,
    pub window_inactive_fg: u8,
    pub window_inactive_bg: u8,
    pub window_muted_fg: u8,
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
            window_active_fg: 231,
            window_active_bg: 24,
            window_inactive_fg: 252,
            window_inactive_bg: 236,
            window_muted_fg: 245,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarItem {
    Text(String),
    ActiveSession,
    WindowList,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarSpec {
    pub enabled: bool,
    pub left: Vec<BarItem>,
    pub center: Vec<BarItem>,
    pub right: Vec<BarItem>,
    pub style: BarStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowTab {
    pub index: usize,
    pub name: String,
    pub is_active: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarRenderState {
    pub active_session_name: Option<String>,
    pub windows: Vec<WindowTab>,
}

#[derive(Clone, Debug)]
pub struct BarRenderer {
    spec: BarSpec,
}

#[derive(Clone, Debug)]
struct BarToken {
    text: String,
    fg: u8,
    bg: u8,
    is_active: bool,
}

#[derive(Clone, Copy, Debug)]
enum Density {
    Full,
    Compact,
    Minimal,
}

impl BarRenderer {
    pub fn from_spec(spec: BarSpec) -> Self {
        Self { spec }
    }

    pub fn render(&self, width: u16, state: &BarRenderState) -> Surface {
        let mut surface = Surface::new(width, 1);
        if !self.spec.enabled || width == 0 {
            return surface;
        }

        self.paint_background(&mut surface);

        if self.spec.center.is_empty() {
            self.render_without_center(&mut surface, width, state);
            return surface;
        }

        for density in [Density::Full, Density::Compact, Density::Minimal] {
            let left = self.expand_section(&self.spec.left, Section::Left, density, state);
            let right_items = match density {
                Density::Full => self.spec.right.as_slice(),
                Density::Compact => self
                    .spec
                    .right
                    .split_first()
                    .map(|(first, _)| std::slice::from_ref(first))
                    .unwrap_or(&[]),
                Density::Minimal => &[],
            };

            let right = self.expand_section(right_items, Section::Right, density, state);
            let center = self.expand_section(&self.spec.center, Section::Center, density, state);
            let fitted_center = fit_tokens(&center, width as usize, active_token_index(&center, &self.spec.center));

            let has_center = !fitted_center.is_empty();
            let center_width = tokens_width(&fitted_center) as u16;
            let center_fits = !center.is_empty() && center_width > 0;
            let should_use_mode =
                self.spec.center.is_empty() || center.is_empty() || center_fits || matches!(density, Density::Minimal);

            if !should_use_mode {
                continue;
            }

            let center_start = width.saturating_sub(center_width) / 2;
            let center_end = center_start.saturating_add(center_width);

            if center_start > 0 {
                self.paint_tokens_left(&mut surface, 0, center_start, &left);
            }

            if has_center {
                self.paint_tokens_left(&mut surface, center_start, center_end, &fitted_center);
            }

            if center_end < width {
                self.paint_tokens_right(&mut surface, center_end, width, &right);
            }

            return surface;
        }

        surface
    }

    fn render_without_center(&self, surface: &mut Surface, width: u16, state: &BarRenderState) {
        let left = self.expand_section(&self.spec.left, Section::Left, Density::Full, state);
        let left_width = self.paint_tokens_left(surface, 0, width, &left);

        for density in [Density::Full, Density::Compact, Density::Minimal] {
            let right_items = match density {
                Density::Full => self.spec.right.as_slice(),
                Density::Compact => self
                    .spec
                    .right
                    .split_first()
                    .map(|(first, _)| std::slice::from_ref(first))
                    .unwrap_or(&[]),
                Density::Minimal => &[],
            };
            let right = self.expand_section(right_items, Section::Right, density, state);
            let right_width = tokens_width(&right) as u16;
            let right_gap = u16::from(!right.is_empty() && right_width < width);
            let min_right_start = left_width.saturating_add(u16::from(!left.is_empty()));
            let right_start =
                if right_width == 0 || min_right_start.saturating_add(right_gap).saturating_add(right_width) > width {
                    width
                } else {
                    width.saturating_sub(right_width)
                };

            if right_start < width {
                self.paint_tokens_right(surface, right_start, width, &right);
            }
            return;
        }
    }

    fn expand_section(
        &self,
        items: &[BarItem],
        section: Section,
        density: Density,
        state: &BarRenderState,
    ) -> Vec<BarToken> {
        let mut tokens = Vec::new();

        for item in items {
            match item {
                BarItem::Text(text) if !text.is_empty() => {
                    tokens.push(section.text_token(text.clone(), &self.spec.style));
                }
                BarItem::ActiveSession => {
                    if let Some(name) = state.active_session_name.clone().filter(|name| !name.is_empty()) {
                        tokens.push(section.session_token(name, &self.spec.style));
                    }
                }
                BarItem::WindowList => {
                    tokens.extend(
                        state
                            .windows
                            .iter()
                            .map(|window| window_token(window, density, &self.spec.style)),
                    );
                }
                _ => {}
            }
        }

        tokens
    }

    fn paint_background(&self, surface: &mut Surface) {
        for x in 0..surface.width() {
            let mut cell = RemuxCell::default();
            cell.set_fg_color(vt100::Color::Idx(self.spec.style.background_fg));
            cell.set_bg_color(vt100::Color::Idx(self.spec.style.background_bg));
            surface.paint_cell(x, 0, cell);
        }
    }

    fn paint_tokens_left(&self, surface: &mut Surface, start_x: u16, end_x: u16, tokens: &[BarToken]) -> u16 {
        self.paint_tokens(surface, start_x, end_x, tokens, Alignment::Left)
    }

    fn paint_tokens_right(&self, surface: &mut Surface, start_x: u16, end_x: u16, tokens: &[BarToken]) -> u16 {
        self.paint_tokens(surface, start_x, end_x, tokens, Alignment::Right)
    }

    fn paint_tokens(
        &self,
        surface: &mut Surface,
        start_x: u16,
        end_x: u16,
        tokens: &[BarToken],
        alignment: Alignment,
    ) -> u16 {
        let max_chars = usize::from(end_x.saturating_sub(start_x));
        let cells = flatten_token_cells(tokens);
        let cells = fit_cells(cells, max_chars, alignment);

        let mut x = match alignment {
            Alignment::Left => start_x,
            Alignment::Right => end_x.saturating_sub(cells.len() as u16),
        };
        for (ch, fg, bg) in cells {
            if x >= end_x {
                break;
            }

            let mut cell = RemuxCell::default();
            cell.set_content(&[ch as u8]);
            cell.set_fg_color(vt100::Color::Idx(fg));
            cell.set_bg_color(vt100::Color::Idx(bg));
            surface.paint_cell(x, 0, cell);
            x += 1;
        }

        x.saturating_sub(start_x)
    }
}

#[derive(Clone, Copy, Debug)]
enum Alignment {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
enum Section {
    Left,
    Center,
    Right,
}

impl Section {
    fn fg(self, style: &BarStyle) -> u8 {
        match self {
            Self::Left => style.left_fg,
            Self::Center => style.center_fg,
            Self::Right => style.right_fg,
        }
    }

    fn bg(self, style: &BarStyle) -> u8 {
        match self {
            Self::Left => style.left_bg,
            Self::Center => style.center_bg,
            Self::Right => style.right_bg,
        }
    }

    fn text_token(self, text: String, style: &BarStyle) -> BarToken {
        BarToken {
            text: format!(" {} ", text),
            fg: self.fg(style),
            bg: self.bg(style),
            is_active: false,
        }
    }

    fn session_token(self, text: String, style: &BarStyle) -> BarToken {
        BarToken {
            text: format!(" {} ", text),
            fg: self.fg(style),
            bg: self.bg(style),
            is_active: false,
        }
    }
}

fn window_token(window: &WindowTab, density: Density, style: &BarStyle) -> BarToken {
    let label = match density {
        Density::Full => format!("{} {}", window.index, window.name),
        Density::Compact => format!("{} {}", window.index, abbreviate(&window.name, 8)),
        Density::Minimal => window.index.to_string(),
    };
    let fg = if window.is_active {
        style.window_active_fg
    } else if matches!(density, Density::Minimal) {
        style.window_muted_fg
    } else {
        style.window_inactive_fg
    };
    let bg = if window.is_active {
        style.window_active_bg
    } else {
        style.window_inactive_bg
    };

    BarToken {
        text: format!(" {} ", label),
        fg,
        bg,
        is_active: window.is_active,
    }
}

fn active_token_index(tokens: &[BarToken], items: &[BarItem]) -> Option<usize> {
    if !items.iter().any(|item| matches!(item, BarItem::WindowList)) {
        return None;
    }

    tokens.iter().position(|token| token.is_active)
}

fn tokens_width(tokens: &[BarToken]) -> usize {
    tokens.iter().map(|token| text_width(&token.text)).sum()
}

fn flatten_token_cells(tokens: &[BarToken]) -> Vec<(char, u8, u8)> {
    let mut cells = Vec::new();
    for token in tokens {
        for ch in token.text.chars() {
            cells.push((ch, token.fg, token.bg));
        }
    }
    cells
}

fn fit_tokens(tokens: &[BarToken], max_width: usize, active_index: Option<usize>) -> Vec<BarToken> {
    if tokens.is_empty() || max_width == 0 {
        return Vec::new();
    }
    if tokens_width(tokens) <= max_width {
        return tokens.to_vec();
    }

    if let Some(active_index) = active_index.filter(|index| *index < tokens.len()) {
        let active = tokens[active_index].clone();
        if text_width(&active.text) > max_width {
            return vec![truncate_token(active, max_width)];
        }

        let mut chosen = vec![active_index];
        let mut left = active_index.checked_sub(1);
        let mut right = (active_index + 1 < tokens.len()).then_some(active_index + 1);
        let mut used_width = text_width(&tokens[active_index].text);
        let mut pick_left = true;

        while left.is_some() || right.is_some() {
            let next = if pick_left {
                left.take().map(|index| ("left", index))
            } else {
                right.take().map(|index| ("right", index))
            }
            .or_else(|| left.take().map(|index| ("left", index)))
            .or_else(|| right.take().map(|index| ("right", index)));

            let Some((side, index)) = next else {
                break;
            };
            let width = text_width(&tokens[index].text);
            if used_width + width > max_width {
                if side == "left" {
                    left = None;
                } else {
                    right = None;
                }
            } else {
                chosen.push(index);
                used_width += width;
                if side == "left" {
                    left = index.checked_sub(1);
                } else {
                    right = (index + 1 < tokens.len()).then_some(index + 1);
                }
            }
            pick_left = !pick_left;
        }

        chosen.sort_unstable();
        return chosen
            .into_iter()
            .filter_map(|index| tokens.get(index).cloned())
            .collect();
    }

    let mut fitted = Vec::new();
    let mut used_width = 0;
    for token in tokens {
        let width = text_width(&token.text);
        if fitted.is_empty() && width > max_width {
            fitted.push(truncate_token(token.clone(), max_width));
            break;
        }
        if used_width + width > max_width {
            break;
        }
        fitted.push(token.clone());
        used_width += width;
    }
    fitted
}

fn truncate_token(mut token: BarToken, max_width: usize) -> BarToken {
    token.text = truncate_text(&token.text, max_width);
    token
}

fn fit_cells(cells: Vec<(char, u8, u8)>, max_chars: usize, alignment: Alignment) -> Vec<(char, u8, u8)> {
    if cells.len() <= max_chars {
        return cells;
    }
    if max_chars == 0 {
        return Vec::new();
    }
    if max_chars <= 3 {
        let (_, fg, bg) = cells.first().copied().unwrap_or((' ', 0, 0));
        return vec![('.', fg, bg); max_chars];
    }

    match alignment {
        Alignment::Left => {
            let mut cells = cells;
            let (_, fg, bg) = cells
                .get(max_chars.saturating_sub(4))
                .copied()
                .or_else(|| cells.first().copied())
                .unwrap_or((' ', 0, 0));
            cells.truncate(max_chars - 3);
            cells.extend(std::iter::repeat_n(('.', fg, bg), 3));
            cells
        }
        Alignment::Right => {
            let keep_start = cells.len().saturating_sub(max_chars - 3);
            let (_, fg, bg) = cells
                .get(keep_start.saturating_sub(1))
                .copied()
                .or_else(|| cells.first().copied())
                .unwrap_or((' ', 0, 0));
            let mut truncated = vec![('.', fg, bg); 3];
            truncated.extend(cells.into_iter().skip(keep_start));
            truncated
        }
    }
}

fn abbreviate(text: &str, max_chars: usize) -> String {
    truncate_text(text, max_chars)
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

fn text_width(text: &str) -> usize {
    text.chars().count()
}
