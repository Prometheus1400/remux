use std::{fs, time::Duration};

use color_eyre::eyre;
use mlua::Lua;
use tokio::sync::broadcast;

use crate::{prelude::*, states::status_line_state::StatusLineState};

fn initialize_lua_state(lua: &mut Lua) -> Result<()> {
    info!("Initializing lua state");
    let sections_table = lua.create_table()?;
    let section_a = lua.create_table()?;
    let section_b = lua.create_table()?;
    let section_c = lua.create_table()?;
    sections_table.set("a", section_a)?;
    sections_table.set("b", section_b)?;
    sections_table.set("c", section_c)?;
    let status_line_table = lua.create_table()?;
    status_line_table.set("sections", sections_table)?;
    status_line_table.set("enabled", true)?;
    let ui_table = lua.create_table()?;
    ui_table.set("status_line", status_line_table)?;
    lua.globals().set("ui", ui_table)?;
    Ok(())
}

pub fn start_status_line_task(tx: broadcast::Sender<StatusLineState>) -> Result<CliTask> {
    let mut lua = Lua::default();
    initialize_lua_state(&mut lua)?;
    let code = fs::read_to_string("defaults/statusbar.lua")?;

    info!("Starting lua status line task");
    let task: CliTask = tokio::spawn({
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                lua.load(&code).exec().unwrap();
                let ui_table: mlua::Table = lua.globals().get("ui")?;
                let status_line_config: mlua::Table = ui_table.get("status_line")?;
                let sections_config: mlua::Table = status_line_config.get("sections")?;
                let enabled: mlua::Value = status_line_config.get("enabled")?;

                if let mlua::Value::Boolean(false) = enabled {
                    let _ = tx.send(StatusLineState::disabled());
                    continue;
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
                let _ = tx.send(status_line_state);
            }
        }
    });

    eyre::Ok(task)
}
