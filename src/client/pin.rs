//! The attach protocol as the other side reads it, pinned (the remote plan's S-C): a host and a client of different
//! builds — two machines' — agree when their `wire::V` does, so whatever one side reads of the other's bytes is here:
//! one of every message (`tarae_wire::check`'s samples, `Code`'s and `MouseKind`'s order in them), the escapes a frame
//! is written in (`Screen::present_drawn`, whole and patched — the client's resting redraw is the same writer), and the
//! escapes the client's `screen::Held` reads (a frame holding one it doesn't is shown, never held). A change here with
//! the same `V` is refused: `V` goes up first, then `TARAE_BLESS=1`.

use std::fmt::Write as _;
use std::path::Path;

use tarae_wire::{self as wire, Message, ToHost, ToInterface, check};

const PIN: &str = "src/snapshots/wire.txt";

/// A message's name — every variant here by name, so a new one doesn't compile until it's a sample (`check`) too.
fn host_name(m: &ToHost) -> &'static str {
    match m {
        ToHost::Hello(_) => "Hello",
        ToHost::Input { .. } => "Input",
        ToHost::Resize { .. } => "Resize",
        ToHost::Ack { .. } => "Ack",
        ToHost::Answer { .. } => "Answer",
        ToHost::Zone { .. } => "Zone",
        ToHost::Bye => "Bye",
        ToHost::Ping { .. } => "Ping",
    }
}

