//! Parsing a slide deck from its Markdown-ish source.
//!
//! The format is deliberately small; see the README for the full description.
//!
//! ```text
//! +++
//! title: My talk
//! footer: Somewhere, 2026
//! +++
//!
//! # First slide title
//!
//! A paragraph with **strong** and `code`.
//!
//! ---
//!
//! # Second slide
//! <!-- style: dialog -->
//!
//! ```art accent="═║╔╗╚╝"
//! ╔══╗
//! ╚══╝
//! ```
//! ```

use std::fmt;

use crate::theme::{Color16, WindowStyle};
use crate::transition;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deck {
    /// Shown at the right of the menu bar.
    pub title: String,
    /// Shown at the left of the status bar, after the key hints.
    pub footer: String,
    /// How slides arrive, unless a slide says otherwise.
    pub transition: transition::Kind,
    pub slides: Vec<Slide>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slide {
    /// From the slide's `# heading`; drawn in the window frame.
    pub title: String,
    pub style: WindowStyle,
    pub align: Align,
    /// Wrap width for paragraphs and lists, if narrower than the maximum.
    pub width: Option<u16>,
    /// How this slide arrives, overriding the deck's default.
    pub transition: Option<transition::Kind>,
    pub blocks: Vec<Block>,
    /// 1-based line in the source where the slide starts, for diagnostics.
    pub line: usize,
}

/// Horizontal placement of paragraphs within a slide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(Vec<Span>),
    Paragraph(Vec<Span>),
    List(Vec<Vec<Span>>),
    /// Verbatim text drawn in the code colours.
    Code(Vec<String>),
    /// Verbatim text art. Characters in `accent` are drawn in the accent colour.
    Art {
        lines: Vec<String>,
        color: Option<Color16>,
        accent: Vec<char>,
        accent_color: Option<Color16>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub kind: SpanKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    Plain,
    Strong,
    Emphasis,
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

fn err<T>(line: usize, message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError {
        line,
        message: message.into(),
    })
}

/// Parses a whole deck.
pub fn parse(source: &str) -> Result<Deck, ParseError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut deck = Deck::default();
    let mut i = 0;

    if lines.first().map(|l| l.trim_end()) == Some("+++") {
        i = 1;
        loop {
            let Some(line) = lines.get(i) else {
                return err(1, "front matter opened with +++ is never closed");
            };
            let n = i + 1;
            i += 1;
            let line = line.trim();
            if line == "+++" {
                break;
            }
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return err(
                    n,
                    format!("expected `key: value` in front matter, got {line:?}"),
                );
            };
            let value = value.trim().to_string();
            match key.trim() {
                "title" => deck.title = value,
                "footer" => deck.footer = value,
                "transition" => deck.transition = parse_transition(&value, n)?,
                other => return err(n, format!("unknown front matter key {other:?}")),
            }
        }
    }

    // Split into slides on `---` lines that are not inside a fence.
    let mut start = i;
    let mut in_fence = false;
    for (j, line) in lines.iter().enumerate().skip(i) {
        // Same fence rules as `parse_slide`: any ``` line opens, only a bare ``` closes.
        if in_fence {
            in_fence = line.trim() != "```";
        } else if line.trim_start().starts_with("```") {
            in_fence = true;
        } else if line.trim_end() == "---" {
            push_slide(&mut deck, &lines[start..j], start + 1)?;
            start = j + 1;
        }
    }
    push_slide(&mut deck, &lines[start..], start + 1)?;

    if deck.slides.is_empty() {
        return err(1, "the deck has no slides");
    }
    Ok(deck)
}

/// Parses one slide unless it is entirely blank (e.g. after a trailing `---`).
fn push_slide(deck: &mut Deck, lines: &[&str], first_line: usize) -> Result<(), ParseError> {
    if lines.iter().all(|l| l.trim().is_empty()) {
        return Ok(());
    }
    deck.slides.push(parse_slide(lines, first_line)?);
    Ok(())
}

