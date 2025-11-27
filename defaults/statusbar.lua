ui.status_line = {
	sections = {
		a = {
			get_active_session(),
		},
		b = {
			function()
				local s = os.date("%Y-%m-%d %H:%M:%S")
				return s
			end,
		},
		c = {
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
