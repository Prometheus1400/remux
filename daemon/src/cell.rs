use crate::prelude::*;
use std::io::Write;
const CONTENT_LENGTH: usize = 22; // size of vt100 cell content

#[derive(Clone, Debug, PartialEq)]
pub struct RemuxCell {
    pub contents: Vec<u8>, // change to fixed array
    pub fg_color: vt100::Color,
    pub bg_color: vt100::Color,

    // will switch to bit-packing later
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    
    pub is_wide: bool,
    pub is_wide_spacer: bool,
}

impl Default for RemuxCell {
    fn default() -> Self {
        Self {
            contents: Default::default(),
            fg_color: vt100::Color::Default,
            bg_color: vt100::Color::Default,
            bold: false,
            italic: false,
            underline: false,
            is_wide: false,
            is_wide_spacer: false,
        }
    }
}

impl RemuxCell {
    pub fn render_diff(prev_grid: &Vec<Vec<RemuxCell>>, curr_grid: &Vec<Vec<RemuxCell>>, is_rerender: bool) -> Vec<u8> {
        let mut output = Vec::new();
        let mut current_fg_color = vt100::Color::Default;
        let mut current_bg_color = vt100::Color::Default;

        let mut cursor_y = 0;
        let mut cursor_x = 0; 
        let mut cursor_invalid = true; // Force a move on first draw

        for (r, row) in curr_grid.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                if cell.is_wide_spacer {
                    continue;
                }

                // if the cell hasn't changed, skip it.
                if !is_rerender && r < prev_grid.len() && c < prev_grid[0].len() {
                    if cell.eq(&prev_grid[r][c]) {
                        continue;
                    }
                }

                // if the cursor is not currently at this cell, move it there
                if cursor_invalid || cursor_y != r || cursor_x != c {
                    write!(output, "\x1b[{};{}H", r + 1, c + 1).unwrap(); // terminals are 1-indexed
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

                cursor_x += if cell.is_wide { 2 } else { 1 };
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
