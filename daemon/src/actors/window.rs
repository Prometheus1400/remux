use std::collections::BTreeMap;

use bytes::Bytes;

use crate::{
    cell::RemuxCell,
    layout::{LayoutNode, Rect, SplitDirection},
    lua::config::PaneStyle,
    prelude::*,
    render::surface::Surface,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Debug, Clone)]
pub enum WindowAction {
    SendInputToPane { id: usize, bytes: Bytes },
    ResizePane { id: usize, rect: Rect },
    KillPane { id: usize },
    RerenderPane { id: usize },
    SpawnPane { id: usize, rect: Rect },
}

#[derive(Debug)]
pub struct Window {
    layout: LayoutNode,
    layout_sizing_map: BTreeMap<usize, Rect>,
    pane_cursors: BTreeMap<usize, (u16, u16)>,
    active_pane_id: usize,
    root_rect: Rect,
    content_rect: Rect,
}

impl Window {
    pub fn new(rows: u16, cols: u16, content_rect: Rect, init_pane_id: usize) -> Result<(Self, Vec<WindowAction>)> {
        let layout = LayoutNode::Pane { id: init_pane_id };

        let root_rect = Rect {
            x: 0,
            y: 0,
            width: cols,
            height: rows,
        };

        let mut layout_sizing_map = BTreeMap::new();
        layout.calculate_layout(content_rect, &mut layout_sizing_map)?;
        let init_rect = *layout_sizing_map
            .get(&init_pane_id)
            .ok_or_else(|| Error::msg("initial pane rect missing"))?;

        Ok((
            Self {
                layout,
                layout_sizing_map,
                pane_cursors: BTreeMap::new(),
                active_pane_id: init_pane_id,
                root_rect,
                content_rect,
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

    pub fn focus_pane(&mut self, direction: FocusDirection) -> Result<Vec<WindowAction>> {
        let Some(current) = self.layout_sizing_map.get(&self.active_pane_id).copied() else {
            return Ok(Vec::new());
        };

        let next = self
            .layout_sizing_map
            .iter()
            .filter_map(|(&id, &rect)| {
                if id == self.active_pane_id {
                    return None;
                }
                focus_candidate(direction, current, id, rect)
            })
            .min_by_key(|candidate| candidate.sort_key());

        if let Some(candidate) = next {
            self.active_pane_id = candidate.id;
        }

        Ok(Vec::new())
    }

    pub fn split_active_pane(&mut self, new_pane_id: usize, direction: SplitDirection) -> Result<Vec<WindowAction>> {
        self.layout.add_split(self.active_pane_id, new_pane_id, direction);
        self.recalculate_layout()?;

        let rect = *self
            .layout_sizing_map
            .get(&new_pane_id)
            .ok_or_else(|| Error::msg("new pane rect missing after split"))?;

        self.active_pane_id = new_pane_id;

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

    pub fn resize_terminal(&mut self, rows: u16, cols: u16, content_rect: Rect) -> Result<Vec<WindowAction>> {
        self.root_rect = Rect {
            x: 0,
            y: 0,
            width: cols,
            height: rows,
        };
        self.content_rect = content_rect;
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
        self.layout
            .calculate_layout(self.content_rect, &mut self.layout_sizing_map)
    }

    fn resize_actions(&self) -> Vec<WindowAction> {
        self.layout_sizing_map
            .iter()
            .map(|(&id, &rect)| WindowAction::ResizePane { id, rect })
            .collect()
    }

    pub fn pane_ids(&self) -> Vec<usize> {
        self.layout_sizing_map.keys().copied().collect()
    }

    pub fn active_pane_id(&self) -> usize {
        self.active_pane_id
    }

    pub fn pane_rect(&self, id: usize) -> Option<Rect> {
        self.layout_sizing_map.get(&id).copied()
    }

    pub fn compose_surface(&self, pane_surfaces: &BTreeMap<usize, Surface>, pane_style: &PaneStyle) -> Result<Surface> {
        let mut surface = Surface::new(self.root_rect.width, self.root_rect.height);

        for (id, rect) in &self.layout_sizing_map {
            if let Some(pane_surface) = pane_surfaces.get(id) {
                surface.overlay_at(pane_surface, rect.x, rect.y);
            }
        }

        self.paint_pane_borders(&mut surface, pane_style);

        if let Some(rect) = self.layout_sizing_map.get(&self.active_pane_id) {
            if let Some((cursor_x, cursor_y)) = self.pane_cursors.get(&self.active_pane_id) {
                surface.set_cursor(Some((rect.x + cursor_x, rect.y + cursor_y)), true);
            }
        }

        Ok(surface)
    }

    fn paint_pane_borders(&self, surface: &mut Surface, pane_style: &PaneStyle) {
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
                    vt100::Color::Idx(pane_style.active_border_fg)
                } else {
                    vt100::Color::Idx(pane_style.inactive_border_fg)
                });
                surface.paint_cell(x, y, cell);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FocusCandidate {
    id: usize,
    gap: u16,
    overlap: u16,
}

impl FocusCandidate {
    fn sort_key(self) -> (u16, std::cmp::Reverse<u16>, usize) {
        (self.gap, std::cmp::Reverse(self.overlap), self.id)
    }
}

fn focus_candidate(direction: FocusDirection, current: Rect, id: usize, other: Rect) -> Option<FocusCandidate> {
    match direction {
        FocusDirection::Left => {
            let overlap = vertical_overlap(current, other)?;
            let gap = gap_before(current.x, other.x, other.width)?;
            Some(FocusCandidate { id, gap, overlap })
        }
        FocusDirection::Right => {
            let overlap = vertical_overlap(current, other)?;
            let gap = gap_before(other.x, current.x, current.width)?;
            Some(FocusCandidate { id, gap, overlap })
        }
        FocusDirection::Up => {
            let overlap = horizontal_overlap(current, other)?;
            let gap = gap_before(current.y, other.y, other.height)?;
            Some(FocusCandidate { id, gap, overlap })
        }
        FocusDirection::Down => {
            let overlap = horizontal_overlap(current, other)?;
            let gap = gap_before(other.y, current.y, current.height)?;
            Some(FocusCandidate { id, gap, overlap })
        }
    }
}

fn vertical_overlap(a: Rect, b: Rect) -> Option<u16> {
    overlap_1d(a.y, a.height, b.y, b.height)
}

fn horizontal_overlap(a: Rect, b: Rect) -> Option<u16> {
    overlap_1d(a.x, a.width, b.x, b.width)
}

fn overlap_1d(a_start: u16, a_len: u16, b_start: u16, b_len: u16) -> Option<u16> {
    let a_end = u32::from(a_start) + u32::from(a_len);
    let b_end = u32::from(b_start) + u32::from(b_len);
    let start = u32::from(a_start.max(b_start));
    let end = a_end.min(b_end);
    if end > start { Some((end - start) as u16) } else { None }
}

fn gap_before(target_start: u16, source_start: u16, source_len: u16) -> Option<u16> {
    let source_end = u32::from(source_start) + u32::from(source_len);
    let target_start = u32::from(target_start);
    if target_start >= source_end {
        Some((target_start - source_end) as u16)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{FocusDirection, Window, WindowAction};
    use crate::layout::SplitDirection;

    fn make_window() -> Window {
        let (window, startup) = Window::new(
            12,
            24,
            crate::layout::Rect {
                x: 0,
                y: 0,
                width: 24,
                height: 12,
            },
            0,
        )
        .unwrap();
        assert!(matches!(startup.as_slice(), [WindowAction::SpawnPane { id: 0, .. }]));
        window
    }

    fn split(window: &mut Window, direction: SplitDirection) {
        let new_pane_id = window.pane_ids().into_iter().max().unwrap_or(0) + 1;
        let _ = window.split_active_pane(new_pane_id, direction).unwrap();
    }

    #[test]
    fn focus_left_and_right_moves_across_vertical_split() {
        let mut window = make_window();
        split(&mut window, SplitDirection::Vertical);

        assert_eq!(window.active_pane_id, 1);
        window.focus_pane(FocusDirection::Left).unwrap();
        assert_eq!(window.active_pane_id, 0);
        window.focus_pane(FocusDirection::Right).unwrap();
        assert_eq!(window.active_pane_id, 1);
    }

    #[test]
    fn focus_up_and_down_moves_across_horizontal_split() {
        let mut window = make_window();
        split(&mut window, SplitDirection::Horizontal);

        assert_eq!(window.active_pane_id, 1);
        window.focus_pane(FocusDirection::Up).unwrap();
        assert_eq!(window.active_pane_id, 0);
        window.focus_pane(FocusDirection::Down).unwrap();
        assert_eq!(window.active_pane_id, 1);
    }

    #[test]
    fn focus_is_noop_when_no_pane_exists_in_direction() {
        let mut window = make_window();
        split(&mut window, SplitDirection::Vertical);

        assert_eq!(window.active_pane_id, 1);
        window.focus_pane(FocusDirection::Right).unwrap();
        assert_eq!(window.active_pane_id, 1);
    }

    #[test]
    fn focus_chooses_nearest_candidate_in_requested_direction() {
        let mut window = make_window();
        split(&mut window, SplitDirection::Vertical);
        window.focus_pane(FocusDirection::Left).unwrap();
        split(&mut window, SplitDirection::Horizontal);

        assert_eq!(window.active_pane_id, 2);
        window.focus_pane(FocusDirection::Right).unwrap();
        assert_eq!(window.active_pane_id, 1);
    }

    #[test]
    fn focus_breaks_ties_by_overlap_then_pane_id() {
        let mut window = make_window();
        split(&mut window, SplitDirection::Vertical);
        window.focus_pane(FocusDirection::Left).unwrap();
        split(&mut window, SplitDirection::Horizontal);
        window.focus_pane(FocusDirection::Up).unwrap();

        assert_eq!(window.active_pane_id, 0);
        window.focus_pane(FocusDirection::Right).unwrap();
        assert_eq!(window.active_pane_id, 1);
    }

    #[test]
    fn focus_after_pane_removal_uses_remaining_layout() {
        let mut window = make_window();
        split(&mut window, SplitDirection::Vertical);
        window.focus_pane(FocusDirection::Left).unwrap();
        split(&mut window, SplitDirection::Horizontal);

        let _ = window.remove_pane(2).unwrap();
        assert_eq!(window.active_pane_id, 0);
        window.focus_pane(FocusDirection::Right).unwrap();
        assert_eq!(window.active_pane_id, 1);
    }
}