fn parse_slide(lines: &[&str], first_line: usize) -> Result<Slide, ParseError> {
    let mut slide = Slide {
        line: first_line,
        ..Slide::default()
    };
    let mut i = 0;
    // Lines of the paragraph or list currently being accumulated.
    let mut para: Vec<&str> = Vec::new();
    let mut items: Vec<String> = Vec::new();

    fn flush(slide: &mut Slide, para: &mut Vec<&str>, items: &mut Vec<String>) {
        if !para.is_empty() {
            let text = para.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
            slide.blocks.push(Block::Paragraph(parse_inline(&text)));
            para.clear();
        }
        if !items.is_empty() {
            let list = items.iter().map(|t| parse_inline(t)).collect();
            slide.blocks.push(Block::List(list));
            items.clear();
        }
    }

    while i < lines.len() {
        let n = first_line + i;
        let raw = lines[i];
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        i += 1;

        if trimmed.is_empty() {
            flush(&mut slide, &mut para, &mut items);
            continue;
        }

        if let Some(info) = trimmed.strip_prefix("```") {
            flush(&mut slide, &mut para, &mut items);
            let mut body = Vec::new();
            loop {
                let Some(l) = lines.get(i) else {
                    return err(n, "code fence is never closed");
                };
                i += 1;
                if l.trim() == "```" {
                    break;
                }
                if l.contains('\t') {
                    return err(
                        first_line + i - 1,
                        "tab inside a fenced block; use spaces so alignment is unambiguous",
                    );
                }
                body.push(l.trim_end().to_string());
            }
            slide.blocks.push(fence_block(info.trim(), body, n)?);
            continue;
        }

        if let Some(comment) = trimmed.strip_prefix("<!--") {
            flush(&mut slide, &mut para, &mut items);
            // Comments may span lines; collect until the closing marker.
            let mut text = comment.to_string();
            while !text.contains("-->") {
                let Some(l) = lines.get(i) else {
                    return err(n, "HTML comment is never closed");
                };
                i += 1;
                text.push('\n');
                text.push_str(l);
            }
            let (inner, after) = text.split_once("-->").expect("checked above");
            if !after.trim().is_empty() {
                return err(n, "text after the end of a comment");
            }
            directive(&mut slide, inner.trim(), n)?;
            continue;
        }

        if let Some(title) = trimmed.strip_prefix("# ") {
            flush(&mut slide, &mut para, &mut items);
            if !slide.title.is_empty() {
                return err(n, "slide already has a `# title`; use `##` for headings");
            }
            slide.title = title.trim().to_string();
            continue;
        }

        if let Some(heading) = trimmed
            .strip_prefix("## ")
            .or_else(|| trimmed.strip_prefix("### "))
        {
            flush(&mut slide, &mut para, &mut items);
            slide
                .blocks
                .push(Block::Heading(parse_inline(heading.trim())));
            continue;
        }

        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            if !para.is_empty() {
                flush(&mut slide, &mut para, &mut items);
            }
            items.push(item.trim().to_string());
            continue;
        }

        // An indented line straight after a list item continues that item.
        if !items.is_empty() && raw.starts_with([' ', '\t']) {
            let last = items.last_mut().expect("non-empty");
            last.push(' ');
            last.push_str(trimmed);
            continue;
        }

        if !items.is_empty() {
            flush(&mut slide, &mut para, &mut items);
        }
        para.push(line);
    }
    flush(&mut slide, &mut para, &mut items);
    Ok(slide)
}

/// Applies a `<!-- key: value -->` directive. Comments that are not
/// directives (no recognised key) are speaker notes and are ignored.
fn directive(slide: &mut Slide, text: &str, n: usize) -> Result<(), ParseError> {
    let Some((key, value)) = text.split_once(':') else {
        return Ok(());
    };
    let value = value.trim();
    match key.trim() {
        "style" => {
            slide.style = match value {
                "window" => WindowStyle::Window,
                "dialog" => WindowStyle::Dialog,
                _ => return err(n, format!("unknown style {value:?} (window, dialog)")),
            }
        }
        "align" => {
            slide.align = match value {
                "left" => Align::Left,
                "center" | "centre" => Align::Center,
                _ => return err(n, format!("unknown align {value:?} (left, center)")),
            }
        }
        "transition" => slide.transition = Some(parse_transition(value, n)?),
        "width" => match value.parse::<u16>() {
            Ok(w) if w >= 10 => slide.width = Some(w),
            _ => {
                return err(
                    n,
                    format!("width must be a number of columns >= 10, got {value:?}"),
                )
            }
        },
        _ => {}
    }
    Ok(())
}

fn parse_transition(name: &str, n: usize) -> Result<transition::Kind, ParseError> {
    transition::Kind::parse(name).ok_or_else(|| ParseError {
        line: n,
        message: format!("unknown transition {name:?} ({})", transition::Kind::NAMES),
    })
}

