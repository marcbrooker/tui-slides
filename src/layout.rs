//! Turning a slide's blocks into positioned, styled lines of text.
//!
//! Layout is independent of the terminal: every slide is laid out for the
//! same fixed content area, which is what keeps the 80x25 look on any
//! terminal size.

use unicode_width::UnicodeWidthStr;

use crate::deck::{Align, Block, Slide, Span, SpanKind};
use crate::theme::Role;

/// Widest content a window can hold: 80 columns less the window frame,
/// four columns of padding either side, and the two-column drop shadow.
pub const MAX_CONTENT_WIDTH: u16 = 68;
/// Tallest content a window can hold: 25 rows less the menu bar, status bar,
/// window frame, a row of padding above and below, and the one-row shadow.
pub const MAX_CONTENT_HEIGHT: u16 = 18;

const BULLET: &str = "► ";

/// A run of text drawn in one role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub role: Role,
}

/// One row of content, starting `x` columns into the content area.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub x: u16,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub lines: Vec<Line>,
    /// Columns spanned by the widest line.
    pub width: u16,
}

impl Layout {
    pub fn height(&self) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }
}

pub fn text_width(s: &str) -> u16 {
    u16::try_from(UnicodeWidthStr::width(s)).unwrap_or(u16::MAX)
}

/// How a block's lines are placed within the overall content width.
enum Placement {
    /// Every line flush left.
    Left,
    /// Every line centred on its own.
    CenterLines,
    /// The block keeps its internal alignment and is centred as a whole.
    CenterBlock,
}

struct BlockLines {
    lines: Vec<Vec<Run>>,
    placement: Placement,
}

pub fn layout(slide: &Slide) -> Layout {
    let wrap = slide
        .width
        .unwrap_or(MAX_CONTENT_WIDTH)
        .min(MAX_CONTENT_WIDTH);
    let centered = slide.align == Align::Center;

    let blocks: Vec<BlockLines> = slide
        .blocks
        .iter()
        .map(|block| match block {
            Block::Heading(spans) => BlockLines {
                lines: wrap_spans(spans, wrap, Role::Heading),
                placement: Placement::CenterLines,
            },
            Block::Paragraph(spans) => BlockLines {
                lines: wrap_spans(spans, wrap, Role::Text),
                placement: if centered {
                    Placement::CenterLines
                } else {
                    Placement::Left
                },
            },
            Block::List(items) => BlockLines {
                lines: list_lines(items, wrap),
                placement: if centered {
                    Placement::CenterBlock
                } else {
                    Placement::Left
                },
            },
            Block::Code(lines) => BlockLines {
                lines: code_lines(lines),
                placement: if centered {
                    Placement::CenterBlock
                } else {
                    Placement::Left
                },
            },
            Block::Art {
                lines,
                color,
                accent,
                accent_color,
            } => {
                let base = color.map(Role::ArtColor).unwrap_or(Role::Art);
                let accent_role = accent_color.map(Role::ArtColor).unwrap_or(Role::ArtAccent);
                BlockLines {
                    lines: lines
                        .iter()
                        .map(|l| art_line(l, accent, base, accent_role))
                        .collect(),
                    placement: Placement::CenterBlock,
                }
            }
        })
        .collect();

    let line_width = |runs: &[Run]| -> u16 { runs.iter().map(|r| text_width(&r.text)).sum() };
    let block_width =
        |b: &BlockLines| -> u16 { b.lines.iter().map(|l| line_width(l)).max().unwrap_or(0) };
    let width = blocks.iter().map(block_width).max().unwrap_or(0);

    let mut out = Layout {
        lines: Vec::new(),
        width,
    };
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.lines.push(Line::default());
        }
        let bw = block_width(block);
        for runs in &block.lines {
            let x = match block.placement {
                Placement::Left => 0,
                Placement::CenterLines => (width - line_width(runs)) / 2,
                Placement::CenterBlock => (width - bw) / 2,
            };
            out.lines.push(Line {
                x,
                runs: runs.clone(),
            });
        }
    }
    out
}

/// Things about a slide that will not display properly at 80x25.
pub fn problems(slide: &Slide) -> Vec<String> {
    let l = layout(slide);
    let mut out = Vec::new();
    if l.width > MAX_CONTENT_WIDTH {
        out.push(format!(
            "content is {} columns wide; the most that fits is {MAX_CONTENT_WIDTH}",
            l.width
        ));
    }
    if l.height() > MAX_CONTENT_HEIGHT {
        out.push(format!(
            "content is {} rows tall; the most that fits is {MAX_CONTENT_HEIGHT}",
            l.height()
        ));
    }
    let title_room = MAX_CONTENT_WIDTH - 8;
    if text_width(&slide.title) > title_room {
        out.push(format!(
            "title is {} columns wide; the most that fits is {title_room}",
            text_width(&slide.title)
        ));
    }
    out
}

fn role_for(kind: SpanKind, base: Role) -> Role {
    match kind {
        SpanKind::Plain => base,
        SpanKind::Strong => Role::Strong,
        SpanKind::Emphasis => Role::Emphasis,
        SpanKind::Code => Role::InlineCode,
    }
}

/// Appends text to a run list, merging with the previous run when the role matches.
fn push_run(runs: &mut Vec<Run>, text: &str, role: Role) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.role == role => last.text.push_str(text),
        _ => runs.push(Run {
            text: text.to_string(),
            role,
        }),
    }
}

