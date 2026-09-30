//! Turning a grid of terminal cells into pixels, for the native window.
//!
//! Box-drawing, block and shade characters are drawn as geometry rather than
//! taken from a font, so lines meet exactly at cell edges at any scale and
//! the art joins up the way it did on a VGA screen. Everything else is
//! rasterised from a monospace font.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::ui::{SCREEN_HEIGHT, SCREEN_WIDTH};

/// Colour of the letterbox around the grid, and of anything uncoloured.
const BLACK: u32 = 0x000000;

/// Where the character grid sits on a pixel surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u32,
    pub cell_h: u32,
    /// Top-left of the grid; the leftover pixels are split evenly around it.
    pub x0: u32,
    pub y0: u32,
}

impl Grid {
    /// The largest cells that fit an 80x25 screen onto `width` x `height`
    /// pixels, keeping them roughly VGA-shaped (9:16). Surfaces of a
    /// different shape get extra columns or rows rather than distorted
    /// cells, just as a bigger terminal gets a bigger desktop.
    pub fn fit(width: u32, height: u32) -> Grid {
        let (sw, sh) = (u32::from(SCREEN_WIDTH), u32::from(SCREEN_HEIGHT));
        let cell_w = (width / sw).min(height / sh * 9 / 16).max(1);
        let cell_h = (height / sh).min(cell_w * 9 / 4).max(1);
        let cols = (width / cell_w).min(u32::from(u16::MAX));
        let rows = (height / cell_h).min(u32::from(u16::MAX));
        Grid {
            cols: cols as u16,
            rows: rows as u16,
            cell_w,
            cell_h,
            x0: (width - cols * cell_w) / 2,
            y0: (height - rows * cell_h) / 2,
        }
    }
}

/// The fonts glyphs are drawn from.
pub struct Fonts {
    regular: FontVec,
    bold: FontVec,
    /// Consulted, in order, for characters the main font lacks.
    fallback: Vec<FontVec>,
}

/// Monospace fonts to try, as (path, regular index, bold index) in a
/// collection. Menlo ships with every Mac.
const FONT_CANDIDATES: &[(&str, u32, u32)] = &[
    ("/System/Library/Fonts/Menlo.ttc", 0, 1),
    ("/System/Library/Fonts/SFNSMono.ttf", 0, 0),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 0, 0),
    ("/usr/share/fonts/TTF/DejaVuSansMono.ttf", 0, 0),
    ("C:\\Windows\\Fonts\\consola.ttf", 0, 0),
];

/// Wide-coverage fonts for symbols and CJK the monospace font lacks.
const FALLBACK_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

fn load_font(path: &Path, index: u32) -> Result<FontVec, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    FontVec::try_from_vec_and_index(data, index).map_err(|e| format!("{}: {e}", path.display()))
}

impl Fonts {
    /// Loads `path` if given, otherwise the first system monospace font found.
    pub fn load(path: Option<&Path>) -> Result<Fonts, String> {
        let (regular, bold) = match path {
            Some(p) => (load_font(p, 0)?, load_font(p, 0)?),
            None => {
                let (p, r, b) = FONT_CANDIDATES
                    .iter()
                    .find(|(p, _, _)| Path::new(p).exists())
                    .ok_or("no monospace font found; pass one with --font PATH")?;
                (load_font(Path::new(p), *r)?, load_font(Path::new(p), *b)?)
            }
        };
        let fallback = FALLBACK_CANDIDATES
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .filter_map(|p| load_font(&p, 0).ok())
            .collect();
        Ok(Fonts {
            regular,
            bold,
            fallback,
        })
    }

    /// The font to draw `c` with, preferring the bold face if asked.
    fn for_char(&self, c: char, bold: bool) -> Option<&FontVec> {
        let main = if bold { &self.bold } else { &self.regular };
        std::iter::once(main)
            .chain(std::iter::once(&self.regular))
            .chain(self.fallback.iter())
            .find(|f| f.glyph_id(c).0 != 0)
    }
}

/// A rasterised glyph, positioned relative to its cell's top-left corner.
struct Glyph {
    x: i32,
    y: i32,
    width: usize,
    height: usize,
    coverage: Vec<u8>,
}

