use crate::app::AppState;

#[derive(Debug, Clone)]
pub struct StatusLineState {
    pub enabled: bool,
    pub a: Vec<String>,
    pub b: Vec<String>,
    pub c: Vec<String>,
}

impl Default for StatusLineState {
    fn default() -> Self {
        Self {
            enabled: true,
            a: Default::default(),
            b: Default::default(),
            c: Default::default(),
        }
    }
}
impl StatusLineState {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    pub fn apply_built_ins(&mut self, state: &AppState) {
        for item in self.a.iter_mut().chain(self.b.iter_mut()).chain(self.c.iter_mut()) {
            if item.as_str() == "active-session" {
                if let Some(s) = state.daemon.active_session.map(|s| s.to_string()) {
                    *item = s;
                } else {
                    *item = "".to_owned();
                }
            }
        }
    }
}
