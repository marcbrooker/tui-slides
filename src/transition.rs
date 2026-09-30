//! Slide transitions: blending the frame that was on screen into the one
//! that replaces it.
//!
//! Everything here works on whole rendered buffers, so a transition knows
//! nothing about slides, windows or layout. That keeps it composable with
//! every screen the app can draw, full screen or not.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// How one slide replaces another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Cut straight to the new slide.
    #[default]
    None,
    /// The new slide is uncovered by an edge sweeping across the screen.
    Wipe,
    /// The new slide pushes the old one off the side of the screen.
    Push,
    /// Cells switch to the new slide one by one, in a fixed random order.
    Dissolve,
    /// The new slide grows out of the centre of the screen.
    Zoom,
}

impl Kind {
    pub const NAMES: &'static str = "none, wipe, push, dissolve, zoom";

    pub fn parse(name: &str) -> Option<Kind> {
        match name {
            "none" => Some(Kind::None),
            "wipe" => Some(Kind::Wipe),
            "push" => Some(Kind::Push),
            "dissolve" => Some(Kind::Dissolve),
            "zoom" => Some(Kind::Zoom),
            _ => None,
        }
    }

    fn duration(self) -> Duration {
        match self {
            Kind::None => Duration::ZERO,
            Kind::Wipe | Kind::Push => Duration::from_millis(300),
            Kind::Zoom => Duration::from_millis(350),
            Kind::Dissolve => Duration::from_millis(450),
        }
    }
}

/// Which way the presenter moved; wipes and pushes run the other way when
/// going back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// A transition in progress.
pub struct Transition {
    kind: Kind,
    direction: Direction,
    from: Buffer,
    started: Instant,
}

impl Transition {
    /// Starts a transition away from `from`, the frame currently on screen.
    pub fn new(kind: Kind, direction: Direction, from: Buffer, now: Instant) -> Transition {
        Transition {
            kind,
            direction,
            from,
            started: now,
        }
    }

    /// Fraction of the way through, from 0 to 1.
    pub fn progress(&self, now: Instant) -> f32 {
        let total = self.kind.duration();
        if total.is_zero() {
            return 1.0;
        }
        (now.saturating_duration_since(self.started).as_secs_f32() / total.as_secs_f32()).min(1.0)
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }

    /// Replaces `region` of `to`, the fully drawn new frame, with the
    /// in-between frame for time `now`. The rest of `to` is left alone, so
    /// the menu and status bars stay put. Does nothing if the screen was
    /// resized since the transition started, since the old frame no longer
    /// lines up.
    pub fn apply(&self, to: &mut Buffer, region: Rect, now: Instant) {
        if self.from.area != to.area {
            return;
        }
        let region = region.intersection(to.area);
        let progress = self.progress(now);
        blend(self.kind, self.direction, &self.from, to, region, progress);
    }
}