fn interface_name(m: &ToInterface) -> &'static str {
    match m {
        ToInterface::Welcome { .. } => "Welcome",
        ToInterface::Frame { .. } => "Frame",
        ToInterface::Ask { .. } => "Ask",
        ToInterface::Sound(_) => "Sound",
        ToInterface::Gone { .. } => "Gone",
        ToInterface::Pong { .. } => "Pong",
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

/// Bytes as text, an escape as `\e` and every other control character by its escape.
fn shown(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .chars()
        .map(|c| match c {
            '\x1b' => "\\e".to_string(),
            '\x07' => "\\a".to_string(),
            c if c.is_control() => c.escape_debug().to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Whether the client holds a frame with `seq` in it (`screen::last_writes` reads as `Held` does).
fn reads(seq: &str) -> bool {
    crate::screen::last_writes(format!("\x1b[1;1H{seq}x").as_bytes(), 6, 2).is_some()
}

/// The pin's text, as this build has it.
pub fn text() -> String {
    let mut s = String::from(
        "# The attach protocol as the other side reads it (src/client/pin.rs). A change here goes with wire::V up.\n",
    );
    writeln!(s, "V {}", wire::V).unwrap();
    s.push_str("\n## Messages: one of every kind and variant, its bytes (length, kind, body)\n");
    for m in check::to_host() {
        let mut b = Vec::new();
        wire::encode(&m, &mut b).unwrap();
        writeln!(s, "to host {:02x} {:<8} {}", m.kind(), host_name(&m), hex(&b)).unwrap();
    }
    for m in check::to_interface() {
        let mut b = Vec::new();
        wire::encode(&m, &mut b).unwrap();
        writeln!(s, "to interface {:02x} {:<8} {}", m.kind(), interface_name(&m), hex(&b)).unwrap();
    }
    // A frame with every style, colour form, a wide character, an erase and the cursor's shape, as tarae draws one
    let first = "\x1b[?2026h\x1b[?25l\x1b[1;1H\x1b[0;38;5;3;48;2;10;20;30;1;3mab\x1b[0;2;7;9m한\x1b[0;4mu\
                 \x1b[0;4:3m\x1b[58;5;9mc\x1b[0;38;2;1;2;3m\x1b[58:2::4:5:6m\x1b[4:2mz\x1b[2;1H\x1b[0;90;101mq\x1b[0m\
                 \x1b[K\x1b[2;4H\x1b[5 q\x1b[?25h\x1b[0m\x1b[?2026l";
    let then = "\x1b[?2026h\x1b[1;1H\x1b[0;38;5;3;48;2;10;20;30;1;3mab\x1b[0;2;7;9m한\x1b[0;4mu\x1b[0;4:3m\x1b[58;5;9mc\
                \x1b[0;38;2;1;2;3m\x1b[58:2::4:5:6m\x1b[4:2mz\x1b[2;1H\x1b[0;90;101mQ\x1b[0m\x1b[K\x1b[?25l\x1b[0m\x1b[?2026l";
    assert!(
        reads("") && crate::screen::last_writes(first.as_bytes(), 8, 2).is_some(),
        "the sample frame is read"
    );
    let mut screen = crate::screen::Screen::new();
    let whole = screen.present(first.as_bytes(), 8, 2).to_vec();
    let patch = screen.present(then.as_bytes(), 8, 2).to_vec();
    s.push_str("\n## A frame's escapes as the host's writer sends them (Screen::present_drawn)\n");
    writeln!(s, "whole  {}", shown(&whole)).unwrap();
    writeln!(s, "patch  {}", shown(&patch)).unwrap();
    s.push_str("\n## What the client's Held reads (a frame with any other escape is shown, not held)\n");
    let sgr: Vec<u32> = (0..=109).filter(|n| reads(&format!("\x1b[{n}m"))).collect();
    writeln!(s, "SGR {}", ranges(&sgr)).unwrap();
    let probes = [
        "\x1b[38;5;200m",
        "\x1b[48;2;1;2;3m",
        "\x1b[58;5;1m",
        "\x1b[38;3;1m",
        "\x1b[58:5:9m",
        "\x1b[58:2::1:2:3m",
        "\x1b[58:2:1:2:3m",
        "\x1b[4:0m",
        "\x1b[4:5m",
        "\x1b[38:5:1m",
        "\x1b[H",
        "\x1b[2H",
        "\x1b[;2H",
        "\x1b[1;2;3H",
        "\x1b[?1H",
        "\x1b[K",
        "\x1b[0K",
        "\x1b[1K",
        "\x1b[2K",
        "\x1b[4m\x1b[K",
        "\x1b[J",
        "\x1b[2J",
        "\x1b[A",
        "\x1b[?25h",
        "\x1b[?25l",
        "\x1b[?2026h",
        "\x1b[?2026l",
        "\x1b[?1049h",
        "\x1b[0 q",
        "\x1b[6 q",
        "\x1b]0;title\x07",
        "\x1bM",
        "\t",
        "\r",
        "\u{85}",
    ];
    for p in probes {
        writeln!(s, "{:<18} {}", shown(p.as_bytes()), if reads(p) { "read" } else { "not read" }).unwrap();
    }
    s
}

/// `1 2 3 7` as `1-3 7`.
fn ranges(ns: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < ns.len() {
        let mut j = i;
        while j + 1 < ns.len() && ns[j + 1] == ns[j] + 1 {
            j += 1;
        }
        out.push(if j > i { format!("{}-{}", ns[i], ns[j]) } else { ns[i].to_string() });
        i = j + 1;
    }
    out.join(" ")
}

/// The `V` a pin's text was written at.
fn v_of(text: &str) -> Option<u16> {
    text.lines().find_map(|l| l.strip_prefix("V ")).and_then(|v| v.trim().parse().ok())
}

/// Whether `actual` may replace the pin `expected` — Err: why not, in the words the test says.
pub fn judged(expected: &str, actual: &str, bless: bool) -> Result<bool, String> {
    if expected == actual {
        return Ok(false);
    }
    if v_of(expected) == v_of(actual) {
        return Err(format!(
            "what the other side reads changed with protocol {} as it was: a host or client of another build would \
             misread it. wire::V goes up first (crates/tarae-wire/src/msg.rs), then TARAE_BLESS=1 cargo test \
             the_wire_is_pinned",
            wire::V
        ));
    }
    if !bless {
        return Err(format!(
            "wire::V is {} now: TARAE_BLESS=1 cargo test the_wire_is_pinned writes {PIN}",
            wire::V
        ));
    }
    Ok(true)
}

#[test]
fn the_wire_is_pinned() {
    let actual = text();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(PIN);
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    let bless = std::env::var_os("TARAE_BLESS").is_some_and(|v| v == "1");
    match judged(&expected, &actual, bless) {
        Ok(true) => std::fs::write(&path, &actual).unwrap(),
        Ok(false) => {}
        Err(why) => panic!("{} differs — {why}\n\n{actual}", path.display()),
    }
}

/// Broken once, each way: a byte changed with the same `V` is refused, blessed or not; with `V` up it's written only
/// when blessed; the same text passes.
#[test]
fn a_change_to_the_wire_with_the_same_v_is_refused() {
    let pin = text();
    let changed = pin.replacen("Bye      0100000007", "Bye      0100000009", 1);
    assert_ne!(changed, pin, "the sample's Bye is in the pin");
    assert!(judged(&pin, &changed, true).unwrap_err().contains("wire::V goes up first"));
    let up = changed.replacen(&format!("V {}\n", wire::V), &format!("V {}\n", wire::V + 1), 1);
    assert!(judged(&pin, &up, false).unwrap_err().contains("TARAE_BLESS=1"));
    assert_eq!(judged(&pin, &up, true), Ok(true));
    assert_eq!(judged(&pin, &pin, false), Ok(false));
    // (an escape the writer sends differently, the same)
    let escapes = pin.replacen("\\e[?2026h", "\\e[?2026h\\e[2J", 1);
    assert!(judged(&pin, &escapes, true).is_err());
}
