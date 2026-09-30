# tvslides

Present a Markdown slide deck as a full-screen terminal application in the
style of Borland's Turbo Vision: a dithered blue-and-grey desktop, a menu bar
and status bar, double-framed windows with close boxes and drop shadows, and
grey dialog boxes with green buttons. Every slide is laid out for an 80x25
screen, and text or Unicode art does the job of diagrams.

```text
  ≡   File   Edit   View   Slide   Help                                Dogwood
╔═[■]══════════════════════ Agent safety is a box ═════════════════════2═[↕]═╗░░
║  ┌─────────────────────────────┐                                           ▲░░
║  │  ┌──────────┐               │                         ┌──────────────┐  ▒░░
║  │  │  Agent   │         ┌─────┴───┐╔══════╗             │              │  ▒░░
║  │  └──────────┘         │         │║      ╟─────────────┤     Tool     │  ▒░░
║  │                       │ Gateway │║Policy║        ┌────┤              │  ▒░░
║  │          ┌──────────┐ │         │║      ╟─────┐  │    └──────────────┘  ▒░░
║  │          │  Agent   │ └─────┬───┘╚══════╝     │  │                      ▒░░
```

## Running

To present, give the deck its own full screen window:

```sh
cargo run --release -- --present slides/dogwood.md
```

It fills whichever display it opens on. To present on a projector, start it
with `--windowed`, drag the window onto the projector's display, and press
`f`. The window draws the 80x25 screen itself: box-drawing, block and shade
characters are drawn as geometry, so lines meet exactly and the art joins up
at any size, and colours are the exact VGA palette. Text uses Menlo (or
`--font PATH` for any TrueType or OpenType font). The mouse pointer is hidden.

The deck also runs in any terminal, which is handy for writing it:

```sh
cargo run --release -- slides/dogwood.md
```

| Key | Action |
|-----|--------|
| `→` `Space` `PgDn` `Enter` `n` `l` | Next slide |
| `←` `Backspace` `PgUp` `p` `h` | Previous slide |
| `Home` / `End` | First / last slide |
| `F2` or `g` | Go-to-slide list |
| `F5` or `r` | Reload the deck file from disk |
| `z` | Zoom the slide window to fill the screen |
| `f`, `F11` or `Ctrl-Cmd-F` | Full screen on / off (`--present` only) |
| `F1` or `?` | Help |
| `Esc` `q` `Alt-X` `Ctrl-C` `Cmd-Q` | Exit (`Esc` closes a dialog first) |

The status bar shows the slide number and the time since the presentation
started. Zooming (`z`, or start with `--zoom`) makes the slide
window fill the screen and hides the desktop, menu bar and status bar, as
Turbo Vision's zoom box did; content stays centred at the same scale. The scroll bar on the right of each window is a progress bar through
the deck.

To export the whole deck as a PDF, one slide per page, for handouts or as a
backup in case the presenting laptop fails:

```sh
tvslides --pdf dogwood.pdf slides/dogwood.md
tvslides --pdf dogwood.pdf --zoom slides/dogwood.md   # slides zoomed
```

Pages are drawn exactly as `--present` draws them, at 3840x2150 pixels on a
13.3 x 7.5 inch page (the usual 16:9 slide size), without the status bar
clock. They are images, so text in the PDF cannot be selected or searched.
`--font PATH` works here too.

Two other modes are useful while writing a deck:

```sh
tvslides --check deck.md           # report slides that will not fit at 80x25
tvslides --dump 2 deck.md          # print slide 2 as an 80x25 screen of text
tvslides --dump all --color deck.md | less -R
tvslides --dump 1 --zoom deck.md   # as it looks zoomed
```

`r` in the presenter reloads the deck, so you can keep it open in one
terminal and edit it in another. If the new version does not parse, the old
one stays up and the error is shown in a dialog.

### Terminal setup

This only matters when presenting from a terminal rather than `--present`.
Size the terminal to at least 80x25 and scale the font up until it fills the
projector. Larger terminals work: the desktop grows, but windows stay at 80x25
scale. Smaller ones get a message asking for more room.

With `COLORTERM=truecolor` (or `24bit`) the exact VGA palette is used;
otherwise the nearest ANSI colours, which your terminal theme may remap. Use a
font with good box-drawing and block-element coverage (Menlo, SF Mono,
JetBrains Mono, Cascadia) and a line spacing of 1.0 so the art joins up.

## Deck format

A deck is a Markdown file. Slides are separated by lines containing only
`---`.

````markdown
+++
title: Dogwood
footer: Seattle Systems
transition: push
+++

# Slide title
<!-- style: dialog -->

## A centred heading

A paragraph, wrapped to fit, with **strong**, *emphasised* and `code` text.

- A bullet list. Indented lines
  continue the previous item.
- Another item.

```art accent="╔╗╚╝═║" accent-color=lightcyan
╔══════╗
║ box  ║
╚══════╝
```

