//! The CGA 16-colour palette Turbo Vision drew with, and the mapping from
//! semantic roles (body text, headings, art accents, ...) onto it.

use ratatui::style::{Color, Modifier, Style};

/// One of the sixteen text-mode colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color16 {
    Black,
    Blue,
    Green,
    Cyan,
    Red,
    Magenta,
    Brown,
    LightGray,
    DarkGray,
    LightBlue,
    LightGreen,
    LightCyan,
    LightRed,
    LightMagenta,
    Yellow,
    White,
}

impl Color16 {
    pub const ALL: [(&'static str, Color16); 16] = [
        ("black", Color16::Black),
        ("blue", Color16::Blue),
        ("green", Color16::Green),
        ("cyan", Color16::Cyan),
        ("red", Color16::Red),
        ("magenta", Color16::Magenta),
        ("brown", Color16::Brown),
        ("lightgray", Color16::LightGray),
        ("darkgray", Color16::DarkGray),
        ("lightblue", Color16::LightBlue),
        ("lightgreen", Color16::LightGreen),
        ("lightcyan", Color16::LightCyan),
        ("lightred", Color16::LightRed),
        ("lightmagenta", Color16::LightMagenta),
        ("yellow", Color16::Yellow),
        ("white", Color16::White),
    ];

    /// Parses a colour name (`lightcyan`, `light-cyan` and `LightCyan` are all accepted).
    pub fn parse(name: &str) -> Option<Color16> {
        let wanted: String = name
            .chars()
            .filter(|c| *c != '-' && *c != '_')
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Self::ALL
            .iter()
            .find(|(n, _)| *n == wanted)
            .map(|(_, c)| *c)
    }

    /// The exact VGA text-mode RGB value.
    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Color16::Black => (0x00, 0x00, 0x00),
            Color16::Blue => (0x00, 0x00, 0xAA),
            Color16::Green => (0x00, 0xAA, 0x00),
            Color16::Cyan => (0x00, 0xAA, 0xAA),
            Color16::Red => (0xAA, 0x00, 0x00),
            Color16::Magenta => (0xAA, 0x00, 0xAA),
            Color16::Brown => (0xAA, 0x55, 0x00),
            Color16::LightGray => (0xAA, 0xAA, 0xAA),
            Color16::DarkGray => (0x55, 0x55, 0x55),
            Color16::LightBlue => (0x55, 0x55, 0xFF),
            Color16::LightGreen => (0x55, 0xFF, 0x55),
            Color16::LightCyan => (0x55, 0xFF, 0xFF),
            Color16::LightRed => (0xFF, 0x55, 0x55),
            Color16::LightMagenta => (0xFF, 0x55, 0xFF),
            Color16::Yellow => (0xFF, 0xFF, 0x55),
            Color16::White => (0xFF, 0xFF, 0xFF),
        }
    }

    /// The closest ANSI colour, for terminals without 24-bit colour.
    fn ansi(self) -> Color {
        match self {
            Color16::Black => Color::Black,
            Color16::Blue => Color::Blue,
            Color16::Green => Color::Green,
            Color16::Cyan => Color::Cyan,
            Color16::Red => Color::Red,
            Color16::Magenta => Color::Magenta,
            Color16::Brown => Color::Yellow,
            Color16::LightGray => Color::Gray,
            Color16::DarkGray => Color::DarkGray,
            Color16::LightBlue => Color::LightBlue,
            Color16::LightGreen => Color::LightGreen,
            Color16::LightCyan => Color::LightCyan,
            Color16::LightRed => Color::LightRed,
            Color16::LightMagenta => Color::LightMagenta,
            Color16::Yellow => Color::LightYellow,
            Color16::White => Color::White,
        }
    }
}

/// What a piece of slide content *is*; the theme decides what it looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Text,
    Strong,
    /// `*emphasis*`. Text-mode screens had no italic, so it is a colour.
    Emphasis,
    InlineCode,
    Heading,
    Bullet,
    CodeBlock,
    Art,
    /// Art characters listed in the fence's `accent=` attribute.
    ArtAccent,
    /// Art drawn in an explicit colour from the fence's `color=` attribute.
    ArtColor(Color16),
}

