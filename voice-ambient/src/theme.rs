use ratatui::style::Color;

#[derive(Copy, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub primary: Color,    // borders, titles, active breadcrumb
    pub secondary: Color,  // command preview, accent text, stats
    pub success: Color,    // pass indicators, positive states
    pub text: Color,       // primary body text
    pub dim: Color,        // hints, timestamps, inactive text
    pub warn: Color,       // warning glyphs
    pub err: Color,        // error/fail glyphs
    pub rec_on: Color,     // REC blink "on" state in ambient TUI
    pub hi_fg: Color,      // selected-row foreground
    pub hi_bg: Color,      // selected-row background
}

pub const THEMES: &[Theme] = &[
    Theme {
        name: "Cyan",
        primary:   Color::Cyan,
        secondary: Color::Yellow,
        success:   Color::Green,
        text:      Color::White,
        dim:       Color::DarkGray,
        warn:      Color::Yellow,
        err:       Color::Red,
        rec_on:    Color::Red,
        hi_fg:     Color::Black,
        hi_bg:     Color::Cyan,
    },
    Theme {
        name: "Amber",
        primary:   Color::Yellow,
        secondary: Color::LightYellow,
        success:   Color::LightGreen,
        text:      Color::White,
        dim:       Color::DarkGray,
        warn:      Color::Yellow,
        err:       Color::LightRed,
        rec_on:    Color::LightRed,
        hi_fg:     Color::Black,
        hi_bg:     Color::Yellow,
    },
    Theme {
        name: "Phosphor",
        primary:   Color::Green,
        secondary: Color::LightGreen,
        success:   Color::Green,
        text:      Color::LightGreen,
        dim:       Color::DarkGray,
        warn:      Color::LightYellow,
        err:       Color::LightRed,
        rec_on:    Color::LightGreen,
        hi_fg:     Color::Black,
        hi_bg:     Color::Green,
    },
    Theme {
        name: "Neon",
        primary:   Color::LightMagenta,
        secondary: Color::LightCyan,
        success:   Color::LightGreen,
        text:      Color::White,
        dim:       Color::DarkGray,
        warn:      Color::LightYellow,
        err:       Color::LightRed,
        rec_on:    Color::LightMagenta,
        hi_fg:     Color::Black,
        hi_bg:     Color::Magenta,
    },
    Theme {
        name: "Ocean",
        primary:   Color::LightBlue,
        secondary: Color::Cyan,
        success:   Color::LightGreen,
        text:      Color::White,
        dim:       Color::DarkGray,
        warn:      Color::LightYellow,
        err:       Color::LightRed,
        rec_on:    Color::LightBlue,
        hi_fg:     Color::White,
        hi_bg:     Color::Blue,
    },
];

const THEME_FILE: &str = "/.config/voice-input/theme";

pub fn load_theme_idx() -> usize {
    let home = std::env::var("HOME").unwrap_or_default();
    let name = std::fs::read_to_string(format!("{}{}", home, THEME_FILE))
        .unwrap_or_default();
    let name = name.trim();
    THEMES.iter().position(|t| t.name == name).unwrap_or(0)
}

pub fn save_theme_idx(idx: usize) {
    let home = std::env::var("HOME").unwrap_or_default();
    let dir = format!("{}/.config/voice-input", home);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(format!("{}{}", home, THEME_FILE), THEMES[idx % THEMES.len()].name);
}
