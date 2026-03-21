use std::collections::BTreeMap;

use bytes::Bytes;

use crate::{
    cell::RemuxCell,
    layout::{LayoutNode, Rect, SplitDirection},
    prelude::*,
    render::surface::Surface,
};

#[derive(Debug, Clone)]
pub enum WindowAction {
    SendOutput(Bytes),
    SendInputToPane { id: usize, bytes: Bytes },
    ResizePane { id: usize, rect: Rect },
    KillPane { id: usize },
    RerenderPane { id: usize },
    SpawnPane { id: usize, rect: Rect },
}

#[allow(unused)]
#[derive(Debug)]
pub enum WindowState {
    Focused,
    Unfocused,
}

#[derive(Debug)]
pub struct Window {
    layout: LayoutNode,
    layout_sizing_map: BTreeMap<usize, Rect>,
    pane_cursors: BTreeMap<usize, (u16, u16)>,
    active_pane_id: usize,
    next_pane_id: usize,
    root_rect: Rect,
    status_line_enabled: bool,

    #[allow(unused)]
    window_state: WindowState,
}

impl Window {
    pub fn new(status_line_enabled: bool) -> Result<(Self, Vec<WindowAction>)> {
        let init_pane_id = 0;
        let layout = LayoutNode::Pane { id: init_pane_id };

        let root_rect = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };

        let mut layout_sizing_map = BTreeMap::new();
        layout.calculate_layout(content_rect(root_rect, status_line_enabled), &mut layout_sizing_map)?;
        let init_rect = *layout_sizing_map
            .get(&init_pane_id)
            .ok_or_else(|| Error::msg("initial pane rect missing"))?;

