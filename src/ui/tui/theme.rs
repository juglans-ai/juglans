use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub border: Color,
    pub _border_focused: Color,
    pub _user_msg: Color,
    pub _assistant_msg: Color,
    pub tool_ok: Color,
    pub tool_err: Color,
    pub tool_pending: Color,
    pub muted: Color,
    pub heading: Color,
    pub code: Color,
    pub thinking: Color,
    pub input_bg: Color,
    pub input_border: Color,
    pub logo_dim: Color,
    pub logo_bright: Color,
    pub tip: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::Rgb(9, 9, 9),
            fg: Color::Rgb(200, 200, 210),
            accent: Color::Rgb(191, 255, 0), // #bfff00
            border: Color::Rgb(60, 60, 70),
            _border_focused: Color::Rgb(191, 255, 0),
            _user_msg: Color::Rgb(200, 200, 210),
            _assistant_msg: Color::Rgb(200, 200, 210),
            tool_ok: Color::Rgb(100, 200, 100),
            tool_err: Color::Rgb(230, 80, 80),
            tool_pending: Color::Rgb(200, 200, 80),
            muted: Color::Rgb(100, 100, 120),
            heading: Color::Rgb(200, 200, 220),
            code: Color::Rgb(130, 200, 130),
            thinking: Color::Rgb(210, 170, 80),
            input_bg: Color::Rgb(20, 20, 20),
            input_border: Color::Rgb(191, 255, 0),
            logo_dim: Color::Rgb(120, 120, 140),
            logo_bright: Color::Rgb(200, 200, 220),
            tip: Color::Rgb(210, 170, 80),
        }
    }
}
