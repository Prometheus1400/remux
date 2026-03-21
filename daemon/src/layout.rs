use std::collections::BTreeMap;

use crate::prelude::*;

#[derive(Debug, Clone, Copy)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone)]
pub enum LayoutNode {
    // container node
    Split {
        direction: SplitDirection,
        left: Box<LayoutNode>,
        right: Box<LayoutNode>,
        left_weight: u32,  // used for sizing
        right_weight: u32, // used for sizing
    },

    // leaf node
    Pane {
        id: usize,
    },
}
impl LayoutNode {
    pub fn add_split(&mut self, target_id: usize, new_id: usize, direction: SplitDirection) -> bool {
        match self {
            LayoutNode::Pane { id } => {
                if *id == target_id {
                    let left_node = Box::new(LayoutNode::Pane { id: *id });
                    let right_node = Box::new(LayoutNode::Pane { id: new_id });

                    *self = LayoutNode::Split {
                        direction,
                        left: left_node,
                        right: right_node,
                        left_weight: 1,
                        right_weight: 1,
                    };

                    return true;
                }
                false
            }
            LayoutNode::Split { left, right, .. } => {
                if left.add_split(target_id, new_id, direction) {
                    return true;
                }
                right.add_split(target_id, new_id, direction)
            }
        }
    }

    pub fn remove_node(self, target_id: usize) -> Option<LayoutNode> {
        match self {
            LayoutNode::Pane { id } => {
                if id == target_id {
                    None
                } else {
                    Some(LayoutNode::Pane { id })
                }
            }
            LayoutNode::Split {
                direction,
                left,
                right,
                left_weight,
                right_weight,
            } => {
                let new_left = left.remove_node(target_id);
                let new_right = right.remove_node(target_id);

                match (new_left, new_right) {
                    (Some(l), Some(r)) => Some(LayoutNode::Split {
                        direction,
                        left: Box::new(l),
                        right: Box::new(r),
                        left_weight,
                        right_weight,
                    }),
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    (None, None) => None,
                }
            }
        }
    }

    pub fn calculate_layout(&self, area: Rect, results: &mut BTreeMap<usize, Rect>) -> Result<()> {
        match self {
            LayoutNode::Pane { id } => {
                results.insert(*id, area);
                Ok(())
            }
            LayoutNode::Split {
                direction,
                left,
                right,
                left_weight,
                right_weight,
            } => {
                let total_weight = left_weight + right_weight;

                match direction {
                    SplitDirection::Vertical => {
                        // remove a column for the border
                        let available_width = area.width.saturating_sub(1);

                        let left_width = (available_width as u32 * left_weight / total_weight) as u16;
                        let right_width = available_width - left_width;

                        let left_rect = Rect {
                            width: left_width,
                            ..area
                        };

                        // +1 for the border
                        let right_rect = Rect {
                            width: right_width,
                            x: area.x + left_width + 1,
                            ..area
                        };

                        trace!("left: {:?} left rect: {:?}", left, left_rect);
                        trace!("right: {:?} right rect: {:?}", right, right_rect);

                        left.calculate_layout(left_rect, results)?;
                        right.calculate_layout(right_rect, results)?;
                        Ok(())
                    }
                    SplitDirection::Horizontal => {
                        // remove a row for the border
                        let available_height = area.height.saturating_sub(1);

                        let top_height = (available_height as u32 * left_weight / total_weight) as u16;
                        let bottom_height = available_height - top_height;

                        let top_rect = Rect {
                            height: top_height,
                            ..area
                        };

                        // +1 for the border
                        let bottom_rect = Rect {
                            height: bottom_height,
                            y: area.y + top_height + 1,
                            ..area
                        };

                        trace!("left: {:?} top rect: {:?}", left, top_rect);
                        trace!("right: {:?} bottom rect: {:?}", right, bottom_rect);

                        left.calculate_layout(top_rect, results)?;
                        right.calculate_layout(bottom_rect, results)?;
                        Ok(())
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{LayoutNode, Rect, SplitDirection};

    #[test]
    fn add_split_replaces_target_pane_with_split() {
        let mut root = LayoutNode::Pane { id: 1 };

        assert!(root.add_split(1, 2, SplitDirection::Vertical));

        match root {
            LayoutNode::Split {
                direction: SplitDirection::Vertical,
                left,
                right,
                ..
            } => {
                assert!(matches!(*left, LayoutNode::Pane { id: 1 }));
                assert!(matches!(*right, LayoutNode::Pane { id: 2 }));
            }
            other => panic!("expected split node, got {other:?}"),
        }
    }

    #[test]
    fn remove_node_collapses_single_child_split() {
        let layout = LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(LayoutNode::Pane { id: 1 }),
            right: Box::new(LayoutNode::Pane { id: 2 }),
            left_weight: 1,
            right_weight: 1,
        };

        let updated = layout.remove_node(1).expect("right pane should remain");

        assert!(matches!(updated, LayoutNode::Pane { id: 2 }));
    }

    #[test]
    fn vertical_layout_splits_width_and_reserves_border_column() {
        let layout = LayoutNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(LayoutNode::Pane { id: 1 }),
            right: Box::new(LayoutNode::Pane { id: 2 }),
            left_weight: 1,
            right_weight: 1,
        };
        let mut results = BTreeMap::new();

        layout
            .calculate_layout(
                Rect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 4,
                },
                &mut results,
            )
            .expect("layout should calculate");

        let left = results.get(&1).expect("left pane should exist");
        let right = results.get(&2).expect("right pane should exist");
        assert_eq!((left.x, left.width), (0, 4));
        assert_eq!((right.x, right.width), (5, 5));
    }

    #[test]
    fn horizontal_layout_splits_height_and_reserves_border_row() {
        let layout = LayoutNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(LayoutNode::Pane { id: 1 }),
            right: Box::new(LayoutNode::Pane { id: 2 }),
            left_weight: 1,
            right_weight: 3,
        };
        let mut results = BTreeMap::new();

        layout
            .calculate_layout(
                Rect {
                    x: 2,
                    y: 3,
                    width: 6,
                    height: 9,
                },
                &mut results,
            )
            .expect("layout should calculate");

        let top = results.get(&1).expect("top pane should exist");
        let bottom = results.get(&2).expect("bottom pane should exist");
        assert_eq!((top.y, top.height), (3, 2));
        assert_eq!((bottom.y, bottom.height), (6, 6));
    }
}