/// Eases in and out, so movement starts and stops gently.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Overwrites `region` of `to` with the frame `progress` of the way from
/// `from` to `to`. At 0 the region shows `from`; at 1 it is `to`, unchanged.
/// Both buffers must cover `region`.
pub fn blend(
    kind: Kind,
    direction: Direction,
    from: &Buffer,
    to: &mut Buffer,
    region: Rect,
    progress: f32,
) {
    if progress >= 1.0 || kind == Kind::None || region.is_empty() {
        return;
    }
    let area = region;
    let (w, h) = (area.width, area.height);
    let eased = smoothstep(progress);
    let next = to.clone();
    let forward = direction == Direction::Forward;

    // For each cell on screen, which buffer it comes from and where in it.
    for y in 0..h {
        for x in 0..w {
            let source = match kind {
                Kind::None => unreachable!("returned above"),
                Kind::Wipe => {
                    let edge = (eased * f32::from(w)).round() as u16;
                    let uncovered = if forward { x < edge } else { x >= w - edge };
                    if uncovered {
                        (&next, x)
                    } else {
                        (from, x)
                    }
                }
                Kind::Push => {
                    let shift = (eased * f32::from(w)).round() as u16;
                    if forward {
                        if x < w - shift {
                            (from, x + shift)
                        } else {
                            (&next, x - (w - shift))
                        }
                    } else if x < shift {
                        (&next, x + (w - shift))
                    } else {
                        (from, x - shift)
                    }
                }
                Kind::Dissolve => {
                    if threshold(x, y) < progress {
                        (&next, x)
                    } else {
                        (from, x)
                    }
                }
                Kind::Zoom => {
                    // A box growing from the centre, keeping the screen's
                    // proportions (cells are about twice as tall as wide).
                    let half_w = eased * f32::from(w) / 2.0;
                    let half_h = eased * f32::from(h) / 2.0;
                    let dx = (f32::from(x) + 0.5 - f32::from(w) / 2.0).abs();
                    let dy = (f32::from(y) + 0.5 - f32::from(h) / 2.0).abs();
                    if dx <= half_w && dy <= half_h {
                        (&next, x)
                    } else {
                        (from, x)
                    }
                }
            };
            let (buf, sx) = source;
            let cell = buf[(area.x + sx, area.y + y)].clone();
            to[(area.x + x, area.y + y)] = cell;
        }
    }
}

