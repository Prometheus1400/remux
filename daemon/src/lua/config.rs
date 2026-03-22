use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use mlua::{Lua, Table, Value};
use remux_core::{
    config::{BuiltinAction, KeyAction, KeyBinding},
    daemon_utils::get_config_path,
};

use crate::{
    prelude::*,
    render::{
        bar::{BarRenderer, BarSpec, BarStyle},
        overlay::SessionSwitcherStyle,
        surface::Surface,
    },
};

#[derive(Clone, Debug)]
pub struct ConfigRuntime {
    key_bindings: Vec<KeyBinding>,
    named_actions: HashMap<String, Vec<BuiltinAction>>,
    status_bar_spec: BarSpec,
    session_switcher_style: SessionSwitcherStyle,
}

impl ConfigRuntime {
    pub fn load() -> Result<Self> {
        let config_path = get_config_path().ok();
        let code = match config_path {
            Some(path) if path.exists() => fs::read_to_string(path)?,
            _ => fs::read_to_string(default_init_path())?,
        };
        Self::from_code(code)
    }

    pub fn key_bindings(&self) -> &[KeyBinding] {
        &self.key_bindings
    }

    pub fn status_bar_enabled(&self) -> Result<bool> {
        Ok(self.status_bar_spec.enabled)
    }

    pub fn render_status_bar(&self, width: u16, active_session_name: Option<&str>) -> Result<Surface> {
        let mut spec = self.status_bar_spec.clone();
        hydrate_bar_items(&mut spec.left, active_session_name);
        hydrate_bar_items(&mut spec.center, active_session_name);
        hydrate_bar_items(&mut spec.right, active_session_name);
        Ok(BarRenderer::from_spec(spec).render(width))
    }

    pub fn session_switcher_style(&self) -> Result<SessionSwitcherStyle> {
        Ok(self.session_switcher_style.clone())
    }

    pub fn invoke_named_action(&self, name: &str) -> Result<Vec<BuiltinAction>> {
        self.named_actions
            .get(name)
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("unknown named action '{name}'"))
    }

    fn from_code(code: String) -> Result<Self> {
        let lua = Lua::default();
        initialize_lua_state(&lua)?;
        lua.load(&code).exec()?;
        let remux = read_remux_table(&lua)?;

        Ok(Self {
            key_bindings: read_key_bindings(&remux)?,
            named_actions: read_named_actions(&remux)?,
            status_bar_spec: read_bar_spec(&remux, "status")?,
            session_switcher_style: read_session_switcher_style(&remux)?,
        })
    }
}

fn hydrate_bar_items(items: &mut [String], active_session_name: Option<&str>) {
    for item in items {
        if item == "active-session" {
            *item = active_session_name.unwrap_or_default().to_owned();
        }
    }
}

fn default_init_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("daemon crate should live under the workspace root")
        .join("defaults/init.lua")
}

fn initialize_lua_state(lua: &Lua) -> Result<()> {
    let remux = lua.create_table()?;
    remux.set("prefix", "C-b")?;
    remux.set("keymaps", lua.create_table()?)?;
    remux.set("actions", lua.create_table()?)?;
    remux.set("ui", default_ui_table(lua)?)?;
    lua.globals().set("remux", remux)?;
    Ok(())
}

fn default_ui_table(lua: &Lua) -> Result<Table> {
    let status = lua.create_table()?;
    status.set("enabled", true)?;
    status.set("sections", section_table(lua)?)?;
    status.set("style", default_bar_style_table(lua)?)?;

    let bars = lua.create_table()?;
    bars.set("status", status)?;

    let overlays = lua.create_table()?;
    overlays.set("session_switcher", default_session_switcher_style_table(lua)?)?;

    let ui = lua.create_table()?;
    ui.set("bars", bars)?;
    ui.set("overlays", overlays)?;
    Ok(ui)
}

fn section_table(lua: &Lua) -> Result<Table> {
    let sections = lua.create_table()?;
    sections.set("left", lua.create_table()?)?;
    sections.set("center", lua.create_table()?)?;
    sections.set("right", lua.create_table()?)?;
    Ok(sections)
}

fn default_bar_style_table(lua: &Lua) -> Result<Table> {
    let style = BarStyle::default();
    let table = lua.create_table()?;
    write_bar_style(&table, &style)?;
    Ok(table)
}

fn default_session_switcher_style_table(lua: &Lua) -> Result<Table> {
    let style = SessionSwitcherStyle::default();
    let table = lua.create_table()?;
    write_session_switcher_style(&table, &style)?;
    Ok(table)
}

