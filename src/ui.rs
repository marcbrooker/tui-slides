//! Drawing: the desktop, menu and status bars, slide windows and dialogs,
//! all in the style of Borland's Turbo Vision.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, Overlay};
use crate::deck::Slide;
use crate::layout::{self, text_width, Layout, MAX_CONTENT_HEIGHT};
use crate::theme::{Color16, Theme, WindowStyle};

/// The screen the slides are designed for. Larger terminals get a larger
/// desktop, but the windows stay this scale.
pub const SCREEN_WIDTH: u16 = 80;
pub const SCREEN_HEIGHT: u16 = 25;

/// Columns of padding between a slide window's frame and its content.
const PAD_X: u16 = 4;
/// Rows of padding above and below the content: the most a slide gets when
/// there is room, and the least it is squeezed to when there is not.
const PAD_Y_ROOMY: u16 = 2;
const PAD_Y_TIGHT: u16 = 1;
/// Smallest content box a slide window will shrink to.
const MIN_CONTENT_WIDTH: u16 = 30;
const MIN_CONTENT_HEIGHT: u16 = 3;

const DESKTOP_CHAR: &str = "░";
const GOTO_HINT: &str = "↑↓ select   Enter go   Esc cancel";
const OK_LABEL: &str = "   OK   ";

pub fn draw(buf: &mut Buffer, app: &App, theme: &Theme) {
    let area = buf.area;
    if area.width < SCREEN_WIDTH || area.height < SCREEN_HEIGHT {
        too_small(buf, theme);
        return;
    }
    let desktop = desktop(area, app.zoomed);
    if !app.zoomed {
        fill(
            buf,
            desktop,
            DESKTOP_CHAR,
            theme.style(Color16::Blue, Color16::LightGray),
        );
        menu_bar(buf, area, app, theme);
        status_bar(buf, area, app, theme);
    }

    let slide = &app.deck.slides[app.current];
    slide_window(
        buf,
        desktop,
        slide,
        app.current,
        app.deck.slides.len(),
        app.zoomed,
        theme,
    );

    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => help_dialog(buf, desktop, app.native, theme),
        Overlay::Goto { selected } => goto_dialog(buf, desktop, app, *selected, theme),
        Overlay::Message { title, text } => message_dialog(buf, desktop, title, text, theme),
    }
}

/// Where slides and dialogs go: between the menu and status bars normally,
/// the whole screen when the slide window is zoomed.
pub fn desktop(area: Rect, zoomed: bool) -> Rect {
    if zoomed {
        area
    } else {
        Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(2),
        )
    }
}

fn too_small(buf: &mut Buffer, theme: &Theme) {
    let area = buf.area;
    let style = theme.style(Color16::LightGray, Color16::Black);
    fill(buf, area, " ", style);
    let lines = [
        format!("tvslides needs a {SCREEN_WIDTH}x{SCREEN_HEIGHT} terminal"),
        format!("this one is {}x{}", area.width, area.height),
    ];
    let top = area.y + area.height.saturating_sub(2) / 2;
    for (i, line) in lines.iter().enumerate() {
        let x = area.x + area.width.saturating_sub(text_width(line)) / 2;
        buf.set_stringn(x, top + i as u16, line, area.width.into(), style);
    }
}

// ─── primitives ─────────────────────────────────────────────────────────────

fn fill(buf: &mut Buffer, rect: Rect, symbol: &str, style: Style) {
    let rect = rect.intersection(buf.area);
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
                cell.set_symbol(symbol).set_style(style);
            }
        }
    }
}

/// Writes text, clipped to the column `right` (exclusive). Returns the next x.
fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style, right: u16) -> u16 {
    if x >= right || !buf.area.contains((x, y).into()) {
        return x;
    }
    buf.set_stringn(x, y, text, usize::from(right - x), style).0
}

/// Writes text where `~x~` marks a hot key drawn in `hot` rather than `normal`.
fn put_hot(
    buf: &mut Buffer,
    mut x: u16,
    y: u16,
    text: &str,
    normal: Style,
    hot: Style,
    right: u16,
) -> u16 {
    for (i, part) in text.split('~').enumerate() {
        x = put(
            buf,
            x,
            y,
            part,
            if i % 2 == 1 { hot } else { normal },
            right,
        );
    }
    x
}