/// Greedy word wrap. A "word" may cross span boundaries (e.g. `**x**,`), and
/// words wider than the line are broken between characters.
fn wrap_spans(spans: &[Span], width: u16, base: Role) -> Vec<Vec<Run>> {
    // Split into words, each a list of (char, role).
    let mut words: Vec<Vec<(char, Role)>> = vec![Vec::new()];
    for span in spans {
        let role = role_for(span.kind, base);
        for c in span.text.chars() {
            if c.is_whitespace() {
                if !words.last().expect("non-empty").is_empty() {
                    words.push(Vec::new());
                }
            } else {
                words.last_mut().expect("non-empty").push((c, role));
            }
        }
    }
    words.retain(|w| !w.is_empty());

    let char_width = |c: char| {
        u16::try_from(unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).unwrap_or(0)
    };
    let width = width.max(1);
    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut line: Vec<Run> = Vec::new();
    let mut used: u16 = 0;

    for word in words {
        let w: u16 = word.iter().map(|(c, _)| char_width(*c)).sum();
        if used > 0 && used + 1 + w > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        if used > 0 {
            push_run(&mut line, " ", base);
            used += 1;
        }
        for (c, role) in word {
            let cw = char_width(c);
            if used > 0 && used + cw > width {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            let mut buf = [0u8; 4];
            push_run(&mut line, c.encode_utf8(&mut buf), role);
            used += cw;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn list_lines(items: &[Vec<Span>], width: u16) -> Vec<Vec<Run>> {
    let indent = text_width(BULLET);
    let mut out = Vec::new();
    for item in items {
        for (i, mut line) in wrap_spans(item, width.saturating_sub(indent), Role::Text)
            .into_iter()
            .enumerate()
        {
            let prefix = if i == 0 {
                Run {
                    text: BULLET.to_string(),
                    role: Role::Bullet,
                }
            } else {
                Run {
                    text: " ".repeat(indent.into()),
                    role: Role::Text,
                }
            };
            line.insert(0, prefix);
            out.push(line);
        }
    }
    out
}

/// Code lines are padded to a common width plus a margin, so the block reads
/// as one solid panel of the code background colour.
fn code_lines(lines: &[String]) -> Vec<Vec<Run>> {
    let w = lines.iter().map(|l| text_width(l)).max().unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            let pad = usize::from(w - text_width(l));
            vec![Run {
                text: format!(" {l}{} ", " ".repeat(pad)),
                role: Role::CodeBlock,
            }]
        })
        .collect()
}

fn art_line(line: &str, accent: &[char], base: Role, accent_role: Role) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut buf = [0u8; 4];
    for c in line.chars() {
        let role = if accent.contains(&c) {
            accent_role
        } else {
            base
        };
        push_run(&mut runs, c.encode_utf8(&mut buf), role);
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deck::parse;

    fn texts(l: &Layout) -> Vec<String> {
        l.lines
            .iter()
            .map(|line| {
                let body: String = line.runs.iter().map(|r| r.text.as_str()).collect();
                format!("{}{}", " ".repeat(line.x.into()), body)
            })
            .collect()
    }

    fn slide(src: &str) -> Slide {
        parse(src).unwrap().slides.remove(0)
    }

    #[test]
    fn paragraphs_wrap_at_the_slide_width() {
        let s = slide("# T\n<!-- width: 12 -->\nthe quick brown fox jumps\n");
        assert_eq!(texts(&layout(&s)), vec!["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn long_words_break() {
        let s = slide("# T\n<!-- width: 10 -->\nabcdefghijklmnop\n");
        assert_eq!(texts(&layout(&s)), vec!["abcdefghij", "klmnop"]);
    }

    #[test]
    fn wrapping_counts_display_width() {
        // Each CJK character is two columns wide.
        let s = slide("# T\n<!-- width: 10 -->\n漢字漢字漢字\n");
        assert_eq!(texts(&layout(&s)), vec!["漢字漢字漢", "字"]);
    }

    #[test]
    fn markup_keeps_its_role_across_wraps() {
        let s = slide("# T\n<!-- width: 10 -->\nsee **very bold** text\n");
        let l = layout(&s);
        assert_eq!(texts(&l), vec!["see very", "bold text"]);
        assert_eq!(
            l.lines[1].runs[0],
            Run {
                text: "bold".into(),
                role: Role::Strong
            }
        );
    }

    #[test]
    fn lists_hang_their_continuation_lines() {
        let s = slide("# T\n<!-- width: 12 -->\n- one two three\n- four\n");
        assert_eq!(texts(&layout(&s)), vec!["► one two", "  three", "► four"]);
    }

    #[test]
    fn art_is_centred_as_a_block_and_accented() {
        let s = slide("# T\nsome wider paragraph\n\n```art accent=\"#\"\n#.\n.##\n```\n");
        let l = layout(&s);
        assert_eq!(
            texts(&l),
            vec!["some wider paragraph", "", "        #.", "        .##"]
        );
        assert_eq!(
            l.lines[3].runs,
            vec![
                Run {
                    text: ".".into(),
                    role: Role::Art
                },
                Run {
                    text: "##".into(),
                    role: Role::ArtAccent
                },
            ]
        );
    }

    #[test]
    fn headings_centre_and_code_pads() {
        let s = slide("# T\n## Hi\n\n```\nab\nabcd\n```\n");
        assert_eq!(texts(&layout(&s)), vec!["  Hi", "", " ab   ", " abcd "]);
    }

    #[test]
    fn problems_report_oversized_content() {
        let wide = format!("# T\n```art\n{}\n```\n", "x".repeat(80));
        assert_eq!(problems(&slide(&wide)).len(), 1);
        let tall = format!("# T\n```art\n{}```\n", "x\n".repeat(21));
        assert_eq!(problems(&slide(&tall)).len(), 1);
        assert!(problems(&slide("# T\nfine\n")).is_empty());
    }
}
