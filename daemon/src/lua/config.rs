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
        bar::{BarItem, BarSpec, BarStyle, WindowTab},
        overlay::{FuzzySelectorStyle, SelectorItem, SelectorOverlay, SelectorStyle},
        widget::{DockEdge, DockWidgetKind, DockedWidgetSpec},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSessionState {
    pub id: u32,
    pub name: String,
    pub is_current: bool,
    pub windows: Vec<RuntimeWindowState>,
    pub current_window: Option<RuntimeWindowState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeWindowState {
    pub id: u32,
    pub index: usize,
    pub name: String,
    pub is_active: bool,
    pub panes: Vec<RuntimePaneState>,
    pub active_pane: Option<RuntimePaneState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePaneState {
    pub id: usize,
    pub is_active: bool,
    pub rect: RuntimeRectState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRectState {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeWidgetState {
    pub id: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaRuntimeState {
    pub sessions: Vec<RuntimeSessionState>,
    pub current_session: Option<RuntimeSessionState>,
    pub current_window: Option<RuntimeWindowState>,
    pub current_pane: Option<RuntimePaneState>,
    pub active_widget: Option<RuntimeWidgetState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCommand {
    Builtin(BuiltinAction),
    OpenWidget(String),
    SwitchSession(String),
}

#[derive(Clone, Debug)]
struct SelectorWidgetSpec {
    title: String,
    footer: String,
    style: SelectorStyle,
}

#[derive(Clone, Debug)]
struct FuzzySelectorWidgetSpec {
    title: String,
    footer: String,
    placeholder: String,
    style: FuzzySelectorStyle,
}

#[derive(Clone, Debug)]
enum WidgetSpec {
    Selector(SelectorWidgetSpec),
    FuzzySelector(FuzzySelectorWidgetSpec),
}

#[derive(Clone, Debug)]
struct ParsedWidgets {
    overlay_widgets: HashMap<String, WidgetSpec>,
    docked_widgets: Vec<DockedWidgetSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedFuzzySelectorWidget {
    pub title: String,
    pub footer: String,
    pub placeholder: String,
    pub items: Vec<SelectorItem>,
    pub selected: usize,
    pub style: FuzzySelectorStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadedWidget {
    Selector(SelectorOverlay),
    FuzzySelector(LoadedFuzzySelectorWidget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneStyle {
    pub active_border_fg: u8,
    pub inactive_border_fg: u8,
}

impl Default for PaneStyle {
    fn default() -> Self {
        Self {
            active_border_fg: 14,
            inactive_border_fg: 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ThemeValue {
    Color(u8),
    Ref(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ThemeSpec {
    palette: HashMap<String, u8>,
    roles: HashMap<String, ThemeValue>,
    components: HashMap<String, ThemeValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedTheme {
    bar_style: BarStyle,
    selector_style: SelectorStyle,
    fuzzy_selector_style: FuzzySelectorStyle,
    pane_style: PaneStyle,
}

#[derive(Clone, Debug)]
pub struct ConfigRuntime {
    code: String,
    key_bindings: Vec<KeyBinding>,
    named_action_names: HashSet<String>,
    widgets: HashMap<String, WidgetSpec>,
    docked_widgets: Vec<DockedWidgetSpec>,
    pane_style: PaneStyle,
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

    pub fn docked_widgets(&self) -> &[DockedWidgetSpec] {
        &self.docked_widgets
    }

    pub fn pane_style(&self) -> &PaneStyle {
        &self.pane_style
    }

    pub fn invoke_named_action(&self, name: &str, state: &LuaRuntimeState) -> Result<Vec<RuntimeCommand>> {
        if !self.named_action_names.contains(name) {
            return Err(color_eyre::eyre::eyre!("unknown named action '{name}'"));
        }

        let lua = self.new_runtime_lua(state)?;
        let remux = read_remux_table(&lua)?;
        let actions: Table = remux.get("actions")?;
        let callback: mlua::Function = actions.get(name)?;
        let value = callback.call::<Value>(())?;
        read_runtime_commands(value)
    }

    pub fn load_widget(&self, widget_id: &str, state: &LuaRuntimeState) -> Result<LoadedWidget> {
        let widget = self
            .widgets
            .get(widget_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("unknown widget '{widget_id}'"))?;
        let lua = self.new_runtime_lua(state)?;
        let widget_table = self.read_widget_table(&lua, widget_id)?;
        let items_fn: mlua::Function = widget_table.get("items")?;
        let items = items_fn.call::<Value>(())?;
        let (items, selected) = read_selector_items(items)?;

        match widget {
            WidgetSpec::Selector(spec) => Ok(LoadedWidget::Selector(SelectorOverlay {
                title: spec.title.clone(),
                footer: spec.footer.clone(),
                items,
                selected,
                style: spec.style.clone(),
            })),
            WidgetSpec::FuzzySelector(spec) => Ok(LoadedWidget::FuzzySelector(LoadedFuzzySelectorWidget {
                title: spec.title.clone(),
                footer: spec.footer.clone(),
                placeholder: spec.placeholder.clone(),
                items,
                selected,
                style: spec.style.clone(),
            })),
        }
    }

    pub fn invoke_widget_confirm(
        &self,
        widget_id: &str,
        selected_id: &str,
        state: &LuaRuntimeState,
    ) -> Result<Vec<RuntimeCommand>> {
        let _ = self
            .widgets
            .get(widget_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("unknown widget '{widget_id}'"))?;
        let lua = self.new_runtime_lua(state)?;
        let widget = self.read_widget_table(&lua, widget_id)?;
        let callback: mlua::Function = widget.get("on_confirm")?;
        let value = callback.call::<Value>(selected_id.to_owned())?;
        read_runtime_commands(value)
    }

    fn from_code(code: String) -> Result<Self> {
        let lua = Lua::default();
        initialize_lua_state(&lua)?;
        lua.load(&code).exec()?;
        let remux = read_remux_table(&lua)?;
        reject_legacy_style_config(&remux)?;
        let theme = read_theme(&remux)?;
        let widgets = read_widgets(&remux, &theme)?;

        Ok(Self {
            code,
            key_bindings: read_key_bindings(&remux)?,
            named_action_names: read_action_names(&remux)?,
            widgets: widgets.overlay_widgets,
            docked_widgets: widgets.docked_widgets,
            pane_style: theme.pane_style,
        })
    }

    fn new_runtime_lua(&self, state: &LuaRuntimeState) -> Result<Lua> {
        let lua = Lua::default();
        initialize_lua_state(&lua)?;
        install_runtime_helpers(&lua, state)?;
        lua.load(&self.code).exec()?;
        Ok(lua)
    }

    fn read_widget_table(&self, lua: &Lua, widget_id: &str) -> Result<Table> {
        let remux = read_remux_table(lua)?;
        let widgets: Table = remux.get("widgets")?;
        let widget: Table = widgets.get(widget_id)?;
        let widget_type = widget.get::<String>("type")?;
        match widget_type.as_str() {
            "selector" | "fuzzy-selector" | "bar" => Ok(widget),
            _ => Err(color_eyre::eyre::eyre!(
                "unsupported widget type '{}' for '{}'",
                widget_type,
                widget_id
            )),
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
    remux.set("widgets", lua.create_table()?)?;
    remux.set("ui", default_ui_table(lua)?)?;
    remux.set("theme", default_theme_table(lua)?)?;
    lua.globals().set("remux", remux)?;
    Ok(())
}

fn default_ui_table(lua: &Lua) -> Result<Table> {
    let widgets = lua.create_table()?;
    widgets.set("selector", lua.create_table()?)?;
    widgets.set("fuzzy_selector", lua.create_table()?)?;
    let panes = lua.create_table()?;

    let ui = lua.create_table()?;
    ui.set("widgets", widgets)?;
    ui.set("panes", panes)?;
    Ok(ui)
}

fn default_theme_table(lua: &Lua) -> Result<Table> {
    let table = lua.create_table()?;
    let ThemeSpec {
        palette,
        roles,
        components,
    } = default_theme_spec();
    for (key, value) in palette {
        set_dotted_theme_value(lua, &table, &format!("palette.{key}"), ThemeValue::Color(value))?;
    }
    for (key, value) in roles {
        set_dotted_theme_value(lua, &table, &format!("roles.{key}"), value)?;
    }
    for (key, value) in components {
        set_dotted_theme_value(lua, &table, &format!("components.{key}"), value)?;
    }
    Ok(table)
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

fn read_widgets(remux: &Table, theme: &ResolvedTheme) -> Result<ParsedWidgets> {
    let widgets = match remux.get::<Value>("widgets")? {
        Value::Nil => {
            return Ok(ParsedWidgets {
                overlay_widgets: HashMap::new(),
                docked_widgets: Vec::new(),
            });
        }
        Value::Table(table) => table,
        other => return Err(color_eyre::eyre::eyre!("expected widgets table, got {other:?}")),
    };

    let mut overlay_widgets = HashMap::new();
    let mut docked_widgets = Vec::new();
    for pair in widgets.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(name) = key else {
            continue;
        };
        let Value::Table(widget) = value else {
            continue;
        };
        let widget_type = widget.get::<String>("type")?;
        let name = name.to_str()?.to_owned();
        if !matches!(widget.get::<Value>("style")?, Value::Nil) {
            return Err(color_eyre::eyre::eyre!(
                "widget-local style overrides are no longer supported; use remux.theme.components.widgets"
            ));
        }

        match widget_type.as_str() {
            "bar" => {
                let (edge, size) = read_dock_placement(&widget, &name)?;
                docked_widgets.push(DockedWidgetSpec {
                    id: name,
                    edge,
                    size,
                    kind: DockWidgetKind::Bar(read_bar_widget_spec(&widget, theme.bar_style.clone())?),
                });
            }
            "selector" => {
                ensure_overlay_widget(&widget, &name, &widget_type)?;
                let _: mlua::Function = widget.get("items")?;
                let _: mlua::Function = widget.get("on_confirm")?;
                overlay_widgets.insert(
                    name,
                    WidgetSpec::Selector(SelectorWidgetSpec {
                        title: widget
                            .get::<Option<String>>("title")?
                            .unwrap_or_else(|| theme.selector_style.title.clone()),
                        footer: widget
                            .get::<Option<String>>("footer")?
                            .unwrap_or_else(|| theme.selector_style.footer.clone()),
                        style: theme.selector_style.clone(),
                    }),
                );
            }
            "fuzzy-selector" => {
                ensure_overlay_widget(&widget, &name, &widget_type)?;
                let _: mlua::Function = widget.get("items")?;
                let _: mlua::Function = widget.get("on_confirm")?;
                overlay_widgets.insert(
                    name,
                    WidgetSpec::FuzzySelector(FuzzySelectorWidgetSpec {
                        title: widget
                            .get::<Option<String>>("title")?
                            .unwrap_or_else(|| theme.fuzzy_selector_style.title.clone()),
                        footer: widget
                            .get::<Option<String>>("footer")?
                            .unwrap_or_else(|| theme.fuzzy_selector_style.footer.clone()),
                        placeholder: widget
                            .get::<Option<String>>("placeholder")?
                            .unwrap_or_else(|| "Type to filter".to_owned()),
                        style: theme.fuzzy_selector_style.clone(),
                    }),
                );
            }
            _ => {
                return Err(color_eyre::eyre::eyre!(
                    "unsupported widget type '{widget_type}' for '{}'",
                    name
                ));
            }
        }
    }

    docked_widgets.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(ParsedWidgets {
        overlay_widgets,
        docked_widgets,
    })
}

fn ensure_overlay_widget(widget: &Table, name: &str, widget_type: &str) -> Result<()> {
    let placement = widget
        .get::<Option<String>>("placement")?
        .unwrap_or_else(|| "overlay".to_owned());
    if placement != "overlay" {
        return Err(color_eyre::eyre::eyre!(
            "widget '{}' of type '{}' only supports placement='overlay'",
            name,
            widget_type
        ));
    }
    Ok(())
}

fn read_dock_placement(widget: &Table, name: &str) -> Result<(DockEdge, u16)> {
    let placement = widget
        .get::<Option<String>>("placement")?
        .unwrap_or_else(|| "dock".to_owned());
    if placement != "dock" {
        return Err(color_eyre::eyre::eyre!(
            "widget '{}' of type 'bar' only supports placement='dock'",
            name
        ));
    }

    let edge = match widget
        .get::<Option<String>>("edge")?
        .unwrap_or_else(|| "bottom".to_owned())
        .as_str()
    {
        "top" => DockEdge::Top,
        "bottom" => DockEdge::Bottom,
        "left" => DockEdge::Left,
        "right" => DockEdge::Right,
        other => {
            return Err(color_eyre::eyre::eyre!(
                "unsupported dock edge '{other}' for widget '{name}'"
            ));
        }
    };
    let size = widget.get::<Option<u16>>("size")?.unwrap_or(1);
    if size == 0 {
        return Err(color_eyre::eyre::eyre!("widget '{}' must have size >= 1", name));
    }

    Ok((edge, size))
}

fn read_bar_widget_spec(widget: &Table, style: BarStyle) -> Result<BarSpec> {
    Ok(BarSpec {
        enabled: widget.get::<Option<bool>>("enabled")?.unwrap_or(true),
        left: read_bar_section_value(widget.get::<Value>("left")?)?,
        center: read_bar_section_value(widget.get::<Value>("center")?)?,
        right: read_bar_section_value(widget.get::<Value>("right")?)?,
        style,
    })
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

fn reject_legacy_style_config(remux: &Table) -> Result<()> {
    let ui = match remux.get::<Value>("ui")? {
        Value::Table(table) => table,
        _ => return Ok(()),
    };
    if let Value::Table(bars) = ui.get::<Value>("bars")? {
        if table_has_entries(&bars)? {
            return Err(color_eyre::eyre::eyre!(
                "remux.ui.bars is no longer supported; configure bar widgets under remux.widgets"
            ));
        }
    }
    let widgets = ui.get::<Value>("widgets")?;
    if has_non_empty_style(widgets.clone(), &["selector"])? || has_non_empty_style(widgets, &["fuzzy_selector"])? {
        return Err(color_eyre::eyre::eyre!(
            "remux.ui.widgets.<type>.style is no longer supported; use remux.theme.components.widgets"
        ));
    }
    if has_non_empty_style(ui.get::<Value>("panes")?, &[])? {
        return Err(color_eyre::eyre::eyre!(
            "remux.ui.panes.style is no longer supported; use remux.theme.components.panes"
        ));
    }
    Ok(())
}

fn table_has_entries(table: &Table) -> Result<bool> {
    Ok(table.clone().pairs::<Value, Value>().next().transpose()?.is_some())
}

fn has_non_empty_style(root: Value, child_keys: &[&str]) -> Result<bool> {
    let Value::Table(mut table) = root else {
        return Ok(false);
    };
    for key in child_keys {
        let Value::Table(child) = table.get::<Value>(*key)? else {
            return Ok(false);
        };
        table = child;
    }
    let Some(Value::Table(style)) = table.get::<Option<Value>>("style")? else {
        return Ok(false);
    };
    table_has_entries(&style)
}

fn parse_action_name(action_name: &str, action_names: &HashSet<String>) -> Result<KeyAction> {
    let action = match BuiltinAction::from_config_name(action_name) {
        Some(action) => KeyAction::Builtin(action),
        None if action_names.contains(action_name) => KeyAction::Named(action_name.to_owned()),
        None => return Err(color_eyre::eyre::eyre!("unknown action '{action_name}'")),
    };
    Ok(action)
}

fn read_runtime_commands(value: Value) -> Result<Vec<RuntimeCommand>> {
    match value {
        Value::Nil => Ok(Vec::new()),
        Value::String(name) => Ok(vec![RuntimeCommand::Builtin(parse_builtin_action_name(
            &name.to_str()?,
        )?)]),
        Value::Table(table) => {
            if let Some(command_type) = table.get::<Option<String>>("type")? {
                return Ok(vec![read_command_table(&table, &command_type)?]);
            }

            let mut commands = Vec::new();
            for value in read_array_values(&table)? {
                commands.extend(read_runtime_commands(value)?);
            }
            Ok(commands)
        }
        other => Err(color_eyre::eyre::eyre!(
            "runtime callback returned unsupported value {other:?}"
        )),
    }
}

fn read_command_table(table: &Table, command_type: &str) -> Result<RuntimeCommand> {
    match command_type {
        "open_widget" => Ok(RuntimeCommand::OpenWidget(table.get::<String>("widget")?)),
        "switch_session" => Ok(RuntimeCommand::SwitchSession(table.get::<String>("session")?)),
        other => Err(color_eyre::eyre::eyre!("unknown runtime command '{other}'")),
    }
}

fn read_selector_items(value: Value) -> Result<(Vec<SelectorItem>, usize)> {
    let Value::Table(table) = value else {
        return Err(color_eyre::eyre::eyre!(
            "selector items callback must return an array table"
        ));
    };

    let mut items = Vec::new();
    let mut selected = 0usize;
    for (index, value) in read_array_values(&table)?.into_iter().enumerate() {
        let Value::Table(item) = value else {
            return Err(color_eyre::eyre::eyre!("selector items must be tables"));
        };
        let id = item.get::<String>("id")?;
        let label = item.get::<Option<String>>("label")?.unwrap_or_else(|| id.clone());
        let is_selected = item.get::<Option<bool>>("selected")?.unwrap_or(false);
        if is_selected {
            selected = index;
        }
        items.push(SelectorItem { id, label });
    }

    Ok((items, selected))
}

fn parse_builtin_action_name(name: &str) -> Result<BuiltinAction> {
    BuiltinAction::from_config_name(name).ok_or_else(|| color_eyre::eyre::eyre!("unknown builtin action '{name}'"))
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

fn read_bar_section_value(value: Value) -> Result<Vec<BarItem>> {
    let Value::Table(section) = value else {
        return Ok(Vec::new());
    };
    let mut items = Vec::new();
    for value in read_array_values(&section)? {
        match value {
            Value::String(s) => {
                let text = s.to_str()?.to_owned();
                match text.as_str() {
                    "active-session" => items.push(BarItem::ActiveSession),
                    "window-list" => items.push(BarItem::WindowList),
                    _ if !text.is_empty() => items.push(BarItem::Text(text)),
                    _ => {}
                }
            }
            Value::Function(function) => {
                if let Some(value) = function.call::<Option<String>>(())? {
                    if !value.is_empty() {
                        items.push(BarItem::Text(value));
                    }
                }
            }
            Value::Nil => {}
            other => warn!("ignoring unsupported bar item value: {other:?}"),
        }
    }
    Ok(items)
}

fn read_theme(remux: &Table) -> Result<ResolvedTheme> {
    let mut spec = default_theme_spec();
    match remux.get::<Value>("theme")? {
        Value::Nil => {}
        Value::Table(theme) => apply_theme_table(&theme, &mut spec)?,
        other => return Err(color_eyre::eyre::eyre!("expected theme table, got {other:?}")),
    }
    compile_theme(&spec)
}

fn default_theme_spec() -> ThemeSpec {
    let palette = HashMap::from([
        ("crust".to_owned(), 234),
        ("mantle".to_owned(), 235),
        ("base".to_owned(), 236),
        ("surface0".to_owned(), 238),
        ("surface1".to_owned(), 239),
        ("surface2".to_owned(), 240),
        ("text".to_owned(), 253),
        ("subtext1".to_owned(), 250),
        ("overlay1".to_owned(), 246),
        ("blue".to_owned(), 111),
        ("lavender".to_owned(), 147),
        ("teal".to_owned(), 116),
        ("mauve".to_owned(), 183),
    ]);
    let roles = HashMap::from([
        ("bg.default".to_owned(), ThemeValue::Ref("crust".to_owned())),
        ("bg.surface".to_owned(), ThemeValue::Ref("mantle".to_owned())),
        ("bg.panel".to_owned(), ThemeValue::Ref("base".to_owned())),
        ("fg.default".to_owned(), ThemeValue::Ref("text".to_owned())),
        ("fg.muted".to_owned(), ThemeValue::Ref("overlay1".to_owned())),
        ("fg.subtle".to_owned(), ThemeValue::Ref("subtext1".to_owned())),
        ("accent.primary".to_owned(), ThemeValue::Ref("blue".to_owned())),
        ("accent.secondary".to_owned(), ThemeValue::Ref("lavender".to_owned())),
        ("accent.session".to_owned(), ThemeValue::Ref("mauve".to_owned())),
        ("accent.info".to_owned(), ThemeValue::Ref("teal".to_owned())),
        ("border.default".to_owned(), ThemeValue::Ref("surface2".to_owned())),
        ("border.inactive".to_owned(), ThemeValue::Ref("surface1".to_owned())),
        ("selection.active.fg".to_owned(), ThemeValue::Ref("crust".to_owned())),
        ("selection.active.bg".to_owned(), ThemeValue::Ref("blue".to_owned())),
        ("selection.secondary.fg".to_owned(), ThemeValue::Ref("crust".to_owned())),
        (
            "selection.secondary.bg".to_owned(),
            ThemeValue::Ref("lavender".to_owned()),
        ),
        ("scrim.default.fg".to_owned(), ThemeValue::Ref("crust".to_owned())),
        ("scrim.default.bg".to_owned(), ThemeValue::Ref("crust".to_owned())),
    ]);
    let components = HashMap::from([
        (
            "bars.status.background.fg".to_owned(),
            ThemeValue::Ref("fg.subtle".to_owned()),
        ),
        (
            "bars.status.background.bg".to_owned(),
            ThemeValue::Ref("bg.default".to_owned()),
        ),
        (
            "bars.status.left.fg".to_owned(),
            ThemeValue::Ref("bg.default".to_owned()),
        ),
        (
            "bars.status.left.bg".to_owned(),
            ThemeValue::Ref("accent.session".to_owned()),
        ),
        (
            "bars.status.center.fg".to_owned(),
            ThemeValue::Ref("accent.secondary".to_owned()),
        ),
        (
            "bars.status.center.bg".to_owned(),
            ThemeValue::Ref("bg.default".to_owned()),
        ),
        (
            "bars.status.right.fg".to_owned(),
            ThemeValue::Ref("accent.info".to_owned()),
        ),
        (
            "bars.status.right.bg".to_owned(),
            ThemeValue::Ref("bg.default".to_owned()),
        ),
        (
            "bars.status.window.active.fg".to_owned(),
            ThemeValue::Ref("selection.active.fg".to_owned()),
        ),
        (
            "bars.status.window.active.bg".to_owned(),
            ThemeValue::Ref("selection.active.bg".to_owned()),
        ),
        (
            "bars.status.window.inactive.fg".to_owned(),
            ThemeValue::Ref("fg.muted".to_owned()),
        ),
        (
            "bars.status.window.inactive.bg".to_owned(),
            ThemeValue::Ref("bg.panel".to_owned()),
        ),
        (
            "bars.status.window.muted.fg".to_owned(),
            ThemeValue::Ref("fg.muted".to_owned()),
        ),
        (
            "widgets.selector.scrim.fg".to_owned(),
            ThemeValue::Ref("scrim.default.fg".to_owned()),
        ),
        (
            "widgets.selector.scrim.bg".to_owned(),
            ThemeValue::Ref("scrim.default.bg".to_owned()),
        ),
        (
            "widgets.selector.border.fg".to_owned(),
            ThemeValue::Ref("border.default".to_owned()),
        ),
        (
            "widgets.selector.surface.fg".to_owned(),
            ThemeValue::Ref("fg.default".to_owned()),
        ),
        (
            "widgets.selector.surface.bg".to_owned(),
            ThemeValue::Ref("bg.surface".to_owned()),
        ),
        (
            "widgets.selector.title.fg".to_owned(),
            ThemeValue::Ref("accent.session".to_owned()),
        ),
        (
            "widgets.selector.text.fg".to_owned(),
            ThemeValue::Ref("fg.default".to_owned()),
        ),
        (
            "widgets.selector.selection.fg".to_owned(),
            ThemeValue::Ref("selection.secondary.fg".to_owned()),
        ),
        (
            "widgets.selector.selection.bg".to_owned(),
            ThemeValue::Ref("selection.secondary.bg".to_owned()),
        ),
        (
            "widgets.selector.footer.fg".to_owned(),
            ThemeValue::Ref("fg.subtle".to_owned()),
        ),
        (
            "widgets.selector.empty.fg".to_owned(),
            ThemeValue::Ref("fg.muted".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.scrim.fg".to_owned(),
            ThemeValue::Ref("scrim.default.fg".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.scrim.bg".to_owned(),
            ThemeValue::Ref("scrim.default.bg".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.border.fg".to_owned(),
            ThemeValue::Ref("border.default".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.surface.fg".to_owned(),
            ThemeValue::Ref("fg.default".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.surface.bg".to_owned(),
            ThemeValue::Ref("bg.surface".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.title.fg".to_owned(),
            ThemeValue::Ref("accent.session".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.text.fg".to_owned(),
            ThemeValue::Ref("fg.default".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.selection.fg".to_owned(),
            ThemeValue::Ref("selection.active.fg".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.selection.bg".to_owned(),
            ThemeValue::Ref("selection.active.bg".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.footer.fg".to_owned(),
            ThemeValue::Ref("fg.subtle".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.query.fg".to_owned(),
            ThemeValue::Ref("fg.default".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.query.bg".to_owned(),
            ThemeValue::Ref("bg.panel".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.placeholder.fg".to_owned(),
            ThemeValue::Ref("fg.muted".to_owned()),
        ),
        (
            "widgets.fuzzy_selector.empty.fg".to_owned(),
            ThemeValue::Ref("fg.muted".to_owned()),
        ),
        (
            "panes.border.active.fg".to_owned(),
            ThemeValue::Ref("accent.primary".to_owned()),
        ),
        (
            "panes.border.inactive.fg".to_owned(),
            ThemeValue::Ref("border.inactive".to_owned()),
        ),
    ]);

    ThemeSpec {
        palette,
        roles,
        components,
    }
}

fn apply_theme_table(theme: &Table, spec: &mut ThemeSpec) -> Result<()> {
    if let Value::Table(palette) = theme.get::<Value>("palette")? {
        flatten_theme_colors("", &palette, &mut spec.palette)?;
    }
    if let Value::Table(roles) = theme.get::<Value>("roles")? {
        flatten_theme_values("", &roles, &mut spec.roles)?;
    }
    if let Value::Table(components) = theme.get::<Value>("components")? {
        let mut overrides = HashMap::new();
        flatten_theme_values("", &components, &mut overrides)?;
        for key in overrides.keys() {
            if !spec.components.contains_key(key) {
                return Err(color_eyre::eyre::eyre!("unknown theme component token '{key}'"));
            }
        }
        spec.components.extend(overrides);
    }
    Ok(())
}

fn flatten_theme_colors(prefix: &str, table: &Table, dest: &mut HashMap<String, u8>) -> Result<()> {
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = key else {
            continue;
        };
        let path = join_theme_path(prefix, &key.to_str()?);
        match value {
            Value::Integer(color) if (0..=u8::MAX as i64).contains(&color) => {
                dest.insert(path, color as u8);
            }
            Value::Table(child) => flatten_theme_colors(&path, &child, dest)?,
            other => {
                return Err(color_eyre::eyre::eyre!(
                    "expected theme palette color at '{path}', got {other:?}"
                ));
            }
        }
    }
    Ok(())
}

fn flatten_theme_values(prefix: &str, table: &Table, dest: &mut HashMap<String, ThemeValue>) -> Result<()> {
    for pair in table.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        let Value::String(key) = key else {
            continue;
        };
        let path = join_theme_path(prefix, &key.to_str()?);
        match value {
            Value::Integer(color) if (0..=u8::MAX as i64).contains(&color) => {
                dest.insert(path, ThemeValue::Color(color as u8));
            }
            Value::String(reference) => {
                dest.insert(path, ThemeValue::Ref(reference.to_str()?.to_owned()));
            }
            Value::Table(child) => flatten_theme_values(&path, &child, dest)?,
            other => {
                return Err(color_eyre::eyre::eyre!(
                    "expected theme token at '{path}' to be a color, reference, or table, got {other:?}"
                ));
            }
        }
    }
    Ok(())
}

fn join_theme_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_owned()
    } else {
        format!("{prefix}.{segment}")
    }
}

fn set_dotted_theme_value(lua: &Lua, root: &Table, path: &str, value: ThemeValue) -> Result<()> {
    let mut current = root.clone();
    let mut parts = path.split('.').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            match value.clone() {
                ThemeValue::Color(color) => current.set(part, color)?,
                ThemeValue::Ref(reference) => current.set(part, reference)?,
            }
            return Ok(());
        }

        let next = match current.get::<Value>(part)? {
            Value::Table(table) => table,
            _ => {
                let table = lua.create_table()?;
                current.set(part, table.clone())?;
                table
            }
        };
        current = next;
    }
    Ok(())
}

fn compile_theme(spec: &ThemeSpec) -> Result<ResolvedTheme> {
    Ok(ResolvedTheme {
        bar_style: BarStyle {
            background_fg: resolve_component_color(spec, "bars.status.background.fg")?,
            background_bg: resolve_component_color(spec, "bars.status.background.bg")?,
            left_fg: resolve_component_color(spec, "bars.status.left.fg")?,
            left_bg: resolve_component_color(spec, "bars.status.left.bg")?,
            center_fg: resolve_component_color(spec, "bars.status.center.fg")?,
            center_bg: resolve_component_color(spec, "bars.status.center.bg")?,
            right_fg: resolve_component_color(spec, "bars.status.right.fg")?,
            right_bg: resolve_component_color(spec, "bars.status.right.bg")?,
            window_active_fg: resolve_component_color(spec, "bars.status.window.active.fg")?,
            window_active_bg: resolve_component_color(spec, "bars.status.window.active.bg")?,
            window_inactive_fg: resolve_component_color(spec, "bars.status.window.inactive.fg")?,
            window_inactive_bg: resolve_component_color(spec, "bars.status.window.inactive.bg")?,
            window_muted_fg: resolve_component_color(spec, "bars.status.window.muted.fg")?,
        },
        selector_style: SelectorStyle {
            scrim_fg: resolve_component_color(spec, "widgets.selector.scrim.fg")?,
            scrim_bg: resolve_component_color(spec, "widgets.selector.scrim.bg")?,
            border_fg: resolve_component_color(spec, "widgets.selector.border.fg")?,
            background_fg: resolve_component_color(spec, "widgets.selector.surface.fg")?,
            background_bg: resolve_component_color(spec, "widgets.selector.surface.bg")?,
            title_fg: resolve_component_color(spec, "widgets.selector.title.fg")?,
            text_fg: resolve_component_color(spec, "widgets.selector.text.fg")?,
            selected_fg: resolve_component_color(spec, "widgets.selector.selection.fg")?,
            selected_bg: resolve_component_color(spec, "widgets.selector.selection.bg")?,
            footer_fg: resolve_component_color(spec, "widgets.selector.footer.fg")?,
            empty_fg: resolve_component_color(spec, "widgets.selector.empty.fg")?,
            ..SelectorStyle::default()
        },
        fuzzy_selector_style: FuzzySelectorStyle {
            scrim_fg: resolve_component_color(spec, "widgets.fuzzy_selector.scrim.fg")?,
            scrim_bg: resolve_component_color(spec, "widgets.fuzzy_selector.scrim.bg")?,
            border_fg: resolve_component_color(spec, "widgets.fuzzy_selector.border.fg")?,
            background_fg: resolve_component_color(spec, "widgets.fuzzy_selector.surface.fg")?,
            background_bg: resolve_component_color(spec, "widgets.fuzzy_selector.surface.bg")?,
            title_fg: resolve_component_color(spec, "widgets.fuzzy_selector.title.fg")?,
            text_fg: resolve_component_color(spec, "widgets.fuzzy_selector.text.fg")?,
            selected_fg: resolve_component_color(spec, "widgets.fuzzy_selector.selection.fg")?,
            selected_bg: resolve_component_color(spec, "widgets.fuzzy_selector.selection.bg")?,
            footer_fg: resolve_component_color(spec, "widgets.fuzzy_selector.footer.fg")?,
            query_fg: resolve_component_color(spec, "widgets.fuzzy_selector.query.fg")?,
            query_bg: resolve_component_color(spec, "widgets.fuzzy_selector.query.bg")?,
            placeholder_fg: resolve_component_color(spec, "widgets.fuzzy_selector.placeholder.fg")?,
            empty_fg: resolve_component_color(spec, "widgets.fuzzy_selector.empty.fg")?,
            ..FuzzySelectorStyle::default()
        },
        pane_style: PaneStyle {
            active_border_fg: resolve_component_color(spec, "panes.border.active.fg")?,
            inactive_border_fg: resolve_component_color(spec, "panes.border.inactive.fg")?,
        },
    })
}

fn resolve_component_color(spec: &ThemeSpec, key: &str) -> Result<u8> {
    let value = spec
        .components
        .get(key)
        .ok_or_else(|| color_eyre::eyre::eyre!("missing required theme component '{key}'"))?;
    resolve_theme_value(spec, value, &mut Vec::new())
}

fn resolve_theme_value(spec: &ThemeSpec, value: &ThemeValue, stack: &mut Vec<String>) -> Result<u8> {
    match value {
        ThemeValue::Color(color) => Ok(*color),
        ThemeValue::Ref(reference) => resolve_theme_reference(spec, reference, stack),
    }
}

fn resolve_theme_reference(spec: &ThemeSpec, key: &str, stack: &mut Vec<String>) -> Result<u8> {
    if stack.iter().any(|entry| entry == key) {
        return Err(color_eyre::eyre::eyre!(
            "cyclic theme reference detected: {} -> {key}",
            stack.join(" -> ")
        ));
    }

    if let Some(color) = spec.palette.get(key) {
        return Ok(*color);
    }

    let value = spec
        .roles
        .get(key)
        .ok_or_else(|| color_eyre::eyre::eyre!("unknown theme reference '{key}'"))?;
    stack.push(key.to_owned());
    let resolved = resolve_theme_value(spec, value, stack);
    stack.pop();
    resolved
}

fn install_runtime_helpers(lua: &Lua, state: &LuaRuntimeState) -> Result<()> {
    let remux = read_remux_table(lua)?;

    let state_table = lua.create_table()?;
    let sessions = state.sessions.clone();
    state_table.set("sessions", sessions_to_lua_table(lua, &sessions)?)?;
    state_table.set(
        "current_session",
        optional_session_to_lua_value(lua, state.current_session.as_ref())?,
    )?;
    state_table.set(
        "current_window",
        optional_window_to_lua_value(lua, state.current_window.as_ref())?,
    )?;
    state_table.set(
        "current_pane",
        optional_pane_to_lua_value(lua, state.current_pane.as_ref())?,
    )?;
    state_table.set(
        "active_widget",
        optional_widget_to_lua_value(lua, state.active_widget.as_ref())?,
    )?;

    let sessions_for_lookup = sessions.clone();
    state_table.set(
        "session",
        lua.create_function(move |lua, query: Value| {
            let session = match query {
                Value::Integer(id) => sessions_for_lookup.iter().find(|session| i64::from(session.id) == id),
                Value::String(name) => {
                    let name = name.to_str()?.to_string();
                    sessions_for_lookup.iter().find(|session| session.name == name)
                }
                _ => None,
            };
            match session {
                Some(session) => session_to_lua_table(lua, session).map(Value::Table),
                None => Ok(Value::Nil),
            }
        })?,
    )?;

    let current_session = state.current_session.clone();
    state_table.set(
        "window",
        lua.create_function(move |lua, query: Value| {
            let Some(current_session) = current_session.as_ref() else {
                return Ok(Value::Nil);
            };
            let window = match query {
                Value::Integer(value) => current_session
                    .windows
                    .iter()
                    .find(|window| i64::from(window.id) == value || i64::try_from(window.index).ok() == Some(value)),
                _ => None,
            };
            match window {
                Some(window) => window_to_lua_table(lua, window).map(Value::Table),
                None => Ok(Value::Nil),
            }
        })?,
    )?;

    let current_window = state.current_window.clone();
    state_table.set(
        "pane",
        lua.create_function(move |lua, query: Value| {
            let Some(current_window) = current_window.as_ref() else {
                return Ok(Value::Nil);
            };
            let pane = match query {
                Value::Integer(id) => current_window
                    .panes
                    .iter()
                    .find(|pane| i64::try_from(pane.id).ok() == Some(id)),
                _ => None,
            };
            match pane {
                Some(pane) => pane_to_lua_table(lua, pane).map(Value::Table),
                None => Ok(Value::Nil),
            }
        })?,
    )?;
    remux.set("state", state_table)?;

    remux.set(
        "open_widget",
        lua.create_function(|lua, widget_id: String| {
            let command = lua.create_table()?;
            command.set("type", "open_widget")?;
            command.set("widget", widget_id)?;
            Ok(command)
        })?,
    )?;

    remux.set(
        "switch_session",
        lua.create_function(|lua, session_name: String| {
            let command = lua.create_table()?;
            command.set("type", "switch_session")?;
            command.set("session", session_name)?;
            Ok(command)
        })?,
    )?;

    install_builtin_helpers(lua, &remux)?;

    Ok(())
}

fn session_to_lua_table(lua: &Lua, session: &RuntimeSessionState) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("id", session.id)?;
    table.set("name", session.name.clone())?;
    table.set("is_current", session.is_current)?;
    table.set("windows", windows_to_lua_table(lua, &session.windows)?)?;
    table.set(
        "current_window",
        optional_window_to_lua_value(lua, session.current_window.as_ref())?,
    )?;
    Ok(table)
}

fn sessions_to_lua_table(lua: &Lua, sessions: &[RuntimeSessionState]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (index, session) in sessions.iter().enumerate() {
        table.set(index + 1, session_to_lua_table(lua, session)?)?;
    }
    Ok(table)
}

fn window_to_lua_table(lua: &Lua, window: &RuntimeWindowState) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("id", window.id)?;
    table.set("index", window.index)?;
    table.set("name", window.name.clone())?;
    table.set("is_active", window.is_active)?;
    table.set("panes", panes_to_lua_table(lua, &window.panes)?)?;
    table.set(
        "active_pane",
        optional_pane_to_lua_value(lua, window.active_pane.as_ref())?,
    )?;
    Ok(table)
}

fn windows_to_lua_table(lua: &Lua, windows: &[RuntimeWindowState]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (index, window) in windows.iter().enumerate() {
        table.set(index + 1, window_to_lua_table(lua, window)?)?;
    }
    Ok(table)
}

fn pane_to_lua_table(lua: &Lua, pane: &RuntimePaneState) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("id", pane.id)?;
    table.set("is_active", pane.is_active)?;
    table.set("rect", rect_to_lua_table(lua, &pane.rect)?)?;
    Ok(table)
}

fn panes_to_lua_table(lua: &Lua, panes: &[RuntimePaneState]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (index, pane) in panes.iter().enumerate() {
        table.set(index + 1, pane_to_lua_table(lua, pane)?)?;
    }
    Ok(table)
}

fn rect_to_lua_table(lua: &Lua, rect: &RuntimeRectState) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("x", rect.x)?;
    table.set("y", rect.y)?;
    table.set("width", rect.width)?;
    table.set("height", rect.height)?;
    Ok(table)
}

fn widget_to_lua_table(lua: &Lua, widget: &RuntimeWidgetState) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("id", widget.id.clone())?;
    table.set("kind", widget.kind.clone())?;
    Ok(table)
}

fn optional_session_to_lua_value(lua: &Lua, session: Option<&RuntimeSessionState>) -> mlua::Result<Value> {
    match session {
        Some(session) => session_to_lua_table(lua, session).map(Value::Table),
        None => Ok(Value::Nil),
    }
}

fn optional_window_to_lua_value(lua: &Lua, window: Option<&RuntimeWindowState>) -> mlua::Result<Value> {
    match window {
        Some(window) => window_to_lua_table(lua, window).map(Value::Table),
        None => Ok(Value::Nil),
    }
}

fn optional_pane_to_lua_value(lua: &Lua, pane: Option<&RuntimePaneState>) -> mlua::Result<Value> {
    match pane {
        Some(pane) => pane_to_lua_table(lua, pane).map(Value::Table),
        None => Ok(Value::Nil),
    }
}

fn optional_widget_to_lua_value(lua: &Lua, widget: Option<&RuntimeWidgetState>) -> mlua::Result<Value> {
    match widget {
        Some(widget) => widget_to_lua_table(lua, widget).map(Value::Table),
        None => Ok(Value::Nil),
    }
}

fn install_builtin_helpers(lua: &Lua, remux: &Table) -> Result<()> {
    for (name, action_name) in [
        ("split_pane_vertical", "split-pane-vertical"),
        ("split_pane_horizontal", "split-pane-horizontal"),
        ("focus_pane_left", "focus-pane-left"),
        ("focus_pane_down", "focus-pane-down"),
        ("focus_pane_up", "focus-pane-up"),
        ("focus_pane_right", "focus-pane-right"),
        ("kill_pane", "kill-pane"),
        ("new_window", "new-window"),
        ("next_window", "next-window"),
        ("prev_window", "prev-window"),
        ("kill_window", "kill-window"),
        ("detach", "detach"),
        ("open_session_switcher", "open-session-switcher"),
    ] {
        remux.set(name, builtin_action_helper(lua, action_name)?)?;
    }

    remux.set(
        "select_window",
        lua.create_function(|_, index: usize| match index {
            1..=9 => Ok(format!("select-window-{index}")),
            _ => Err(mlua::Error::external("window index must be between 1 and 9")),
        })?,
    )?;

    Ok(())
}

fn builtin_action_helper(lua: &Lua, action_name: &'static str) -> Result<mlua::Function> {
    Ok(lua.create_function(move |_, ()| Ok(action_name))?)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::render::{bar::BarRenderState, surface::Surface, widget::render_docked_widget};

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

    fn sample_pane(id: usize, is_active: bool, x: u16, y: u16, width: u16, height: u16) -> RuntimePaneState {
        RuntimePaneState {
            id,
            is_active,
            rect: RuntimeRectState { x, y, width, height },
        }
    }

    fn sample_state() -> LuaRuntimeState {
        let alpha_window = RuntimeWindowState {
            id: 20,
            index: 1,
            name: "shell".to_owned(),
            is_active: true,
            panes: vec![sample_pane(200, true, 0, 0, 120, 32)],
            active_pane: Some(sample_pane(200, true, 0, 0, 120, 32)),
        };
        let beta_window_shell = RuntimeWindowState {
            id: 10,
            index: 1,
            name: "shell".to_owned(),
            is_active: true,
            panes: vec![
                sample_pane(100, true, 0, 0, 80, 32),
                sample_pane(101, false, 81, 0, 39, 32),
            ],
            active_pane: Some(sample_pane(100, true, 0, 0, 80, 32)),
        };
        let beta_window_logs = RuntimeWindowState {
            id: 11,
            index: 2,
            name: "logs".to_owned(),
            is_active: false,
            panes: vec![sample_pane(110, true, 0, 0, 120, 32)],
            active_pane: Some(sample_pane(110, true, 0, 0, 120, 32)),
        };
        let current_session = RuntimeSessionState {
            id: 1,
            name: "beta".to_owned(),
            is_current: true,
            windows: vec![beta_window_shell.clone(), beta_window_logs.clone()],
            current_window: Some(beta_window_shell.clone()),
        };
        LuaRuntimeState {
            current_session: Some(current_session.clone()),
            current_window: current_session.current_window.clone(),
            current_pane: current_session
                .current_window
                .as_ref()
                .and_then(|window| window.active_pane.clone()),
            active_widget: Some(RuntimeWidgetState {
                id: "session_switcher".to_owned(),
                kind: "fuzzy-selector".to_owned(),
            }),
            sessions: vec![
                RuntimeSessionState {
                    id: 0,
                    name: "alpha".to_owned(),
                    is_current: false,
                    windows: vec![alpha_window.clone()],
                    current_window: Some(alpha_window),
                },
                current_session,
            ],
        }
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

        let actions = runtime.invoke_named_action("combo", &sample_state())?;

        assert_eq!(
            actions,
            vec![
                RuntimeCommand::Builtin(BuiltinAction::FocusPaneRight),
                RuntimeCommand::Builtin(BuiltinAction::KillPane),
            ]
        );
        Ok(())
    }

    #[test]
    fn named_action_can_open_widget() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.actions.session_switcher = function()
                    return remux.open_widget("session_switcher")
                end
                remux.widgets.session_switcher = {
                    type = "fuzzy-selector",
                    title = "Sessions",
                    footer = "pick",
                    placeholder = "search",
                    items = function() return {} end,
                    on_confirm = function() return nil end,
                }
            "#,
        )?;

        let actions = runtime.invoke_named_action("session_switcher", &sample_state())?;

        assert_eq!(actions, vec![RuntimeCommand::OpenWidget("session_switcher".to_owned())]);
        Ok(())
    }

    #[test]
    fn selector_widget_uses_live_state_and_confirm_callback() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.widgets.session_switcher = {
                    type = "selector",
                    title = "Workspaces",
                    footer = "pick one",
                    items = function()
                        local items = {}
                        local current = remux.state.current_session
                        for _, session in ipairs(remux.state.sessions) do
                            table.insert(items, {
                                id = session.name,
                                label = session.name,
                                selected = current and session.name == current.name,
                            })
                        end
                        return items
                    end,
                    on_confirm = function(id)
                        return remux.switch_session(id)
                    end,
                }
            "#,
        )?;

        let overlay = runtime.load_widget("session_switcher", &sample_state())?;
        let LoadedWidget::Selector(overlay) = overlay else {
            panic!("expected selector widget");
        };
        assert_eq!(overlay.title, "Workspaces");
        assert_eq!(overlay.footer, "pick one");
        assert_eq!(overlay.items.len(), 2);
        assert_eq!(overlay.items[1].id, "beta");
        assert_eq!(overlay.selected, 1);

        let commands = runtime.invoke_widget_confirm("session_switcher", "alpha", &sample_state())?;
        assert_eq!(commands, vec![RuntimeCommand::SwitchSession("alpha".to_owned())]);
        Ok(())
    }

    #[test]
    fn state_snapshot_exposes_nested_runtime_context_and_lookup_helpers() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.widgets.session_switcher = {
                    type = "selector",
                    title = "State Snapshot",
                    footer = "inspect",
                    items = function()
                        local current_session = remux.state.current_session
                        local current_window = remux.state.current_window
                        local current_pane = remux.state.current_pane
                        local active_widget = remux.state.active_widget
                        local beta = remux.state.session("beta")
                        local shell = remux.state.window(1)
                        local active_pane = remux.state.pane(100)
                        return {
                            { id = current_session.name, label = current_window.name },
                            { id = tostring(current_pane.rect.width), label = active_widget.id },
                            { id = beta.current_window.name, label = shell.active_pane.id .. ":" .. active_pane.rect.height },
                        }
                    end,
                    on_confirm = function()
                        return nil
                    end,
                }
            "#,
        )?;

        let overlay = runtime.load_widget("session_switcher", &sample_state())?;
        let LoadedWidget::Selector(overlay) = overlay else {
            panic!("expected selector widget");
        };
        assert_eq!(overlay.items[0].id, "beta");
        assert_eq!(overlay.items[0].label, "shell");
        assert_eq!(overlay.items[1].id, "80");
        assert_eq!(overlay.items[1].label, "session_switcher");
        assert_eq!(overlay.items[2].id, "shell");
        assert_eq!(overlay.items[2].label, "100:32");
        Ok(())
    }

    #[test]
    fn builtin_helpers_compile_to_runtime_commands() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.actions.ops = function()
                    return {
                        remux.focus_pane_right(),
                        remux.kill_pane(),
                        remux.select_window(2),
                        remux.open_session_switcher(),
                    }
                end
            "#,
        )?;

        let actions = runtime.invoke_named_action("ops", &sample_state())?;
        assert_eq!(
            actions,
            vec![
                RuntimeCommand::Builtin(BuiltinAction::FocusPaneRight),
                RuntimeCommand::Builtin(BuiltinAction::KillPane),
                RuntimeCommand::Builtin(BuiltinAction::SelectWindow2),
                RuntimeCommand::Builtin(BuiltinAction::OpenSessionSwitcher),
            ]
        );
        Ok(())
    }

    #[test]
    fn fuzzy_selector_widget_loads_with_placeholder_and_style_override() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.theme.components.widgets.fuzzy_selector = {
                    query = {
                        bg = 44,
                    },
                    footer = {
                        fg = 22,
                    },
                }
                remux.widgets.session_switcher = {
                    type = "fuzzy-selector",
                    title = "Find Session",
                    placeholder = "type a name",
                    footer = "widget footer",
                    items = function()
                        return {
                            { id = "alpha", label = "alpha" },
                            { id = "beta", label = "beta", selected = true },
                        }
                    end,
                    on_confirm = function(id)
                        return remux.switch_session(id)
                    end,
                }
            "#,
        )?;

        let overlay = runtime.load_widget("session_switcher", &sample_state())?;
        let LoadedWidget::FuzzySelector(overlay) = overlay else {
            panic!("expected fuzzy-selector widget");
        };
        assert_eq!(overlay.title, "Find Session");
        assert_eq!(overlay.footer, "widget footer");
        assert_eq!(overlay.placeholder, "type a name");
        assert_eq!(overlay.items.len(), 2);
        assert_eq!(overlay.selected, 1);
        assert_eq!(overlay.style.query_bg, 44);
        assert_eq!(overlay.style.footer_fg, 22);

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
    fn lua_status_widget_renders_active_session_and_preserves_item_order() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.widgets.status = {
                    type = "bar",
                    placement = "dock",
                    edge = "bottom",
                    size = 1,
                    enabled = true,
                    left = { "active-session", "two" },
                    center = {},
                    right = { "three" },
                }
            "#,
        )?;

        let surface = render_docked_widget(
            30,
            1,
            &runtime.docked_widgets()[0].kind,
            runtime.docked_widgets()[0].edge,
            &BarRenderState {
                active_session_name: Some("alpha".to_owned()),
                windows: Vec::new(),
            },
        );
        let rendered = surface_to_string(&surface);

        assert!(rendered.starts_with(" alpha "));
        assert!(rendered.contains(" two "));
        assert!(rendered.trim_end().ends_with(" three"));
        Ok(())
    }

    #[test]
    fn lua_status_widget_falls_back_to_default_style_when_omitted() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.widgets.status = {
                    type = "bar",
                    center = {
                        function()
                            return "clock"
                        end,
                    },
                }
            "#,
        )?;

        let surface = render_docked_widget(
            20,
            1,
            &runtime.docked_widgets()[0].kind,
            runtime.docked_widgets()[0].edge,
            &BarRenderState {
                active_session_name: None,
                windows: Vec::new(),
            },
        );
        let rendered = surface_to_string(&surface);

        assert!(rendered.contains("clock"));
        Ok(())
    }

    #[test]
    fn lua_status_widget_renders_window_list_component() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.widgets.status = {
                    type = "bar",
                    placement = "dock",
                    edge = "bottom",
                    size = 1,
                    enabled = true,
                    left = { "active-session" },
                    center = { "window-list" },
                    right = {},
                }
            "#,
        )?;

        let surface = render_docked_widget(
            50,
            1,
            &runtime.docked_widgets()[0].kind,
            runtime.docked_widgets()[0].edge,
            &BarRenderState {
                active_session_name: Some("beta".to_owned()),
                windows: vec![
                    WindowTab {
                        index: 1,
                        name: "shell".to_owned(),
                        is_active: true,
                    },
                    WindowTab {
                        index: 2,
                        name: "logs".to_owned(),
                        is_active: false,
                    },
                ],
            },
        );
        let rendered = surface_to_string(&surface);

        assert!(rendered.contains("1 shell"));
        assert!(rendered.contains("2 logs"));
        Ok(())
    }

    #[test]
    fn theme_component_overrides_pane_style() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.theme.components.panes.border.active.fg = 33
                remux.theme.components.panes.border.inactive.fg = 44
            "#,
        )?;

        assert_eq!(runtime.pane_style().active_border_fg, 33);
        assert_eq!(runtime.pane_style().inactive_border_fg, 44);
        Ok(())
    }

    #[test]
    fn theme_palette_and_roles_compile_to_widget_styles() -> Result<()> {
        let runtime = runtime_from_code(
            r#"
                remux.theme.palette.brand = 33
                remux.theme.roles.border = { custom = "brand" }
                remux.theme.components.widgets.selector.border.fg = "border.custom"
                remux.widgets.session_switcher = {
                    type = "selector",
                    items = function() return {} end,
                    on_confirm = function() return nil end,
                }
            "#,
        )?;

        let overlay = runtime.load_widget("session_switcher", &sample_state())?;
        let LoadedWidget::Selector(overlay) = overlay else {
            panic!("expected selector widget");
        };
        assert_eq!(overlay.style.border_fg, 33);
        Ok(())
    }

    #[test]
    fn legacy_style_config_is_rejected() {
        let err = runtime_from_code(
            r#"
                remux.ui.widgets.selector.style = { border_fg = 33 }
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("no longer supported"));
    }

    #[test]
    fn widget_local_style_override_is_rejected() {
        let err = runtime_from_code(
            r#"
                remux.widgets.session_switcher = {
                    type = "selector",
                    style = { border_fg = 33 },
                    items = function() return {} end,
                    on_confirm = function() return nil end,
                }
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("widget-local style overrides"));
    }

    #[test]
    fn unknown_theme_reference_is_rejected() {
        let err = runtime_from_code(
            r#"
                remux.theme.components.widgets.selector.border.fg = "does.not.exist"
                remux.widgets.session_switcher = {
                    type = "selector",
                    items = function() return {} end,
                    on_confirm = function() return nil end,
                }
            "#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("unknown theme reference"));
    }
}