fn hot_width(text: &str) -> u16 {
    text_width(&text.replace('~', ""))
}

/// Draws the Turbo Vision drop shadow: the two columns to the right of and
/// the row below `rect` go dark, keeping whatever character was there.
fn shadow(buf: &mut Buffer, rect: Rect, theme: &Theme) {
    let style = theme.style(Color16::DarkGray, Color16::Black);
    let right = Rect::new(rect.right(), rect.y + 1, 2, rect.height);
    let below = Rect::new(rect.x + 2, rect.bottom(), rect.width, 1);
    for r in [right, below] {
        let r = r.intersection(buf.area);
        for y in r.top()..r.bottom() {
            for x in r.left()..r.right() {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(style);
                }
            }
        }
    }
}

/// A double-line frame with a centred title and a close box, as on the
/// active window. Returns the interior.
fn frame(buf: &mut Buffer, rect: Rect, title: &str, bg: Color16, theme: &Theme) -> Rect {
    let frame_style = theme.style(Color16::White, bg);
    let title_style = match bg {
        Color16::Blue => theme.style(Color16::White, bg),
        _ => theme.style(Color16::Black, bg),
    }
    .add_modifier(Modifier::BOLD);
    fill(buf, rect, " ", frame_style);
    let (l, r, t, b) = (rect.left(), rect.right() - 1, rect.top(), rect.bottom() - 1);
    for x in l + 1..r {
        put(buf, x, t, "═", frame_style, r);
        put(buf, x, b, "═", frame_style, r);
    }
    for y in t + 1..b {
        put(buf, l, y, "║", frame_style, l + 1);
        put(buf, r, y, "║", frame_style, r + 1);
    }
    put(buf, l, t, "╔", frame_style, l + 1);
    put(buf, r, t, "╗", frame_style, r + 1);
    put(buf, l, b, "╚", frame_style, l + 1);
    put(buf, r, b, "╝", frame_style, r + 1);

    // Close box: [■]
    let x = put(buf, l + 2, t, "[", frame_style, r);
    let x = put(buf, x, t, "■", theme.style(Color16::LightGreen, bg), r);
    put(buf, x, t, "]", frame_style, r);

    if !title.is_empty() {
        let label = format!(" {title} ");
        let room = rect.width.saturating_sub(12);
        let w = text_width(&label).min(room);
        let x = l + (rect.width - w) / 2;
        put(buf, x, t, &label, title_style, x + w);
    }
    Rect::new(l + 1, t + 1, rect.width - 2, rect.height - 2)
}

/// Centres a box of the given size (plus its shadow) on the desktop.
fn centred(desktop: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(desktop.width.saturating_sub(2));
    let height = height.min(desktop.height.saturating_sub(1));
    Rect::new(
        desktop.x + (desktop.width - width - 2) / 2,
        desktop.y + (desktop.height - height - 1) / 2,
        width,
        height,
    )
}

// ─── bars ───────────────────────────────────────────────────────────────────

fn menu_bar(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let normal = theme.style(Color16::Black, Color16::LightGray);
    let hot = theme.style(Color16::Red, Color16::LightGray);
    let bar = Rect::new(area.x, area.y, area.width, 1);
    fill(buf, bar, " ", normal);
    let right = bar.right();
    let x = put(buf, bar.x + 1, bar.y, " ≡ ", normal, right);
    let mut x = x + 1;
    for item in ["~F~ile", "~E~dit", "~V~iew", "~S~lide", "~H~elp"] {
        x = put_hot(buf, x, bar.y, &format!(" {item} "), normal, hot, right) + 1;
    }
    let title = &app.deck.title;
    if !title.is_empty() {
        let w = text_width(title);
        let tx = right.saturating_sub(w + 2).max(x + 1);
        put(buf, tx, bar.y, title, normal, right - 1);
    }
}

