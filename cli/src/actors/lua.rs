use std::{
    fs,
    sync::{
        Arc, RwLock,
        mpsc::{self, RecvTimeoutError},
    },
    time::Duration,
};

use mlua::Lua as MLua;
use tokio::runtime::Handle;

use crate::{
    actors::ui::UIHandle,
    prelude::*,
    states::{daemon_state::DaemonState, status_line_state::StatusLineState},
};

pub enum LuaEvent {
    Kill,
    SyncDaemonState(DaemonState),
}
use LuaEvent::*;

#[derive(Debug, Clone)]
pub struct LuaHandle {
    tx: mpsc::Sender<LuaEvent>,
}
impl LuaHandle {
    pub fn kill(&mut self) -> Result<()> {
        Ok(self.tx.send(LuaEvent::Kill)?)
    }
    pub fn sync_daemon_state(&mut self, daemon_state: DaemonState) -> Result<()> {
        Ok(self.tx.send(LuaEvent::SyncDaemonState(daemon_state))?)
    }
}

#[derive(Debug)]
pub struct Lua {
    pub _handle: LuaHandle,
    pub rx: mpsc::Receiver<LuaEvent>,
    pub lua: MLua,
    pub ui_handle: UIHandle,
    pub daemon_state: Arc<RwLock<DaemonState>>,
}

impl Lua {
    fn new(ui_handle: UIHandle, handle: LuaHandle, rx: mpsc::Receiver<LuaEvent>) -> Self {
        Self {
            _handle: handle,
            rx,
            lua: MLua::new(),
            ui_handle,
            daemon_state: Arc::new(RwLock::new(DaemonState::default())),
        }
    }

    fn initialize_lua_state(&mut self) -> Result<()> {
        // ui configurations
        let sections_table = self.lua.create_table()?;
        let section_a = self.lua.create_table()?;
        let section_b = self.lua.create_table()?;
        let section_c = self.lua.create_table()?;
        sections_table.set("a", section_a)?;
        sections_table.set("b", section_b)?;
        sections_table.set("c", section_c)?;
        let status_line_table = self.lua.create_table()?;
        status_line_table.set("sections", sections_table)?;
        status_line_table.set("enabled", true)?;
        // 6. Create the parent 'ui' table (if it doesn't exist)
        let ui_table = self.lua.create_table()?;
        ui_table.set("status_line", status_line_table)?;

        let daemon_state_clone = self.daemon_state.clone();
        let get_active_session = self.lua.create_function(move |lua, ()| {
            if let Ok(guard) = daemon_state_clone.read() {
                if let Some(session_id) = guard.active_session {
                    Ok(mlua::Value::String(lua.create_string(session_id.to_string())?))
                } else {
                    Ok(mlua::Value::Nil)
                }
            } else {
                Ok(mlua::Value::Nil)
            }
        })?;

        self.lua.globals().set("ui", ui_table)?;
        self.lua.globals().set("get_active_session", get_active_session)?;

        Ok(())
    }

    fn resolve_status_line_state(&mut self) -> Result<StatusLineState> {
        let ui_table: mlua::Table = self.lua.globals().get("ui")?;
        let status_line_config: mlua::Table = ui_table.get("status_line")?;
        let sections_config: mlua::Table = status_line_config.get("sections")?;
        let enabled: mlua::Value = status_line_config.get("enabled")?;

        if let mlua::Value::Boolean(false) = enabled {
            return Ok(StatusLineState::disabled());
        }

        let mut status_line_state = StatusLineState::default();
        for pair in sections_config.pairs::<String, mlua::Table>() {
            let (key, val) = pair?;
            for pair in val.pairs::<mlua::Value, mlua::Value>() {
                let (_, val) = pair?;
                let resolved_string = match val {
                    mlua::Value::String(s) => {
                        // Item is a direct string
                        s.to_str()?.to_owned()
                    }
                    mlua::Value::Function(func) => {
                        // Item is a Lua function, execute it and get the string result
                        func.call::<String>(())?
                    }
                    mlua::Value::Nil => {
                        // Ignore nil values which can happen in sparse tables
                        continue;
                    }
                    _ => {
                        warn!("Ignoring lua type");
                        continue;
                    }
                };

                match key.as_str() {
                    "a" => {
                        status_line_state.a.push(resolved_string);
                    }
                    "b" => {
                        status_line_state.b.push(resolved_string);
                    }
                    "c" => {
                        status_line_state.c.push(resolved_string);
                    }
                    _ => {}
                }
            }
        }

        Ok(status_line_state)
    }

    pub fn spawn(ui_handle: UIHandle) -> Result<LuaHandle> {
        let (tx, rx) = mpsc::channel();
        let handle = LuaHandle { tx };
        let handle_clone = handle.clone();
        tokio::task::spawn_blocking(|| {
            let mut actor = Self::new(ui_handle, handle, rx);
            actor.initialize_lua_state().unwrap();
            let code = fs::read_to_string("defaults/statusbar.lua").unwrap();
            let runtime = Handle::current();
            loop {
                match actor.rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(event) => match event {
                        Kill => {
                            debug!("killed");
                            break;
                        }
                        SyncDaemonState(daemon_state) => {
                            if let Ok(mut guard) = actor.daemon_state.write() {
                                *guard = daemon_state;
                            }
                        }
                    },
                    Err(RecvTimeoutError::Disconnected) => {
                        warn!("disconnected");
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }

                actor.lua.load(&code).exec().unwrap();
                let status_line_state = actor.resolve_status_line_state().unwrap();
                let ui_handle_clone = actor.ui_handle.clone();
                let status_line_state_clone = status_line_state.clone();
                runtime
                    .block_on(async move { ui_handle_clone.sync_status_line_state(status_line_state_clone).await })?;
            }
            Ok::<(), Error>(())
        });

        Ok(handle_clone)
    }
}
