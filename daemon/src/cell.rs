use std::io::Write;

use crate::{layout::Rect, prelude::*};

pub const CONTENT_LENGTH: usize = 22; // size of vt100 cell content
pub const SPACE: u8 = 0x20;

#[derive(Clone, Debug, PartialEq)]
pub struct RemuxCell {
    pub contents: [u8; CONTENT_LENGTH], // change to fixed array
    pub fg_color: vt100::Color,
    pub bg_color: vt100::Color,
    // will switch to bit-packing later
    // pub bold: bool,
    // pub italic: bool,
    // pub underline: bool,
    //
    // pub is_wide: bool,
    // pub is_wide_spacer: bool,
}

impl Default for RemuxCell {
    fn default() -> Self {
        let mut content = [0u8; CONTENT_LENGTH];
        content[0] = SPACE;
        Self {
            contents: content,
            fg_color: vt100::Color::Default,
            bg_color: vt100::Color::Default,
            // bold: false,
            // italic: false,
            // underline: false,
            // is_wide: false,
            // is_wide_spacer: false,
        }
    }
}

impl RemuxCell {
    pub fn render_diff(
        rect: Rect,
        prev_grid: &Vec<Vec<RemuxCell>>,
        curr_grid: &Vec<Vec<RemuxCell>>,
        force_rerender: bool,
    ) -> Vec<u8> {
        let mut output = Vec::new();
        let mut current_fg_color = vt100::Color::Default;
        let mut current_bg_color = vt100::Color::Default;

        let mut cursor_y = 0;
        let mut cursor_x = 0;
        let mut cursor_invalid = true;

        let rows = rect.height as usize;
        let cols = rect.width as usize;

        for r in 0..rows {
            let mut last_char_index = 0;
            for c in (0..cols).rev() {
                let cell = &curr_grid[r][c];
                if cell.contents[0] != SPACE || cell.bg_color != vt100::Color::Default {
                    last_char_index = c + 1;
                    break;
                }
            }

            for c in 0..cols {
                if c >= last_char_index {
                    let prev_has_content = if r < prev_grid.len() && c < prev_grid[0].len() {
                        let prev_cell = &prev_grid[r][c];
                        prev_cell.contents[0] != SPACE || prev_cell.bg_color != vt100::Color::Default
                    } else {
                        true
                    };

                    if force_rerender || prev_has_content {
                        if current_bg_color != vt100::Color::Default {
                            Self::write_sgr_color(&mut output, vt100::Color::Default, false).unwrap();
                            current_bg_color = vt100::Color::Default;
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

                if !force_rerender && r < prev_grid.len() && c < prev_grid[0].len() {
                    if cell.eq(&prev_grid[r][c]) {
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

                if cell.fg_color != current_fg_color {
                    Self::write_sgr_color(&mut output, cell.fg_color, true).unwrap();
                    current_fg_color = cell.fg_color;
                }

                if cell.bg_color != current_bg_color {
                    Self::write_sgr_color(&mut output, cell.bg_color, false).unwrap();
                    current_bg_color = cell.bg_color;
                }

                let data = &cell.contents;
                let len = data.iter().position(|&x| x == 0).unwrap_or(data.len());
                output.extend_from_slice(&data[..len]);

                cursor_x += 1;
            }
        }
        output.extend_from_slice(b"\x1b[0m");
        output
    }

    fn write_sgr_color(output: &mut Vec<u8>, color: vt100::Color, is_fg: bool) -> Result<()> {
        match color {
            vt100::Color::Default => {
                let code = if is_fg { 39 } else { 49 };
                write!(output, "\x1b[{}m", code).unwrap();
            }
            vt100::Color::Idx(i) => {
                let prefix = if is_fg { 38 } else { 48 };
                write!(output, "\x1b[{};5;{}m", prefix, i).unwrap();
            }
            vt100::Color::Rgb(r, g, b) => {
                let prefix = if is_fg { 38 } else { 48 };
                write!(output, "\x1b[{};2;{};{};{}m", prefix, r, g, b).unwrap();
            }
        }
        Ok(())
    }
}
