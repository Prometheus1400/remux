use std::{fs, path::PathBuf};

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
        let code = fs::read_to_string(default_status_line_path())?;
        Ok(Self { lua, code })
    }

    pub fn enabled(&self) -> Result<bool> {
        Ok(self.read_template_or_fallback().enabled)
    }

    pub fn render(&self, width: u16, active_session_name: Option<&str>) -> Result<crate::render::surface::Surface> {
        let template = self.read_template_or_fallback();
        Ok(StatusLineRenderer::from_template(template).render(width, active_session_name))
    }

    fn read_template(&self) -> Result<StatusLineTemplate> {
        self.lua.load(&self.code).exec()?;
        let ui_table: mlua::Table = self.lua.globals().get("ui")?;
        let status_line_config: mlua::Table = ui_table.get("status_line")?;
        let sections_config: mlua::Table = status_line_config.get("sections")?;
        let enabled = status_line_config.get::<Option<bool>>("enabled")?.unwrap_or(true);

        Ok(StatusLineTemplate {
            enabled,
            a: read_section(&sections_config, "a")?,
            b: read_section(&sections_config, "b")?,
            c: read_section(&sections_config, "c")?,
        })
    }

    fn read_template_or_fallback(&self) -> StatusLineTemplate {
        match self.read_template() {
            Ok(template) => template,
            Err(error) => {
                warn!(error = %error, "failed to evaluate Lua status line config, using fallback template");
                fallback_template()
            }
        }
    }
}

fn default_status_line_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("daemon crate should live under the workspace root")
        .join("defaults/statusbar.lua")
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
    let mut indexed_values = Vec::new();
    for pair in section.pairs::<Value, Value>() {
        let (index, value) = pair?;
        let Some(index) = array_index(index) else {
            continue;
        };
        indexed_values.push((index, value));
    }
    indexed_values.sort_by_key(|(index, _)| *index);

    let mut items = Vec::new();
    for (_, value) in indexed_values {
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

fn array_index(value: Value) -> Option<i64> {
    match value {
        Value::Integer(index) if index > 0 => Some(index),
        Value::Number(index) if index.fract() == 0.0 && index > 0.0 => Some(index as i64),
        _ => None,
    }
}

fn fallback_template() -> StatusLineTemplate {
    StatusLineTemplate {
        enabled: true,
        a: vec!["active-session".to_owned()],
        b: Vec::new(),
        c: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn surface_to_string(surface: &crate::render::surface::Surface) -> String {
        (0..surface.width())
            .map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char)
            .collect()
    }

    #[test]
    fn default_statusbar_lua_path_exists() {
        assert!(default_status_line_path().exists());
    }

    #[test]
    fn default_statusbar_lua_renders_session_name() -> Result<()> {
        let runtime = StatusLineRuntime::load_default()?;

        let surface = runtime.render(60, Some("alpha"))?;
        let rendered = surface_to_string(&surface);

        assert!(rendered.contains("alpha"), "rendered status line: {rendered:?}");
        Ok(())
    }

    #[test]
    fn lua_status_line_preserves_section_item_order() -> Result<()> {
        let mut lua = Lua::default();
        initialize_lua_state(&mut lua)?;
        let runtime = StatusLineRuntime {
            lua,
            code: r#"
                ui.status_line = {
                    sections = {
                        a = {"first", "second", "third"},
                        b = {},
                        c = {},
                    },
                }
            "#
            .to_owned(),
        };

        let surface = runtime.render(40, None)?;
        let rendered = surface_to_string(&surface);

        assert!(
            rendered.contains("first | second | third"),
            "rendered status line: {rendered:?}"
        );
        Ok(())
    }

    #[test]
    fn lua_status_line_falls_back_when_template_eval_fails() -> Result<()> {
        let mut lua = Lua::default();
        initialize_lua_state(&mut lua)?;
        let runtime = StatusLineRuntime {
            lua,
            code: r#"
                ui.status_line = {
                    sections = {
                        a = {
                            function()
                                error("boom")
                            end,
                        },
                        b = {},
                        c = {},
                    },
                }
            "#
            .to_owned(),
        };

        let surface = runtime.render(30, Some("fallback-session"))?;
        let rendered = surface_to_string(&surface);

        assert!(rendered.contains("fallback-session"));
        Ok(())
    }
}
