use std::collections::HashMap;

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
        left_weight: u32,           // used for sizing
        right_weight: u32,          // used for sizing
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
                        right_weight: 1
                    };

                    return true
                }
                false

            }
            LayoutNode::Split { left, right, .. } => {
                if left.add_split(target_id, new_id, direction) {
                    return true
                }
                right.add_split(target_id, new_id, direction)
            }
        }
    }

    pub fn calculate_layout(&self, area: Rect, results: &mut HashMap<usize, Rect>) -> Result<()> {
        match self {
            LayoutNode::Pane { id } => {
                results.insert(*id, area);
                Ok(())
            }
            LayoutNode::Split { direction, left, right, left_weight, right_weight } => {
                let total_weight = left_weight + right_weight;
                
                match direction {
                    SplitDirection::Vertical => {
                        let left_width = (area.width as u32 * left_weight / total_weight) as u16;
                        let right_width = area.width - left_width;

                        let left_rect = Rect {
                            width: left_width,
                            ..area
                        };

                        let right_rect = Rect {
                            width: right_width,
                            x: area.x + left_width,
                            ..area
                        };

                        debug!("left: {:?} left rect: {:?}", left, left_rect);
                        debug!("right: {:?} right rect: {:?}", right, right_rect);

                        left.calculate_layout(left_rect, results)?;
                        right.calculate_layout(right_rect, results)?;
                        Ok(())
                    }
                    SplitDirection::Horizontal => {
                        let top_height = (area.height as u32 * left_weight / total_weight) as u16;
                        let bottom_height = area.height - top_height;

                        let top_rect = Rect {
                            height: top_height,
                            ..area
                        };

                        let bottom_rect = Rect {
                            height: bottom_height,
                            y: area.y + top_height,
                            ..area
                        };

                        left.calculate_layout(top_rect, results)?;
                        right.calculate_layout(bottom_rect, results)?;
                        Ok(())
                    }
                }

            }
        }

    }
}