fn write_bar_style(table: &Table, style: &BarStyle) -> Result<()> {
    table.set("background_fg", style.background_fg)?;
    table.set("background_bg", style.background_bg)?;
    table.set("left_fg", style.left_fg)?;
    table.set("left_bg", style.left_bg)?;
    table.set("center_fg", style.center_fg)?;
    table.set("center_bg", style.center_bg)?;
    table.set("right_fg", style.right_fg)?;
    table.set("right_bg", style.right_bg)?;
    Ok(())
}

fn write_session_switcher_style(table: &Table, style: &SessionSwitcherStyle) -> Result<()> {
    table.set("title", style.title.clone())?;
    table.set("footer", style.footer.clone())?;
    table.set("border_fg", style.border_fg)?;
    table.set("background_fg", style.background_fg)?;
    table.set("background_bg", style.background_bg)?;
    table.set("title_fg", style.title_fg)?;
    table.set("text_fg", style.text_fg)?;
    table.set("selected_fg", style.selected_fg)?;
    table.set("selected_bg", style.selected_bg)?;
    table.set("footer_fg", style.footer_fg)?;
    Ok(())
}

fn read_remux_table(lua: &Lua) -> Result<Table> {
    Ok(lua.globals().get("remux")?)
}

fn read_key_bindings(remux: &Table) -> Result<Vec<KeyBinding>> {
    let action_names = read_action_names(remux)?;
    let prefix = read_prefix_sequence(remux)?;
    let keymaps = match remux.get::<Value>("keymaps")? {
        Value::Nil => return Ok(Vec::new()),
        Value::Table(table) => table,
        other => return Err(color_eyre::eyre::eyre!("expected keymaps table, got {other:?}")),
    };
    let mut bindings = Vec::new();
    let mut seen_sequences = HashSet::new();

    for table in read_array_tables(&keymaps)? {
        let key = table.get::<String>("key")?;
        let action_name = table.get::<String>("action")?;
        let prefixed = table.get::<Option<bool>>("prefix")?.unwrap_or(true);
        let action = parse_action_name(&action_name, &action_names)?;
        let mut sequence = if prefixed { prefix.clone() } else { Vec::new() };
        sequence.extend(parse_key_notation(&key)?);

        if !seen_sequences.insert(sequence.clone()) {
            return Err(color_eyre::eyre::eyre!("duplicate key binding for '{key}'"));
        }

        bindings.push(KeyBinding { sequence, action });
    }

    Ok(bindings)
}

fn read_prefix_sequence(remux: &Table) -> Result<Vec<u8>> {
    let prefix = remux
        .get::<Option<String>>("prefix")?
        .unwrap_or_else(|| "C-b".to_owned());
    parse_key_notation(&prefix)
}

fn read_named_actions(remux: &Table) -> Result<HashMap<String, Vec<BuiltinAction>>> {
    let actions = match remux.get::<Value>("actions")? {
        Value::Nil => return Ok(HashMap::new()),
        Value::Table(table) => table,
        other => return Err(color_eyre::eyre::eyre!("expected actions table, got {other:?}")),
    };

    let mut named_actions = HashMap::new();
    for pair in actions.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(name) = key else {
            continue;
        };
        let Value::Function(function) = value else {
            continue;
        };
        let action_name = name.to_str()?.to_owned();
        let value = function.call::<Value>(())?;
        named_actions.insert(action_name, read_action_return(value)?);
    }

    Ok(named_actions)
}

fn read_action_names(remux: &Table) -> Result<HashSet<String>> {
    let actions = match remux.get::<Value>("actions")? {
        Value::Nil => return Ok(HashSet::new()),
        Value::Table(table) => table,
        other => return Err(color_eyre::eyre::eyre!("expected actions table, got {other:?}")),
    };
    let mut action_names = HashSet::new();
    for pair in actions.pairs::<Value, Value>() {
        let (key, value) = pair?;
        if matches!(value, Value::Function(_)) {
            let Value::String(name) = key else {
                continue;
            };
            action_names.insert(name.to_str()?.to_owned());
        }
    }
    Ok(action_names)
}

