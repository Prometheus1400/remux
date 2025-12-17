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
    fg_color: u32,                  // 4 bytes format: [Type: u8] [R: u8] [G: u8] [B: u8]
    bg_color: u32,                  // 4 bytes format: [Type: u8] [R: u8] [G: u8] [B: u8]
}

// verify each cell is 32 bytes
const _: () = assert!(std::mem::size_of::<RemuxCell>() == 32);

// default cell has 1 space and no styles
impl Default for RemuxCell {
    fn default() -> Self {
        let mut content = [0u8; CONTENT_LENGTH];
        content[0] = SPACE;
        Self {
            contents: content,
            len: 1,
            fg_color: color_to_bytes(vt100::Color::Default),
            bg_color: color_to_bytes(vt100::Color::Default),
            attributes: 0,
        }
    }
}

// equality checks integers first, then compares contents
impl PartialEq for RemuxCell {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len
            || self.fg_color != other.fg_color
            || self.bg_color != other.bg_color
            || self.attributes != other.attributes
        {
            return false;
        }

        self.contents == other.contents
    }
}

impl RemuxCell {
    pub fn render_diff(
        rect: Rect,
        prev_grid: &[RemuxCell], // 2D grid flattened to 1D
        curr_grid: &[RemuxCell], // 2D grid flattened to 1D
        force_rerender: bool,
    ) -> Vec<u8> {
        // average 10 bytes per cell
        let mut output = Vec::with_capacity((rect.width as usize * rect.height as usize) * 10);

        // set intitial values
        let default_color_bytes = color_to_bytes(vt100::Color::Default);
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
            // first pass is a backwards pass to find the last index with a visible change
            // we use this to clear empty space
            let mut last_char_index = 0;
            for c in (0..cols).rev() {
                let cell = &curr_grid[r * cols + c];

                let is_visually_empty = cell.contents[0] == SPACE
                    && cell.bg_color == default_color_bytes
                    && (cell.attributes & VISIBLE_ON_WHITESPACE) == 0;

                if !is_visually_empty {
                    last_char_index = c + 1;
                    break;
                }
            }

            // second pass is a forward pass
            for c in 0..cols {
                // if we are past the last useful char, we check if there is content already there
                // if there is, we check if it was visible content
                if c >= last_char_index {
                    let prev_has_content = if r < prev_grid.len() && c < prev_grid[0].len() {
                        let prev_cell = &prev_grid[r * cols + c];
                        let prev_bg_default = prev_cell.bg_color == default_color_bytes;

                        prev_cell.contents[0] != SPACE
                            || !prev_bg_default
                            || (prev_cell.attributes & VISIBLE_ON_WHITESPACE) != 0
                    } else {
                        true
                    };

                    // if its a full rerender, or if there was content there previously,
                    // we reset everything, move the cursor, remove all characters to the end of
                    // the row, then set the cursor as invalid
                    if force_rerender || prev_has_content {
                        if current_bg_color != default_color_bytes || current_attributes != 0 {
                            output.extend_from_slice(b"\x1b[0m");
                            current_bg_color = default_color_bytes;
                            current_fg_color = default_color_bytes;
                            current_attributes = 0;
                        }

                        let target_x = rect.x + 1 + c as u16;
                        let target_y = rect.y + 1 + r as u16;
                        set_cursor_position(&mut output, target_x, target_y);

                        let count = cols - c;
                        output.extend_from_slice(b"\x1b[");
                        push_u16(&mut output, count as u16);
                        output.push(b'X');

                        cursor_invalid = true;
                        break;
                    } else {
                        continue;
                    }
                }

                // if we are at a useful cell, grab it
                // then we check if its a spacer, if it is we skip it
                let cell = &curr_grid[r * cols + c];
                if cell.has_attribute(WIDE_SPACER) {
                    continue;
                }

                // if this is a NOT full rerender and its within the bound of the previous
                // grid, we do an equality check and skip processing if nothing has changed
                if !force_rerender && r < prev_grid.len() && c < prev_grid[0].len() {
                    if cell == &prev_grid[r * cols + c] {
                        continue;
                    }
                }

                // if we are not at the correct place already, move the cursor
                let target_x = rect.x + 1 + c as u16;
                let target_y = rect.y + 1 + r as u16;
                if cursor_invalid || cursor_y != r || cursor_x != c {
                    set_cursor_position(&mut output, target_x, target_y);
                    cursor_y = r;
                    cursor_x = c;
                    cursor_invalid = false;
                }

                // check if the current attributes are the same as the new ones,
                // if they are, just check colors, otherwise change the attributes
                if cell.attributes != current_attributes {
                    output.extend_from_slice(b"\x1b[0");
                    Self::get_attributes_to_ansi(&mut output, cell);
                    output.push(b'm');

                    current_attributes = cell.attributes;

                    current_bg_color = default_color_bytes;
                    current_fg_color = default_color_bytes;

                    if cell.fg_color != current_fg_color {
                        u32_color_to_ansi(&mut output, cell.fg_color, true);
                    }

                    if cell.bg_color != current_bg_color {
                        u32_color_to_ansi(&mut output, cell.bg_color, false);
                    }
                } else {
                    if cell.fg_color != current_fg_color {
                        u32_color_to_ansi(&mut output, cell.fg_color, true);
                        current_fg_color = cell.fg_color;
                    }
                    if cell.bg_color != current_bg_color {
                        u32_color_to_ansi(&mut output, cell.bg_color, false);
                        current_bg_color = cell.bg_color;
                    }
                }

                // finally add the actual content of the cell, usually a char
                let data = &cell.contents;
                let len = cell.len();
                output.extend_from_slice(&data[..len]);

                // move the cursor after adding content
                cursor_x += if cell.has_attribute(WIDE) { 2 } else { 1 };
            }
        }

