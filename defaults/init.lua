remux.prefix = "C-b"

local catppuccin = {
	crust = 234,
	mantle = 235,
	base = 236,
	surface0 = 238,
	surface1 = 239,
	surface2 = 240,
	text = 253,
	subtext1 = 250,
	overlay1 = 246,
	blue = 111,
	lavender = 147,
	teal = 116,
	mauve = 183,
	flamingo = 210,
	green = 150,
}

remux.keymaps = {
	{ key = "%", action = "split-pane-vertical" },
	{ key = "\"", action = "split-pane-horizontal" },
	{ key = "C-h", action = "focus-pane-left", prefix = false },
	{ key = "C-j", action = "focus-pane-down", prefix = false },
	{ key = "C-k", action = "focus-pane-up", prefix = false },
	{ key = "C-l", action = "focus-pane-right", prefix = false },
	{ key = "x", action = "kill-pane" },
	{ key = "c", action = "new-window" },
	{ key = "n", action = "next-window" },
	{ key = "p", action = "prev-window" },
	{ key = "&", action = "kill-window" },
	{ key = "1", action = "select-window-1" },
	{ key = "2", action = "select-window-2" },
	{ key = "3", action = "select-window-3" },
	{ key = "4", action = "select-window-4" },
	{ key = "5", action = "select-window-5" },
	{ key = "6", action = "select-window-6" },
	{ key = "7", action = "select-window-7" },
	{ key = "8", action = "select-window-8" },
	{ key = "9", action = "select-window-9" },
	{ key = "d", action = "detach" },
	{ key = "s", action = "session_switcher" },
}

remux.actions.session_switcher = function()
	return remux.open_widget("session_switcher")
end

remux.theme = {
	palette = catppuccin,
	roles = {
		bg = {
			default = "crust",
			surface = "mantle",
			panel = "base",
		},
		fg = {
			default = "text",
			muted = "overlay1",
			subtle = "subtext1",
		},
		accent = {
			primary = "blue",
			secondary = "lavender",
			session = "mauve",
			info = "teal",
		},
		border = {
			default = "surface2",
			inactive = "surface1",
		},
		selection = {
			active = {
				fg = "crust",
				bg = "blue",
			},
			secondary = {
				fg = "crust",
				bg = "lavender",
			},
		},
		scrim = {
			default = {
				fg = "crust",
				bg = "crust",
			},
		},
	},
	components = {
		bars = {
			status = {
				background = {
					fg = "fg.subtle",
					bg = "bg.default",
				},
				left = {
					fg = "bg.default",
					bg = "accent.session",
				},
				center = {
					fg = "accent.secondary",
					bg = "bg.default",
				},
				right = {
					fg = "accent.info",
					bg = "bg.default",
				},
				window = {
					active = {
						fg = "selection.active.fg",
						bg = "selection.active.bg",
					},
					inactive = {
						fg = "fg.muted",
						bg = "bg.panel",
					},
					muted = {
						fg = "fg.muted",
					},
				},
			},
		},
		widgets = {
			selector = {
				scrim = {
					fg = "scrim.default.fg",
					bg = "scrim.default.bg",
				},
				border = {
					fg = "border.default",
				},
				surface = {
					fg = "fg.default",
					bg = "bg.surface",
				},
				title = {
					fg = "accent.session",
				},
				text = {
					fg = "fg.default",
				},
				selection = {
					fg = "selection.secondary.fg",
					bg = "selection.secondary.bg",
				},
				footer = {
					fg = "fg.subtle",
				},
				empty = {
					fg = "fg.muted",
				},
			},
			fuzzy_selector = {
				scrim = {
					fg = "scrim.default.fg",
					bg = "scrim.default.bg",
				},
				border = {
					fg = "border.default",
				},
				surface = {
					fg = "fg.default",
					bg = "bg.surface",
				},
				title = {
					fg = "accent.session",
				},
				text = {
					fg = "fg.default",
				},
				selection = {
					fg = "selection.active.fg",
					bg = "selection.active.bg",
				},
				footer = {
					fg = "fg.subtle",
				},
				query = {
					fg = "fg.default",
					bg = "bg.panel",
				},
				placeholder = {
					fg = "fg.muted",
				},
				empty = {
					fg = "fg.muted",
				},
			},
		},
		panes = {
			border = {
				active = {
					fg = "accent.primary",
				},
				inactive = {
					fg = "border.inactive",
				},
			},
		},
	},
}

remux.widgets.session_switcher = {
	type = "fuzzy-selector",
	title = "Session Switcher",
	footer = "Type Filter  Up/Down Move  Enter Open  Esc Cancel",
	placeholder = "Jump to a session",
	items = function()
		local items = {}
		local current = remux.state.current_session
		local current_name = current and current.name or nil
		for _, session in ipairs(remux.state.sessions) do
			table.insert(items, {
				id = session.name,
				label = session.name,
				selected = session.name == current_name,
			})
		end
		return items
	end,
	on_confirm = function(id)
		return remux.switch_session(id)
	end,
}

remux.widgets.status = {
	type = "bar",
	placement = "dock",
	edge = "bottom",
	size = 1,
	enabled = true,
	left = {
		"active-session",
	},
	center = {
		"window-list",
	},
	right = {
		function()
			return os.date("%Y-%m-%d %H:%M:%S")
		end,
		function()
			local f = io.popen("git rev-parse --abbrev-ref HEAD 2>/dev/null")
			if not f then
				return nil
			end
			local branch = f:read("*l")
			f:close()
			return branch
		end,
	},
}