/// A fixed pseudo-random value in [0, 1) for each cell, so a dissolve
/// switches cells in a scattered but repeatable order.
fn threshold(x: u16, y: u16) -> f32 {
    // SplitMix-style integer hash of the coordinates.
    let mut z = (u64::from(x) << 32 | u64::from(y)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u16 = 10;
    const H: u16 = 4;

    fn filled(c: &str) -> Buffer {
        let mut b = Buffer::empty(Rect::new(0, 0, W, H));
        for y in 0..H {
            for x in 0..W {
                b[(x, y)].set_symbol(c);
            }
        }
        b
    }

    /// A buffer whose cells name their own column, to check where they moved.
    fn columns(prefix: char) -> Buffer {
        let mut b = Buffer::empty(Rect::new(0, 0, W, H));
        for y in 0..H {
            for x in 0..W {
                b[(x, y)].set_symbol(&format!("{prefix}{x}"));
            }
        }
        b
    }

    fn row(b: &Buffer, y: u16) -> Vec<String> {
        (0..W).map(|x| b[(x, y)].symbol().to_string()).collect()
    }

    fn blended(kind: Kind, dir: Direction, p: f32) -> Buffer {
        let mut to = columns('n');
        blend(kind, dir, &columns('o'), &mut to, ALL_OF_IT, p);
        to
    }

    const ALL_OF_IT: Rect = Rect::new(0, 0, W, H);

    #[test]
    fn only_the_region_changes() {
        // Rows 1-2 transition; rows 0 and 3 (the "bars") show the new frame throughout.
        let region = Rect::new(0, 1, W, 2);
        for kind in ALL {
            let mut to = columns('n');
            blend(
                kind,
                Direction::Forward,
                &columns('o'),
                &mut to,
                region,
                0.0,
            );
            assert_eq!(row(&to, 0), row(&columns('n'), 0), "{kind:?}");
            assert_eq!(row(&to, 1), row(&columns('o'), 1), "{kind:?}");
            assert_eq!(row(&to, 3), row(&columns('n'), 3), "{kind:?}");
        }
    }

    const ALL: [Kind; 4] = [Kind::Wipe, Kind::Push, Kind::Dissolve, Kind::Zoom];

    #[test]
    fn every_kind_starts_at_the_old_frame_and_ends_at_the_new() {
        for kind in ALL {
            for dir in [Direction::Forward, Direction::Backward] {
                assert_eq!(blended(kind, dir, 0.0), columns('o'), "{kind:?} {dir:?}");
                assert_eq!(blended(kind, dir, 1.0), columns('n'), "{kind:?} {dir:?}");
            }
        }
    }

    #[test]
    fn none_cuts_immediately() {
        assert_eq!(blended(Kind::None, Direction::Forward, 0.0), columns('n'));
    }

    #[test]
    fn wipe_uncovers_from_the_side_it_is_heading_away_from() {
        let fwd = row(&blended(Kind::Wipe, Direction::Forward, 0.5), 0);
        assert_eq!(fwd[..5], ["n0", "n1", "n2", "n3", "n4"]);
        assert_eq!(fwd[5..], ["o5", "o6", "o7", "o8", "o9"]);
        let back = row(&blended(Kind::Wipe, Direction::Backward, 0.5), 0);
        assert_eq!(back[..5], ["o0", "o1", "o2", "o3", "o4"]);
        assert_eq!(back[5..], ["n5", "n6", "n7", "n8", "n9"]);
    }

    #[test]
    fn push_moves_both_frames() {
        let fwd = row(&blended(Kind::Push, Direction::Forward, 0.5), 0);
        assert_eq!(
            fwd,
            ["o5", "o6", "o7", "o8", "o9", "n0", "n1", "n2", "n3", "n4"]
        );
        let back = row(&blended(Kind::Push, Direction::Backward, 0.5), 0);
        assert_eq!(
            back,
            ["n5", "n6", "n7", "n8", "n9", "o0", "o1", "o2", "o3", "o4"]
        );
    }

    #[test]
    fn dissolve_only_ever_adds_new_cells() {
        let count_new = |b: &Buffer| {
            (0..H)
                .flat_map(|y| row(b, y))
                .filter(|s| s.starts_with('n'))
                .count()
        };
        let mut last = 0;
        for step in 0..=10 {
            let p = step as f32 / 10.0;
            let mut to = filled("n");
            blend(
                Kind::Dissolve,
                Direction::Forward,
                &filled("o"),
                &mut to,
                ALL_OF_IT,
                p,
            );
            let now = count_new(&to);
            assert!(now >= last, "cells never switch back");
            // Pixels stay put: each cell comes from its own position.
            if p > 0.0 && p < 1.0 {
                let b = blended(Kind::Dissolve, Direction::Forward, p);
                for y in 0..H {
                    for (x, s) in row(&b, y).iter().enumerate() {
                        assert_eq!(&s[1..], x.to_string());
                    }
                }
            }
            last = now;
        }
        assert_eq!(last, usize::from(W * H));
    }

    #[test]
    fn zoom_grows_from_the_centre() {
        let b = blended(Kind::Zoom, Direction::Forward, 0.5);
        assert!(row(&b, 1)[5].starts_with('n'), "centre is new");
        assert!(row(&b, 0)[0].starts_with('o'), "corner is still old");
    }

    #[test]
    fn a_resize_abandons_the_blend() {
        let t = Transition::new(Kind::Wipe, Direction::Forward, filled("o"), Instant::now());
        let mut to = Buffer::empty(Rect::new(0, 0, W + 1, H));
        let before = to.clone();
        t.apply(&mut to, ALL_OF_IT, Instant::now());
        assert_eq!(to, before);
    }

    #[test]
    fn progress_runs_from_zero_to_one() {
        let start = Instant::now();
        let t = Transition::new(Kind::Wipe, Direction::Forward, filled("o"), start);
        assert_eq!(t.progress(start), 0.0);
        assert!(!t.finished(start));
        assert!(t.finished(start + Duration::from_secs(1)));
        let cut = Transition::new(Kind::None, Direction::Forward, filled("o"), start);
        assert!(cut.finished(start));
    }

    #[test]
    fn names_parse() {
        for name in Kind::NAMES.split(", ") {
            assert!(Kind::parse(name).is_some(), "{name}");
        }
        assert_eq!(Kind::parse("spin"), None);
    }
}
