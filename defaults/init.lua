remux.prefix = "C-b"

remux.keymaps = {
	{ key = "%", action = "split-pane-vertical" },
	{ key = "\"", action = "split-pane-horizontal" },
	{ key = "C-h", action = "focus-pane-left", prefix = false },
	{ key = "C-j", action = "focus-pane-down", prefix = false },
	{ key = "C-k", action = "focus-pane-up", prefix = false },
	{ key = "C-l", action = "focus-pane-right", prefix = false },
	{ key = "x", action = "kill-pane" },
	{ key = "d", action = "detach" },
	{ key = "s", action = "open-session-switcher" },
}

remux.ui.bars.status = {
	enabled = true,
	sections = {
		left = {
			"active-session",
		},
		center = {
			function()
				return os.date("%Y-%m-%d %H:%M:%S")
			end,
		},
		right = {
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
	},
}
