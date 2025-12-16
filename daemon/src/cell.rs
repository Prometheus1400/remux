use std::io::Write;

use bytes::buf;

use crate::layout::Rect;

pub const CONTENT_LENGTH: usize = 22; // size of vt100 cell content
pub const SPACE: u8 = 0x20;

// attributes
const BOLD: u8 = 0b0000_0001;
const ITALIC: u8 = 0b0000_0010;
const UNDERLINE: u8 = 0b0000_0100;
const INVERSE: u8 = 0b0000_1000;
const WIDE: u8 = 0b0001_0000;
const WIDE_SPACER: u8 = 0b0010_0000;

const LEN_BITS: u8 = 0b0001_1111;

#[repr(C)]
#[derive(Clone, Debug)]
pub struct RemuxCell {
    contents: [u8; CONTENT_LENGTH], // 22 Bytes
    len: u8,                        // 1 byte
    attributes: u8,                 // 1 byte

    // format: [Type: u8] [R: u8] [G: u8] [B: u8]
    fg_color: u32, // 4 bytes
    bg_color: u32, // 4 bytes
}

// verify each cell is 32 bytes
const _: () = assert!(std::mem::size_of::<RemuxCell>() == 32);

impl Default for RemuxCell {
    fn default() -> Self {
        let mut content = [0u8; CONTENT_LENGTH];
        content[0] = SPACE;
        Self {
            contents: content,
            len: 1,
            fg_color: Self::color_to_bytes(vt100::Color::Default),
            bg_color: Self::color_to_bytes(vt100::Color::Default),
            attributes: 0,
        }
    }
}

impl PartialEq for RemuxCell {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        if self.fg_color != other.fg_color || self.bg_color != other.bg_color {
            return false;
        }
        if self.attributes != other.attributes {
            return false;
        }
        let len = self.len();
        self.contents[..len] == other.contents[..len]
    }
}

impl RemuxCell {
    pub fn render_diff(
        rect: Rect,
        prev_grid: &Vec<Vec<RemuxCell>>,
        curr_grid: &Vec<Vec<RemuxCell>>,
        force_rerender: bool,
    ) -> Vec<u8> {
        let mut output = Vec::with_capacity((rect.width as usize * rect.height as usize) * 4);

        let default_color_bytes = Self::color_to_bytes(vt100::Color::Default);
        let mut current_fg_color = default_color_bytes;
        let mut current_bg_color = default_color_bytes;
        let mut current_attributes: u8 = 0;

        let mut cursor_y = 0;
        let mut cursor_x = 0;
        let mut cursor_invalid = true;

        let rows = rect.height as usize;
        let cols = rect.width as usize;

        const VISIBLE_ON_WHITESPACE: u8 = INVERSE | UNDERLINE;

        for r in 0..rows {
            let mut last_char_index = 0;
            for c in (0..cols).rev() {
                let cell = &curr_grid[r][c];

                let is_visually_empty = cell.contents[0] == SPACE
                    && cell.bg_color == default_color_bytes
                    && (cell.attributes & VISIBLE_ON_WHITESPACE) == 0;

                if !is_visually_empty {
                    last_char_index = c + 1;
                    break;
                }
            }

            for c in 0..cols {
                if c >= last_char_index {
                    let prev_has_content = if r < prev_grid.len() && c < prev_grid[0].len() {
                        let prev_cell = &prev_grid[r][c];
                        let prev_bg_default = prev_cell.bg_color == default_color_bytes;

                        prev_cell.contents[0] != SPACE
                            || !prev_bg_default
                            || (prev_cell.attributes & VISIBLE_ON_WHITESPACE) != 0
                    } else {
                        true
                    };

                    if force_rerender || prev_has_content {
                        if current_bg_color != default_color_bytes || current_attributes != 0 {
                            output.extend_from_slice(b"\x1b[0m");
                            current_bg_color = default_color_bytes;
                            current_fg_color = default_color_bytes;
                            current_attributes = 0;
                        }

                        let target_x = rect.x + 1 + c as u16;
                        let target_y = rect.y + 1 + r as u16;
                        write!(output, "\x1b[{};{}H", target_y, target_x).unwrap();

                        let count = cols - c;
                        write!(output, "\x1b[{}X", count).unwrap();

                        cursor_invalid = true;
                        break;
                    } else {
                        continue;
                    }
                }

                let cell = &curr_grid[r][c];

                if cell.has_attribute(WIDE_SPACER) {
                    continue;
                }

                if !force_rerender && r < prev_grid.len() && c < prev_grid[0].len() {
                    if cell == &prev_grid[r][c] {
                        continue;
                    }
                }

                let target_x = rect.x + 1 + c as u16;
                let target_y = rect.y + 1 + r as u16;
                if cursor_invalid || cursor_y != r || cursor_x != c {
                    write!(output, "\x1b[{};{}H", target_y, target_x).unwrap();
                    cursor_y = r;
                    cursor_x = c;
                    cursor_invalid = false;
                }

                if cell.attributes != current_attributes {
                    output.extend_from_slice(b"\x1b[0");
                    Self::get_attributes_to_ansi(&mut output, cell);
                    output.push(b'm');

                    current_attributes = cell.attributes;

                    current_bg_color = default_color_bytes;
                    current_fg_color = default_color_bytes;

                    if cell.fg_color != current_fg_color {
                        Self::write_packed_color(&mut output, cell.fg_color, true);
                    }

                    if cell.bg_color != current_bg_color {
                        Self::write_packed_color(&mut output, cell.bg_color, false);
                    }
                } else {
                    if cell.fg_color != current_fg_color {
                        Self::write_packed_color(&mut output, cell.fg_color, true);
                        current_fg_color = cell.fg_color;
                    }
                    if cell.bg_color != current_bg_color {
                        Self::write_packed_color(&mut output, cell.bg_color, false);
                        current_bg_color = cell.bg_color;
                    }
                }

                let data = &cell.contents;
                let len = data.iter().position(|&x| x == 0).unwrap_or(data.len());
                output.extend_from_slice(&data[..len]);

                cursor_x += if cell.has_attribute(WIDE) { 2 } else { 1 };
            }
        }
        output.extend_from_slice(b"\x1b[0m");
        output
    }