fn status_bar(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let normal = theme.style(Color16::Black, Color16::LightGray);
    let hot = theme.style(Color16::Red, Color16::LightGray);
    let y = area.bottom() - 1;
    let bar = Rect::new(area.x, y, area.width, 1);
    fill(buf, bar, " ", normal);

    let mut position = format!("{}/{}", app.current + 1, app.deck.slides.len());
    if app.show_clock {
        position = format!("{position}  {}", clock(app.elapsed()));
    }
    let right = bar.right() - text_width(&position) - 1;
    put(buf, right, y, &position, normal, bar.right());

    let mut x = bar.x + 1;
    for hint in ["~F1~ Help", "~F2~ Slides", "~←→~ Page", "~Alt-X~ Exit"] {
        if x + hot_width(hint) + 2 > right {
            break;
        }
        x = put_hot(buf, x, y, hint, normal, hot, right) + 2;
    }
    let footer = &app.deck.footer;
    if !footer.is_empty() && x + 2 + text_width(footer) < right {
        let x = put(buf, x, y, "│ ", normal, right);
        put(buf, x, y, footer, normal, right);
    }
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

// ─── the slide ──────────────────────────────────────────────────────────────

/// Window geometry for a laid-out slide: (window width, height, top padding).
pub fn window_size(slide: &Slide, layout: &Layout) -> (u16, u16, u16) {
    let title = text_width(&slide.title) + 8;
    let content_w = layout.width.max(title).max(MIN_CONTENT_WIDTH);
    let content_h = layout.height().max(MIN_CONTENT_HEIGHT);
    let pad_y = if content_h + 2 * (PAD_Y_ROOMY - PAD_Y_TIGHT) <= MAX_CONTENT_HEIGHT {
        PAD_Y_ROOMY
    } else {
        PAD_Y_TIGHT
    };
    (content_w + 2 * PAD_X + 2, content_h + 2 * pad_y + 2, pad_y)
}

fn slide_window(
    buf: &mut Buffer,
    desktop: Rect,
    slide: &Slide,
    index: usize,
    count: usize,
    zoomed: bool,
    theme: &Theme,
) {
    let laid = layout::layout(slide);
    let (w, h, pad_y) = window_size(slide, &laid);
    // Zoomed, the window fills the desktop, as Turbo Vision's zoom box did;
    // the content stays centred inside it at the same scale.
    let rect = if zoomed {
        desktop
    } else {
        centred(desktop, w, h)
    };
    let bg = theme.window_bg(slide.style);

    if !zoomed {
        shadow(buf, rect, theme);
    }
    let inner = frame(buf, rect, &slide.title, bg, theme);

    // Window number, as Turbo Vision shows next to the zoom box.
    let frame_style = theme.style(Color16::White, bg);
    let number = format!("{}", index + 1);
    let zoom_x = rect.right() - 5;
    let nx = zoom_x.saturating_sub(text_width(&number) + 1);
    put(buf, nx, rect.y, &number, frame_style, zoom_x);
    let x = put(buf, zoom_x, rect.y, "[", frame_style, rect.right() - 1);
    let x = put(
        buf,
        x,
        rect.y,
        "↕",
        theme.style(Color16::LightGreen, bg),
        rect.right() - 1,
    );
    put(buf, x, rect.y, "]", frame_style, rect.right() - 1);

    if slide.style == WindowStyle::Window {
        progress_bar(buf, rect, index, count, theme);
    }

    // Content: centred within the window's content box, clipped to it.
    let content = Rect::new(
        inner.x + PAD_X,
        inner.y + pad_y,
        inner.width.saturating_sub(2 * PAD_X),
        inner.height.saturating_sub(2 * pad_y),
    );
    let x0 = content.x + content.width.saturating_sub(laid.width) / 2;
    let y0 = content.y + content.height.saturating_sub(laid.height()) / 2;
    for (i, line) in laid.lines.iter().enumerate() {
        let y = y0 + i as u16;
        if y >= content.bottom() {
            break;
        }
        let mut x = x0 + line.x;
        for run in &line.runs {
            x = put(
                buf,
                x,
                y,
                &run.text,
                theme.content(slide.style, run.role),
                content.right(),
            );
        }
    }
}

/// A vertical scroll bar on the right frame whose thumb shows progress
/// through the deck.
fn progress_bar(buf: &mut Buffer, rect: Rect, index: usize, count: usize, theme: &Theme) {
    if rect.height < 6 {
        return;
    }
    let style = theme.style(Color16::Cyan, Color16::Blue);
    let x = rect.right() - 1;
    let top = rect.y + 1;
    let bottom = rect.bottom() - 2;
    put(buf, x, top, "▲", style, x + 1);
    put(buf, x, bottom, "▼", style, x + 1);
    let track = bottom - top - 1;
    for y in top + 1..bottom {
        put(buf, x, y, "▒", style, x + 1);
    }
    let offset = if count > 1 {
        (index * usize::from(track - 1) / (count - 1)) as u16
    } else {
        0
    };
    put(buf, x, top + 1 + offset, "■", style, x + 1);
}

// ─── dialogs ────────────────────────────────────────────────────────────────

/// A grey dialog box with room for `inner_w` x `inner_h` of content. Returns
/// the content rectangle (inside a one-column margin).
fn dialog(
    buf: &mut Buffer,
    desktop: Rect,
    title: &str,
    inner_w: u16,
    inner_h: u16,
    theme: &Theme,
) -> Rect {
    let rect = centred(desktop, inner_w + 4, inner_h + 2);
    shadow(buf, rect, theme);
    let inner = frame(buf, rect, title, Color16::LightGray, theme);
    Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    )
}