fn parse_action_name(action_name: &str, action_names: &HashSet<String>) -> Result<KeyAction> {
    let action = match action_name {
        "split-pane-vertical" => KeyAction::Builtin(BuiltinAction::SplitPaneVertical),
        "split-pane-horizontal" => KeyAction::Builtin(BuiltinAction::SplitPaneHorizontal),
        "focus-pane-left" => KeyAction::Builtin(BuiltinAction::FocusPaneLeft),
        "focus-pane-down" => KeyAction::Builtin(BuiltinAction::FocusPaneDown),
        "focus-pane-up" => KeyAction::Builtin(BuiltinAction::FocusPaneUp),
        "focus-pane-right" => KeyAction::Builtin(BuiltinAction::FocusPaneRight),
        "kill-pane" => KeyAction::Builtin(BuiltinAction::KillPane),
        "detach" => KeyAction::Builtin(BuiltinAction::Detach),
        "open-session-switcher" => KeyAction::Builtin(BuiltinAction::OpenSessionSwitcher),
        name if action_names.contains(name) => KeyAction::Named(name.to_owned()),
        name => return Err(color_eyre::eyre::eyre!("unknown action '{name}'")),
    };
    Ok(action)
}

fn read_action_return(value: Value) -> Result<Vec<BuiltinAction>> {
    match value {
        Value::Nil => Ok(Vec::new()),
        Value::String(s) => Ok(vec![parse_builtin_action_name(&s.to_str()?)?]),
        Value::Table(table) => {
            let mut actions = Vec::new();
            for value in read_array_values(&table)? {
                match value {
                    Value::String(name) => actions.push(parse_builtin_action_name(&name.to_str()?)?),
                    other => {
                        return Err(color_eyre::eyre::eyre!(
                            "named action callback returned unsupported value {other:?}"
                        ));
                    }
                }
            }
            Ok(actions)
        }
        other => Err(color_eyre::eyre::eyre!(
            "named action callback returned unsupported value {other:?}"
        )),
    }
}

fn parse_builtin_action_name(name: &str) -> Result<BuiltinAction> {
    match name {
        "split-pane-vertical" => Ok(BuiltinAction::SplitPaneVertical),
        "split-pane-horizontal" => Ok(BuiltinAction::SplitPaneHorizontal),
        "focus-pane-left" => Ok(BuiltinAction::FocusPaneLeft),
        "focus-pane-down" => Ok(BuiltinAction::FocusPaneDown),
        "focus-pane-up" => Ok(BuiltinAction::FocusPaneUp),
        "focus-pane-right" => Ok(BuiltinAction::FocusPaneRight),
        "kill-pane" => Ok(BuiltinAction::KillPane),
        "detach" => Ok(BuiltinAction::Detach),
        "open-session-switcher" => Ok(BuiltinAction::OpenSessionSwitcher),
        _ => Err(color_eyre::eyre::eyre!("unknown builtin action '{name}'")),
    }
}

fn parse_key_notation(input: &str) -> Result<Vec<u8>> {
    let input = input.trim();
    if input.is_empty() {
        return Err(color_eyre::eyre::eyre!("key notation cannot be empty"));
    }

    let mut parts = input.split('-').peekable();
    let mut ctrl = false;
    let mut alt = false;

    while let Some(part) = parts.peek().copied() {
        if parts.clone().count() == 1 {
            break;
        }

        if part.eq_ignore_ascii_case("c") || part.eq_ignore_ascii_case("ctrl") {
            ctrl = true;
            parts.next();
            continue;
        }
        if part.eq_ignore_ascii_case("m") || part.eq_ignore_ascii_case("a") || part.eq_ignore_ascii_case("alt") {
            alt = true;
            parts.next();
            continue;
        }
        break;
    }

    let key = parts.collect::<Vec<_>>().join("-");
    if key.is_empty() {
        return Err(color_eyre::eyre::eyre!("missing key in notation '{input}'"));
    }

    let mut bytes = parse_base_key(&key, ctrl)?;
    if alt {
        let mut with_alt = vec![0x1b];
        with_alt.append(&mut bytes);
        Ok(with_alt)
    } else {
        Ok(bytes)
    }
}

fn parse_base_key(key: &str, ctrl: bool) -> Result<Vec<u8>> {
    let lowered = key.to_ascii_lowercase();
    let bytes = match lowered.as_str() {
        "enter" | "return" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            vec![b'\r']
        }
        "escape" | "esc" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            vec![0x1b]
        }
        "tab" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            vec![b'\t']
        }
        "backspace" | "bs" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            vec![0x7f]
        }
        "space" => {
            if ctrl {
                vec![0x00]
            } else {
                vec![b' ']
            }
        }
        "up" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            b"\x1b[A".to_vec()
        }
        "down" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            b"\x1b[B".to_vec()
        }
        "right" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            b"\x1b[C".to_vec()
        }
        "left" => {
            ensure_no_ctrl_modifier(key, ctrl)?;
            b"\x1b[D".to_vec()
        }
        _ => parse_literal_key(key, ctrl)?,
    };

    Ok(bytes)
}