```
a code block, drawn on its own background
```
````

**Front matter** (optional, between `+++` lines at the very top): `title` is
shown at the right of the menu bar, `footer` in the status bar, and
`transition` sets how slides arrive (default `none`).

**Slide title**: the slide's `# heading`, drawn in the window frame. One per
slide.

**Blocks**:

- `##` or `###` headings are centred.
- Paragraphs are word-wrapped. Inline markup is `**strong**` (yellow),
  `*emphasis*` (light green) and `` `code` `` (light cyan); in dialogs they
  are blue, magenta and blue. Text-mode screens had no italic, so emphasis
  is a colour. A `*` next to a space is left alone, so `2 * 3` stays as
  written.
- `-` or `*` lists, with hanging indents.
- Fenced code blocks are shown verbatim on a contrasting background. The
  info string (a language name) is ignored.
- Fenced `art` blocks are shown verbatim, centred as a whole so their internal
  alignment is kept. Attributes:
  - `color=NAME` draws the art in that colour.
  - `accent="CHARS"` draws every listed character in the accent colour
    (yellow in windows, blue in dialogs), or in `accent-color=NAME`. Picking
    out characters, rather than marking up spans, keeps the source aligned
    exactly as it will be drawn: the example deck highlights the policy
    boxes by accenting the double-line box characters only they use.

Colour names are the sixteen text-mode colours: `black`, `blue`, `green`,
`cyan`, `red`, `magenta`, `brown`, `lightgray`, `darkgray`, `lightblue`,
`lightgreen`, `lightcyan`, `lightred`, `lightmagenta`, `yellow`, `white`.

**Directives** are HTML comments of the form `<!-- key: value -->`:

| Directive | Effect |
|-----------|--------|
| `style: window` / `style: dialog` | Blue document window (default) or grey dialog box |
| `align: left` / `align: center` | Placement of paragraphs, lists and code |
| `width: N` | Wrap paragraphs and lists at N columns instead of the maximum |
| `transition: KIND` | How this slide arrives, overriding the deck's default |

**Transitions** play when you move to a slide, using that slide's
transition. They cover the desktop (the whole screen when zoomed);
the menu and status bars stay put. Paging during a transition starts the next
one from whatever is on screen, so you can skip through quickly.

| Kind | Effect |
|------|--------|
| `none` | Cut straight to the slide |
| `wipe` | An edge sweeps across, uncovering the new slide |
| `push` | The new slide pushes the old one off the screen |
| `dissolve` | Cells switch over one by one in a scattered order |
| `zoom` | The new slide grows out of the centre |

Wipes and pushes run the other way when you go back.

Any other comment is ignored, which makes comments a place for speaker notes.

**Size limits.** A window can hold 68 columns by 18 rows of content, inside four columns
and one or two rows of padding. Text wraps
to fit; art and code do not, so run `--check` after editing. Tabs are
rejected inside fenced blocks, because their width is ambiguous. Characters
are measured by their Unicode display width, so wide (e.g. CJK) characters
count as two columns. Ambiguous-width symbols can still render differently
between fonts; check the deck in the terminal you will present from.

## Code layout

| File | Contents |
|------|----------|
| `src/deck.rs` | Parsing the deck format into slides and blocks |
| `src/layout.rs` | Wrapping and placing blocks as styled lines for the fixed content area |
| `src/theme.rs` | The CGA palette and the mapping from content roles to colours |
| `src/ui.rs` | Drawing the desktop, bars, windows, shadows and dialogs |
| `src/transition.rs` | Blending one rendered frame into the next |
| `src/raster.rs` | Turning cells into pixels for the native window |
| `src/window.rs` | The `--present` window and its event loop |
| `src/pdf.rs` | A minimal PDF writer: one full-page image per page |
| `src/app.rs` | Presentation state and key handling, free of terminal I/O |
| `src/main.rs` | Command line, the event loop, `--check` and `--dump` |

Built on [ratatui](https://ratatui.rs) and
[crossterm](https://github.com/crossterm-rs/crossterm); the window uses
[winit](https://github.com/rust-windowing/winit),
[softbuffer](https://github.com/rust-windowing/softbuffer) and
[ab_glyph](https://github.com/alexheretic/ab-glyph). Both front ends run the
same app and drawing code: the window just turns the cells into pixels. `cargo test` covers
parsing, layout, key handling, rendering into an in-memory buffer, and the
binary's command-line modes against the example deck.

## The example deck

`slides/dogwood.md` is the deck for a talk about
[Dogwood](https://github.com/dogwood-policy/dogwood) at Seattle Systems. It
has a title slide, the agent box diagram from
[Agent Safety is a Box](https://brooker.co.za/blog/2026/01/12/agent-box.html),
worked Dogwood policies with timelines of their verdicts, and a diagram of
concurrency control.