/// A green Turbo Vision button with its half-block shadow, centred on row `y`.
fn button(buf: &mut Buffer, area: Rect, y: u16, label: &str, theme: &Theme) {
    let w = text_width(label);
    let x = area.x + area.width.saturating_sub(w + 1) / 2;
    put(
        buf,
        x,
        y,
        label,
        theme
            .style(Color16::White, Color16::Green)
            .add_modifier(Modifier::BOLD),
        x + w,
    );
    let shade = theme.style(Color16::Black, Color16::LightGray);
    put(buf, x + w, y, "▄", shade, x + w + 1);
    put(buf, x + 1, y + 1, &"▀".repeat(w.into()), shade, x + w + 1);
}

fn help_dialog(buf: &mut Buffer, desktop: Rect, native: bool, theme: &Theme) {
    let all = [
        ("→ Space PgDn", "Next slide"),
        ("← Bksp  PgUp", "Previous slide"),
        ("Home  End", "First / last slide"),
        ("F2  g", "Go to slide..."),
        ("F5  r", "Reload the deck file"),
        ("z", "Zoom the slide window"),
        ("F11  f", "Full screen on / off"),
        ("F1  ?", "This help"),
        ("Esc  q  Alt-X", "Exit"),
    ];
    // Full screen is a property of the native window; a terminal program
    // cannot resize the terminal it runs in.
    let rows: Vec<_> = all
        .into_iter()
        .filter(|(k, _)| native || !k.starts_with("F11"))
        .collect();
    let key_w = rows.iter().map(|(k, _)| text_width(k)).max().unwrap_or(0);
    let text_w = rows.iter().map(|(_, t)| text_width(t)).max().unwrap_or(0);
    let inner = dialog(
        buf,
        desktop,
        "Help",
        key_w + 3 + text_w,
        rows.len() as u16 + 4,
        theme,
    );
    let key_style = theme.style(Color16::Red, Color16::LightGray);
    let text_style = theme.style(Color16::Black, Color16::LightGray);
    for (i, (k, t)) in rows.iter().enumerate() {
        let y = inner.y + 1 + i as u16;
        put(buf, inner.x, y, k, key_style, inner.right());
        put(buf, inner.x + key_w + 3, y, t, text_style, inner.right());
    }
    button(buf, inner, inner.bottom() - 2, OK_LABEL, theme);
}

