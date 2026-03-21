use remux_core::states::ServerSnapshot;

#[derive(Debug, Clone)]
pub struct StatusLineState {
    pub enabled: bool,
    pub a: Vec<String>,
    pub b: Vec<String>,
    pub c: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StatusLineContext {
    pub active_session_name: Option<String>,
}

impl From<&ServerSnapshot> for StatusLineContext {
    fn from(server_snapshot: &ServerSnapshot) -> Self {
        Self {
            active_session_name: server_snapshot.active_session.and_then(|id| {
                server_snapshot
                    .sessions
                    .iter()
                    .find(|session_info| session_info.id == id)
                    .map(|session_info| session_info.name.clone())
            }),
        }
    }
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

    pub fn with_context(&self, context: &StatusLineContext) -> Self {
        let mut rendered = self.clone();
        for item in rendered
            .a
            .iter_mut()
            .chain(rendered.b.iter_mut())
            .chain(rendered.c.iter_mut())
        {
            if item.as_str() == "active-session" {
                *item = context.active_session_name.clone().unwrap_or_default();
            }
        }

        rendered
    }
}