    fn color_to_bytes(c: vt100::Color) -> u32 {
        match c {
            vt100::Color::Default => 0,
            vt100::Color::Idx(i) => 0x0100_0000 | ((i as u32) << 16),
            vt100::Color::Rgb(r, g, b) => 0x0200_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32),
        }
    }

    fn write_packed_color(output: &mut Vec<u8>, color: u32, is_fg: bool) {
        let color_type = color >> 24;
        let val1 = (color >> 16) & 0xFF;
        let val2 = (color >> 8) & 0xFF;
        let val3 = color & 0xFF;

        match color_type {
            0 => {
                let code = if is_fg { 39 } else { 49 };
                write!(output, "\x1b[{}m", code).unwrap();
            }
            1 => {
                let prefix = if is_fg { 38 } else { 48 };
                write!(output, "\x1b[{};5;{}m", prefix, val1).unwrap();
            }
            2 => {
                let prefix = if is_fg { 38 } else { 48 };
                write!(output, "\x1b[{};2;{};{};{}m", prefix, val1, val2, val3).unwrap();
            }
            _ => {}
        }
    }

    fn get_attributes_to_ansi(buffer: &mut Vec<u8>, cell: &RemuxCell) {
        if cell.has_attribute(BOLD) {
            buffer.extend_from_slice(b";1");
        }
        if cell.has_attribute(ITALIC) {
            buffer.extend_from_slice(b";3");
        }
        if cell.has_attribute(UNDERLINE) {
            buffer.extend_from_slice(b";4");
        }
        if cell.has_attribute(INVERSE) {
            buffer.extend_from_slice(b";7");
        }
    }
}

// setters and getters
impl RemuxCell {
    pub fn set_attributes_from_vt100(&mut self, cell: &vt100::Cell) {
        self.set_attribute(BOLD, cell.bold());
        self.set_attribute(ITALIC, cell.italic());
        self.set_attribute(UNDERLINE, cell.underline());
        self.set_attribute(INVERSE, cell.inverse());
        self.set_attribute(WIDE, cell.is_wide());
        self.set_attribute(WIDE_SPACER, cell.is_wide_continuation());
    }
    pub fn set_attribute(&mut self, attribute: u8, enable: bool) {
        if enable {
            self.attributes |= attribute;
        } else {
            self.attributes &= !attribute;
        }
    }

    pub fn has_attribute(&self, attribute: u8) -> bool {
        (self.attributes & attribute) != 0
    }

    pub fn len(&self) -> usize {
        usize::from(self.len & LEN_BITS)
    }

    pub fn set_content(&mut self, contents: &[u8]) {
        if contents.is_empty() || contents[0] == 0 {
            self.contents[0] = SPACE;
        } else {
            let len = contents.len().min(CONTENT_LENGTH);
            self.contents[..len].copy_from_slice(&contents[..len]);
            self.len = len as u8;
        }
    }

    pub fn set_fg_color(&mut self, color: vt100::Color) {
        self.fg_color = Self::color_to_bytes(color);
    }

    pub fn set_bg_color(&mut self, color: vt100::Color) {
        self.bg_color = Self::color_to_bytes(color);
    }
}
