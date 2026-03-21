use std::fs;

use mlua::{Lua, Value};

use crate::{
    prelude::*,
    render::status_line::{StatusLineRenderer, StatusLineTemplate},
};

pub struct StatusLineRuntime {
    lua: Lua,
    code: String,
}

impl StatusLineRuntime {
    pub fn load_default() -> Result<Self> {
        let mut lua = Lua::default();
        initialize_lua_state(&mut lua)?;
        let code = fs::read_to_string("defaults/statusbar.lua")?;
        Ok(Self { lua, code })
    }

    pub fn enabled(&self) -> Result<bool> {
        Ok(self.read_template()?.enabled)
    }

    pub fn render(&self, width: u16, active_session_name: Option<&str>) -> Result<crate::render::surface::Surface> {
        let template = self.read_template()?;
        Ok(StatusLineRenderer::from_template(template).render(width, active_session_name))
    }

    fn read_template(&self) -> Result<StatusLineTemplate> {
        self.lua.load(&self.code).exec()?;
        let ui_table: mlua::Table = self.lua.globals().get("ui")?;
        let status_line_config: mlua::Table = ui_table.get("status_line")?;
        let sections_config: mlua::Table = status_line_config.get("sections")?;
        let enabled: bool = status_line_config.get("enabled").unwrap_or(true);

        Ok(StatusLineTemplate {
            enabled,
            a: read_section(&sections_config, "a")?,
            b: read_section(&sections_config, "b")?,
            c: read_section(&sections_config, "c")?,
        })
    }
}

fn initialize_lua_state(lua: &mut Lua) -> Result<()> {
    let sections_table = lua.create_table()?;
    sections_table.set("a", lua.create_table()?)?;
    sections_table.set("b", lua.create_table()?)?;
    sections_table.set("c", lua.create_table()?)?;

    let status_line_table = lua.create_table()?;
    status_line_table.set("sections", sections_table)?;
    status_line_table.set("enabled", true)?;

    let ui_table = lua.create_table()?;
    ui_table.set("status_line", status_line_table)?;
    lua.globals().set("ui", ui_table)?;
    Ok(())
}

fn read_section(table: &mlua::Table, key: &str) -> Result<Vec<String>> {
    let section: mlua::Table = table.get(key)?;
    let mut items = Vec::new();
    for pair in section.pairs::<Value, Value>() {
        let (_, value) = pair?;
        match value {
            Value::String(s) => items.push(s.to_str()?.to_owned()),
            Value::Function(func) => {
                if let Some(value) = func.call::<Option<String>>(())? {
                    items.push(value);
                }
            }
            Value::Nil => {}
            _ => warn!(section = key, "Ignoring unsupported Lua status line value"),
        }
    }
    Ok(items)
}