/// Draws cell buffers into a pixel framebuffer (`0x00RRGGBB` per pixel),
/// redrawing only the rows that changed since the last frame.
pub struct Renderer {
    fonts: Option<Fonts>,
    width: u32,
    height: u32,
    grid: Grid,
    /// Font size in pixels, and the baseline's offset from the cell top.
    px: f32,
    baseline: f32,
    glyphs: HashMap<(char, bool, u8), Option<Glyph>>,
    pixels: Vec<u32>,
    last: Option<Buffer>,
}

impl Renderer {
    /// A renderer for a `width` x `height` pixel surface. Without fonts only
    /// the geometric characters are drawn, which is what the tests use.
    pub fn new(fonts: Option<Fonts>, width: u32, height: u32) -> Renderer {
        let mut r = Renderer {
            fonts,
            width: 0,
            height: 0,
            grid: Grid::fit(1, 1),
            px: 0.0,
            baseline: 0.0,
            glyphs: HashMap::new(),
            pixels: Vec::new(),
            last: None,
        };
        r.resize(width, height);
        r
    }

    pub fn grid(&self) -> Grid {
        self.grid
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return;
        }
        self.width = width;
        self.height = height;
        self.grid = Grid::fit(width, height);
        self.pixels = vec![BLACK; width as usize * height as usize];
        self.glyphs.clear();
        self.last = None;
        let (cw, ch) = (self.grid.cell_w as f32, self.grid.cell_h as f32);
        if let Some(fonts) = &self.fonts {
            // Size the font so one advance fills a cell and a line fits its
            // height, then centre the line vertically.
            // In ab_glyph a scale of N pixels makes ascent - descent N
            // pixels tall, so work in fractions of that line height.
            let f = &fonts.regular;
            let line = (f.ascent_unscaled() - f.descent_unscaled()).max(1.0);
            let advance = (f.h_advance_unscaled(f.glyph_id('M')) / line).max(0.01);
            self.px = (cw / advance).min(ch);
            self.baseline = (ch - self.px) / 2.0 + self.px * f.ascent_unscaled() / line;
        }
    }

    /// Draws `buf`, which must be `grid().cols` x `grid().rows`, and returns
    /// the whole framebuffer.
    pub fn render(&mut self, buf: &Buffer) -> &[u32] {
        let area = buf.area;
        let full = self.last.as_ref().map(|l| l.area) != Some(area);
        for y in 0..area.height {
            let changed = full
                || self.last.as_ref().is_some_and(|last| {
                    (0..area.width)
                        .any(|x| last[(area.x + x, area.y + y)] != buf[(area.x + x, area.y + y)])
                });
            if changed {
                self.draw_row(buf, y);
            }
        }
        self.last = Some(buf.clone());
        &self.pixels
    }

    fn draw_row(&mut self, buf: &Buffer, y: u16) {
        let area = buf.area;
        let cols = area.width.min(self.grid.cols);
        if y >= self.grid.rows {
            return;
        }
        // Backgrounds first, so a wide glyph is not painted over by the
        // blank cell after it.
        for x in 0..cols {
            let cell = &buf[(area.x + x, area.y + y)];
            let (_, bg) = colours(cell);
            let r = self.cell_rect(x, y, 1);
            self.fill(r, bg);
        }
        for x in 0..cols {
            let cell = &buf[(area.x + x, area.y + y)];
            let sym = cell.symbol();
            let mut chars = sym.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                continue; // blank, or a combining sequence we do not draw
            };
            if c == ' ' {
                continue;
            }
            let (fg, _) = colours(cell);
            let span = if sym.width() == 2 && x + 1 < cols {
                2
            } else {
                1
            };
            let r = self.cell_rect(x, y, span);
            if !self.draw_geometric(c, r, fg) {
                let bold = cell.modifier.contains(Modifier::BOLD);
                self.draw_glyph(c, bold, span, r, fg);
            }
        }
    }

    fn cell_rect(&self, x: u16, y: u16, span: u32) -> Rect {
        let g = &self.grid;
        Rect {
            x: g.x0 + u32::from(x) * g.cell_w,
            y: g.y0 + u32::from(y) * g.cell_h,
            w: g.cell_w * span,
            h: g.cell_h,
        }
    }

    /// Fills `x0..x1` by `y0..y1`, clipped to the cell `clip`.
    fn fill_clipped(&mut self, clip: Rect, x0: i64, y0: i64, x1: i64, y1: i64, colour: u32) {
        let xa = x0.max(i64::from(clip.x));
        let xb = x1.min(i64::from(clip.x + clip.w));
        let ya = y0.max(i64::from(clip.y));
        let yb = y1.min(i64::from(clip.y + clip.h));
        if xa >= xb || ya >= yb {
            return;
        }
        let stride = self.width as usize;
        for y in ya as usize..yb as usize {
            self.pixels[y * stride + xa as usize..y * stride + xb as usize].fill(colour);
        }
    }

    fn fill(&mut self, r: Rect, colour: u32) {
        let (x0, y0) = (i64::from(r.x), i64::from(r.y));
        self.fill_clipped(r, x0, y0, x0 + i64::from(r.w), y0 + i64::from(r.h), colour);
    }

    /// Draws box-drawing, block and shade characters. Returns false for any
    /// other character.
    fn draw_geometric(&mut self, c: char, r: Rect, fg: u32) -> bool {
        if let Some(arms) = box_arms(c) {
            self.draw_box(arms, r, fg);
            return true;
        }
        let (x0, y0) = (i64::from(r.x), i64::from(r.y));
        let (w, h) = (i64::from(r.w), i64::from(r.h));
        // Block elements, as fractions of the cell in eighths.
        let block = match c {
            '█' => Some((0, 0, 8, 8)),
            '▀' => Some((0, 0, 8, 4)),
            '▄' => Some((0, 4, 8, 8)),
            '▌' => Some((0, 0, 4, 8)),
            '▐' => Some((4, 0, 8, 8)),
            '▔' => Some((0, 0, 8, 1)),
            '▁' => Some((0, 7, 8, 8)),
            '▂' => Some((0, 6, 8, 8)),
            '▃' => Some((0, 5, 8, 8)),
            '▅' => Some((0, 3, 8, 8)),
            '▆' => Some((0, 2, 8, 8)),
            '▇' => Some((0, 1, 8, 8)),
            '▏' => Some((0, 0, 1, 8)),
            '▎' => Some((0, 0, 2, 8)),
            '▍' => Some((0, 0, 3, 8)),
            '▋' => Some((0, 0, 5, 8)),
            '▊' => Some((0, 0, 6, 8)),
            '▉' => Some((0, 0, 7, 8)),
            '▕' => Some((7, 0, 8, 8)),
            _ => None,
        };
        if let Some((a, b, cc, d)) = block {
            self.fill_clipped(
                r,
                x0 + w * a / 8,
                y0 + h * b / 8,
                x0 + w * cc / 8,
                y0 + h * d / 8,
                fg,
            );
            return true;
        }
        // Shades, as the VGA font drew them: a dot pattern on a grid of
        // "font pixels" (8 across a cell), anchored to the screen so the
        // pattern runs seamlessly from cell to cell.
        let shade: Option<fn(u32, u32) -> bool> = match c {
            '░' => Some(|i, j| i % 4 == (j % 2) * 2),
            '▒' => Some(|i, j| (i + j) % 2 == 0),
            '▓' => Some(|i, j| i % 4 != (j % 2) * 2),
            _ => None,
        };
        if let Some(on) = shade {
            let dot_w = (r.w / 8).max(1);
            let dot_h = (r.h / 16).max(1);
            let stride = self.width as usize;
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    if on(x / dot_w, y / dot_h) {
                        self.pixels[y as usize * stride + x as usize] = fg;
                    }
                }
            }
            return true;
        }
        false
    }

    fn draw_box(&mut self, arms: Arms, r: Rect, fg: u32) {
        let (x0, y0) = (i64::from(r.x), i64::from(r.y));
        let (x1, y1) = (x0 + i64::from(r.w), y0 + i64::from(r.h));
        let cx = x0 + i64::from(r.w) / 2;
        let cy = y0 + i64::from(r.h) / 2;
        let lw = (i64::from(r.w) / 8).max(1);
        // Offset of each line of a double pair from the centre line.
        let g = (i64::from(r.w) / 6).max(lw);
        let Arms {
            up,
            right,
            down,
            left,
        } = arms;
        let v_double = up == Line::Double || down == Line::Double;
        let h_double = left == Line::Double || right == Line::Double;
        let hbar = |s: &mut Self, xa: i64, xb: i64, yc: i64, t: i64| {
            s.fill_clipped(r, xa, yc - t / 2, xb, yc - t / 2 + t, fg)
        };
        let vbar = |s: &mut Self, ya: i64, yb: i64, xc: i64, t: i64| {
            s.fill_clipped(r, xc - t / 2, ya, xc - t / 2 + t, yb, fg)
        };

        // How far an arm reaches back past the centre, towards the far
        // side, to meet the perpendicular line. `near` and `far` are the
        // perpendicular double lines on this arm's side and the other side.
        //
        // A single arm meeting a double line joins the nearer of the pair
        // at a tee and the farther at a corner. A line of a double arm stops
        // at the nearer line of a perpendicular double if there is an arm on
        // its own side, and runs on to the farther one otherwise.
        let single_reach = |perp_double: bool, both_sides: bool| -> i64 {
            match (perp_double, both_sides) {
                (false, _) => -lw / 2 - lw % 2,
                (true, true) => g - lw / 2,
                (true, false) => -g - lw / 2,
            }
        };
        let double_reach = |perp_double: bool, own_side: bool| -> i64 {
            match (perp_double, own_side) {
                (false, _) => -g - lw / 2,
                (true, true) => g - lw / 2,
                (true, false) => -g - lw / 2,
            }
        };

        // Horizontal arms.
        let v_both = up != Line::None && down != Line::None;
        for (kind, dir) in [(right, 1i64), (left, -1i64)] {
            let span = |s: &mut Self, reach: i64, yc: i64, t: i64| {
                if dir > 0 {
                    hbar(s, cx - reach, x1, yc, t)
                } else {
                    hbar(s, x0, cx + reach + 1, yc, t)
                }
            };
            match kind {
                Line::None => {}
                Line::Light | Line::Heavy => {
                    let t = if kind == Line::Heavy { lw * 2 } else { lw };
                    let reach = -single_reach(v_double, v_both);
                    span(self, reach, cy, t);
                }
                Line::Double => {
                    for (yc, own) in [(cy - g, up == Line::Double), (cy + g, down == Line::Double)]
                    {
                        let reach = -double_reach(v_double, own);
                        span(self, reach, yc, lw);
                    }
                }
            }
        }
        // Vertical arms.
        let h_both = left != Line::None && right != Line::None;
        for (kind, dir) in [(down, 1i64), (up, -1i64)] {
            let span = |s: &mut Self, reach: i64, xc: i64, t: i64| {
                if dir > 0 {
                    vbar(s, cy - reach, y1, xc, t)
                } else {
                    vbar(s, y0, cy + reach + 1, xc, t)
                }
            };
            match kind {
                Line::None => {}
                Line::Light | Line::Heavy => {
                    let t = if kind == Line::Heavy { lw * 2 } else { lw };
                    let reach = -single_reach(h_double, h_both);
                    span(self, reach, cx, t);
                }
                Line::Double => {
                    for (xc, own) in [
                        (cx - g, left == Line::Double),
                        (cx + g, right == Line::Double),
                    ] {
                        let reach = -double_reach(h_double, own);
                        span(self, reach, xc, lw);
                    }
                }
            }
        }
    }

    fn draw_glyph(&mut self, c: char, bold: bool, span: u32, r: Rect, fg: u32) {
        let key = (c, bold, span as u8);
        if !self.glyphs.contains_key(&key) {
            let glyph = self.rasterise(c, bold, span);
            self.glyphs.insert(key, glyph);
        }
        let Some(glyph) = &self.glyphs[&key] else {
            return;
        };
        let stride = self.width as usize;
        let (fr, fgc, fb) = split(fg);
        for gy in 0..glyph.height {
            let py = i64::from(r.y) + i64::from(glyph.y) + gy as i64;
            if py < i64::from(r.y) || py >= i64::from(r.y + r.h) {
                continue;
            }
            for gx in 0..glyph.width {
                let px = i64::from(r.x) + i64::from(glyph.x) + gx as i64;
                if px < i64::from(r.x) || px >= i64::from(r.x + r.w) {
                    continue;
                }
                let a = u32::from(glyph.coverage[gy * glyph.width + gx]);
                if a == 0 {
                    continue;
                }
                let i = py as usize * stride + px as usize;
                let (br, bgc, bb) = split(self.pixels[i]);
                let mix = |f: u32, b: u32| (f * a + b * (255 - a)) / 255;
                self.pixels[i] = mix(fr, br) << 16 | mix(fgc, bgc) << 8 | mix(fb, bb);
            }
        }
    }

    fn rasterise(&self, c: char, bold: bool, span: u32) -> Option<Glyph> {
        let font = self.fonts.as_ref()?.for_char(c, bold)?;
        let scale = PxScale::from(self.px);
        let id = font.glyph_id(c);
        let advance = font.as_scaled(scale).h_advance(id);
        let cell_w = (self.grid.cell_w * span) as f32;
        // Centre the advance in the cell; the font was sized so it fits.
        let pad = ((cell_w - advance) / 2.0).max(0.0);
        let outline =
            font.outline_glyph(id.with_scale_and_position(scale, point(pad, self.baseline)))?;
        let bounds = outline.px_bounds();
        let (width, height) = (bounds.width() as usize, bounds.height() as usize);
        let mut coverage = vec![0u8; width * height];
        outline.draw(|x, y, v| {
            let i = y as usize * width + x as usize;
            if let Some(c) = coverage.get_mut(i) {
                *c = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        Some(Glyph {
            x: bounds.min.x as i32,
            y: bounds.min.y as i32,
            width,
            height,
            coverage,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn split(c: u32) -> (u32, u32, u32) {
    (c >> 16 & 0xFF, c >> 8 & 0xFF, c & 0xFF)
}

/// A cell's foreground and background as pixels, honouring reverse video.
fn colours(cell: &Cell) -> (u32, u32) {
    let fg = pixel(cell.fg, 0xAAAAAA);
    let bg = pixel(cell.bg, BLACK);
    if cell.modifier.contains(Modifier::REVERSED) {
        (bg, fg)
    } else {
        (fg, bg)
    }
}

fn pixel(c: Color, reset: u32) -> u32 {
    match c {
        Color::Rgb(r, g, b) => u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
        Color::Reset => reset,
        Color::Black => 0x000000,
        Color::Red => 0xAA0000,
        Color::Green => 0x00AA00,
        Color::Yellow => 0xAA5500,
        Color::Blue => 0x0000AA,
        Color::Magenta => 0xAA00AA,
        Color::Cyan => 0x00AAAA,
        Color::Gray => 0xAAAAAA,
        Color::DarkGray => 0x555555,
        Color::LightRed => 0xFF5555,
        Color::LightGreen => 0x55FF55,
        Color::LightYellow => 0xFFFF55,
        Color::LightBlue => 0x5555FF,
        Color::LightMagenta => 0xFF55FF,
        Color::LightCyan => 0x55FFFF,
        Color::White => 0xFFFFFF,
        Color::Indexed(_) => reset,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    None,
    Light,
    Heavy,
    Double,
}

/// The lines leaving the centre of a box-drawing character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arms {
    up: Line,
    right: Line,
    down: Line,
    left: Line,
}

fn box_arms(c: char) -> Option<Arms> {
    use Line::{Double as D, Heavy as H, Light as L, None as N};
    let (up, right, down, left) = match c {
        '─' | '┄' | '┈' | '╌' => (N, L, N, L),
        '│' | '┆' | '┊' | '╎' => (L, N, L, N),
        '┌' | '╭' => (N, L, L, N),
        '┐' | '╮' => (N, N, L, L),
        '└' | '╰' => (L, L, N, N),
        '┘' | '╯' => (L, N, N, L),
        '├' => (L, L, L, N),
        '┤' => (L, N, L, L),
        '┬' => (N, L, L, L),
        '┴' => (L, L, N, L),
        '┼' => (L, L, L, L),
        '╴' => (N, N, N, L),
        '╵' => (L, N, N, N),
        '╶' => (N, L, N, N),
        '╷' => (N, N, L, N),
        '━' | '┅' | '┉' | '╍' => (N, H, N, H),
        '┃' | '┇' | '┋' | '╏' => (H, N, H, N),
        '┏' => (N, H, H, N),
        '┓' => (N, N, H, H),
        '┗' => (H, H, N, N),
        '┛' => (H, N, N, H),
        '┣' => (H, H, H, N),
        '┫' => (H, N, H, H),
        '┳' => (N, H, H, H),
        '┻' => (H, H, N, H),
        '╋' => (H, H, H, H),
        '═' => (N, D, N, D),
        '║' => (D, N, D, N),
        '╔' => (N, D, D, N),
        '╗' => (N, N, D, D),
        '╚' => (D, D, N, N),
        '╝' => (D, N, N, D),
        '╠' => (D, D, D, N),
        '╣' => (D, N, D, D),
        '╦' => (N, D, D, D),
        '╩' => (D, D, N, D),
        '╬' => (D, D, D, D),
        '╒' => (N, D, L, N),
        '╓' => (N, L, D, N),
        '╕' => (N, N, L, D),
        '╖' => (N, N, D, L),
        '╘' => (L, D, N, N),
        '╙' => (D, L, N, N),
        '╛' => (L, N, N, D),
        '╜' => (D, N, N, L),
        '╞' => (L, D, L, N),
        '╟' => (D, L, D, N),
        '╡' => (L, N, L, D),
        '╢' => (D, N, D, L),
        '╤' => (N, D, L, D),
        '╥' => (N, L, D, L),
        '╧' => (L, D, N, D),
        '╨' => (D, L, N, L),
        '╪' => (L, D, L, D),
        '╫' => (D, L, D, L),
        _ => return None,
    };
    Some(Arms {
        up,
        right,
        down,
        left,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect as CellRect;
    use ratatui::style::Style;

    const FG: u32 = 0xFFFFFF;

    /// 8x16-pixel cells, so a VGA-sized grid.
    fn renderer(cols: u16, rows: u16) -> Renderer {
        let mut r = Renderer::new(None, 8, 16);
        r.grid = Grid {
            cols,
            rows,
            cell_w: 8,
            cell_h: 16,
            x0: 0,
            y0: 0,
        };
        r.width = u32::from(cols) * 8;
        r.height = u32::from(rows) * 16;
        r.pixels = vec![BLACK; (r.width * r.height) as usize];
        r
    }

    fn buffer(lines: &[&str]) -> Buffer {
        let w = lines.iter().map(|l| l.chars().count()).max().unwrap() as u16;
        let mut b = Buffer::empty(CellRect::new(0, 0, w, lines.len() as u16));
        let style = Style::new()
            .fg(Color::Rgb(255, 255, 255))
            .bg(Color::Rgb(0, 0, 0));
        for (y, l) in lines.iter().enumerate() {
            b.set_string(0, y as u16, l, style);
        }
        b
    }

    fn lit(r: &Renderer, x: u32, y: u32) -> bool {
        r.pixels[(y * r.width + x) as usize] == FG
    }

    #[test]
    fn grid_fits_common_displays() {
        // 16:9 at 1080p: exactly 80x25 of 24x43 cells.
        let g = Grid::fit(1920, 1080);
        assert_eq!((g.cols, g.rows, g.cell_w, g.cell_h), (80, 25, 24, 43));
        // A 16" MacBook Pro's native resolution.
        let g = Grid::fit(3456, 2234);
        assert_eq!((g.cols, g.rows), (80, 25));
        assert!(g.x0 * 2 + u32::from(g.cols) * g.cell_w <= 3456);
        // A 4:3 projector: cells stay a sane shape, and extra rows appear.
        let g = Grid::fit(1024, 768);
        assert!(g.cell_h <= g.cell_w * 9 / 4);
        assert!(g.cols >= 80 && g.rows >= 25);
        // An ultra-wide: extra columns, not stretched cells.
        let g = Grid::fit(5120, 1440);
        assert!(g.cols > 80 && g.rows == 25);
        // Degenerate sizes do not panic.
        Grid::fit(1, 1);
    }

    #[test]
    fn horizontal_lines_join_across_cells() {
        let mut r = renderer(3, 1);
        r.render(&buffer(&["───"]));
        let cy = 8;
        assert!((0..24).all(|x| lit(&r, x, cy)), "one unbroken line");
        assert!(!lit(&r, 4, 0) && !lit(&r, 4, 15), "and only a line");
    }

    #[test]
    fn corners_meet_their_neighbours() {
        let mut r = renderer(2, 2);
        r.render(&buffer(&["┌─", "│ "]));
        let (cx, cy) = (4, 8);
        // Across the top from the corner to the far edge, and down from the
        // corner to the bottom edge, without a gap at the joins.
        assert!((cx..16).all(|x| lit(&r, x, cy)));
        assert!((cy..32).all(|y| lit(&r, cx, y)));
        // Nothing sticks out above or left of the corner.
        assert!(!lit(&r, cx, 2) && !lit(&r, 0, cy));
    }

    #[test]
    fn double_corners_nest() {
        let mut r = renderer(2, 2);
        r.render(&buffer(&["╔═", "║ "]));
        // With 8px cells: line width 1, the pair sits at centre ± 1.
        let (cx, cy, g) = (4, 8, 1);
        // Outer lines: along the top and down the left, meeting at the corner.
        assert!((cx - g..16).all(|x| lit(&r, x, cy - g)));
        assert!((cy - g..32).all(|y| lit(&r, cx - g, y)));
        // Inner lines start at the inner corner, not before it.
        assert!((cx + g..16).all(|x| lit(&r, x, cy + g)));
        assert!(!lit(&r, cx - g + 1, cy + g), "gap between the pair");
        assert!(
            !lit(&r, cx - g - 1, cy + g),
            "inner line does not cross the outer"
        );
    }

    #[test]
    fn single_tees_onto_double_lines() {
        // ╟ joins a light line onto the nearer (right) line of a double pair.
        let mut r = renderer(2, 1);
        r.render(&buffer(&["╟─"]));
        let (cx, cy, g) = (4, 8, 1);
        assert!(
            (0..16).all(|y| lit(&r, cx - g, y) && lit(&r, cx + g, y)),
            "both verticals unbroken"
        );
        assert!((cx + g..16).all(|x| lit(&r, x, cy)));
        assert!(
            !lit(&r, cx, cy),
            "the light line stops at the nearer double line"
        );
    }

    #[test]
    fn half_blocks_tile_a_cell() {
        let mut r = renderer(1, 2);
        r.render(&buffer(&["▀", "▄"]));
        for y in 0..32 {
            let expected = !(8..24).contains(&y);
            assert_eq!(lit(&r, 3, y), expected, "row {y}");
        }
    }

    #[test]
    fn shades_have_the_right_density() {
        for (c, percent) in [("░", 25), ("▒", 50), ("▓", 75), ("█", 100)] {
            let mut r = renderer(4, 2);
            r.render(&buffer(&[&c.repeat(4), &c.repeat(4)]));
            let on = r.pixels.iter().filter(|p| **p == FG).count();
            assert_eq!(on * 100 / r.pixels.len(), percent, "{c}");
        }
    }

    #[test]
    fn only_changed_rows_are_redrawn() {
        let mut r = renderer(2, 2);
        r.render(&buffer(&["──", "  "]));
        // Scribble on row 0's pixels; an unchanged row 0 must not be redrawn.
        r.pixels[3] = 0x123456;
        r.render(&buffer(&["──", "█ "]));
        assert_eq!(r.pixels[3], 0x123456);
        assert!(lit(&r, 3, 20), "the changed row is drawn");
        // A resize forces everything to be redrawn.
        r.last = None;
        r.render(&buffer(&["──", "█ "]));
        assert_eq!(r.pixels[3], BLACK);
    }

    #[test]
    fn backgrounds_and_reverse_video() {
        let mut r = renderer(1, 1);
        let mut b = Buffer::empty(CellRect::new(0, 0, 1, 1));
        b[(0, 0)]
            .set_symbol(" ")
            .set_bg(Color::Rgb(0, 0, 0xAA))
            .set_fg(Color::Rgb(0xFF, 0xFF, 0x55));
        r.render(&b);
        assert!(r.pixels.iter().all(|p| *p == 0x0000AA));
        b[(0, 0)].modifier = Modifier::REVERSED;
        r.render(&b);
        assert!(r.pixels.iter().all(|p| *p == 0xFFFF55));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_font_draws_text() {
        let fonts = Fonts::load(None).expect("Menlo ships with macOS");
        let mut r = Renderer::new(Some(fonts), 1920, 1080);
        let mut b = Buffer::empty(CellRect::new(0, 0, r.grid().cols, r.grid().rows));
        b.set_string(0, 0, "A", Style::new().fg(Color::Rgb(255, 255, 255)));
        let pixels = r.render(&b).to_vec();
        let g = r.grid();
        let lit = (0..g.cell_h)
            .flat_map(|y| (0..g.cell_w).map(move |x| (g.x0 + x, g.y0 + y)))
            .filter(|(x, y)| pixels[(y * 1920 + x) as usize] != BLACK)
            .count();
        let area = (g.cell_w * g.cell_h) as usize;
        assert!(lit > area / 20 && lit < area / 2, "{lit} of {area}");
        // Symbols come from the fallback fonts if Menlo lacks them.
        let fonts = r.fonts.as_ref().unwrap();
        assert!(fonts.for_char('●', false).is_some());
        // Latin-1 characters map to their own glyphs: fontdue read Menlo's
        // legacy Mac Roman table and drew U+00B7 · as U+2211 ∑.
        assert_ne!(fonts.regular.glyph_id('·'), fonts.regular.glyph_id('∑'));
    }
}