fn goto_dialog(buf: &mut Buffer, desktop: Rect, app: &App, selected: usize, theme: &Theme) {
    let slides = &app.deck.slides;
    let visible = slides.len().min(12);
    let entries: Vec<String> = slides
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let title = if s.title.is_empty() {
                "(untitled)"
            } else {
                &s.title
            };
            format!(" {:>2}  {title} ", i + 1)
        })
        .collect();
    let list_w = entries
        .iter()
        .map(|e| text_width(e))
        .max()
        .unwrap_or(0)
        .clamp(30, 60);
    let inner = dialog(
        buf,
        desktop,
        "Go to slide",
        list_w.max(text_width(GOTO_HINT)),
        visible as u16 + 2,
        theme,
    );

    let list = Rect::new(inner.x, inner.y + 1, inner.width, visible as u16);
    let normal = theme.style(Color16::Black, Color16::Cyan);
    let chosen = theme
        .style(Color16::White, Color16::Green)
        .add_modifier(Modifier::BOLD);
    let current = theme.style(Color16::Yellow, Color16::Cyan);
    fill(buf, list, " ", normal);
    let first = (selected + 1).saturating_sub(visible);
    for (row, i) in (first..first + visible).enumerate() {
        let y = list.y + row as u16;
        let style = if i == selected {
            chosen
        } else if i == app.current {
            current
        } else {
            normal
        };
        fill(buf, Rect::new(list.x, y, list.width, 1), " ", style);
        put(buf, list.x, y, &entries[i], style, list.right());
    }
    let hint = theme.style(Color16::Black, Color16::LightGray);
    put(
        buf,
        inner.x,
        inner.bottom() - 1,
        GOTO_HINT,
        hint,
        inner.right(),
    );
}

fn message_dialog(buf: &mut Buffer, desktop: Rect, title: &str, text: &str, theme: &Theme) {
    let width = 56;
    let lines: Vec<String> = wrap_plain(text, width);
    let inner = dialog(buf, desktop, title, width, lines.len() as u16 + 4, theme);
    let style = theme.style(Color16::Black, Color16::LightGray);
    for (i, line) in lines.iter().enumerate() {
        put(
            buf,
            inner.x,
            inner.y + 1 + i as u16,
            line,
            style,
            inner.right(),
        );
    }
    button(buf, inner, inner.bottom() - 2, OK_LABEL, theme);
}