fn fence_block(info: &str, lines: Vec<String>, n: usize) -> Result<Block, ParseError> {
    let (kind, rest) = info.split_once(char::is_whitespace).unwrap_or((info, ""));
    if kind != "art" {
        // Any other info string (a language name, or nothing) is a code block.
        return Ok(Block::Code(lines));
    }
    let mut color = None;
    let mut accent = Vec::new();
    let mut accent_color = None;
    for (key, value) in parse_attributes(rest, n)? {
        let colour = |v: &str| {
            Color16::parse(v).ok_or_else(|| ParseError {
                line: n,
                message: format!("unknown colour {v:?}"),
            })
        };
        match key.as_str() {
            "color" | "colour" => color = Some(colour(&value)?),
            "accent" => accent = value.chars().collect(),
            "accent-color" | "accent-colour" => accent_color = Some(colour(&value)?),
            _ => return err(n, format!("unknown art attribute {key:?}")),
        }
    }
    Ok(Block::Art {
        lines,
        color,
        accent,
        accent_color,
    })
}

/// Parses `key=value key="quoted value"` pairs.
fn parse_attributes(s: &str, n: usize) -> Result<Vec<(String, String)>, ParseError> {
    let mut out = Vec::new();
    let mut rest = s.trim_start();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else {
            return err(
                n,
                format!("expected key=value in fence attributes, got {rest:?}"),
            );
        };
        let key = rest[..eq].trim().to_string();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return err(n, format!("bad attribute name {key:?}"));
        }
        rest = &rest[eq + 1..];
        let value;
        if let Some(q) = rest.strip_prefix('"') {
            let Some(end) = q.find('"') else {
                return err(n, "unterminated quoted attribute value");
            };
            value = q[..end].to_string();
            rest = &q[end + 1..];
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            value = rest[..end].to_string();
            rest = &rest[end..];
        }
        out.push((key, value));
        rest = rest.trim_start();
    }
    Ok(out)
}

/// Splits text into plain, `**strong**`, `*emphasis*` and `` `code` ``
/// spans. Unmatched markers are kept as literal text.
///
/// As in Markdown, a single `*` only opens emphasis when the next character
/// is not a space, and only closes it after one that is not, so `2 * 3 * 4`
/// stays arithmetic.
pub fn parse_inline(text: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut rest = text;

    fn push(spans: &mut Vec<Span>, text: String, kind: SpanKind) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = spans.last_mut() {
            if last.kind == kind {
                last.text.push_str(&text);
                return;
            }
        }
        spans.push(Span { text, kind });
    }

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                push(&mut spans, std::mem::take(&mut plain), SpanKind::Plain);
                push(&mut spans, after[..end].to_string(), SpanKind::Strong);
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = emphasis_end(after) {
                push(&mut spans, std::mem::take(&mut plain), SpanKind::Plain);
                push(&mut spans, after[..end].to_string(), SpanKind::Emphasis);
                rest = &after[end + 1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                push(&mut spans, std::mem::take(&mut plain), SpanKind::Plain);
                push(&mut spans, after[..end].to_string(), SpanKind::Code);
                rest = &after[end + 1..];
                continue;
            }
        }
        let c = rest.chars().next().expect("non-empty");
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    push(&mut spans, plain, SpanKind::Plain);
    spans
}

