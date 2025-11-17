use crate::pane::Pane;

mod tree;

pub struct Window {
    panes: Vec<Pane>
}

impl Window {
    pub fn new() -> Self {
        Self {
            panes: vec![]
        }
    }
}
