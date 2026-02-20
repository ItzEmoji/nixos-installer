use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// A complete color theme for the installer TUI.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub name: &'static str,
    pub accent: Color,
    pub accent_dim: Color,
    pub bg: Color,
    pub surface: Color,
    pub text: Color,
    pub text_dim: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
}

/// Theme names that can be specified in config or CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    CatppuccinMocha,
    Nord,
    Dracula,
    TokyoNight,
    Gruvbox,
}

impl Default for ThemeName {
    fn default() -> Self {
        Self::CatppuccinMocha
    }
}

impl std::fmt::Display for ThemeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CatppuccinMocha => write!(f, "catppuccin-mocha"),
            Self::Nord => write!(f, "nord"),
            Self::Dracula => write!(f, "dracula"),
            Self::TokyoNight => write!(f, "tokyo-night"),
            Self::Gruvbox => write!(f, "gruvbox"),
        }
    }
}

impl ThemeName {
    /// Parse a theme name from a string (case-insensitive, accepts kebab-case).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('_', "-").as_str() {
            "catppuccin-mocha" | "catppuccin" | "mocha" => Some(Self::CatppuccinMocha),
            "nord" => Some(Self::Nord),
            "dracula" => Some(Self::Dracula),
            "tokyo-night" | "tokyonight" => Some(Self::TokyoNight),
            "gruvbox" => Some(Self::Gruvbox),
            _ => None,
        }
    }

    /// List all available theme names for help text.
    pub fn all_names() -> &'static [&'static str] {
        &[
            "catppuccin-mocha",
            "nord",
            "dracula",
            "tokyo-night",
            "gruvbox",
        ]
    }

    /// Build the actual Theme from this name.
    pub fn to_theme(&self) -> Theme {
        match self {
            Self::CatppuccinMocha => Theme {
                name: "catppuccin-mocha",
                accent: Color::Rgb(137, 180, 250),    // Blue
                accent_dim: Color::Rgb(88, 91, 112),   // Overlay0
                bg: Color::Rgb(30, 30, 46),             // Base
                surface: Color::Rgb(49, 50, 68),        // Surface0
                text: Color::Rgb(205, 214, 244),        // Text
                text_dim: Color::Rgb(147, 153, 178),    // Overlay1
                red: Color::Rgb(243, 139, 168),         // Red
                green: Color::Rgb(166, 227, 161),       // Green
                yellow: Color::Rgb(249, 226, 175),      // Yellow
            },
            Self::Nord => Theme {
                name: "nord",
                accent: Color::Rgb(136, 192, 208),     // Nord8 (frost)
                accent_dim: Color::Rgb(76, 86, 106),    // Nord3
                bg: Color::Rgb(46, 52, 64),              // Nord0
                surface: Color::Rgb(59, 66, 82),         // Nord1
                text: Color::Rgb(236, 239, 244),         // Nord6
                text_dim: Color::Rgb(216, 222, 233),     // Nord4
                red: Color::Rgb(191, 97, 106),           // Nord11
                green: Color::Rgb(163, 190, 140),        // Nord14
                yellow: Color::Rgb(235, 203, 139),       // Nord13
            },
            Self::Dracula => Theme {
                name: "dracula",
                accent: Color::Rgb(189, 147, 249),     // Purple
                accent_dim: Color::Rgb(98, 114, 164),    // Comment
                bg: Color::Rgb(40, 42, 54),              // Background
                surface: Color::Rgb(68, 71, 90),         // Current Line
                text: Color::Rgb(248, 248, 242),         // Foreground
                text_dim: Color::Rgb(98, 114, 164),      // Comment
                red: Color::Rgb(255, 85, 85),            // Red
                green: Color::Rgb(80, 250, 123),         // Green
                yellow: Color::Rgb(241, 250, 140),       // Yellow
            },
            Self::TokyoNight => Theme {
                name: "tokyo-night",
                accent: Color::Rgb(122, 162, 247),     // Blue
                accent_dim: Color::Rgb(61, 89, 161),    // Blue dim
                bg: Color::Rgb(26, 27, 38),              // bg_dark
                surface: Color::Rgb(36, 40, 59),         // bg_highlight
                text: Color::Rgb(192, 202, 245),         // fg
                text_dim: Color::Rgb(86, 95, 137),       // dark5
                red: Color::Rgb(247, 118, 142),          // red
                green: Color::Rgb(158, 206, 106),        // green
                yellow: Color::Rgb(224, 175, 104),       // yellow
            },
            Self::Gruvbox => Theme {
                name: "gruvbox",
                accent: Color::Rgb(131, 165, 152),     // aqua
                accent_dim: Color::Rgb(102, 92, 84),    // bg3
                bg: Color::Rgb(40, 40, 40),              // bg0
                surface: Color::Rgb(60, 56, 54),         // bg1
                text: Color::Rgb(235, 219, 178),         // fg
                text_dim: Color::Rgb(168, 153, 132),     // gray
                red: Color::Rgb(251, 73, 52),            // red
                green: Color::Rgb(184, 187, 38),         // green
                yellow: Color::Rgb(250, 189, 47),        // yellow
            },
        }
    }
}
