use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub accent: Color,
    pub border: Color,
    pub inactive: Color,
    pub muted: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub status_background: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color::Reset,
            foreground: Color::Reset,
            accent: Color::Cyan,
            border: Color::Gray,
            inactive: Color::DarkGray,
            muted: Color::DarkGray,
            error: Color::LightRed,
            warning: Color::Yellow,
            success: Color::Green,
            status_background: Color::Reset,
        }
    }
}
