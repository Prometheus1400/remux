ui.status_line = {
	sections = {
		a = {
			get_active_session(),
		},
		b = {
		},
		c = {
			function()
        local s = os.date("%Y-%m-%d %H:%M:%S")
        return s
			end,
		},
	},
}
