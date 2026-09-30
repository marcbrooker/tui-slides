//! Presentation state and what each key does to it. Kept free of terminal I/O
//! so the behaviour is testable.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::deck::{self, Deck};
use crate::transition::{Direction, Kind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    /// The "Go to slide" list box, with the highlighted entry.
    Goto {
        selected: usize,
    },
    /// A message box, e.g. a parse error after reloading.
    Message {
        title: String,
        text: String,
    },
}

pub struct App {
    pub deck: Deck,
    /// Where the deck came from, for reloading. `None` for decks built in memory.
    pub path: Option<PathBuf>,
    pub current: usize,
    pub overlay: Overlay,
    pub started: Instant,
    pub quit: bool,
    /// The slide window fills the screen, hiding the desktop and bars, as
    /// Turbo Vision's zoom box did.
    pub zoomed: bool,
    /// Running in a native window (`--present`) rather than a terminal, so
    /// the window can be made to fill the display.
    pub native: bool,
    /// Whether the native window should fill the display.
    pub fullscreen: bool,
    /// Whether the status bar shows the elapsed-time clock. Off for exports,
    /// where a frozen 00:00:00 would just be noise.
    pub show_clock: bool,
}

impl App {
    pub fn new(deck: Deck, path: Option<PathBuf>) -> App {
        App {
            deck,
            path,
            current: 0,
            overlay: Overlay::None,
            started: Instant::now(),
            quit: false,
            zoomed: false,
            native: false,
            fullscreen: false,
            show_clock: true,
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// The transition to play when the presenter moves from slide `from` to
    /// the current one, if there is one to play.
    pub fn transition_from(&self, from: usize) -> Option<(Kind, Direction)> {
        if from == self.current {
            return None;
        }
        let slide = &self.deck.slides[self.current];
        let kind = slide.transition.unwrap_or(self.deck.transition);
        let direction = if self.current > from {
            Direction::Forward
        } else {
            Direction::Backward
        };
        (kind != Kind::None).then_some((kind, direction))
    }

    fn last(&self) -> usize {
        self.deck.slides.len().saturating_sub(1)
    }

    pub fn go(&mut self, index: usize) {
        self.current = index.min(self.last());
    }

    /// Re-reads the deck from disk, keeping the current position where possible.
    /// On failure the old deck stays up and the error is shown in a message box.
    pub fn reload(&mut self) {
        let Some(path) = &self.path else { return };
        let result = std::fs::read_to_string(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|src| deck::parse(&src).map_err(|e| format!("{}: {e}", path.display())));
        match result {
            Ok(deck) => {
                self.deck = deck;
                self.go(self.current);
            }
            Err(text) => {
                self.overlay = Overlay::Message {
                    title: "Reload failed".into(),
                    text,
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        // These work everywhere, including inside dialogs.
        match key.code {
            KeyCode::Char('c') if ctrl => return self.quit = true,
            KeyCode::Char('x') if alt => return self.quit = true,
            _ => {}
        }

        match self.overlay.clone() {
            Overlay::None => self.handle_slide_key(key.code),
            Overlay::Help | Overlay::Message { .. } => {
                if matches!(
                    key.code,
                    KeyCode::Esc
                        | KeyCode::Enter
                        | KeyCode::Char(' ')
                        | KeyCode::Char('q')
                        | KeyCode::F(1)
                ) {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::Goto { selected } => {
                let n = self.deck.slides.len();
                let page = 10;
                let selected = match key.code {
                    KeyCode::Esc | KeyCode::F(2) | KeyCode::Char('q') => {
                        self.overlay = Overlay::None;
                        return;
                    }
                    KeyCode::Enter => {
                        self.go(selected);
                        self.overlay = Overlay::None;
                        return;
                    }
                    KeyCode::Up | KeyCode::Char('k') => selected.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => (selected + 1).min(n - 1),
                    KeyCode::PageUp => selected.saturating_sub(page),
                    KeyCode::PageDown => (selected + page).min(n - 1),
                    KeyCode::Home => 0,
                    KeyCode::End => n - 1,
                    _ => selected,
                };
                self.overlay = Overlay::Goto { selected };
            }
        }
    }

    fn handle_slide_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Right
            | KeyCode::Down
            | KeyCode::PageDown
            | KeyCode::Enter
            | KeyCode::Char(' ' | 'l' | 'n' | 'j') => self.go(self.current + 1),
            KeyCode::Left
            | KeyCode::Up
            | KeyCode::PageUp
            | KeyCode::Backspace
            | KeyCode::Char('h' | 'p' | 'k') => self.go(self.current.saturating_sub(1)),
            KeyCode::Home => self.go(0),
            KeyCode::End => self.go(self.last()),
            KeyCode::F(1) | KeyCode::Char('?') => self.overlay = Overlay::Help,
            KeyCode::F(2) | KeyCode::Char('g') => {
                self.overlay = Overlay::Goto {
                    selected: self.current,
                }
            }
            KeyCode::F(5) | KeyCode::Char('r') => self.reload(),
            KeyCode::Char('z') => self.zoomed = !self.zoomed,
            KeyCode::F(11) | KeyCode::Char('f') if self.native => {
                self.fullscreen = !self.fullscreen
            }
            KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(n: usize) -> App {
        let src: Vec<String> = (0..n).map(|i| format!("# Slide {i}\n")).collect();
        App::new(deck::parse(&src.join("---\n")).unwrap(), None)
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut a = app(3);
        press(&mut a, KeyCode::Left);
        assert_eq!(a.current, 0);
        for _ in 0..5 {
            press(&mut a, KeyCode::Char(' '));
        }
        assert_eq!(a.current, 2);
        press(&mut a, KeyCode::Home);
        assert_eq!(a.current, 0);
        press(&mut a, KeyCode::End);
        assert_eq!(a.current, 2);
    }

    #[test]
    fn goto_dialog_selects_and_cancels() {
        let mut a = app(5);
        press(&mut a, KeyCode::F(2));
        assert_eq!(a.overlay, Overlay::Goto { selected: 0 });
        press(&mut a, KeyCode::Down);
        press(&mut a, KeyCode::Down);
        // Slide keys do not leak through the dialog.
        press(&mut a, KeyCode::Right);
        assert_eq!(a.current, 0);
        press(&mut a, KeyCode::Enter);
        assert_eq!((a.current, a.overlay.clone()), (2, Overlay::None));

        press(&mut a, KeyCode::Char('g'));
        press(&mut a, KeyCode::End);
        press(&mut a, KeyCode::Esc);
        assert_eq!((a.current, a.overlay.clone()), (2, Overlay::None));
        assert!(!a.quit, "Esc closes the dialog, not the app");
    }

    #[test]
    fn zoom_and_full_screen_toggle() {
        let mut a = app(2);
        press(&mut a, KeyCode::Char('z'));
        assert!(a.zoomed);
        press(&mut a, KeyCode::Char('z'));
        assert!(!a.zoomed);

        // Full screen only means something in a native window.
        press(&mut a, KeyCode::Char('f'));
        assert!(!a.fullscreen);
        a.native = true;
        press(&mut a, KeyCode::Char('f'));
        assert!(a.fullscreen);
        press(&mut a, KeyCode::F(11));
        assert!(!a.fullscreen);
    }

    #[test]
    fn transitions_follow_the_arriving_slide() {
        let src = "+++\ntransition: wipe\n+++\n# A\n---\n# B\n<!-- transition: none -->\n---\n# C\n<!-- transition: zoom -->\n";
        let mut a = App::new(deck::parse(src).unwrap(), None);
        assert_eq!(a.transition_from(0), None, "no move, no transition");
        a.go(1);
        assert_eq!(a.transition_from(0), None, "slide B cuts");
        a.go(2);
        assert_eq!(a.transition_from(1), Some((Kind::Zoom, Direction::Forward)));
        a.go(0);
        assert_eq!(
            a.transition_from(2),
            Some((Kind::Wipe, Direction::Backward))
        );
    }

    #[test]
    fn quitting() {
        let mut a = app(2);
        press(&mut a, KeyCode::F(1));
        press(&mut a, KeyCode::Esc);
        assert!(!a.quit);
        a.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
        assert!(a.quit);
    }

    #[test]
    fn failed_reload_keeps_the_deck_and_reports() {
        let mut a = app(2);
        a.path = Some(PathBuf::from("/nonexistent/deck.md"));
        a.current = 1;
        a.reload();
        assert_eq!(a.deck.slides.len(), 2);
        assert_eq!(a.current, 1);
        assert!(matches!(a.overlay, Overlay::Message { .. }));
    }
}