fn ensure_no_ctrl_modifier(key: &str, ctrl: bool) -> Result<()> {
    if ctrl {
        return Err(color_eyre::eyre::eyre!(
            "Ctrl modifier is not supported for key '{key}'"
        ));
    }
    Ok(())
}

fn parse_literal_key(key: &str, ctrl: bool) -> Result<Vec<u8>> {
    let mut chars = key.chars();
    let ch = chars
        .next()
        .ok_or_else(|| color_eyre::eyre::eyre!("key notation cannot be empty"))?;
    if chars.next().is_some() {
        return Err(color_eyre::eyre::eyre!("unsupported key notation '{key}'"));
    }
    if !ch.is_ascii() {
        return Err(color_eyre::eyre::eyre!("non-ASCII key '{key}' is not supported"));
    }

    let byte = if ctrl { ctrl_byte(ch)? } else { ch as u8 };
    Ok(vec![byte])
}

fn ctrl_byte(ch: char) -> Result<u8> {
    let lower = ch.to_ascii_lowercase();
    let byte = match lower {
        '@' | ' ' => 0x00,
        'a'..='z' => (lower as u8) - b'a' + 1,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        '?' => 0x7f,
        _ => return Err(color_eyre::eyre::eyre!("Ctrl modifier is not supported for key '{ch}'")),
    };
    Ok(byte)
}

fn read_bar_spec(remux: &Table, name: &str) -> Result<BarSpec> {
    let ui = match remux.get::<Value>("ui")? {
        Value::Table(table) => table,
        _ => return Ok(fallback_bar_spec()),
    };
    let bars = match ui.get::<Value>("bars")? {
        Value::Table(table) => table,
        _ => return Ok(fallback_bar_spec()),
    };
    let bar = match bars.get::<Value>(name)? {
        Value::Table(table) => table,
        _ => return Ok(fallback_bar_spec()),
    };
    let sections = match bar.get::<Value>("sections")? {
        Value::Table(table) => table,
        _ => return Ok(fallback_bar_spec()),
    };
    let style = match bar.get::<Value>("style")? {
        Value::Table(table) => read_bar_style(table)?,
        _ => BarStyle::default(),
    };

    Ok(BarSpec {
        enabled: bar.get::<Option<bool>>("enabled")?.unwrap_or(true),
        left: read_bar_section(&sections, "left")?,
        center: read_bar_section(&sections, "center")?,
        right: read_bar_section(&sections, "right")?,
        style,
    })
}

fn read_bar_style(table: Table) -> Result<BarStyle> {
    let mut style = BarStyle::default();
    style.background_fg = table.get::<Option<u8>>("background_fg")?.unwrap_or(style.background_fg);
    style.background_bg = table.get::<Option<u8>>("background_bg")?.unwrap_or(style.background_bg);
    style.left_fg = table.get::<Option<u8>>("left_fg")?.unwrap_or(style.left_fg);
    style.left_bg = table.get::<Option<u8>>("left_bg")?.unwrap_or(style.left_bg);
    style.center_fg = table.get::<Option<u8>>("center_fg")?.unwrap_or(style.center_fg);
    style.center_bg = table.get::<Option<u8>>("center_bg")?.unwrap_or(style.center_bg);
    style.right_fg = table.get::<Option<u8>>("right_fg")?.unwrap_or(style.right_fg);
    style.right_bg = table.get::<Option<u8>>("right_bg")?.unwrap_or(style.right_bg);
    Ok(style)
}

fn read_bar_section(table: &Table, key: &str) -> Result<Vec<String>> {
    let section: Table = table.get(key)?;
    let mut items = Vec::new();
    for value in read_array_values(&section)? {
        match value {
            Value::String(s) => {
                let text = s.to_str()?.to_owned();
                if !text.is_empty() {
                    items.push(text);
                }
            }
            Value::Function(function) => {
                if let Some(value) = function.call::<Option<String>>(())? {
                    if !value.is_empty() {
                        items.push(value);
                    }
                }
            }
            Value::Nil => {}
            other => warn!(section = key, "ignoring unsupported bar item value: {other:?}"),
        }
    }
    Ok(items)
}