        Ok((
            Self {
                layout,
                layout_sizing_map,
                pane_cursors: BTreeMap::new(),
                active_pane_id: init_pane_id,
                next_pane_id: init_pane_id + 1,
                root_rect,
                status_line_enabled,
                window_state: WindowState::Focused,
            },
            vec![WindowAction::SpawnPane {
                id: init_pane_id,
                rect: init_rect,
            }],
        ))
    }

    pub fn route_input_to_active_pane(&self, bytes: Bytes) -> Result<Vec<WindowAction>> {
        Ok(vec![WindowAction::SendInputToPane {
            id: self.active_pane_id,
            bytes,
        }])
    }

    pub fn handle_pane_output(&mut self, id: usize, cursor: Option<(u16, u16)>) -> Result<Vec<WindowAction>> {
        if let Some(pos) = cursor {
            self.pane_cursors.insert(id, pos);
        }
        Ok(Vec::new())
    }

    pub fn redraw(&mut self) -> Result<Vec<WindowAction>> {
        let mut actions = Vec::new();

        for id in self.layout_sizing_map.keys().copied() {
            actions.push(WindowAction::RerenderPane { id });
        }

        Ok(actions)
    }

    pub fn iterate_active_pane(&mut self, is_next: bool) -> Result<Vec<WindowAction>> {
        let ids: Vec<usize> = self.layout_sizing_map.keys().copied().collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let current_idx = ids.iter().position(|&id| id == self.active_pane_id).unwrap_or(0);
        let new_idx = if is_next {
            (current_idx + 1) % ids.len()
        } else if current_idx == 0 {
            ids.len() - 1
        } else {
            current_idx - 1
        };

        self.active_pane_id = ids[new_idx];
        Ok(Vec::new())
    }

    pub fn split_active_pane(&mut self, direction: SplitDirection) -> Result<Vec<WindowAction>> {
        let new_pane_id = self.next_pane_id;
        self.layout.add_split(self.active_pane_id, new_pane_id, direction);
        self.recalculate_layout()?;

        let rect = *self
            .layout_sizing_map
            .get(&new_pane_id)
            .ok_or_else(|| Error::msg("new pane rect missing after split"))?;

        self.active_pane_id = new_pane_id;
        self.next_pane_id += 1;

        let mut actions = vec![WindowAction::SpawnPane { id: new_pane_id, rect }];
        actions.extend(self.resize_actions());
        actions.extend(self.redraw()?);
        Ok(actions)
    }

    pub fn kill_active_pane(&mut self) -> Result<Vec<WindowAction>> {
        if self.layout_sizing_map.len() <= 1 {
            warn!("Can't kill last pane {}", self.active_pane_id);
            return Ok(Vec::new());
        }

        Ok(vec![WindowAction::KillPane {
            id: self.active_pane_id,
        }])
    }

    pub fn resize_terminal(&mut self, rows: u16, cols: u16) -> Result<Vec<WindowAction>> {
        self.root_rect = Rect {
            x: 0,
            y: 0,
            width: cols,
            height: rows,
        };
        self.recalculate_layout()?;

        let mut actions = self.resize_actions();
        actions.extend(self.redraw()?);
        Ok(actions)
    }

    pub fn remove_pane(&mut self, id: usize) -> Result<Vec<WindowAction>> {
        self.pane_cursors.remove(&id);

        let old_layout = std::mem::replace(&mut self.layout, LayoutNode::Pane { id });
        let Some(new_layout) = old_layout.remove_node(id) else {
            self.layout = LayoutNode::Pane { id };
            self.layout_sizing_map.clear();
            return Ok(Vec::new());
        };

        self.layout = new_layout;
        self.recalculate_layout()?;

        let ids: Vec<usize> = self.layout_sizing_map.keys().copied().collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        if self.active_pane_id == id || !self.layout_sizing_map.contains_key(&self.active_pane_id) {
            self.active_pane_id = ids[0];
        }

        let mut actions = self.resize_actions();
        actions.extend(self.redraw()?);
        Ok(actions)
    }

    fn recalculate_layout(&mut self) -> Result<()> {
        self.layout_sizing_map.clear();
        self.layout.calculate_layout(
            content_rect(self.root_rect, self.status_line_enabled),
            &mut self.layout_sizing_map,
        )
    }

    fn resize_actions(&self) -> Vec<WindowAction> {
        self.layout_sizing_map
            .iter()
            .map(|(&id, &rect)| WindowAction::ResizePane { id, rect })
            .collect()
    }

    pub fn compose_surface(
        &self,
        pane_surfaces: &BTreeMap<usize, Surface>,
        status_line: Option<&Surface>,
    ) -> Result<Surface> {
        let mut surface = Surface::new(self.root_rect.width, self.root_rect.height);

        for (id, rect) in &self.layout_sizing_map {
            if let Some(pane_surface) = pane_surfaces.get(id) {
                surface.overlay_at(pane_surface, rect.x, rect.y);
            }
        }

        self.paint_pane_borders(&mut surface);

        if let Some(rect) = self.layout_sizing_map.get(&self.active_pane_id) {
            if let Some((cursor_x, cursor_y)) = self.pane_cursors.get(&self.active_pane_id) {
                surface.set_cursor(Some((rect.x + cursor_x, rect.y + cursor_y)), true);
            }
        }

        if let Some(status_line) = status_line {
            surface.overlay_at(status_line, 0, self.root_rect.height.saturating_sub(1));
        }

        Ok(surface)
    }

    fn paint_pane_borders(&self, surface: &mut Surface) {
        let cols = self.root_rect.width;
        let rows = self.root_rect.height;
        let active_rect = self.layout_sizing_map.get(&self.active_pane_id);

        let is_content = |x: u16, y: u16, map: &BTreeMap<usize, Rect>| -> bool {
            for rect in map.values() {
                if x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height {
                    return true;
                }
            }
            false
        };

        for y in 0..rows {
            for x in 0..cols {
                if is_content(x, y, &self.layout_sizing_map) {
                    continue;
                }

                let north = y > 0 && !is_content(x, y - 1, &self.layout_sizing_map);
                let south = y < rows - 1 && !is_content(x, y + 1, &self.layout_sizing_map);
                let west = x > 0 && !is_content(x - 1, y, &self.layout_sizing_map);
                let east = x < cols - 1 && !is_content(x + 1, y, &self.layout_sizing_map);

                let border_char = match (north, south, east, west) {
                    (true, true, false, false) => '│',
                    (false, false, true, true) => '─',
                    (false, true, true, false) => '┌',
                    (false, true, false, true) => '┐',
                    (true, false, true, false) => '└',
                    (true, false, false, true) => '┘',
                    (true, true, true, false) => '├',
                    (true, true, false, true) => '┤',
                    (false, true, true, true) => '┬',
                    (true, false, true, true) => '┴',
                    (true, true, true, true) => '┼',
                    (true, false, false, false) => '│',
                    (false, true, false, false) => '│',
                    (false, false, true, false) => '─',
                    (false, false, false, true) => '─',
                    _ => ' ',
                };

                let mut is_active_border = false;
                if let Some(rect) = active_rect {
                    if x >= rect.x.saturating_sub(1)
                        && x < rect.x + rect.width + 1
                        && y >= rect.y.saturating_sub(1)
                        && y < rect.y + rect.height + 1
                    {
                        is_active_border = true;
                    }
                }

                let mut cell = RemuxCell::default();
                let mut border_char_buf = [0u8; 4];
                let str_slice = border_char.encode_utf8(&mut border_char_buf);
                cell.set_content(str_slice.as_bytes());
                cell.set_fg_color(if is_active_border {
                    vt100::Color::Idx(14)
                } else {
                    vt100::Color::Idx(8)
                });
                surface.paint_cell(x, y, cell);
            }
        }
    }
}

fn content_rect(root_rect: Rect, status_line_enabled: bool) -> Rect {
    if status_line_enabled && root_rect.height > 0 {
        Rect {
            x: root_rect.x,
            y: root_rect.y,
            width: root_rect.width,
            height: root_rect.height.saturating_sub(1),
        }
    } else {
        root_rect
    }
}
