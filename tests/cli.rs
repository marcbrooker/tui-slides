//! End-to-end checks of the binary against the example deck.

use std::process::Command;

/// A fixed nine-slide deck (a snapshot of the example talk), so these tests
/// do not change every time the talk itself is edited.
const DECK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/deck.md");
/// The talk itself, which should always fit.
const TALK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/slides/dogwood.md");

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tui-slides"))
        .args(args)
        .output()
        .expect("run tui-slides")
}

#[test]
fn example_deck_fits() {
    let out = run(&["--check", DECK]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("9 slides, all fit at 80x25"), "{stdout}");
}

#[test]
fn the_talk_fits() {
    let out = run(&["--check", TALK]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("all fit at 80x25"), "{stdout}");
}

#[test]
fn dump_prints_one_80x25_screen_per_slide() {
    let out = run(&["--dump", "all", DECK]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let screens: Vec<&str> = stdout.split("\n\n").collect();
    assert_eq!(screens.len(), 9);
    for screen in &screens {
        let rows: Vec<&str> = screen.trim_end_matches('\n').split('\n').collect();
        assert_eq!(rows.len(), 25);
        for row in rows {
            assert_eq!(row.chars().count(), 80, "{row:?}");
        }
    }
    assert!(screens[1].contains("Microservice"));
    assert!(screens[2].contains("formerly within 1h"));
    assert!(screens[5].contains("previous within 10m"));
    assert!(screens[7].contains("$8,000 out"));
}

#[test]
fn dump_can_render_zoomed() {
    let out = run(&["--dump", "1", "--zoom", DECK]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("╔═[■]"), "{stdout}");
    assert!(!stdout.contains("Alt-X Exit"));
}

#[test]
fn pdf_has_a_page_per_slide() {
    let dir = std::env::temp_dir().join(format!("tui-slides-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("deck.pdf");
    let out = run(&["--pdf", path.to_str().unwrap(), DECK]);
    let pdf = std::fs::read(&path).unwrap_or_default();
    std::fs::remove_file(&path).ok();
    std::fs::remove_dir(&dir).ok();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(pdf.starts_with(b"%PDF-1.4\n") && pdf.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 9"));
    assert_eq!(text.matches("/Type /Page ").count(), 9);
    assert!(String::from_utf8_lossy(&out.stderr).contains("wrote 9 pages"));
}

#[test]
fn check_fails_on_oversized_slides() {
    let dir = std::env::temp_dir().join(format!("tui-slides-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wide.md");
    std::fs::write(&path, format!("# Wide\n```art\n{}\n```\n", "x".repeat(90))).unwrap();
    let out = run(&["--check", path.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("90 columns wide"), "{stdout}");
}

#[test]
fn bad_arguments_and_decks_fail_cleanly() {
    assert_eq!(run(&[]).status.code(), Some(2));
    assert_eq!(run(&["--dump", "0", DECK]).status.code(), Some(2));
    assert_eq!(run(&["--color", DECK]).status.code(), Some(2));
    assert_eq!(run(&["--windowed", DECK]).status.code(), Some(2));
    assert_eq!(
        run(&["--pdf", "x.pdf", "--windowed", DECK]).status.code(),
        Some(2)
    );
    assert_eq!(run(&["--pdf"]).status.code(), Some(2));
    assert_eq!(run(&["--font", DECK]).status.code(), Some(2));
    let out = run(&["--present", "--font", "/nonexistent.ttf", DECK]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("/nonexistent.ttf"));
    assert_eq!(run(&["--check", "--zoom", DECK]).status.code(), Some(2));
    let out = run(&["--dump", "99", DECK]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("the deck has 9"));
    let out = run(&["/nonexistent/deck.md"]);
    assert!(!out.status.success());
}