/// Where the `*` closing an emphasis span starts in `after` (the text just
/// past the opening `*`), if the opener is valid and a closer exists.
fn emphasis_end(after: &str) -> Option<usize> {
    let first = after.chars().next()?;
    if first.is_whitespace() || first == '*' {
        return None;
    }
    let mut prev = first;
    for (i, c) in after.char_indices().skip(1) {
        if c == '*' && !prev.is_whitespace() {
            // `**` inside is the start of strong text, not a closer.
            if after[i + 1..].starts_with('*') {
                return None;
            }
            return Some(i);
        }
        prev = c;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> Span {
        Span {
            text: s.into(),
            kind: SpanKind::Plain,
        }
    }

    #[test]
    fn front_matter_and_slides() {
        let deck =
            parse("+++\ntitle: Talk\nfooter: Here\n+++\n# One\n\nHello\n---\n# Two\n").unwrap();
        assert_eq!(deck.title, "Talk");
        assert_eq!(deck.footer, "Here");
        assert_eq!(deck.slides.len(), 2);
        assert_eq!(deck.slides[0].title, "One");
        assert_eq!(
            deck.slides[0].blocks,
            vec![Block::Paragraph(vec![plain("Hello")])]
        );
        assert_eq!(deck.slides[1].title, "Two");
        assert_eq!(deck.slides[1].line, 9);
    }

    #[test]
    fn unknown_front_matter_key_is_an_error() {
        let e = parse("+++\nauthor: me\n+++\n# x\n").unwrap_err();
        assert_eq!(e.line, 2);
    }

    #[test]
    fn separator_inside_fence_does_not_split() {
        let deck = parse("# A\n```\n---\n```\n").unwrap();
        assert_eq!(deck.slides.len(), 1);
        assert_eq!(deck.slides[0].blocks, vec![Block::Code(vec!["---".into()])]);
    }

    #[test]
    fn trailing_separator_does_not_make_an_empty_slide() {
        let deck = parse("# A\n---\n\n").unwrap();
        assert_eq!(deck.slides.len(), 1);
    }

    #[test]
    fn paragraphs_join_lines_and_lists_continue() {
        let deck = parse("# A\none\ntwo\n\n- first\n  more\n- second\nafter\n").unwrap();
        assert_eq!(
            deck.slides[0].blocks,
            vec![
                Block::Paragraph(vec![plain("one two")]),
                Block::List(vec![vec![plain("first more")], vec![plain("second")]]),
                Block::Paragraph(vec![plain("after")]),
            ]
        );
    }

    #[test]
    fn inline_markup() {
        assert_eq!(
            parse_inline("a **b** `c` **d"),
            vec![
                plain("a "),
                Span {
                    text: "b".into(),
                    kind: SpanKind::Strong
                },
                plain(" "),
                Span {
                    text: "c".into(),
                    kind: SpanKind::Code
                },
                plain(" **d"),
            ]
        );
    }

    #[test]
    fn emphasis() {
        let em = |s: &str| Span {
            text: s.into(),
            kind: SpanKind::Emphasis,
        };
        assert_eq!(
            parse_inline("an *aside* here"),
            vec![plain("an "), em("aside"), plain(" here")]
        );
        assert_eq!(parse_inline("*two words*"), vec![em("two words")]);
        assert_eq!(parse_inline("**b** *e*")[2], em("e"));
        // Not emphasis: spaced stars, unclosed stars, a lone star, and stars
        // inside code.
        for literal in ["2 * 3 * 4", "a *b", "*", "* x*", "*x *"] {
            assert_eq!(parse_inline(literal), vec![plain(literal)], "{literal}");
        }
        assert_eq!(parse_inline("`a*b*c`")[0].kind, SpanKind::Code);
    }

    #[test]
    fn art_attributes() {
        let deck = parse("# A\n```art color=lightcyan accent=\"═ ║\" accent-color=red\n╔═╗\n```\n")
            .unwrap();
        assert_eq!(
            deck.slides[0].blocks,
            vec![Block::Art {
                lines: vec!["╔═╗".into()],
                color: Some(Color16::LightCyan),
                accent: vec!['═', ' ', '║'],
                accent_color: Some(Color16::Red),
            }]
        );
    }

    #[test]
    fn bad_art_attributes_are_errors() {
        assert!(parse("# A\n```art color=mauve\n```\n").is_err());
        assert!(parse("# A\n```art sparkle=yes\n```\n").is_err());
        assert!(parse("# A\n```art accent=\"x\n```\n").is_err());
    }

    #[test]
    fn directives_and_notes() {
        let deck = parse(
            "# A\n<!-- style: dialog -->\n<!-- align: center -->\n<!-- width: 40 -->\n<!--\nspeaker notes\nspan lines -->\nx\n",
        )
        .unwrap();
        let s = &deck.slides[0];
        assert_eq!(s.style, WindowStyle::Dialog);
        assert_eq!(s.align, Align::Center);
        assert_eq!(s.width, Some(40));
        assert_eq!(s.blocks.len(), 1);
    }

    #[test]
    fn transitions() {
        let deck = parse("+++\ntransition: wipe\n+++\n# A\n---\n# B\n<!-- transition: none -->\n")
            .unwrap();
        assert_eq!(deck.transition, transition::Kind::Wipe);
        assert_eq!(deck.slides[0].transition, None);
        assert_eq!(deck.slides[1].transition, Some(transition::Kind::None));
        assert_eq!(
            parse("# A\n<!-- transition: spin -->\n").unwrap_err().line,
            2
        );
        assert_eq!(
            parse("+++\ntransition: spin\n+++\n# A\n").unwrap_err().line,
            2
        );
    }

    #[test]
    fn errors_carry_line_numbers() {
        assert_eq!(parse("# A\n\n```\nnever closed\n").unwrap_err().line, 3);
        assert_eq!(parse("# A\n# B\n").unwrap_err().line, 2);
        assert_eq!(parse("# A\n<!-- style: fancy -->\n").unwrap_err().line, 2);
        assert_eq!(parse("# A\n```art\n\tx\n```\n").unwrap_err().line, 3);
        assert!(parse("\n\n").is_err());
    }
}
