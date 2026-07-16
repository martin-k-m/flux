//! Minimal, dependency-free styled logging.
//!
//! We emit ANSI escape codes directly. Colour is disabled when `NO_COLOR` is
//! set (see <https://no-color.org>) or when stdout is redirected is not
//! detected here — we keep it simple and honour `NO_COLOR` only.

use std::sync::atomic::{AtomicBool, Ordering};

static COLOR: AtomicBool = AtomicBool::new(true);

/// Initialise colour support. Call once at startup.
pub fn init() {
    let enabled = std::env::var_os("NO_COLOR").is_none();
    COLOR.store(enabled, Ordering::Relaxed);
}

fn color_enabled() -> bool {
    COLOR.load(Ordering::Relaxed)
}

fn paint(code: &str, s: &str) -> String {
    if color_enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    paint("1", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn cyan(s: &str) -> String {
    paint("36", s)
}

// Status glyphs used throughout the UI.
pub const CHECK: &str = "\u{2713}"; // ✓
pub const CROSS: &str = "\u{2717}"; // ✗
pub const ARROW: &str = "\u{25b6}"; // ▶
pub const DOT: &str = "\u{2022}"; // •

/// A top-of-command banner, e.g. `Flux v0.2`.
pub fn banner(subtitle: &str) {
    println!("{} {}", bold(&cyan("Flux")), dim(subtitle));
}

/// An informational key/value line: `Language: Rust`.
pub fn field(key: &str, value: &str) {
    println!("  {} {}", dim(&format!("{key}:")), value);
}

/// A section heading such as `Pipeline:`.
pub fn heading(text: &str) {
    println!("\n{}", bold(text));
}

pub fn ok_line(text: &str) {
    println!("  {} {}", green(CHECK), text);
}

pub fn fail_line(text: &str) {
    println!("  {} {}", red(CROSS), text);
}

pub fn info_line(text: &str) {
    println!("{text}");
}
