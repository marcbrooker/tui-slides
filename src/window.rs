//! Presentation mode: the deck in its own native window, full screen on
//! whichever display it is on.
//!
//! The window runs exactly the same app and drawing code as the terminal
//! presenter. The only difference is where the cells go: into a buffer that
//! `raster` turns into pixels, instead of out to a terminal. So everything
//! the terminal version does, this does, and `--dump` shows what both draw.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Fullscreen, Window, WindowId};

use crate::app::App;
use crate::raster::{Fonts, Renderer};
use crate::theme::Theme;
use crate::transition::Transition;
use crate::ui;

/// How often to redraw while a transition runs, and while idle (so the
/// status bar clock ticks).
const ANIMATION_FRAME: Duration = Duration::from_millis(16);
const IDLE_TICK: Duration = Duration::from_millis(500);

/// Opens the presentation window and runs until the presenter quits.
pub fn present(mut app: App, fonts: Fonts, fullscreen: bool) -> Result<(), String> {
    app.native = true;
    app.fullscreen = fullscreen;
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let mut presenter = Presenter {
        app,
        theme: Theme::new(true),
        fonts: Some(fonts),
        view: None,
        transition: None,
        modifiers: ModifiersState::empty(),
        error: None,
    };
    event_loop
        .run_app(&mut presenter)
        .map_err(|e| e.to_string())?;
    match presenter.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// The window and what draws into it; created once the event loop is running.
struct View {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    renderer: Renderer,
    /// The last frame drawn, which a transition starts from.
    shown: Option<Buffer>,
}

struct Presenter {
    app: App,
    theme: Theme,
    /// Moved into the renderer when the window is created.
    fonts: Option<Fonts>,
    view: Option<View>,
    transition: Option<Transition>,
    modifiers: ModifiersState,
    /// Set if something went wrong inside the event loop, which cannot
    /// return errors itself.
    error: Option<String>,
}

impl Presenter {
    fn fail(&mut self, event_loop: &ActiveEventLoop, e: impl ToString) {
        self.error = Some(e.to_string());
        event_loop.exit();
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<View, String> {
        let title = if self.app.deck.title.is_empty() {
            "tui-slides".to_string()
        } else {
            format!("{} - tui-slides", self.app.deck.title)
        };
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_fullscreen(self.app.fullscreen.then_some(Fullscreen::Borderless(None)));
        let window = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );
        // Nobody wants a mouse pointer in the middle of a slide.
        window.set_cursor_visible(false);
        let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
        let surface =
            softbuffer::Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let renderer = Renderer::new(self.fonts.take(), size.width, size.height);
        Ok(View {
            window,
            surface,
            renderer,
            shown: None,
        })
    }

    fn redraw(&mut self) -> Result<(), String> {
        let Some(view) = &mut self.view else {
            return Ok(());
        };
        let size = view.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(()); // minimised
        };
        view.renderer.resize(size.width, size.height);
        let grid = view.renderer.grid();

        let now = Instant::now();
        let mut buf = Buffer::empty(Rect::new(0, 0, grid.cols, grid.rows));
        ui::draw(&mut buf, &self.app, &self.theme);
        if let Some(t) = &self.transition {
            let region = ui::desktop(buf.area, self.app.zoomed);
            t.apply(&mut buf, region, now);
            if t.finished(now) {
                self.transition = None;
            }
        }

        view.surface.resize(w, h).map_err(|e| e.to_string())?;
        let mut pixels = view.surface.buffer_mut().map_err(|e| e.to_string())?;
        pixels.copy_from_slice(view.renderer.render(&buf));
        pixels.present().map_err(|e| e.to_string())?;
        view.shown = Some(buf);
        Ok(())
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let m = self.modifiers;
        // Cmd-Q and Cmd-W are how every Mac app quits; Ctrl-Cmd-F is how
        // every Mac app goes full screen.
        if m.super_key() {
            match event.key_without_modifiers() {
                Key::Character(c) if c == "q" || c == "w" => self.app.quit = true,
                Key::Character(c) if c == "f" && m.control_key() => {
                    self.app.fullscreen = !self.app.fullscreen
                }
                _ => {}
            }
        } else if let Some(key) = to_crossterm(event, m) {
            let before = self.app.current;
            self.app.handle_key(key);
            if let (Some((kind, dir)), Some(view)) =
                (self.app.transition_from(before), &mut self.view)
            {
                if let Some(from) = view.shown.clone() {
                    self.transition = Some(Transition::new(kind, dir, from, Instant::now()));
                }
            }
        }

        if self.app.quit {
            event_loop.exit();
            return;
        }
        if let Some(view) = &self.view {
            let want = self.app.fullscreen.then_some(Fullscreen::Borderless(None));
            if view.window.fullscreen().is_some() != want.is_some() {
                view.window.set_fullscreen(want);
            }
            view.window.request_redraw();
        }
    }
}

impl ApplicationHandler for Presenter {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        match self.open(event_loop) {
            Ok(view) => {
                view.window.request_redraw();
                self.view = Some(view);
            }
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn new_events(&mut self, _: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            if let Some(view) = &self.view {
                view.window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => self.key(event_loop, &event),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(view) = &self.view {
                    // The green button and the Window menu change full screen
                    // behind the app's back; follow them so `f` stays in step.
                    self.app.fullscreen = view.window.fullscreen().is_some();
                    view.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, e);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let wait = if self.transition.is_some() {
            ANIMATION_FRAME
        } else {
            IDLE_TICK
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
    }
}

/// Translates a winit key press into the crossterm event the app handles,
/// so both front ends share one set of key bindings.
fn to_crossterm(event: &winit::event::KeyEvent, m: ModifiersState) -> Option<KeyEvent> {
    let mut mods = KeyModifiers::NONE;
    if m.control_key() {
        mods |= KeyModifiers::CONTROL;
    }
    if m.alt_key() {
        mods |= KeyModifiers::ALT;
    }
    if m.shift_key() {
        mods |= KeyModifiers::SHIFT;
    }
    // With Option held, macOS turns x into ≈; bindings want the plain key.
    let key = if m.alt_key() || m.control_key() {
        event.key_without_modifiers()
    } else {
        event.logical_key.clone()
    };
    let code = match key {
        Key::Named(named) => match named {
            NamedKey::ArrowRight => KeyCode::Right,
            NamedKey::ArrowLeft => KeyCode::Left,
            NamedKey::ArrowUp => KeyCode::Up,
            NamedKey::ArrowDown => KeyCode::Down,
            NamedKey::PageUp => KeyCode::PageUp,
            NamedKey::PageDown => KeyCode::PageDown,
            NamedKey::Home => KeyCode::Home,
            NamedKey::End => KeyCode::End,
            NamedKey::Enter => KeyCode::Enter,
            NamedKey::Backspace => KeyCode::Backspace,
            NamedKey::Escape => KeyCode::Esc,
            NamedKey::Space => KeyCode::Char(' '),
            NamedKey::F1 => KeyCode::F(1),
            NamedKey::F2 => KeyCode::F(2),
            NamedKey::F5 => KeyCode::F(5),
            NamedKey::F11 => KeyCode::F(11),
            _ => return None,
        },
        Key::Character(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(KeyEvent::new_with_kind(code, mods, KeyEventKind::Press))
}
