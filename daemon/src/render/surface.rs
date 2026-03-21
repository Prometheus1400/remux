use crate::cell::RemuxCell;

#[derive(Clone, Debug)]
pub struct Surface {
    width: u16,
    height: u16,
    cells: Vec<RemuxCell>,
    cursor: Option<(u16, u16)>,
    cursor_visible: bool,
}

impl Surface {
    pub fn new(width: u16, height: u16) -> Self {
        let len = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![RemuxCell::default(); len],
            cursor: None,
            cursor_visible: false,
        }
    }

    pub fn from_parts(
        width: u16,
        height: u16,
        cells: Vec<RemuxCell>,
        cursor: Option<(u16, u16)>,
        cursor_visible: bool,
    ) -> Self {
        Self {
            width,
            height,
            cells,
            cursor,
            cursor_visible,
        }
    }

    pub fn paint_byte(&mut self, x: u16, y: u16, byte: u8) {
        let Some(cell) = self.cell_mut(x, y) else {
            return;
        };

        cell.set_content(&[byte]);
    }

    pub fn paint_cell(&mut self, x: u16, y: u16, cell: RemuxCell) {
        let Some(dest) = self.cell_mut(x, y) else {
            return;
        };

        *dest = cell;
    }

    pub fn byte_at(&self, x: u16, y: u16) -> Option<u8> {
        self.cell(x, y).and_then(|cell| cell.content_bytes().first().copied())
    }

    pub fn overlay_at(&mut self, other: &Surface, x: u16, y: u16) {
        for row in 0..other.height {
            for col in 0..other.width {
                let Some(source) = other.cell(col, row) else {
                    continue;
                };
                let Some(dest) = self.cell_mut(x + col, y + row) else {
                    continue;
                };
                *dest = source.clone();
            }
        }

        if let Some((cursor_x, cursor_y)) = other.cursor {
            self.cursor = Some((x + cursor_x, y + cursor_y));
            self.cursor_visible = other.cursor_visible;
        }
    }

    pub fn overlay_transparent_at(&mut self, other: &Surface, x: u16, y: u16) {
        let transparent = RemuxCell::default();

        for row in 0..other.height {
            for col in 0..other.width {
                let Some(source) = other.cell(col, row) else {
                    continue;
                };
                if source == &transparent {
                    continue;
                }

                let Some(dest) = self.cell_mut(x + col, y + row) else {
                    continue;
                };
                *dest = source.clone();
            }
        }

        if let Some((cursor_x, cursor_y)) = other.cursor {
            self.cursor = Some((x + cursor_x, y + cursor_y));
            self.cursor_visible = other.cursor_visible;
        }
    }

    pub fn set_cursor(&mut self, cursor: Option<(u16, u16)>, visible: bool) {
        self.cursor = cursor;
        self.cursor_visible = visible;
    }

    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor
    }

    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn cells(&self) -> &[RemuxCell] {
        &self.cells
    }

    fn cell(&self, x: u16, y: u16) -> Option<&RemuxCell> {
        let idx = self.idx(x, y)?;
        self.cells.get(idx)
    }

    fn cell_mut(&mut self, x: u16, y: u16) -> Option<&mut RemuxCell> {
        let idx = self.idx(x, y)?;
        self.cells.get_mut(idx)
    }

    fn idx(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some(usize::from(y) * usize::from(self.width) + usize::from(x))
    }
}