/// The two window flavours Turbo Vision is remembered for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WindowStyle {
    /// White-on-blue document window.
    #[default]
    Window,
    /// Black-on-grey dialog box.
    Dialog,
}

/// Turns colours into terminal styles. Truecolor terminals get the exact VGA
/// palette; everything else gets the nearest ANSI colours.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    truecolor: bool,
}

impl Theme {
    pub fn new(truecolor: bool) -> Theme {
        Theme { truecolor }
    }

    /// Guesses 24-bit support from `COLORTERM`, the de-facto convention.
    pub fn detect() -> Theme {
        let truecolor = std::env::var("COLORTERM")
            .map(|v| v == "truecolor" || v == "24bit")
            .unwrap_or(false);
        Theme::new(truecolor)
    }

    pub fn color(&self, c: Color16) -> Color {
        if self.truecolor {
            let (r, g, b) = c.rgb();
            Color::Rgb(r, g, b)
        } else {
            c.ansi()
        }
    }

    pub fn style(&self, fg: Color16, bg: Color16) -> Style {
        Style::new().fg(self.color(fg)).bg(self.color(bg))
    }

    pub fn window_bg(&self, w: WindowStyle) -> Color16 {
        match w {
            WindowStyle::Window => Color16::Blue,
            WindowStyle::Dialog => Color16::LightGray,
        }
    }

    /// Style for slide content with the given role inside a window.
    pub fn content(&self, w: WindowStyle, role: Role) -> Style {
        use Color16::*;
        let bg = self.window_bg(w);
        let (fg, bg, bold) = match (w, role) {
            (WindowStyle::Window, Role::Text) => (White, bg, false),
            (WindowStyle::Window, Role::Strong) => (Yellow, bg, true),
            (WindowStyle::Window, Role::Emphasis) => (LightGreen, bg, false),
            (WindowStyle::Window, Role::InlineCode) => (LightCyan, bg, false),
            (WindowStyle::Window, Role::Heading) => (Yellow, bg, true),
            (WindowStyle::Window, Role::Bullet) => (LightCyan, bg, false),
            (WindowStyle::Window, Role::CodeBlock) => (Black, Cyan, false),
            (WindowStyle::Window, Role::Art) => (White, bg, false),
            (WindowStyle::Window, Role::ArtAccent) => (Yellow, bg, false),
            (WindowStyle::Dialog, Role::Text) => (Black, bg, false),
            (WindowStyle::Dialog, Role::Strong) => (Blue, bg, true),
            (WindowStyle::Dialog, Role::Emphasis) => (Magenta, bg, false),
            (WindowStyle::Dialog, Role::InlineCode) => (Blue, bg, false),
            (WindowStyle::Dialog, Role::Heading) => (Red, bg, true),
            (WindowStyle::Dialog, Role::Bullet) => (Blue, bg, false),
            (WindowStyle::Dialog, Role::CodeBlock) => (Yellow, Blue, false),
            (WindowStyle::Dialog, Role::Art) => (Black, bg, false),
            (WindowStyle::Dialog, Role::ArtAccent) => (Blue, bg, false),
            (_, Role::ArtColor(c)) => (c, bg, false),
        };
        let style = self.style(fg, bg);
        if bold {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_names_parse_loosely() {
        assert_eq!(Color16::parse("lightcyan"), Some(Color16::LightCyan));
        assert_eq!(Color16::parse("Light-Cyan"), Some(Color16::LightCyan));
        assert_eq!(Color16::parse("light_gray"), Some(Color16::LightGray));
        assert_eq!(Color16::parse("chartreuse"), None);
    }

    #[test]
    fn truecolor_uses_vga_values() {
        assert_eq!(
            Theme::new(true).color(Color16::Blue),
            Color::Rgb(0, 0, 0xAA)
        );
        assert_eq!(Theme::new(false).color(Color16::Blue), Color::Blue);
    }
}