        // reset styles (clean up thing) and return the buffer
        output.extend_from_slice(b"\x1b[0m");
        output
    }

    // adds all attributes from RemuxCell as ANSI codes
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
        self.contents = [0u8; CONTENT_LENGTH];

        if contents.is_empty() || contents[0] == 0 {
            self.contents[0] = SPACE;
            self.len = 1;
        } else {
            let len = contents.len().min(CONTENT_LENGTH);
            self.contents[..len].copy_from_slice(&contents[..len]);
            self.len = len as u8;
        }
    }

    pub fn set_fg_color(&mut self, color: vt100::Color) {
        self.fg_color = color_to_bytes(color);
    }

    pub fn set_bg_color(&mut self, color: vt100::Color) {
        self.bg_color = color_to_bytes(color);
    }
}

// helper functions
// push_u8 is faster than write!() due to under-the-hood Rust stuff
#[inline]
fn push_u8(buf: &mut Vec<u8>, n: u8) {
    if n == 0 {
        buf.push(b'0');
        return;
    }
    if n < 10 {
        buf.push(b'0' + n);
        return;
    }
    if n < 100 {
        buf.push(b'0' + (n / 10));
        buf.push(b'0' + (n % 10));
        return;
    }
    buf.push(b'0' + (n / 100));
    buf.push(b'0' + ((n / 10) % 10));
    buf.push(b'0' + (n % 10));
}

// push_u16 is faster than write!() due to under-the-hood Rust stuff
#[inline]
fn push_u16(buf: &mut Vec<u8>, mut n: u16) {
    if n == 0 {
        buf.push(b'0');
        return;
    }

    // create temp buffer and write backwards
    let mut buffer = [0u8; 5];
    let mut i = 5;

    while n > 0 {
        i -= 1;
        buffer[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }

    buf.extend_from_slice(&buffer[i..]);
}

#[inline]
fn set_cursor_position(buf: &mut Vec<u8>, x: u16, y: u16) {
    buf.extend_from_slice(b"\x1b[");
    push_u16(buf, y);
    buf.push(b';');
    push_u16(buf, x);
    buf.push(b'H');
}

// convert vt100 color to bytes, the 0, 1, 2 are different modes
#[inline]
fn color_to_bytes(c: vt100::Color) -> u32 {
    match c {
        vt100::Color::Default => 0,
        vt100::Color::Idx(i) => 0x0100_0000 | ((i as u32) << 16),
        vt100::Color::Rgb(r, g, b) => 0x0200_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32),
    }
}

// converts the colors as bytes to ANSI codes
#[inline]
fn u32_color_to_ansi(output: &mut Vec<u8>, color: u32, is_fg: bool) {
    let color_type = color >> 24;
    let val1 = ((color >> 16) & 0xFF) as u8;
    let val2 = ((color >> 8) & 0xFF) as u8;
    let val3 = (color & 0xFF) as u8;

    match color_type {
        0 => {
            // Default
            if is_fg {
                output.extend_from_slice(b"\x1b[39m");
            } else {
                output.extend_from_slice(b"\x1b[49m");
            }
        }
        1 => {
            // Indexed
            if is_fg {
                output.extend_from_slice(b"\x1b[38;5;");
            } else {
                output.extend_from_slice(b"\x1b[48;5;");
            }
            push_u8(output, val1);
            output.push(b'm');
        }
        2 => {
            // RGB
            if is_fg {
                output.extend_from_slice(b"\x1b[38;2;");
            } else {
                output.extend_from_slice(b"\x1b[48;2;");
            }
            push_u8(output, val1);
            output.push(b';');
            push_u8(output, val2);
            output.push(b';');
            push_u8(output, val3);
            output.push(b'm');
        }
        _ => {}
    }
}