fn read_session_switcher_style(remux: &Table) -> Result<SessionSwitcherStyle> {
    let ui = match remux.get::<Value>("ui")? {
        Value::Table(table) => table,
        _ => return Ok(SessionSwitcherStyle::default()),
    };
    let overlays = match ui.get::<Value>("overlays")? {
        Value::Table(table) => table,
        _ => return Ok(SessionSwitcherStyle::default()),
    };
    let table = match overlays.get::<Value>("session_switcher")? {
        Value::Table(table) => table,
        _ => return Ok(SessionSwitcherStyle::default()),
    };
    let mut style = SessionSwitcherStyle::default();
    style.title = table.get::<Option<String>>("title")?.unwrap_or(style.title);
    style.footer = table.get::<Option<String>>("footer")?.unwrap_or(style.footer);
    style.border_fg = table.get::<Option<u8>>("border_fg")?.unwrap_or(style.border_fg);
    style.background_fg = table.get::<Option<u8>>("background_fg")?.unwrap_or(style.background_fg);
    style.background_bg = table.get::<Option<u8>>("background_bg")?.unwrap_or(style.background_bg);
    style.title_fg = table.get::<Option<u8>>("title_fg")?.unwrap_or(style.title_fg);
    style.text_fg = table.get::<Option<u8>>("text_fg")?.unwrap_or(style.text_fg);
    style.selected_fg = table.get::<Option<u8>>("selected_fg")?.unwrap_or(style.selected_fg);
    style.selected_bg = table.get::<Option<u8>>("selected_bg")?.unwrap_or(style.selected_bg);
    style.footer_fg = table.get::<Option<u8>>("footer_fg")?.unwrap_or(style.footer_fg);
    Ok(style)
}

fn read_array_tables(table: &Table) -> Result<Vec<Table>> {
    read_array_values(table)?
        .into_iter()
        .map(|value| match value {
            Value::Table(table) => Ok(table),
            other => Err(color_eyre::eyre::eyre!("expected Lua table, got {other:?}")),
        })
        .collect()
}

fn read_array_values(table: &Table) -> Result<Vec<Value>> {
    let mut indexed_values = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (index, value) = pair?;
        let Some(index) = array_index(index) else {
            continue;
        };
        indexed_values.push((index, value));
    }
    indexed_values.sort_by_key(|(index, _)| *index);
    Ok(indexed_values.into_iter().map(|(_, value)| value).collect())
}

fn array_index(value: Value) -> Option<i64> {
    match value {
        Value::Integer(index) if index > 0 => Some(index),
        Value::Number(index) if index.fract() == 0.0 && index > 0.0 => Some(index as i64),
        _ => None,
    }
}

