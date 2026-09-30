//! tui-slides: present a Markdown slide deck as a full-screen, Turbo
//! Vision-style terminal application.

mod app;
mod deck;
mod layout;
mod pdf;
mod raster;
mod theme;
mod transition;
mod ui;
mod window;

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use app::App;
use theme::Theme;
use transition::Transition;

const USAGE: &str = "\
usage: tui-slides [--zoom] DECK.md      present the deck in this terminal
       tui-slides --present [--windowed] [--zoom] [--font PATH] DECK.md
                                      present the deck in its own window, full
                                      screen on the display it opens on
       tui-slides --pdf OUT.pdf [--zoom] [--font PATH] DECK.md
                                      export every slide as a page of a PDF
       tui-slides --check DECK.md       report slides that will not fit at 80x25
       tui-slides --dump N|all [--color] [--zoom] DECK.md
                                      print slide N (1-based) as an 80x25 screen";

enum Mode {
    Terminal,
    Window {
        fullscreen: bool,
        font: Option<PathBuf>,
    },
    Pdf {
        out: PathBuf,
        font: Option<PathBuf>,
    },
    Check,
    Dump {
        which: Option<usize>,
        color: bool,
    },
}

struct Options {
    mode: Mode,
    path: PathBuf,
    zoomed: bool,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut mode = Mode::Terminal;
    let mut color = false;
    let mut zoomed = false;
    let mut windowed = false;
    let mut font = None;
    let mut path = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--check" => mode = Mode::Check,
            "--color" | "--colour" => color = true,
            "--zoom" => zoomed = true,
            "--present" => {
                mode = Mode::Window {
                    fullscreen: true,
                    font: None,
                }
            }
            "--windowed" => windowed = true,
            "--pdf" => {
                mode = Mode::Pdf {
                    out: PathBuf::from(it.next().ok_or("--pdf needs an output file")?),
                    font: None,
                }
            }
            "--font" => font = Some(PathBuf::from(it.next().ok_or("--font needs a path")?)),
            "--dump" => {
                let which = it.next().ok_or("--dump needs a slide number or `all`")?;
                let which = match which.as_str() {
                    "all" => None,
                    n => match n.parse::<usize>() {
                        Ok(n) if n >= 1 => Some(n - 1),
                        _ => return Err(format!("bad slide number {n:?}")),
                    },
                };
                mode = Mode::Dump {
                    which,
                    color: false,
                };
            }
            a if a.starts_with('-') => return Err(format!("unknown option {a}")),
            a => {
                if path.replace(PathBuf::from(a)).is_some() {
                    return Err("only one deck file, please".into());
                }
            }
        }
    }
    if let Mode::Dump { color: c, .. } = &mut mode {
        *c = color;
    } else if color {
        return Err("--color only applies to --dump".into());
    }
    match &mut mode {
        Mode::Window {
            fullscreen,
            font: f,
        } => {
            *fullscreen = !windowed;
            *f = font;
        }
        _ if windowed => return Err("--windowed only applies to --present".into()),
        Mode::Pdf { font: f, .. } => *f = font,
        _ if font.is_some() => return Err("--font only applies to --present and --pdf".into()),
        _ => {}
    }
    if zoomed && matches!(mode, Mode::Check) {
        return Err("--zoom does not apply to --check".into());
    }
    Ok(Options {
        mode,
        path: path.ok_or("no deck file given")?,
        zoomed,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Options { mode, path, zoomed } = match parse_args(&args) {
        Ok(x) => x,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("tui-slides: {e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let deck = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|src| deck::parse(&src).map_err(|e| e.to_string()))
    {
        Ok(deck) => deck,
        Err(e) => {
            eprintln!("tui-slides: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    if let Mode::Check = mode {
        return check(&deck, &path);
    }
    let mut app = App::new(deck, Some(path));
    app.zoomed = zoomed;
    let result = match mode {
        Mode::Check => unreachable!("handled above"),
        Mode::Dump { which, color } => dump(app, which, color),
        Mode::Terminal => present(app),
        Mode::Pdf { out, font } => raster::Fonts::load(font.as_deref())
            .map_err(io::Error::other)
            .and_then(|fonts| export_pdf(app, fonts, &out)),
        Mode::Window { fullscreen, font } => raster::Fonts::load(font.as_deref())
            .and_then(|fonts| window::present(app, fonts, fullscreen))
            .map_err(io::Error::other),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tui-slides: {e}");
            ExitCode::FAILURE
        }
    }
}

fn check(deck: &deck::Deck, path: &std::path::Path) -> ExitCode {
    let mut bad = 0;
    for (i, slide) in deck.slides.iter().enumerate() {
        for problem in layout::problems(slide) {
            println!(
                "{}:{}: slide {} ({}): {problem}",
                path.display(),
                slide.line,
                i + 1,
                slide.title
            );
            bad += 1;
        }
    }
    if bad == 0 {
        println!(
            "{}: {} slides, all fit at 80x25",
            path.display(),
            deck.slides.len()
        );
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn present(mut app: App) -> io::Result<()> {
    let theme = Theme::detect();
    let mut terminal = ratatui::try_init()?;
    let result = (|| -> io::Result<()> {
        let mut transition: Option<Transition> = None;
        while !app.quit {
            let now = Instant::now();
            let frame = terminal.draw(|frame| {
                ui::draw(frame.buffer_mut(), &app, &theme);
                if let Some(t) = &transition {
                    let region = ui::desktop(frame.area(), app.zoomed);
                    t.apply(frame.buffer_mut(), region, now);
                }
            })?;
            // The frame now on screen, which the next transition starts from.
            let shown = frame.buffer.clone();
            if transition.as_ref().is_some_and(|t| t.finished(now)) {
                transition = None;
            }

            // Animate at about 60 fps during a transition; otherwise wake
            // twice a second so the elapsed-time clock ticks.
            let wait = if transition.is_some() { 16 } else { 500 };
            if event::poll(Duration::from_millis(wait))? {
                if let Event::Key(key) = event::read()? {
                    let before = app.current;
                    app.handle_key(key);
                    // A move during a transition starts the next one from
                    // whatever is on screen, so rapid paging stays smooth.
                    if let Some((kind, dir)) = app.transition_from(before) {
                        transition = Some(Transition::new(kind, dir, shown, Instant::now()));
                    }
                }
            }
        }
        Ok(())
    })();
    ratatui::try_restore()?;
    result
}

/// Pixel size of an exported page: 80x25 cells of 48x86 pixels, the
/// VGA 9:16 cell shape at a resolution that stays sharp when projected or
/// printed.
const PDF_WIDTH: u32 = 3840;
const PDF_HEIGHT: u32 = 2150;

/// Renders every slide the way `--present` draws it and writes them to `out`
/// as the pages of one PDF.
fn export_pdf(mut app: App, fonts: raster::Fonts, out: &std::path::Path) -> io::Result<()> {
    app.native = true;
    app.show_clock = false;
    let mut renderer = raster::Renderer::new(Some(fonts), PDF_WIDTH, PDF_HEIGHT);
    let grid = renderer.grid();
    let theme = Theme::new(true);
    let mut pdf = pdf::Pdf::new(&app.deck.title);
    let n = app.deck.slides.len();
    for i in 0..n {
        app.go(i);
        let mut buf = Buffer::empty(Rect::new(0, 0, grid.cols, grid.rows));
        ui::draw(&mut buf, &app, &theme);
        pdf.add_page(PDF_WIDTH, PDF_HEIGHT, renderer.render(&buf));
    }
    std::fs::write(out, pdf.finish())
        .map_err(|e| io::Error::other(format!("{}: {e}", out.display())))?;
    eprintln!("tui-slides: wrote {n} pages to {}", out.display());
    Ok(())
}

fn dump(mut app: App, which: Option<usize>, color: bool) -> io::Result<()> {
    let theme = if color {
        Theme::detect()
    } else {
        Theme::new(false)
    };
    let n = app.deck.slides.len();
    let indices: Vec<usize> = match which {
        Some(i) if i < n => vec![i],
        Some(i) => {
            return Err(io::Error::other(format!(
                "slide {} does not exist; the deck has {n}",
                i + 1
            )))
        }
        None => (0..n).collect(),
    };
    let mut out = io::stdout().lock();
    for (k, i) in indices.into_iter().enumerate() {
        if k > 0 {
            writeln!(out)?;
        }
        app.go(i);
        let mut buf = Buffer::empty(Rect::new(0, 0, ui::SCREEN_WIDTH, ui::SCREEN_HEIGHT));
        ui::draw(&mut buf, &app, &theme);
        write_buffer(&mut out, &buf, color)?;
    }
    Ok(())
}

/// Prints a buffer as text, optionally with ANSI colour escapes.
fn write_buffer(out: &mut impl Write, buf: &Buffer, color: bool) -> io::Result<()> {
    for y in 0..buf.area.height {
        let mut x = 0;
        while x < buf.area.width {
            let cell = &buf[(x, y)];
            if color {
                write!(out, "\x1b[0;{};{}", sgr(cell.fg, false), sgr(cell.bg, true))?;
                if cell.modifier.contains(Modifier::BOLD) {
                    write!(out, ";1")?;
                }
                write!(out, "m")?;
            }
            write!(out, "{}", cell.symbol())?;
            // A wide character covers the cells after it.
            x += u16::try_from(cell.symbol().width().max(1)).unwrap_or(1);
        }
        if color {
            write!(out, "\x1b[0m")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn sgr(c: Color, background: bool) -> String {
    let base = if background { 40 } else { 30 };
    let bright = if background { 100 } else { 90 };
    match c {
        Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", base + 8),
        Color::Black => format!("{base}"),
        Color::Red => format!("{}", base + 1),
        Color::Green => format!("{}", base + 2),
        Color::Yellow => format!("{}", base + 3),
        Color::Blue => format!("{}", base + 4),
        Color::Magenta => format!("{}", base + 5),
        Color::Cyan => format!("{}", base + 6),
        Color::Gray => format!("{}", base + 7),
        Color::DarkGray => format!("{bright}"),
        Color::LightRed => format!("{}", bright + 1),
        Color::LightGreen => format!("{}", bright + 2),
        Color::LightYellow => format!("{}", bright + 3),
        Color::LightBlue => format!("{}", bright + 4),
        Color::LightMagenta => format!("{}", bright + 5),
        Color::LightCyan => format!("{}", bright + 6),
        Color::White => format!("{}", bright + 7),
        Color::Indexed(i) => format!("{};5;{i}", base + 8),
        Color::Reset => format!("{}", base + 9),
    }
}