fn wrap_plain(text: &str, width: u16) -> Vec<String> {
    let slide = Slide {
        blocks: vec![crate::deck::Block::Paragraph(crate::deck::parse_inline(
            text,
        ))],
        width: Some(width),
        ..Slide::default()
    };
    layout::layout(&slide)
        .lines
        .iter()
        .map(|l| l.runs.iter().map(|r| r.text.as_str()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::parse;
    use crate::layout::MAX_CONTENT_WIDTH;

    fn render(app: &App) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT));
        draw(&mut buf, app, &Theme::new(true));
        buf
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    fn app(src: &str) -> App {
        App::new(parse(src).unwrap(), None)
    }

    #[test]
    fn draws_chrome_and_title() {
        let a = app("+++\ntitle: My Talk\n+++\n# Hello\n\nWorld\n---\n# Two\n");
        let buf = render(&a);
        assert!(row(&buf, 0).contains("File"));
        assert!(row(&buf, 0).trim_end().ends_with("My Talk"));
        assert!(row(&buf, 24).contains("1/2"));
        assert!(row(&buf, 24).contains("F1 Help"));
        let screen: Vec<String> = (0..25).map(|y| row(&buf, y)).collect();
        let title_row = screen.iter().position(|r| r.contains(" Hello ")).unwrap();
        assert!(screen[title_row].contains("╔═[■]"));
        assert!(screen.iter().any(|r| r.contains("World")));
        assert!(screen[1].starts_with("░░░"));
    }

    #[test]
    fn the_widest_and_tallest_content_fits_on_screen() {
        let row_text = "x".repeat(MAX_CONTENT_WIDTH.into());
        let art = format!("{row_text}\n").repeat(MAX_CONTENT_HEIGHT.into());
        let a = app(&format!("# T\n```art\n{art}```\n"));
        let slide = &a.deck.slides[0];
        let (w, h, pad_y) = window_size(slide, &layout::layout(slide));
        // Window plus shadow exactly fills the desktop between the bars.
        assert_eq!((w + 2, h + 1), (SCREEN_WIDTH, SCREEN_HEIGHT - 2));
        assert_eq!(pad_y, PAD_Y_TIGHT);
        let buf = render(&a);
        let full = (0..25)
            .filter(|y| row(&buf, *y).contains(&row_text))
            .count();
        assert_eq!(full, usize::from(MAX_CONTENT_HEIGHT));
    }

    #[test]
    fn content_is_padded_from_the_frame() {
        let buf = render(&app("# T\n\nWorld\n"));
        let rows: Vec<String> = (0..25).map(|y| row(&buf, y)).collect();
        let y = rows.iter().position(|r| r.contains("World")).unwrap();
        let chars: Vec<char> = rows[y].chars().collect();
        let frame = chars.iter().position(|c| *c == '║').unwrap();
        let text = chars.iter().position(|c| *c == 'W').unwrap();
        assert!(text - frame > usize::from(PAD_X), "at least PAD_X columns");
        // At least PAD_Y_ROOMY blank rows between the top frame and the text.
        let top = rows.iter().position(|r| r.contains('╔')).unwrap();
        assert!(y - top > usize::from(PAD_Y_ROOMY));
    }

    #[test]
    fn zoom_fills_the_screen_with_the_window() {
        let mut a = app("+++\ntitle: My Talk\n+++\n# Hello\n\nWorld\n");
        a.zoomed = true;
        let buf = render(&a);
        let top = row(&buf, 0);
        assert!(top.starts_with("╔═[■]") && top.ends_with('╗'), "{top}");
        assert!(top.contains(" Hello "));
        assert!(!top.contains("File"), "no menu bar");
        assert!(row(&buf, 24).starts_with('╚'), "no status bar");
        let screen: Vec<String> = (0..25).map(|y| row(&buf, y)).collect();
        assert!(!screen.iter().any(|r| r.contains(DESKTOP_CHAR)));
        // Content is still centred.
        let y = screen.iter().position(|r| r.contains("World")).unwrap();
        assert!((11..=13).contains(&y), "{y}");
    }

    #[test]
    fn small_terminals_get_a_message() {
        let a = app("# T\n");
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 20));
        draw(&mut buf, &a, &Theme::new(false));
        let screen: String = (0..20).map(|y| row(&buf, y)).collect();
        assert!(screen.contains("needs a 80x25 terminal"));
        assert!(screen.contains("this one is 60x20"));
    }

    #[test]
    fn overlays_render() {
        let mut a = app("# Alpha\n---\n# Beta\n");
        a.overlay = Overlay::Goto { selected: 1 };
        let screen: String = (0..25).map(|y| row(&render(&a), y)).collect();
        assert!(screen.contains("Go to slide"));
        assert!(screen.contains(" 2  Beta"));
        assert!(screen.contains("Esc cancel"), "the hint is not truncated");

        a.overlay = Overlay::Help;
        let screen: String = (0..25).map(|y| row(&render(&a), y)).collect();
        assert!(screen.contains("Reload the deck file"));
        assert!(screen.contains("OK"));
    }

    #[test]
    fn larger_terminals_keep_the_window_scale() {
        let a = app("# Hello\n\nWorld\n");
        let mut buf = Buffer::empty(Rect::new(0, 0, 160, 50));
        draw(&mut buf, &a, &Theme::new(true));
        let title_row = (0..50).find(|y| row(&buf, *y).contains(" Hello ")).unwrap();
        let chars: Vec<char> = row(&buf, title_row).chars().collect();
        let left = chars.iter().position(|c| *c == '╔').unwrap();
        let right = chars.iter().position(|c| *c == '╗').unwrap();
        assert_eq!(
            right - left + 1,
            usize::from(MIN_CONTENT_WIDTH + 2 * PAD_X + 2)
        );
        // ...and the window sits in the middle of the big desktop.
        assert!(left > 40 && title_row > 15);
    }
}