fn fallback_bar_spec() -> BarSpec {
    BarSpec {
        enabled: true,
        left: vec!["active-session".to_owned()],
        center: Vec::new(),
        right: Vec::new(),
        style: BarStyle::default(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    fn unique_config_home() -> PathBuf {
        std::env::temp_dir().join(format!(
            "remux-config-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos()
        ))
    }

    fn runtime_from_code(code: &str) -> Result<ConfigRuntime> {
        ConfigRuntime::from_code(code.to_owned())
    }

    fn surface_to_string(surface: &Surface) -> String {
        (0..surface.width())
            .map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char)
            .collect()
    }

    #[test]
    fn default_init_path_exists() {
        assert!(default_init_path().exists());
    }

    #[test]
    fn runtime_loads_user_init_lua_when_present() -> Result<()> {
        let config_home = unique_config_home();
        let remux_dir = config_home.join("remux");
        fs::create_dir_all(&remux_dir)?;
        fs::write(remux_dir.join("init.lua"), "remux.keymaps = {}")?;

        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
            std::env::remove_var("HOME");
        }

        let runtime = ConfigRuntime::load()?;
        let bindings = runtime.key_bindings();
        assert!(bindings.is_empty());

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        fs::remove_dir_all(config_home)?;
        Ok(())
    }

    #[test]
    fn lua_key_bindings_preserve_order_and_resolve_named_actions() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.prefix = "C-b"
                remux.actions.pick_next = function()
                    return "focus-pane-right"
                end
                remux.keymaps = {
                    { key = "n", action = "focus-pane-right" },
                    { key = "a", action = "pick_next" },
                }
            "#,
        )?;

        let bindings = runtime.key_bindings();

        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].sequence, b"\x02n");
        assert!(matches!(
            bindings[0].action,
            KeyAction::Builtin(BuiltinAction::FocusPaneRight)
        ));
        assert!(matches!(bindings[1].action, KeyAction::Named(ref name) if name == "pick_next"));
        Ok(())
    }

    #[test]
    fn named_action_callback_can_return_multiple_builtin_actions() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.actions.combo = function()
                    return { "focus-pane-right", "kill-pane" }
                end
            "#,
        )?;

        let actions = runtime.invoke_named_action("combo")?;

        assert_eq!(actions, vec![BuiltinAction::FocusPaneRight, BuiltinAction::KillPane]);
        Ok(())
    }

    #[test]
    fn readable_key_notation_supports_special_keys_and_modifiers() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.prefix = "Escape"
                remux.actions.combo = function()
                    return "focus-pane-right"
                end
                remux.keymaps = {
                    { key = "Up", action = "focus-pane-right" },
                    { key = "Enter", action = "combo" },
                    { key = "M-x", action = "kill-pane" },
                }
            "#,
        )?;

        let bindings = runtime.key_bindings();

        assert_eq!(bindings[0].sequence, b"\x1b\x1b[A");
        assert_eq!(bindings[1].sequence, b"\x1b\r");
        assert_eq!(bindings[2].sequence, b"\x1b\x1bx");
        Ok(())
    }

    #[test]
    fn ctrl_space_prefix_compiles_to_nul_byte() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.prefix = "C-space"
                remux.keymaps = {
                    { key = "n", action = "focus-pane-right" },
                }
            "#,
        )?;

        let bindings = runtime.key_bindings();

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].sequence, b"\x00n");
        Ok(())
    }

    #[test]
    fn invalid_readable_key_notation_fails_config_load() {
        let err = runtime_from_code(
            r#"
                remux.prefix = "C-b"
                remux.keymaps = {
                    { key = "F1", action = "focus-pane-right" },
                }
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unsupported key notation"));
    }

    #[test]
    fn duplicate_compiled_bindings_fail_config_load() {
        let err = runtime_from_code(
            r#"
                remux.prefix = "C-b"
                remux.keymaps = {
                    { key = "n", action = "focus-pane-right" },
                    { key = "n", action = "focus-pane-left" },
                }
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("duplicate key binding"));
    }

    #[test]
    fn unprefixed_keymap_skips_prefix_bytes() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.prefix = "C-b"
                remux.keymaps = {
                    { key = "C-h", action = "focus-pane-left", prefix = false },
                    { key = "C-l", action = "focus-pane-right" },
                }
            "#,
        )?;

        let bindings = runtime.key_bindings();

        assert_eq!(bindings[0].sequence, b"\x08");
        assert_eq!(bindings[1].sequence, b"\x02\x0c");
        Ok(())
    }

    #[test]
    fn removed_iterative_pane_actions_fail_config_load() {
        let err = runtime_from_code(
            r#"
                remux.keymaps = {
                    { key = "n", action = "next-pane" },
                }
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unknown action 'next-pane'"));
    }

    #[test]
    fn lua_status_bar_renders_active_session_and_preserves_item_order() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.ui.bars.status = {
                    enabled = true,
                    sections = {
                        left = { "active-session", "two" },
                        center = {},
                        right = { "three" },
                    },
                    style = {},
                }
            "#,
        )?;

        let surface = runtime.render_status_bar(30, Some("alpha"))?;
        let rendered = surface_to_string(&surface);

        assert!(rendered.starts_with("alpha | two"));
        assert!(rendered.trim_end().ends_with("three"));
        Ok(())
    }

    #[test]
    fn lua_status_bar_falls_back_to_default_style_when_omitted() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.ui.bars.status.sections.center = {
                    function()
                        return "clock"
                    end,
                }
            "#,
        )?;

        let surface = runtime.render_status_bar(20, None)?;
        let rendered = surface_to_string(&surface);

        assert!(rendered.contains("clock"));
        Ok(())
    }

    #[test]
    fn session_switcher_style_reads_lua_overrides() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.ui.overlays.session_switcher = {
                    title = "Workspaces",
                    footer = "j/k move",
                    border_fg = 33,
                }
            "#,
        )?;

        let style = runtime.session_switcher_style()?;

        assert_eq!(style.title, "Workspaces");
        assert_eq!(style.footer, "j/k move");
        assert_eq!(style.border_fg, 33);
        Ok(())
    }
}
