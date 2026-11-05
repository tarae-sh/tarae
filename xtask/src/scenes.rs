//! `cargo xtask scenes [--out DIR] [scene …]` — tarae's main surfaces as shots, each in both themes (meok and
//! hanji) at 80×24 and 160×45: `<DIR>/<scene>-<dark|light>-<80|160>.png` (default `target/scenes`; with
//! `--transparent`, on the transparent themes, `…-transparent.png`); every scene
//! when none is named, `--list` names them. For a change to the screen: the same scenes before and after, and
//! `--compare BEFORE AFTER` says which shots differ, pixel for pixel, and where.
//!
//! Each shot gets a demo crate of its own — a git repository in a fresh temp folder: one commit, one file changed
//! since, one new — so what one shot saves never shows in the next. Its config is empty but for `editor.lsp =
//! false` (when a server answers would change the picture from run to run) and `llm.command` = this xtask as a
//! stand-in claude, `scenes --claude <script>`: stream-json both ways, a scripted turn, no network, no account.
//! The shot profile's tarae is built once. Each shot runs tarae on a pty (where terminals may be opened).
//!
//! `--record` judges a scene in motion instead: its keys run as they do for a still (`@2` a pause a person would see,
//! `@beat:<caption>` a moment said under it), every frame kept from its `@rec` (none: from the start), in docshots'
//! isolation (homes and clock of its own, `none_of_yours` on every frame) — at 120×34 unless `--size`, both themes
//! unless `--theme dark|light` — into `<DIR>/<scene>-<theme>-<cols>/` (default `target/rec`): `rec.json` in the
//! homepage player's format (`recording.rs`, the hero's encoder), `sheet.png` (a frame every ~2 s and at each wait's
//! end and beat, the time under each; past six frames also `sheet-01.png` …, six a page) and `play.html` (one file
//! that plays it, nothing fetched — on a phone too).
//!
//! `--video` makes the take a short video (`video.rs`): 1920×1080 at 30 fps, its `@beat:` captions in a band under the
//! screen, a title card and an end card with the seal — `<DIR>/<scene>-<theme>-video/<scene>.mp4` through ffmpeg, and
//! `sheet.png` of the video's own frames. `--video --shorts` makes it a YouTube Short instead: 1080×1920 upright, the
//! scene at 80 columns and the rows that fill the window (`video::short_size`), a camera over it from the scene's
//! `@zoom:<target>` lines (`camera.rs`), its captions a phrase at a time in a band under the screen, `@note:<words> →
//! <target>` (`captions.rs`), and tarae's own keyboard sound for the keys typed (`@type:<text>` types a character at
//! a time), `@sound:stamp`, `@sound:settle` and Claude's writing as the reveal plays it (between `@sound:writing` or
//! `@sound:landing` and `@sound:still`); a `@punch` opens it on the take from there — into
//! `<DIR>/<scene>-<theme>-shorts/`. The wide video keeps its captions and ignores `@zoom`, `@note`, `@sound` and
//! `@punch`.

use std::io::{self, BufRead, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use crate::shot::{self, Setup};
use crate::{Result, root};
use crate::{recording, video};
use docshots::{Bg, Named, fixed_clock, homes, none_of_yours, now};

// (`scenes --docs`: the docs' pictures, each from a scene here)
pub(crate) mod docshots;

const USAGE: &str = "usage: cargo xtask scenes [--out DIR] [--size COLSxROWS] [--transparent] [--bin TARAE] [scene …] | scenes --record|--video [--shorts]|--wire scene … [--size COLSxROWS] [--theme dark|light] [--out DIR] [--bin TARAE] | scenes --tint scene … [--size COLSxROWS …] [--theme dark|light] [--out DIR] [--bin TARAE] | scenes --list | scenes --compare A B";

/// A recording's size unless `--size` says (the polish week's: its flows judged at 120×34).
const RECORD_SIZE: (u16, u16) = (120, 34);

/// One surface: the stand-in claude's script, the files tarae opens, the keys sent (as `cargo xtask shot`
/// takes them: `@1` waits a second, `@?text` until `text` is on screen, `@20?text` at most 20 s).
#[derive(Debug)]
struct Scene {
    name: &'static str,
    claude: &'static str,
    files: &'static [&'static str],
    keys: &'static [&'static str],
}

/// The `:sh` lines in `keys` whose next key after their `enter` waits a fixed time, not on text (#342). `:sh` is a job:
/// what follows it waits on its end — the line it prints, said on the status line (`&& echo checkout made`,
/// `@20?checkout made`), or what tarae draws of what it did — never on a guess of how long it takes.
#[cfg(test)]
pub(crate) fn sh_waits_by_time<S: AsRef<str>>(keys: &[S]) -> Vec<String> {
    let on_text = |k: &str| {
        let w = k.strip_prefix('@').and_then(|k| k.split_once('?'));
        w.is_some_and(|(n, t)| n.chars().all(|c| c.is_ascii_digit() || c == '.') && !t.is_empty())
    };
    let keys: Vec<&str> = keys.iter().map(AsRef::as_ref).collect();
    (keys.windows(4))
        .filter(|w| w[0] == ":" && w[1].starts_with("sh ") && w[2] == "\r" && !on_text(w[3]))
        .map(|w| format!("{:?} then {:?}", w[1], w[3]))
        .collect()
}

const MAIN: &[&str] = &["src/main.rs"];
const WIDE: &[&str] = &["src/wide.rs"];
/// Focus A1c's file (`SPOOL_RS`).
const SPOOL: &[&str] = &["src/spool.rs"];

/// Motion §2's scenes: the chat, a question — every frame kept from its `enter` through the edit's landing (~1.1 s)
/// until it's dry — and the picture then, before `six`'s turn ends (`TARAE_MOTION_AT` pins the drying's step).
const WET_KEYS: &[&str] = &["@1", " ", "l", "@1", "keep the first strand", "@~2600", "\r", "@?in one go"];

/// Motion §3's scenes: the chat, follow off (`C-f`: the chat stays open), a question, every frame kept from its `enter` —
/// 1 s of prose, 2.4 s of a think and its landing, 3.2 s of prose, a stall and prose again; the think with follow on
/// (the follow card's head); 4.2 s of prose, a read, the wait for the next answer and the answer.
const SHIMMER_KEYS: [&[&str]; 5] = [
    &["@1", " ", "l", "@1", "\x06", "how is the map filled?", "@~1000", "\r", "@3"],
    &["@1", " ", "l", "@1", "\x06", "how is the map filled?", "@~2400", "\r", "@3"],
    &["@1", " ", "l", "@1", "\x06", "how is the map filled?", "@~3200", "\r", "@3"],
    &["@1", " ", "l", "@1", "how is the map filled?", "@~2400", "\r", "@3"],
    &["@1", " ", "l", "@1", "\x06", "how is the map filled?", "@~4200", "\r", "@3"],
];

/// The working sign's scenes (#159): every frame from the question's `enter` through one turn that goes through every
/// state (`shimmer-all`, ~6.4 s) — the chat open, follow off (`C-f`); following, the chat minimized (the code's cursor
/// beside the follow card and the now line).
const WORKING_KEYS: [&[&str]; 5] = [
    &["@1", " ", "l", "@1", "\x06", "how is the map filled?", "@~7500", "\r", "@1"],
    &["@1", " ", "l", "@1", "how is the map filled?", "@~7500", "\r", "@1"],
    // (following, the code given the keys — `esc esc`, the chat minimized: its block cursor in sight beside the cell)
    &[
        "@1",
        " ",
        "l",
        "@1",
        "how is the map filled?",
        "\r",
        "@0.3",
        "\x1b",
        "\x1b",
        "@0.2",
        "@~6000",
        "\x1b",
        "@1",
    ],
    &["@1", " ", "l", "@1", "\x06", "run the tests", "\r", "@?Its reason", "@1"],
    &["@1", " ", "l", "@1", "run the tests", "\r", "@?Its reason", "@1"],
];

/// `shimmer-sign`'s turn (`Turn::sign_turn`, `sign.rs`): each phase and how long it lasts, in milliseconds — the
/// stall's first second still the stream's (tarae's `shimmer::STILL_AFTER`), the read's wait from its call to the next
/// answer's first words.
pub(crate) const SIGN_TURN: [(&str, u64); 6] = [
    ("start", 1000),
    ("think", 1600),
    ("prose", 2000),
    ("stall", 2400),
    ("read's wait", 2000),
    ("prose", 2000),
];

/// `shimmer-sign`'s prose as a real answer streamed (`cargo xtask stream`, kept 2026-10-04): each text delta's gap
/// after the one before, in ms, and its size — about 50 ms apart, then a quiet of 0.15–0.5 s and a lump of several at once.
pub(crate) const SIGN_STREAM: [(u64, usize); 56] = [
    (0, 4),
    (57, 13),
    (35, 9),
    (53, 24),
    (53, 24),
    (48, 17),
    (48, 10),
    (59, 6),
    (479, 23),
    (0, 29),
    (0, 5),
    (0, 12),
    (0, 8),
    (0, 8),
    (0, 22),
    (0, 24),
    (0, 17),
    (15, 27),
    (47, 17),
    (51, 21),
    (387, 8),
    (0, 17),
    (0, 3),
    (0, 24),
    (0, 17),
    (0, 13),
    (0, 18),
    (13, 25),
    (50, 4),
    (52, 24),
    (53, 16),
    (48, 12),
    (285, 27),
    (0, 25),
    (0, 20),
    (0, 12),
    (0, 22),
    (13, 10),
    (50, 31),
    (50, 25),
    (49, 4),
    (50, 5),
    (49, 9),
    (50, 7),
    (241, 10),
    (1, 14),
    (1, 32),
    (0, 5),
    (9, 30),
    (54, 20),
    (46, 19),
    (50, 5),
    (48, 6),
    (52, 5),
    (47, 16),
    (51, 22),
];

/// #163's scenes: a question answered in prose, the chat open (follow off); then wider by you, `C-w >` ten times from
/// its empty input (as far as the code's room lets it).
const MEASURE_KEYS: [&[&str]; 2] = [
    &["@1", " ", "l", "@1", "\x06", "how is the map filled?", "\r", "@?ready", "@1"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "\x06",
        "how is the map filled?",
        "\r",
        "@?ready",
        "@1",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "\x17",
        ">",
        "@1",
    ],
];

/// What Claude is doing now on the status line (`now.rs`), a turn running the tests: `C-f` in the empty chat keeps it
/// open; following, `esc esc` gives the code the keys and the chat minimizes; `j` there pauses follow; the card's turn,
/// `C-w h` to the code; the mouse over the line, following.
const NOW_KEYS: [&[&str]; 5] = [
    &["@1", " ", "l", "@1", "\x06", "keep the first strand on a repeat", "\r", "@10?unit tests", "@2"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        "@2",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        "@1",
        "j",
        "@2",
    ],
    &["@1", " ", "l", "@1", "keep the first strand on a repeat", "\r", "\x17", "h", "@10?Its reason", "@2"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        "@1",
        "@hover?running cargo test",
        "@1",
    ],
];

/// Focus B1's scenes: following (its default), the turn held reading, running, then waiting on its card — each with the
/// chat brought back beside the code (`esc esc`, `space l`: following minimizes it as the turn starts), then minimized,
/// the keys in the code (`esc esc`).
const B1_KEYS: [&[&str]; 7] = [
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?thread.rs",
        "\x1b",
        "\x1b",
        " ",
        "l",
        "@2",
    ],
    &["@1", " ", "l", "@1", "keep the first strand on a repeat", "\r", "@10?thread.rs", "\x1b", "\x1b", "@2"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        " ",
        "l",
        "@2",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        "@2",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "\x1b",
        "\x1b",
        " ",
        "l",
        "@10?Its reason",
        "@2",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "\x1b",
        "\x1b",
        "@10?Its reason",
        "@2",
    ],
    &["@1", " ", "l", "@1", "keep the first strand on a repeat", "\r", "\x1b", "\x1b", "@10?writing", "@2"],
];

/// #267's keys: following, the turn running its tests, the chat minimized with the keys — then `$more` (the follow card's
/// left line pressed and dragged; `C-w =`).
macro_rules! f267_keys {
    ($($more:expr),* $(,)?) => {
        &["@1", " ", "l", "@1", "keep the first strand on a repeat", "\r", "@10?cargo test", $($more,)* "@2"]
    };
}

/// #267's scenes: the follow card dragged wide by its left line (to the code's `threads.insert`), dragged narrow (12
/// cells in), and the wide one back to automatic by `C-w =` from the minimized chat.
const F267_KEYS: [&[&str]; 3] = [
    f267_keys!("@press?│ A repeated", "@drag?threads.insert", "@release?main.rs"),
    f267_keys!("@press?│ A repeated", "@drag+12?│ A repeated", "@release?main.rs"),
    f267_keys!("@press?│ A repeated", "@drag?threads.insert", "@release?main.rs", "@1", "\x17", "="),
];

/// Focus A1's keys: the question, its turn's end, the keys in the code, then `$more`.
macro_rules! a1_keys {
    ($($more:expr),* $(,)?) => {
        &["@1", " ", "l", "@1", "what does the loop print?", "\r", "@30=done", "@2", $($more,)* "@1"]
    };
}

/// Focus A1's scenes: following (its default), a question, the turn's end (its last words: the chat comes back with
/// them), the keys in the code (`esc esc`) — the chat left open, or hidden (`space L`, #270: its conversation going on); then the line ends, or the
/// answer's card: `gg]n` (the place's one note), `]n` (the last read's first line). `A1_LINE`: focus-line's turn —
/// `space n`'s question at line 9 (the chat open, the keys back in the code), answered in its thread: the card there,
/// no other note.
const A1_KEYS: [&[&str]; 6] = [
    a1_keys!(),
    a1_keys!("gg]n"),
    a1_keys!(" ", "L"),
    a1_keys!(" ", "L", "gg]n"),
    a1_keys!("]n"),
    a1_keys!(" ", "L", "]n"),
];
/// `a1-flow`'s: its place's card — the chat open, the view left on the last read (`thread.rs`): `gg]n]n`, its note,
/// then round to the loop's; closed, the loop's file in front (`:o`).
const A1_FLOW: [&[&str]; 2] = [a1_keys!("gg]n]n"), a1_keys!(" ", "L", ":", "o src/main.rs", "\r", "gg]n")];
/// Focus E1's scenes: a turn with reads, a search, an edit and a `run_tests`, its review answered, the turn over and the
/// keys back in the code — then `$more`.
macro_rules! e1_keys {
    ($($more:expr),*) => {
        &[
            "@1",
            " ",
            "l",
            "@1",
            "the length test fails, fix it",
            "\r",
            "@20?Change 1 of 1",
            "@1",
            "y",
            "@30=done",
            "@1",
            $($more),*
        ]
    };
}
// (the tree: each file's roles · the outline's · the chat closed, the code wide · a stop stepped back to, lit on the map ·
// a folder folded)
const E1_TREE: &[&str] = e1_keys!(" ", "e", "@?main.rs", "@1");
const E1_OUTLINE: &[&str] = e1_keys!(" ", "o", "@1");
const E1_CLOSED: &[&str] = e1_keys!(" ", "L", "@1", " ", "e", "@?main.rs", "@1");
const E1_HAND: &[&str] = e1_keys!(" ", "e", "@?main.rs", "@1", "\x1b", "[", "r", "[", "r", "[", "r", "@1");
const E1_FOLDED: &[&str] = e1_keys!(" ", "e", "@?main.rs", "@1", "g", "@0.5", "\r", "@1");

/// Focus F0's keys: `F0_ASKS` to `f0-saw`, its edit taken, its turn's end, the keys in the code (`esc esc`), then `$more`.
macro_rules! f0_keys {
    ($($more:expr),* $(,)?) => {
        &["@1", " ", "l", "@1", "why is a strand too long?", "\r", "@20?Change 1 of 1", "@1", "y", "@30=done",
          "@2", "@20!walk-through", $($more,)* "@1"]
    };
}
/// `f0_keys`, then a second question to its turn's end, then `$more`.
macro_rules! f0_two_keys {
    ($($more:expr),* $(,)?) => {
        &["@1", " ", "l", "@1", "why is a strand too long?", "\r", "@20?Change 1 of 1", "@1", "y", "@30=done",
          "@2", " ", "l", "@1", "does anything else read it?", "\r", "@30=done", "@2", "@20!walk-through",
          "q", " ", "L", ":", "o src/thread.rs", "\r", "@1", $($more,)* "@1"]
    };
}
/// Focus F0's scenes: the turn's end, its card open · the card closed, the line ends · the chat closed too · an edit of
/// yours since the read · a second answer: an older answer's card, then a line both answers name, the earlier unfolded.
const F0_CARD: &[&str] = f0_keys!();
const F0_LINES: &[&str] = f0_keys!("q");
const F0_WIDE: &[&str] = f0_keys!("q", " ", "L");
const F0_EDITED: &[&str] = f0_keys!("q", "4G", "ix", "\x1b", "j");
const F0_OLDER: &[&str] = f0_two_keys!("4G", "\r");
const F0_LINE: &[&str] = f0_two_keys!("10G", "\r", "\r");
/// Focus A1c's keys: a real answer's shape (`a1c`) — a question nine rows long, its answer read in `src/spool.rs`, the
/// keys in the code at its turn's end (#388: its places flags, the answer the column's), then `$more`.
macro_rules! a1c_keys {
    ($($more:expr),* $(,)?) => {
        &["@1", " ", "l", "@1", A1C_ASKED, "\r", "@30=done", "@2", "\x1b", "\x1b", "@20!walk-through",
          $($more,)* "@1"]
    };
}
/// Focus A1c's question: one first line, nine rows of the card's width or more.
const A1C_ASKED: &str = "I've been reading src/spool.rs and I can't tell where the order of the strands goes wrong: the doc \
    comment on `Spool` says the spool keeps its order, a strand wound in the order it came, but when I wind a queue of ten \
    strands the wound list comes out in a different order every time I change the knots, and the strands that slipped seem \
    to end up in the middle rather than at the end, so could you walk me through `wind` from the start to the end, tell me \
    which lines change the order and why, whether the sort at the end is meant to be there or is a leftover, and what the \
    smallest change would be that keeps the order the doc comment promises without breaking the retie of a slipped strand? \
    I'd rather not lose the progress lines it prints either, they're how I watch a long spool wind.";
/// Focus A1c's scenes: the turn's end (its flow's card, past its cap) · `]n` through every note of that answer, the doc
/// comment's among them · `enter` on the cut card (the chat at its paragraph) · the doc comment's own `enter` · the chat
/// closed, the code wide.
const A1C_END: &[&str] = a1c_keys!();
const A1C_N1: &[&str] = a1c_keys!("]n");
const A1C_N2: &[&str] = a1c_keys!("]n", "@0.5", "]n");
const A1C_N3: &[&str] = a1c_keys!("]n", "@0.5", "]n", "@0.5", "]n");
const A1C_N4: &[&str] = a1c_keys!("]n", "@0.5", "]n", "@0.5", "]n", "@0.5", "]n");
const A1C_N5: &[&str] = a1c_keys!("]n", "@0.5", "]n", "@0.5", "]n", "@0.5", "]n", "@0.5", "]n");
const A1C_READ: &[&str] = a1c_keys!("21G", "\r");
const A1C_DOC: &[&str] = a1c_keys!("5G", "\r");
const A1C_WIDE: &[&str] = a1c_keys!(" ", "L");
/// #273's scenes, on A1c's turn: no follow card stays for the walk-through's stop after it — your cursor in the stop's
/// lines (`40G`), `space n` there, `[r` (a replay's stop has the card), `[r` then `j` off it, `space F` after the turn.
const F273_STOP: &[&str] = a1c_keys!("40G");
const F273_ASK: &[&str] = a1c_keys!("40G", " ", "n");
const F273_REPLAY: &[&str] = a1c_keys!("[r");
const F273_REPLAY_OFF: &[&str] = a1c_keys!("[r", "@1", "j");
const F273_FOLLOW: &[&str] = a1c_keys!(" ", "F");
/// The flow plan's piece 1, the reader, on A1c's answer: `space L` from the line of the list's paragraph (21) — hidden,
/// then shown again · the chat floated by `C-w e`, docked again by it · floating, `space L` hides it and brings it back
/// at line 21's paragraph · floating and hidden, the cut card's `enter reads on` (#274). (A1c's `src/spool.rs` is
/// theirs too — `run_files`.)
const FLOW1_L: &[&str] = a1c_keys!("21G", " ", "L", "@0.5", " ", "L");
const FLOW1_FLOAT: &[&str] = a1c_keys!(" ", "l", "@0.5", "\x17", "e");
const FLOW1_DOCK: &[&str] = a1c_keys!(" ", "l", "@0.5", "\x17", "e", "@0.5", "\x17", "e");
// (hidden by `space L` from its view mode — `esc`'s count to close it depends on where that lands, a selection let go
// first; from its input `space L` is typed)
const FLOW1_FLOAT_L: &[&str] =
    a1c_keys!(" ", "l", "@0.5", "\x17", "e", "@0.5", "\x1b", " ", "L", "@0.5", "21G", " ", "L");
const FLOW1_FLOAT_READ: &[&str] =
    a1c_keys!(" ", "l", "@0.5", "\x17", "e", "@0.5", "\x1b", " ", "L", "@0.5", "21G", "\r");
/// The baseline's other two answers in the floating reader, read by hand: `a1-none` (no place: its words on the card at the
/// last read, `]n`) and `a1-flow` (one paragraph at a line: `gg]n]n`), the chat floated and hidden, then `space L` from
/// that line.
const FLOW1_NONE_READ: &[&str] =
    a1_keys!(" ", "l", "@0.5", "\x17", "e", "@0.5", "\x1b", " ", "L", "@0.5", "]n", " ", "L");
const FLOW1_FLOW_READ: &[&str] =
    a1_keys!(" ", "l", "@0.5", "\x17", "e", "@0.5", "\x1b", " ", "L", "@0.5", "gg]n]n", " ", "L");
/// The flow plan's piece 2, the ask line, on A1c's file: `space l` on line 21 · on three lines selected · a quote of two
/// (`space >`) · typing · sent (the turn under way) · its answer · `pageup` (the reader) · `space L` after the answer (the
/// reader's foot: usage, `warm until`) · an empty chat with a conversation to continue · two sessions, the chat hidden
/// (the strip) and the ask line over them.
const FLOW2_ASKED: &str = "why does wind sort the strands at the end?";
const FLOW2_LINE: &[&str] = &["@1", "21G", " ", "l", "@1"];
const FLOW2_SEL: &[&str] = &["@1", "21G", "x", "x", "x", " ", "l", "@1"];
const FLOW2_QUOTE: &[&str] = &["@1", "21G", "x", "x", " ", ">", "@1"];
const FLOW2_TYPING: &[&str] = &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "@1"];
const FLOW2_SENT: &[&str] = &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "\r", "@2"];
const FLOW2_ANSWERED: &[&str] =
    &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "\r", "@20?wind is the whole", "@2", "\x1b", "\x1b", "@1"];
const FLOW2_GROW: &[&str] = &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "@0.5", "\x1b[5~", "@1"];
/// The flow plan's piece 3, the minimize gone: A1c's answer come while the chat is out of sight — the status line's
/// `◂ new · space L`, its card at the line · then `space L` off that line: the docked column at what's new, `── new ──`
/// over it · the answer's `¶` pressed (`flow3-pilcrow`): the column at its paragraph, the keys in the code, no card · an
/// answer no place holds (`flow3-none`): the column shown by itself, the keys in the code · an error said only in the
/// conversation (`flow3-error`), the same.
const FLOW3_NEW: &[&str] = &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "\r", "@30=done", "@2"];
const FLOW3_PILCROW: &[&str] =
    &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "\r", "@30=done", "@2", "@press?¶", "@1"];
const FLOW3_READ: &[&str] =
    &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "\r", "@30=done", "@2", "g", "g", " ", "L", "@1"];
/// The follow card with the keys (#267, the flow plan's piece 3): a turn running the tests, asked from the ask line —
/// `C-w w` gives the card the keys (its frame lit, its sizes said on its bottom line), then `C-w >` twice widens it.
const FLOW3_FOLLOW_KEYS: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "keep the first strand on a repeat",
    "\r",
    "@10?unit tests",
    "@1",
    "\x17",
    "w",
    "@1",
];
const FLOW3_FOLLOW_WIDER: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "keep the first strand on a repeat",
    "\r",
    "@10?unit tests",
    "@1",
    "\x17",
    "w",
    "@0.3",
    "\x17",
    ">",
    "@0.3",
    "\x17",
    ">",
    "@1",
];
/// panes4's polish decision 2 in piece 3: a turn running with the column in sight (`space L`, asked there) — the follow
/// card only its head, the turn's words the column's.
const FLOW3_FOLLOW_HEAD: &[&str] =
    &["@1", " ", "L", "@1", "keep the first strand on a repeat", "\r", "@10?unit tests", "@1"];
const FLOW3_ALONE: &[&str] = &["@1", " ", "l", "@1", "why a hash map?", "\r", "@30=done", "@2"];
/// #389's scenes: asked in the column, the answer in, its view mode (`esc`), then `keys` there.
macro_rules! chatlink_keys {
    ($($k:expr),*) => {
        &["@1", " ", "L", "@1", "where is the length summed?", "\r", "@30=done", "@1", "\x1b", "@0.5", $($k,)* "@1.5"]
    };
}
/// The flow plan's piece 5, steering: asked in the column, Claude at work (its read done, the turn held), a second message
/// queued (`steer-queued`: `tab steer` beside `↑ edit queued`), then `tab` (`steer-steering`: the steering line), then
/// Claude's next tool answer carrying it (`steer-steered`: its row where Claude heard it). `steer-ask`: the ask line at
/// work, `enter queue  tab steer` on its foot.
const STEER_QUEUED: &[&str] = &[
    "@1",
    " ",
    "L",
    "@1",
    "keep the first strand on a repeat",
    "\r",
    "@10?Let me look",
    "@1",
    "use thread.rs instead",
    "\r",
    "@1",
];
const STEER_STEERING: &[&str] = &[
    "@1",
    " ",
    "L",
    "@1",
    "keep the first strand on a repeat",
    "\r",
    "@10?Let me look",
    "@1",
    "use thread.rs instead",
    "\r",
    "\t",
    "@1",
];
const STEER_STEERED: &[&str] = &[
    "@1",
    " ",
    "L",
    "@1",
    "keep the first strand on a repeat",
    "\r",
    "@10?Let me look",
    "@1",
    "use thread.rs instead",
    "\r",
    "\t",
    "@30?Reading src",
    "@1",
];
const STEER_ASK: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "keep the first strand on a repeat",
    "\r",
    "@10?Let me look",
    "@1",
    " ",
    "l",
    "@1",
    "use thread.rs instead",
    "@1",
];
const FLOW2_FOOT: &[&str] =
    &["@1", "21G", " ", "l", "@1", FLOW2_ASKED, "\r", "@30=done", "@2", " ", "L", "@1"];
const FLOW2_CONTINUE: &[&str] = &[
    "@1",
    "21G",
    " ",
    "l",
    "@1",
    FLOW2_ASKED,
    "\r",
    "@30=done",
    "@2",
    " ",
    "l",
    "@1",
    "\x0c",
    "\x0c",
    "@1",
    "\x1b",
    "@1",
    " ",
    "l",
    "@1",
];

/// #269's scenes, on A1c's file with no answer: a drag over the folded doc comment (lines 3–7), one from the code above
/// into it, `x` on its first line and four times, INSERT on its line 5.
const F269_DRAG: &[&str] = &["@1", "@press?A spool:", "@drag?goes back", "@1", "@release?goes back", "@1"];
const F269_INTO: &[&str] = &["@1", "@press?use std", "@drag?goes back", "@1", "@release?goes back", "@1"];
const F269_X: &[&str] = &["@1", "3G", "x", "@1"];
const F269_XS: &[&str] = &["@1", "3G", "x", "x", "x", "x", "@1"];
const F269_INSERT: &[&str] = &["@1", "5G", "i", "@1"];
/// … a drag back from its last row onto the code above, `x` `d` on its first line, then `gg` off it. (A double
/// press can't be shot: the shot waits 0.35 s after each press, past `DOUBLE_CLICK`.)
const F269_BACK: &[&str] = &["@1", "@press?goes back", "@drag?use std", "@1", "@release?use std", "@1"];
const F269_D: &[&str] = &["@1", "3G", "x", "d", "@1"];
const F269_DG: &[&str] = &["@1", "3G", "x", "d", "@1", "gg", "@1"];
/// Focus C1's: a question typed on three lines, nothing else open · the chat beside it · a card a line near the screen's
/// foot (the file pasted on, so both sizes have one there).
const C1_NEW: &[&str] = &["@1", "8G", "x", "x", "x", " ", "n", "@1", "why a HashMap and not a Vec?", "@1"];
const C1_NEW_CHAT: &[&str] =
    &["@1", " ", "l", "@1", "\x1b", "8G", "x", "x", " ", "n", "@1", "why a HashMap?", "@1"];
const C1_BOTTOM: &[&str] = &[
    "@1",
    "%",
    "y",
    "p",
    "p",
    "p",
    "p",
    "\x1b",
    "60G",
    "zb",
    "@1",
    " ",
    "n",
    "@1",
    "what does this line do?",
    "@1",
];

const A1_LINE: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "\x1b",
    "@0.5",
    "9",
    "G",
    "x",
    " ",
    "n",
    "@1",
    "why a Vec here?",
    "\r",
    "@20?It's a map",
    "@2",
    "\x1b",
    "\x1b",
    "9G",
    "@1",
];
/// Focus A1b's: `a1-flow`'s card, the chat closed, the view scrolled a row at a time (`zj`) until the screen's top cuts the
/// card — its top line gone, the rows in sight drawn.
const A1B_CUT: &[&str] =
    a1_keys!(" ", "L", ":", "o src/main.rs", "\r", "gg]n", "gg", "zjzjzjzjzjzjzjzjzjzjzjzjzj");
/// Focus A1b step 2's keys: two questions to `a1b-two`, each to its turn's end, the keys in the code (`esc esc`), then
/// `$more`.
macro_rules! a1b_two_keys {
    ($($more:expr),* $(,)?) => {
        &["@1", " ", "l", "@1", "what does the loop print?", "\r", "@30=done", "@2", " ", "l", "@1", "why a hash map?",
          "\r", "@30=done", "@2", $($more,)* "@1"]
    };
}

/// Focus A1b step 2's scenes: the turn's end with its card opened by itself — the loop's line's two answers newest first,
/// the earlier folded; the insert's line only the first answer names (a faint `¶`) — the chat open, then closed; the
/// earlier unfolded (`enter` on the loop's line); a new conversation (`C-l`: they go) and the first continued (`C-p`,
/// `enter`: they're back, as after `:restart`'s `--resume`), its line ends, then the loop's card by `enter`.
const A1B_TWO: [&[&str]; 5] = [
    a1b_two_keys!(),
    a1b_two_keys!(" ", "L"),
    a1b_two_keys!(" ", "L", "11G", "\r"),
    a1b_two_keys!(" ", "l", "@1", "\x0c", "@1", "\x10", "@2", "\r", "@2", "\x1b", "\x1b", " ", "L", "9G"),
    a1b_two_keys!(
        " ", "l", "@1", "\x0c", "@1", "\x10", "@2", "\r", "@2", "\x1b", "\x1b", " ", "L", "11G", "\r"
    ),
];

/// #388 step 2's scenes, the chat whole: both answers as the turn ends (where step 3 of focus A1b folded the first to
/// stubs), and scrolled up to the first; `]n` with the column in sight (the newest answer's first flag, then on to the
/// older answer's: its paragraph whole, `▎` beside it); `space N` with flag rows.
const WHOLE_OLDER: &[&str] = a1b_two_keys!();
const WHOLE_OLDER_UP: &[&str] = a1b_two_keys!("@wheel-up*6?replay its", "@1");
/// (`C-w h`: the keys to the code, the column staying)
const WHOLE_NEXT: &[&str] = a1b_two_keys!("\x17", "h", "]n", "@1");
const WHOLE_NEXT_OLDER: &[&str] = a1b_two_keys!("\x17", "h", "]n", "@1", "]n", "@1", "]n", "@1");
const WHOLE_LIST: &[&str] = a1b_two_keys!("\x17", "h", " ", "N", "@1");

/// #388 step 1's scenes, on `a1b-two`'s two answers (lines 10 and 11 flagged, 11 by both): the flags lit, their paragraphs
/// in the column · dim, the column hidden · the mouse on line 11's `¶` (a row an answer) · that `¶` pressed with the column
/// hidden (shown at the newest's paragraph, the keys in the code) · a note of yours on the same line, kept for later.
const FLAG_LIT: &[&str] = a1b_two_keys!();
const FLAG_DIM: &[&str] = a1b_two_keys!("\x17", "h", " ", "L");
const FLAG_HOVER: &[&str] = a1b_two_keys!("@hover+13?&threads {", "@1");
/// #391: the pointer onto line 11's `¶`, its card only after the rest (every frame of 0.9 s kept), then up onto line 10's
/// (its card under line 11's covers the lines below) — handed off at once (0.4 s kept).
const FLAG_REST: &[&str] =
    a1b_two_keys!("@~900", "@hover+13?&threads {", "@1", "@~400", "@hover+10?first);", "@1");
const FLAG_PRESS: &[&str] = a1b_two_keys!("\x17", "h", " ", "L", "@1", "@press+13?&threads {", "@1");
const FLAG_NOTE: &[&str] =
    a1b_two_keys!("\x17", "h", " ", "L", "11G", " ", "n", "@1", "keep the first one?", "\t", "@2");

/// #271/#272's keys: `a1b_two_keys!` on `f271` (each answer after a thinking summary), the chat's view mode at its top.
macro_rules! f271_keys {
    ($($more:expr),* $(,)?) => {
        a1b_two_keys!(" ", "l", "@1", "\x1b", "@1", "gg", $($more,)*)
    };
}
/// #271's scene: `x` on the line that names the first answer's place. #272's: the mouse on the first and on the last row
/// of the first answer's thought, folded to two rows (the chat's view mode left by `esc`: no row in hand).
const F271_X: &[&str] = f271_keys!("/", "main.rs:11", "\r", "\x1b", "x");
const F272_FIRST: &[&str] = f271_keys!("\x1b", "@1", "@hover+4?Checking how");
const F272_LAST: &[&str] = f271_keys!("\x1b", "@1", "@hover?…");
const F272_HAND: &[&str] = f271_keys!("/", "Checking", "\r", "\x1b");

/// Focus A1b's: `A1_LINE`'s answer, `]n` to its flag (#388: the column at its paragraph).
const A1B_LINE: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "\x1b",
    "@0.5",
    "9",
    "G",
    "x",
    " ",
    "n",
    "@1",
    "why a Vec here?",
    "\r",
    "@20?It's a map",
    "@2",
    "\x1b",
    "\x1b",
    "9G",
    "]n",
    "@1",
];

/// #388 step 3's: `space n` on two lines — the ask line, the lines quoted, a question typed; and `A1_LINE`'s answer listed
/// by `space N` — the asked line's row its place and question, the row in hand's card the answer's opening.
const S3_ASK: &[&str] = &["@1", "10G", "x", "x", " ", "n", "@1", "why a HashMap here?", "@1"];
const S3_LIST: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "\x1b",
    "@0.5",
    "9",
    "G",
    "x",
    " ",
    "n",
    "@1",
    "why a Vec here?",
    "\r",
    "@20?It's a map",
    "@2",
    "\x1b",
    "\x1b",
    " ",
    "N",
    "@1",
];

/// Motion §4's scenes: the stand-in's 20-stop turn (one run failing), held at its last stop — the chat open, follow off
/// (its trail rows); follow on (the follow card's top line, its strip); and the strip's second mark hovered (its words on
/// the card's bottom line); the keys in the code (`C-w h`) and `[r` back to the failed run (its replay: the strip around
/// it); `[r` back to the read before the last edit (a `◦` in hand, replaying).
const STRIP_KEYS: [&[&str]; 5] = [
    &["@1", " ", "l", "@1", "\x06", "tidy the threads", "\r", "@?One more look", "@1"],
    &["@1", " ", "l", "@1", "tidy the threads", "\r", "@?One more look", "@1"],
    &["@1", " ", "l", "@1", "tidy the threads", "\r", "@?One more look", "@1", "@hover?▲◦●▸", "@1"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "tidy the threads",
        "\r",
        "@?One more look",
        "@1",
        "\x17",
        "h",
        "[r",
        "[r",
        "[r",
        "[r",
        "[r",
        "[r",
        "[r",
        "[r",
        "@?13/20",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "tidy the threads",
        "\r",
        "@?One more look",
        "@1",
        "\x17",
        "h",
        "[r",
        "[r",
        "[r",
        "[r",
        "@?17/20",
        "@1",
    ],
];

/// W, a word that does its key lit under the mouse: following paused by `j` in the code, the chat open — its top line's
/// `paused`; minimized, the follow card's `space l open the chat`; the turn over, the chat's foot's `esc normal` and its
/// top line's `follow`.
const WORDS_KEYS: [&[&str]; 4] = [
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "\x17",
        "h",
        "@10?cargo test",
        "j",
        "@1",
        "@hover?paused",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        "@1",
        "@hover?space l",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "how is the map filled?",
        "\r",
        " ",
        "L",
        "@30=done",
        "@1",
        "@hover?esc normal",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "how is the map filled?",
        "\r",
        " ",
        "L",
        "@30=done",
        "@1",
        "@hover?follow",
        "@1",
    ],
];

/// #197a: a buffer's name on the code's line under the mouse, the chat open with the keys (the line faded) — only its
/// ground lightens, its text's colour as drawn.
const WORDS_TAB: &[&str] = &["@1", " ", "l", "@1", "@hover?wide.rs", "@1"];

/// #200: a card's answers lit under the mouse as every word that does its key — Claude's card focused (`C-q`), the mouse
/// on its `y`; waiting, on its way in (`C-q answer`).
const WORDS_ANSWER: [&[&str]; 2] = [
    &[
        "@1",
        " ",
        "l",
        "@1",
        "run the tests",
        "\r",
        "\x17",
        "h",
        "@?Its reason",
        "@1",
        "\x11",
        "@1",
        "@hover?y Run",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "run the tests",
        "\r",
        "\x17",
        "h",
        "@?Its reason",
        "@1",
        "@hover+1?C-q answer",
        "@1",
    ],
];

/// #185: what goes with the message, the mouse over its chip over the chat's input — the whole file quoted
/// (`%`, `space >`), in its colours; a line quoted, the screen (`C-s`), then lines 3–9 from the code (`C-w h`), folded
/// to `3 things`; the whole file's chip pressed — the quote whole in a read-only buffer.
const WITH_KEYS: [&[&str]; 3] = [
    &["@1", "%", " ", ">", "@?goes with it", "@1", "@hover?src/main.rs", "@1"],
    &[
        "@1",
        "x",
        " ",
        ">",
        "@?goes with it",
        "\x13",
        "@?go with it",
        "\x17",
        "h",
        "j",
        "j",
        "7x",
        " ",
        ">",
        "@?things go with it",
        "@1",
        "@hover?things",
        "@1",
    ],
    &["@1", "%", " ", ">", "@?goes with it", "@1", "@press?src/main.rs L1", "@release?src/main.rs L1", "@1"],
];

/// A file whose name the side panel never has room for (#187's list in hand).
const LONG_NAME: &[&str] = &["src/a_component_whose_name_runs_far_past_the_panel.rs"];

/// A message far longer than a card's rows, queued while Claude works (#187's `↓ N more lines`).
const LONG_QUEUED: &str = "Before the next step, read every place the strands are kept and tell me which of them still \
                           clone a name they could borrow; then look at how the loop ties the knots, whether a strand \
                           that crosses itself is tied twice, and what happens to the first strand when two share a \
                           name; then the tests: which of them cover the entry change, which only the insert, and \
                           which would still pass if the loop tied nothing at all; then the docs, whether they say \
                           the first strand is kept or the last; then the bench, whether it measures the clone or the \
                           loop; then the comments, which of them still describe the old way; then the changelog, \
                           whether the change is said there in the words the docs use; then the examples, whether \
                           any of them relies on the last strand winning; and last, the error messages, whether a \
                           strand refused says its name and where it crossed, so the person reading it can find it \
                           without a debugger, and say goodbye";

/// #239: a question, its answer naming places, the keys back to the code (`esc esc`) and a move there (`j`, follow
/// paused: the cursor's line is the card's) — the line ends; then `]n` to the first, its card.
const CHAT_NOTES_KEYS: [&[&str]; 2] = [
    &[
        "@1",
        " ",
        "l",
        "@1",
        "where is the map filled?",
        "\r",
        "@30?sums the knots",
        "@1",
        "\x1b",
        "\x1b",
        "j",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "where is the map filled?",
        "\r",
        "@30?sums the knots",
        "@1",
        "\x1b",
        "\x1b",
        "jgg]n",
        "@1",
    ],
];

/// #187: what's cut is whole on a card over it, one way — the chat's view mode's cursor on a cut `run` row (its card on
/// the selected row's ground, the cursor kept); the side panel with the keys, its selected long name; a queued message
/// past a card's rows, `↓ N more lines` under the mouse; then pressed — the message whole in a read-only buffer.
/// #229: an answer's Rust block, its long line cut with `…` at the chat's edge — the pointer on it: the line whole on a
/// card over it, in its colours, its rows after the first under where the code starts.
const CUT_CODE_KEYS: &[&str] =
    &["@1", " ", "l", "@1", "count them", "\r", "@30?That's all.", "@1", "@hover+6?let counts", "@1"];

const CUT_KEYS: [&[&str]; 4] = [
    &[
        "@1",
        " ",
        "l",
        "@1",
        "tidy it",
        "\r",
        " ",
        "L",
        "@30=done",
        "@1",
        "\x1b",
        "/",
        "cargo fmt",
        "\r",
        "@1",
    ],
    &["@1", " ", "e", "@?a_component", "@1"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "tidy the threads",
        "\r",
        "@?One more look",
        LONG_QUEUED,
        "\r",
        "@?queued",
        "@1",
        "@hover+12?queued",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "tidy the threads",
        "\r",
        "@?One more look",
        LONG_QUEUED,
        "\r",
        "@?queued",
        "@1",
        "@hover+12?queued",
        "@?more lines",
        "@hover+2?more lines",
        "@press+2?more lines",
        "@?whole · chat",
        "@release?whole · chat",
        "@1",
    ],
];

/// Claude asked to run outside the sandbox, the keys back in the code (`C-w h`) as its card comes; then `j j` there.
const FOCUS_CARD: &[&str] =
    &["@1", " ", "l", "@1", "run the tests", "\r", "\x17", "h", "@?Its reason", "@1", "j", "j", "@1"];

/// `FOCUS_CARD`, then `y` in the code — a yank, never the card's yes.
const FOCUS_CARD_Y: &[&str] =
    &["@1", " ", "l", "@1", "run the tests", "\r", "\x17", "h", "@?Its reason", "@1", "j", "j", "y", "@1"];

/// #189: the mouse on a floating frame without the keys lights its lines half: Claude's card waiting (`FOCUS_CARD`), on
/// its text; the follow card, the chat minimized (`words-card`'s), and `:messages` given its keys back (`esc`), on the
/// bottom-left corner (no word of its own there).
const FRAME_HOVER: [&[&str]; 3] = [
    &[
        "@1",
        " ",
        "l",
        "@1",
        "run the tests",
        "\r",
        "\x17",
        "h",
        "@?Its reason",
        "@1",
        "@hover?Its reason",
        "@1",
    ],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        "@1",
        "@hover?╰",
        "@1",
    ],
    &["@1", ":w\r", "@1", ":nope\r", "@1", ":messages\r", "@?messages", "@1", "\x1b", "@1", "@hover?╰", "@1"],
];

/// `FOCUS_CARD`, then `C-q`.
const FOCUS_CARD_Q: &[&str] =
    &["@1", " ", "l", "@1", "run the tests", "\r", "\x17", "h", "@?Its reason", "@1", "j", "j", "\x11", "@1"];

/// The chat opened, a question asked, its answer waited for to its end.
const FOOT: &[&str] = &["@1", " ", "l", "@1", "how is the map filled?", "\r", " ", "L", "@30=done", "@1"];

/// Three messages said one after another: a save, two commands that don't exist (errors, so they stay).
const TOAST_THREE: &[&str] = &["@1", ":w\r", "@1", ":nope\r", "@1", ":wat\r", "@1"];

/// `TOAST_THREE`, then `:messages`.
const MESSAGES: &[&str] =
    &["@1", ":w\r", "@1", ":nope\r", "@1", ":wat\r", "@1", ":messages\r", "@?messages", "@1"];

/// `FOCUS_CARD`, then `:features-reset` — your own card over Claude's.
const FOCUS_CARD_YOURS: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "run the tests",
    "\r",
    "\x17",
    "h",
    "@?Its reason",
    "@1",
    "j",
    "j",
    ":features-reset\r",
    "@1",
];

/// The finished turn (`chatdone`'s), in its view mode a line selected (`enter quote` said) — then `enter`: the
/// chip over the input.
const QUOTE_VIEW: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@?Change 1 of 1",
    "@1",
    "y",
    "@20?Nothing else",
    "@1",
    "\x1b",
    "@?normal · chat",
    "x",
    "x",
    // (no wait on `enter quote`: a build from before quoting takes this one too)
    "@1",
];
/// Claude writing src/main.rs in the checkout tarae started in, you in another checkout of the repository with the
/// same file at the lines it writes (`presence` scenes).
macro_rules! presence_keys {
    ($($more:expr),* $(,)?) => {
        &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@20?knots\", t.k",
            "\x1b",
            // (another checkout beside it, opened as a session — the shot's own folders, not tarae's worktrees')
            ":",
            "sh git worktree add -q ../fix && echo checkout made",
            "\r",
            "@20?checkout made",
            ":",
            "workspace-open ../fix",
            "\r",
            "@3",
            ":",
            "e src/main.rs",
            "\r",
            "@1",
            "/",
            "threads.insert",
            "\r",
            "z",
            "z",
            "@2",
            $($more),*
        ]
    };
}
const PRESENCE: &[&str] = presence_keys!();
const PRESENCE_LIST: &[&str] = presence_keys!(" ", "W", "@2");
const PRESENCE_FILES: &[&str] = presence_keys!(" ", "e", "@2");
const PRESENCE_FOLLOW: &[&str] = presence_keys!(" ", "F", "@2");
// (frictions #166: `:procs` from fix, main at work behind — its heading's one working sign, as every place a session works)
const PRESENCE_PROCS: &[&str] = presence_keys!(":procs", "\r", "@2");
// (a third checkout, idle, then back in fix with its chat open: the strip over it, each session's name in its colour,
// main's `working` and its file after it, fix in front in bold)
const PRESENCE_STRIP: &[&str] = presence_keys!(
    ":",
    "sh git worktree add -q ../docs && echo checkout made",
    "\r",
    "@20?checkout made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    " ",
    "W",
    "@1",
    "2",
    "@2",
    " ",
    "l",
    "@2"
);
/// The main checkout's Claude asking for a run outside the sandbox (its card), you in another checkout: the card waits
/// behind with main (`presencewaits` scenes).
macro_rules! waits_keys {
    ($($more:expr),* $(,)?) => {
        &[
            "@1",
            ":",
            "sh git worktree add -q ../fix && echo checkout made",
            "\r",
            "@20?checkout made",
            ":",
            "workspace-open ../fix",
            "\r",
            "@3",
            // (back in main: its Claude asked, its card up)
            " ",
            "W",
            "@1",
            "1",
            "@2",
            " ",
            "l",
            "@1",
            "run the tests",
            "\r",
            "@?Its reason",
            "@1",
            "\x1b",
            // (into fix: main's card waits behind)
            " ",
            "W",
            "@1",
            "2",
            "@2",
            $($more),*
        ]
    };
}
/// The polish week's landing 1 in motion, the second Short V2 "A card never covers what you opened" (`--video
/// --shorts waits-60s`, 80×34): you ask main's Claude to run the whole suite, then run this file's tests yourself and
/// float them (`C-w e`: half the screen, as every floating pane first stands); main's card comes ~10 s into the turn
/// (`cardlater`) and, the tests in its way, folds to its peek — the one emphasis, the punchline; `C-q` stands it whole
/// with the keys, `esc` folds it again, the tests closed it stands; `C-q y` answers it and its run goes. Off camera
/// the tests are built once, so yours on camera take a moment. No beat waits to be read: the scene goes at the
/// stand-in Claude's pace, the tests' and a hand's, but for the fold, which it stands on while the camera eases in,
/// holds and eases back out (CLAUDE.md's Shorts bullet).
const WAITS_60S: &[&str] = &[
    "@1",
    // (the tests built off camera: on camera they run at once)
    " ",
    "x",
    "f",
    "@90?right: 8",
    "@1",
    "\x17",
    "j",
    "@0.5",
    "q",
    "@1",
    "@rec",
    "@beat:You ask main's Claude to run the whole suite",
    " ",
    "l",
    // (the question in the caption as it's typed, from its first letter)
    "@type:r",
    "@note:you ask: {} → card:ask claude",
    "@type:un the whole test suite",
    "\r",
    "@beat:Your own tests of this file, floating over the code",
    " ",
    "x",
    "f",
    "@20?right: 8",
    "\x17",
    "j",
    "\x17",
    "e",
    "@10?floating",
    // (its toast, top right: the float itself reaches down under the caption's place)
    "@note:your tests, floating → card:tests floating",
    // (one session: the card's frame is its ask, no session's name)
    "@30?Claude wants to run",
    "@beat:Claude's card arrives — folded to a peek, off your tests",
    // (the peek with its air is 48 columns, under 2.5 mm a glyph on a phone: read at the legible 46, its first 44 the
    // target — its air, its `▎ ` and its words whole — so the window, centred on them, ends on `sandbox`, your tests and
    // the follow card over it)
    "@zoom:card:Claude wants to run|44",
    // (the fold's phrases on the status line's `C-q answer`: there only while the card is folded — whole, it's the
    // card's own bottom line's, and the card stands under the caption's place)
    "@note:Claude asks to run → C-q answer",
    "@punch",
    // (the one wait for a look: the emphasis's — the camera's 1.2 s move in, 2 s held, 1.2 s back out —, its second
    // phrase as the camera holds)
    "@2",
    "@note:folded, off your tests → C-q answer",
    "@2.4",
    // (no caption from here: the card stands whole under the caption's place, its own bottom line saying its keys, and
    // a hand's keys come too close for a phrase each)
    "@beat:C-q stands it whole, the keys on it; esc folds it again",
    "\x11",
    "@10?Its reason",
    "@0.6",
    "\x1b",
    "@10!Its reason",
    "@0.6",
    // (the tests given the keys back and closed)
    "@beat:Your tests closed: the card stands",
    "q",
    "@10?Its reason",
    "@0.6",
    "\x11",
    "@0.6",
    "@beat:y — the run goes",
    "y",
    // (the take ends on the last frame before the run's output shows — the card answered, the follow card's `▸
    // running`: its output slot, and the chat's column the turn's place-less answer opens by itself as it ends, are
    // nothing the Short is about. Its first frame is told by the slot's top line, the command after its `─` — its
    // lines open at the failure at 43 rows, past cargo's `running 2 tests`, and its keys fold where the chat's column
    // stands beside it; the card's row and the follow card's have no `─` before it)
    "@cut?─ cargo test --quiet",
];
// (in fix, main's card waiting as a peek: a file saved, its cursor off the top — the toast at the top right)
const TOAST_PEEK: &[&str] = waits_keys!(":e src/main.rs\r", "@1", "G", ":w\r", "@1");
// (fix's chat open: the strip over it — main's `▲ waits` in warning, each name in its colour)
const PRESENCE_WAITS: &[&str] = waits_keys!(" ", "l", "@2");
// (…the list: main's `▲ waits for you` in warning, each name in its colour)
const PRESENCE_WAITS_LIST: &[&str] = waits_keys!(" ", "W", "@2");
// (your cursor at the top, then this checkout's own Claude asked too: two Claudes writing one file — fix's caret and rows
// in its colour at full strength, followed, main's mark and tag in its own, dim; the view on fix's writing, your
// cursor left on line 1)
const PRESENCE_TWO: &[&str] = presence_keys!(
    "g",
    "g",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    "\x1b",
    "@2"
);
// (…follow off in fix before its turn: no card)
const SIGN1_NOCARD: &[&str] = presence_keys!(
    ":",
    "set llm.follow false",
    "\r",
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    "\x1b",
    "@2"
);
// (…the follow card drawn, the session list over it)
const SIGN1_LIST: &[&str] = presence_keys!(
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    "\x1b",
    " ",
    "W",
    "@2"
);
/// The overlay scenes (`.sb/overlays-plan.md` §3 K): the demo plugin let in (its card, `C-q y`; its toast waited out),
/// its keycast row shown in the asks' corner (`:overlay keys`, ten minutes), then `$more`.
macro_rules! overlay_keys {
    ($($more:expr),* $(,)?) => {
        &["@?waiting", "\x11", "y", "@!waiting", "@6", ":", "overlay keys 600000", "\r", "@1", $($more),*]
    };
}
const OVERLAY_KEYS: &[&str] = overlay_keys!();
/// …and a question to Claude, whose card comes to that corner: the keycast gives way while the ask stands there.
const OVERLAY_KEYS_ASK: &[&str] =
    overlay_keys!(" ", "l", "@1", "why does insert clone the name?", "\r", "@10?C-q", "\x1b", "\x1b", "@2");
/// The line `threads.insert` is on changed and saved in the checkout tarae started in, then another checkout opened as
/// a session and the same line changed its own way and saved there: their changes would conflict (`overlap` scenes,
/// collab C2 — said once the trees are still, a few seconds after the save).
macro_rules! overlap_keys {
    ($($more:expr),* $(,)?) => {
        &[
            "@1",
            "/",
            "threads.insert",
            "\r",
            "A",
            " // main's way",
            "\x1b",
            ":",
            "w",
            "\r",
            "@1",
            ":",
            "sh git worktree add -q ../fix && echo checkout made",
            "\r",
            "@20?checkout made",
            ":",
            "workspace-open ../fix",
            "\r",
            "@3",
            ":",
            "e src/main.rs",
            "\r",
            "@1",
            "/",
            "threads.insert",
            "\r",
            "A",
            " // fix's way",
            "\x1b",
            ":",
            "w",
            "\r",
            "@12?fix ≈ main",
            "@1",
            $($more),*
        ]
    };
}
// (the toast, once)
const OVERLAP: &[&str] = overlap_keys!();
// (the flow plan's piece 2: two sessions, the chat hidden — the strip — and the ask line over them)
const FLOW2_STRIP: &[&str] = overlap_keys!("\x1b", "@3");
const FLOW2_STRIP_L: &[&str] = overlap_keys!("\x1b", "@3", " ", "l", "@1");
// (…then fix's Claude at work, a third checkout whose Claude asks for a run outside the sandbox, main's change
// committed — fix and docs a commit behind —, main's chat open and the session list over it)
const SESSIONS_FLOAT: &[&str] = overlap_keys!(
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    ":",
    "sh git worktree add -q ../docs && echo checkout made",
    "\r",
    "@20?checkout made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "run the tests",
    "\r",
    "@?Its reason",
    "@1",
    "\x1b",
    ":",
    "workspace-switch 0",
    "\r",
    "@2",
    ":",
    "sh git commit -qam main && echo main committed",
    "\r",
    "@20?main committed",
    " ",
    "l",
    "@1",
    "\x1b",
    " ",
    "W",
    "@3"
);
// (…the strip over fix's chat: `fix ≈ main`, `main ≈ fix`, each name in its colour)
const OVERLAP_STRIP: &[&str] = overlap_keys!(" ", "l", "@2");
// (focus E1, 2+ sessions: fix's turn on the map, main's overlap and marks beside it)
const E1_SESSIONS: &[&str] = overlap_keys!(
    " ",
    "l",
    "@1",
    "the length test fails, fix it",
    "\r",
    "@20?Change 1 of 1",
    "@1",
    "y",
    "@30=done",
    "@1",
    " ",
    "e",
    "@?main.rs",
    "@1"
);
// (…the list: `≈ main` on fix's row, `≈ fix` on main's)
const OVERLAP_LIST: &[&str] = overlap_keys!(" ", "W", "@2");
// (…the file tree: `≈m` on src/main.rs — main's initial in its colour — and the row in hand's words at its foot)
const OVERLAP_FILES: &[&str] = overlap_keys!(" ", "e", "@?main.rs", "@1");
// (collab C2b: …and fix's `println!` line changed too, unsaved, then `:weave` — the conflict side by side where it's
// wide, stacked at 80; fix's change in its colour, `fix unsaved` in the path bar)
const WEAVE: &[&str] = overlap_keys!(
    "/",
    "println",
    "\r",
    "A",
    " // and fix's",
    "\x1b",
    ":",
    "weave",
    "\r",
    "@10?conflict 1 of 1",
    "@1"
);
/// Claude's card, the keys back in the code, then the chat closed (`asks-closed`).
const ASKS_CLOSED: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "run the tests",
    "\r",
    "\x17",
    "h",
    "@?Its reason",
    "@1",
    ":",
    "chat-close",
    "\r",
    "@2",
];
/// Claude's card, the keys back in the code, then the chat open again (`asks-open`).
const ASKS_OPEN: &[&str] =
    &["@1", " ", "l", "@1", "run the tests", "\r", "\x17", "h", "@?Its reason", "@1", " ", "l", "@2"];
/// #244: a long answer and Claude's card, the keys back in the code — the card waiting (`asks-beside`)…
/// (following folds the chat as the keys leave it: `space l` opens it again, `C-w h` gives the code the keys)
const ASKS_BESIDE: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "explain, then run the tests",
    "\r",
    "\x17",
    "h",
    "@?Its reason",
    "@1",
    " ",
    "l",
    "@1",
    "\x17",
    "h",
    "@2",
];
/// …and then focused by `C-q` (`asks-beside-focused`).
const ASKS_BESIDE_FOCUSED: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "explain, then run the tests",
    "\r",
    "\x17",
    "h",
    "@?Its reason",
    "@1",
    " ",
    "l",
    "@1",
    "\x17",
    "h",
    "@1",
    "\x11",
    "@2",
];
/// Claude's set of sessions asked for (orchestra P1a), its card focused by `C-q` (`sessionset-*`).
/// (the chat open again over following, as `asks-open`: the card above its input)
const SESSION_SET: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x17",
    "h",
    "@1",
    " ",
    "l",
    "@1",
    "\x11",
    "@1",
];
/// The set's `y`, then into the child (`sessionset-child`): its chat opened behind with main's brief as its first message,
/// headed `main's Claude`, its Claude's answer, then a message of yours under it — shown from the transcript's top.
const SESSION_SET_CHILD: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?space W lists them",
    "@2",
    "\x1b",
    " ",
    "W",
    "@1",
    "2",
    "@10?Each thread",
    "@1",
    "and keep ties in name order",
    "\r",
    "@10?Right —",
    "@1",
    // (the transcript from its top, in its view mode: the brief's heading in sight at 80 columns too)
    "\x1b",
    "g",
    "g",
    "@?main's Claude",
    "@1",
];
/// orchestra P2 (`orchestra-woke`): main starts knot-count asking to be woken as its turn ends — the turn ends, main is
/// woken with the news (its `told Claude: …` row, tarae's message), answers, and its row says what the wake cost.
const ORCHESTRA_WOKE: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started",
    "@20?woke for",
    "@2",
];
/// orchestra P2 (`orchestra-end`): main starts knot-count; in it a commit (yours, by `:sh`), then `:workspace-bring` — its
/// card's `y` brings it into main, and its end is offered at once: `knot-count is in main now — end it?`, focused by `C-q`.
const ORCHESTRA_END: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?space W lists them",
    "@2",
    "\x1b",
    " ",
    "W",
    "@1",
    "2",
    "@10?Each thread",
    "@1",
    "\x1b",
    "\x17",
    "h",
    "@1",
    ":",
    "sh git commit --allow-empty -qm count && echo count committed",
    "\r",
    "@20?count committed",
    ":",
    "workspace-bring knot-count",
    "\r",
    "@20?Bring knot-count",
    "@1",
    "y",
    "@20?is in main now",
    "@1",
    "\x11",
    "@1",
];
/// orchestra P1b (`orchestra-list`): a session of yours (fix) opened, then main's Claude starts three — knot-count's turn
/// ends (done), thread-docs asks for a run (▲ waits), spool-clones writes (working) — and main's chat open with the
/// session list over it: the children right after main, each with a faint `›`, fix after them; the chat's top line
/// the same order.
const ORCHESTRA_LIST: &[&str] = &[
    "@1",
    ":",
    "sh git worktree add -q ../fix && echo checkout made",
    "\r",
    "@20?checkout made",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "workspace-switch 0",
    "\r",
    "@2",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started all",
    "@20?▲1 waiting",
    "@3",
    "\x1b",
    " ",
    "W",
    "@2",
];
/// orchestra P1b's parent (`orchestra-parent`, `-long`): main starts knot-count, its brief's turn ends — news for main —
/// then you ask main how it goes: the news told with your message (its `told Claude: …` row), main's `sessions` row, its
/// answer.
const ORCHESTRA_PARENT: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started",
    "@20?turn ended",
    "@1",
    "how is it going",
    "\r",
    "@20?its change next",
    "@2",
];
/// …the mouse on the `told Claude` row's `…` (`orchestra-parent-hover`).
const ORCHESTRA_PARENT_HOVER: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started",
    "@20?turn ended",
    "@1",
    "how is it going",
    "\r",
    "@20?its change next",
    "@1",
    "@hover?told Claude",
    "@1",
];
/// …then `start it over`: main's `send anew` row (`orchestra-fresh-parent`).
const ORCHESTRA_FRESH_PARENT: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started",
    "@20?turn ended",
    "@1",
    "how is it going",
    "\r",
    "@20?its change next",
    "@1",
    "start it over",
    "\r",
    "@20?brief.",
    "@2",
];
/// …the mouse on its row's `…` (`orchestra-fresh-parent-hover`).
const ORCHESTRA_FRESH_PARENT_HOVER: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started",
    "@20?turn ended",
    "@1",
    "how is it going",
    "\r",
    "@20?its change next",
    "@1",
    "start it over",
    "\r",
    "@20?brief.",
    "@1",
    "@hover?anew:",
    "@1",
];
/// …and into knot-count (`orchestra-fresh`): its new conversation, said whose, main's message its first, its answer.
const ORCHESTRA_FRESH: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started",
    "@20?turn ended",
    "@1",
    "how is it going",
    "\r",
    "@20?its change next",
    "@1",
    "start it over",
    "\r",
    "@20?brief.",
    "@1",
    "\x1b",
    " ",
    "W",
    "@1",
    "2",
    "@10?a new conversation",
    "@10?Each thread",
    "@2",
];
/// …the chat's top line alone, the list closed (`orchestra-strip`).
const ORCHESTRA_STRIP: &[&str] = &[
    "@1",
    ":",
    "sh git worktree add -q ../fix && echo checkout made",
    "\r",
    "@20?checkout made",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "workspace-switch 0",
    "\r",
    "@2",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?Started all",
    "@20?▲1 waiting",
    "@3",
];
/// orchestra P3 (`orchestra-propose-bring`): main starts knot-count; in it a commit (yours, by `:sh`); back in main you
/// say “bring it in” — main's Claude proposes the bring, and the real bring card comes, its first line who proposed it
/// and why, focused by `C-q`.
const ORCHESTRA_PROPOSE_BRING: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?space W lists them",
    "@2",
    "\x1b",
    " ",
    "W",
    "@1",
    "2",
    "@10?Each thread",
    "@1",
    "\x1b",
    "\x17",
    "h",
    "@1",
    ":",
    "sh git commit --allow-empty -qm count && echo count committed",
    "\r",
    "@20?count committed",
    " ",
    "W",
    "@1",
    "1",
    "@3",
    " ",
    "l",
    "@1",
    "bring it in",
    "\r",
    "@20?Bring knot-count",
    "@1",
    // (`C-q`: main's card, where it stands — never the child it brings)
    "\x11",
    "@1",
];
/// …as it comes, waiting unfocused in main's chat (`orchestra-propose-bring-waits`).
const ORCHESTRA_PROPOSE_BRING_WAITS: &[&str] =
    ORCHESTRA_PROPOSE_BRING.split_at(ORCHESTRA_PROPOSE_BRING.len() - 2).0;
/// …and (`orchestra-propose-board`) a change of its committed there, then “get a board on it”: the board's card, asked in
/// knot-count where the board would read, its first line who proposed it and why — `C-q` takes you to it.
const ORCHESTRA_PROPOSE_BOARD: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "split the work",
    "\r",
    "@?Start ",
    "\x11",
    "@1",
    "y",
    "@20?space W lists them",
    "@2",
    "\x1b",
    " ",
    "W",
    "@1",
    "2",
    "@10?Each thread",
    "@1",
    "\x1b",
    "\x17",
    "h",
    "@1",
    ":",
    "sh printf '\\n// counted\\n' >> src/main.rs && git commit -qam count && echo count committed",
    "\r",
    "@20?count committed",
    " ",
    "W",
    "@1",
    "1",
    "@3",
    " ",
    "l",
    "@1",
    "get a board on it",
    "\r",
    "@20?A board spends",
    "@2",
    "\x11",
    "@20?Start a board",
    "@1",
];
/// …and as it comes, waiting (`sessionset-waits`).
const SESSION_SET_WAITS: &[&str] =
    &["@1", " ", "l", "@1", "split the work", "\r", "@?Start ", "\x17", "h", "@1", " ", "l", "@2"];
// (main's card behind, fix's Claude asking too, then two cards of yours: four in the stack, over two sessions)
const ASKS_STACK: &[&str] = waits_keys!(
    " ",
    "l",
    "@1",
    "run the tests",
    "\r",
    "@?Its reason",
    "@1",
    "\x17",
    "h",
    "@1",
    ":",
    "features-reset",
    "\r",
    "@1",
    "\x1b",
    ":",
    "restart",
    "\r",
    "@2"
);
/// One session following its Claude's edit being written, the keys back in the code (`follow-one`).
const FOLLOW_ONE: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    "\x1b",
    "@2",
];
/// Per-pane follow (panes P3 step 8): the turn asked from the left pane of two on main.rs, then `C-w l` and moves in the
/// right one — the left pane's view on the writing, Claude's caret there and `● follow` on its line, your cursor yours
/// (`follow-pane`).
const FOLLOW_PANE: &[&str] = &[
    "@1",
    "\x17",
    "v",
    "\x17",
    "h",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    "\x1b",
    "\x17",
    "l",
    "j",
    "j",
    "j",
    "@2",
];
/// …the right pane on thread.rs (`follow-pane-files`).
const FOLLOW_PANE_FILES: &[&str] = &[
    "@1",
    "\x17",
    "v",
    ":",
    "open src/thread.rs",
    "\r",
    "@1",
    "\x17",
    "h",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    "\x1b",
    "\x17",
    "l",
    "j",
    "j",
    "@2",
];
/// #157: a split's pane that had the code's keys asks a turn that reads the file's last lines, then the keys back in the
/// code and the chat minimized — the follow card out of the lines' way, at the top of its pane (`follow-split-s`: `C-w s`,
/// the top pane; `follow-split-v`: `C-w v`, the left).
const FOLLOW_SPLIT: [&[&str]; 2] = [
    &[
        "@1",
        "\x17",
        "s",
        "\x17",
        "k",
        " ",
        "l",
        "@1",
        "where do the strands end?",
        "\r",
        "@10?last strands",
        "\x1b",
        "\x1b",
        "@2",
    ],
    &[
        "@1",
        "\x17",
        "v",
        "\x17",
        "h",
        " ",
        "l",
        "@1",
        "where do the strands end?",
        "\r",
        "@10?last strands",
        "\x1b",
        "\x1b",
        "@2",
    ],
];
/// …then `C-f` from the chat with the right pane the code's last: follow moves into it, said, at the writing
/// (`follow-pane-cf`).
const FOLLOW_PANE_CF: &[&str] = &[
    "@1",
    "\x17",
    "v",
    ":",
    "open src/thread.rs",
    "\r",
    "@1",
    "\x17",
    "h",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?knots\", t.k",
    "\x1b",
    "\x1b",
    "\x17",
    "l",
    " ",
    "l",
    "@1",
    "\x06",
    "@1",
    "\x1b",
    "@2",
];
/// The session list docked right (`C-w L`), `j` and `enter`: in fix, the list staying (`sessions-docked`).
const SESSIONS_DOCKED: &[&str] = waits_keys!(" ", "W", "@1", "\x17", "L", "@1", "k", "\r", "@2");
/// Two files open: the buffer tabs on the code's top line (`hover-tabs`).
const TWO: &[&str] = &["src/main.rs", "src/thread.rs"];
/// The hover audit (2026-10-28): the pointer over one thing, the frame after it. In order: the code with the side panel's
/// keys; the code with its own; a split's divider; the split's other pane (the new one has the keys); a slot's top line
/// (the terminal, the keys back in the code); a side panel's row, the keys in the code; the review's rail, the keys in the
/// code; a chat tool row cut, the keys in the code; the chat's foot `i type`, its view mode; a buffer tab; the status
/// line's `▲ waiting`; a waiting ask's answer; a toast; the follow card's trail; a `:messages` row, its keys given back;
/// a picker's row; the mode's word; the line numbers.
const HOVER_KEYS: [&[&str]; 18] = [
    &["@1", " ", "e", "@?main.rs", "@1", "@hover+4?fn main", "@1"],
    &["@1", "@hover+4?fn main", "@1"],
    &["@1", "\x17", "v", "@1", "@hover?│", "@1"],
    &["@1", "\x17", "v", "@1", "@hover+4?fn main", "@1"],
    &["@1", " ", "T", "@2", "\x17", "k", "@1", "@hover+4?terminal", "@1"],
    &["@1", " ", "e", "@?main.rs", "@1", "\x17", "l", "@1", "@hover?README", "@1"],
    &["@1", " ", "v", "v", "@?knot.rs", "@1", "\x17", "l", "@1", "@hover?thread.rs", "@1"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "tidy it",
        "\r",
        " ",
        "L",
        "@30=done",
        "@1",
        "\x17",
        "h",
        "@1",
        "@hover?cargo fmt",
        "@1",
    ],
    &["@1", " ", "l", "@1", "tidy it", "\r", " ", "L", "@30=done", "@1", "\x1b", "@1", "@hover?i type", "@1"],
    &["@1", "@hover?thread.rs", "@1"],
    &["@1", " ", "l", "@1", "run the tests", "\r", "\x17", "h", "@?Its reason", "@1", "@hover?waiting", "@1"],
    &["@1", " ", "l", "@1", "run the tests", "\r", "\x17", "h", "@?Its reason", "@1", "@hover?y Run", "@1"],
    &["@1", ":nope\r", "@1", "@hover?nope", "@1"],
    &[
        "@1",
        " ",
        "l",
        "@1",
        "keep the first strand on a repeat",
        "\r",
        "@10?cargo test",
        "\x1b",
        "\x1b",
        "@1",
        "@hover?cargo test",
        "@1",
    ],
    &[
        "@1",
        ":w\r",
        "@1",
        ":nope\r",
        "@1",
        ":messages\r",
        "@?messages",
        "@1",
        "\x1b",
        "@1",
        "@hover?nope",
        "@1",
    ],
    &["@1", " ", "f", "@?README", "@1", "@hover?Cargo.toml", "@1"],
    &["@1", "@hover+2?normal", "@1"],
    &["@1", "@hover+1? 3 ", "@1"],
];
/// More of the hover audit, by cell where no text tells the place: the side panel's edge, the keys in the code; a split's
/// divider with the side panel's keys (neither pane's); the line between a split over and under, the keys in the top
/// pane; the review's path bar name (the preview's, a word); the rail with the keys, the pointer in the code.
const HOVER_MORE: [&[&str]; 5] = [
    &["@1", " ", "e", "@?main.rs", "@1", "\x17", "l", "@1", "@hover?│", "@1"],
    &["@1", "\x17", "v", "@1", " ", "e", "@?README", "@1", "@hover:88,20", "@1"],
    &["@1", "\x17", "s", "@1", "\x17", "k", "@1", "@hover:60,20", "@1"],
    &["@1", " ", "v", "v", "@?knot.rs", "@1", "@hover?knot.rs ›", "@1"],
    &["@1", " ", "v", "v", "@?knot.rs", "@1", "\x17", "h", "@1", "@hover+4?pub fn", "@1"],
];
/// The hover audit: in fix, main waiting — fix's chat open, the pointer on main's name on its top line; the peek of
/// main's card; the session list, the pointer on main's row (its report line cut).
const HOVER_CHAT_NAME: &[&str] = waits_keys!(" ", "l", "@2", "@hover?main ▲", "@1");
const HOVER_PEEK: &[&str] = waits_keys!("@1", "@hover?asks", "@1");

/// Kinds C: the trust card of a `.tarae.toml` with forty tasks (`run_files`), cut to fit as it comes — a card tarae puts
/// up itself, no sandbox needed; then the wheel over it to its end — its place kept.
const KINDSC_CARD: [&[&str]; 2] = [
    &["@20?more lines", "@1"],
    &["@20?more lines", "@1", "@wheel-down*20?step12.sh", "@20?lines above", "@1"],
];
const HOVER_SESSIONS: &[&str] = waits_keys!(" ", "W", "@2", "@hover+4?The tests need", "@1");
/// Nine sessions (`strip-nine`): main and fix overlapping, seven more checkouts — docs and parser waiting on their
/// Claude's card, keys's Claude on its run's card (a shot's sandbox is none: no run goes unasked, so none is stuck —
/// `session_tabs`' tests have that), web's turn ended behind (done) — then main's chat open, its top line naming them;
/// then `$more`.
macro_rules! strip_nine_keys {
    ($($more:expr),* $(,)?) => {
        overlap_keys!(
    ":",
    "sh for n in docs parser sbspeed keys pull web motion; do git worktree add -q ../$n || exit; done && echo checkouts made",
    "\r",
    "@20?checkouts made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "run the tests",
    "\r",
    "@?Its reason",
    "\x1b",
    ":",
    "workspace-open ../parser",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "run the tests",
    "\r",
    "@?Its reason",
    "\x1b",
    ":",
    "workspace-open ../sbspeed",
    "\r",
    "@3",
    ":",
    "workspace-open ../keys",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "wait for the build",
    "\r",
    "@2",
    "\x1b",
    ":",
    "workspace-open ../pull",
    "\r",
    "@3",
    ":",
    "workspace-open ../web",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?Change 1 of",
    "@1",
    "y",
    "@20?Nothing else",
    "\x1b",
    ":",
    "workspace-open ../motion",
    "\r",
    "@3",
    ":",
    "workspace-switch 0",
    "\r",
    "@2",
    " ",
    "l",
    "@2",
    $($more),*
        )
    };
}
const STRIP_NINE: &[&str] = strip_nine_keys!();
/// #237's shots (`follow` and `no sandbox` keep their room after the names on the chat's top line, the names folding
/// first): main and fix overlapping, fix's Claude on its run's card; then `$more`; then back in main, its chat open
/// (follow on: `llm.follow`). `--size` 100, 160 and 240 wide are where they fold.
macro_rules! f237_keys {
    ($($more:expr),* $(,)?) => {
        overlap_keys!(
            " ", "l", "@1", "run the tests", "\r", "@?Its reason", "\x1b",
            $($more,)*
            ":", "workspace-switch 0", "\r", "@2", " ", "l", "@2",
        )
    };
}
const F237_TWO: &[&str] = f237_keys!();
/// …and three more: docs waiting on its card, web's turn ended (done), motion idle.
const F237_FIVE: &[&str] = f237_keys!(
    ":",
    "sh for n in docs web motion; do git worktree add -q ../$n || exit; done && echo checkouts made",
    "\r",
    "@20?checkouts made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "run the tests",
    "\r",
    "@?Its reason",
    "\x1b",
    ":",
    "workspace-open ../web",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@20?Change 1 of",
    "@1",
    "y",
    "@20?Nothing else",
    "\x1b",
    ":",
    "workspace-open ../motion",
    "\r",
    "@3",
);
/// …the pointer on its `+N` (the hover audit).
const HOVER_CHAT_MORE: &[&str] = strip_nine_keys!("@hover?+", "@1");
/// collab C3: two more checkouts — fix's own commit on the `threads.insert` line, docs untouched — then main's own way
/// of that line committed in the checkout tarae started in: main moved, and its one card names both workspaces behind
/// it, fix's would-be conflict in words (`basemoved`).
const BASE_MOVED: &[&str] = &[
    "@1",
    ":",
    "sh git worktree add -q ../fix && git worktree add -q ../docs && echo checkouts made",
    "\r",
    "@20?checkouts made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    "/",
    "threads.insert",
    "\r",
    "A",
    " // fix's way",
    "\x1b",
    ":",
    "w",
    "\r",
    "@1",
    ":",
    "sh git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -qm fix src/main.rs \
     && echo fix committed",
    "\r",
    "@20?fix committed",
    " ",
    "W",
    "@1",
    "1",
    "@2",
    "/",
    "threads.insert",
    "\r",
    "A",
    " // main's way",
    "\x1b",
    ":",
    "w",
    "\r",
    "@1",
    ":",
    "sh git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -qm main src/main.rs",
    "\r",
    "@20?main moved",
    "@2",
];
/// The third Short, V3 "When main moves, every workspace hears" (`--video --shorts main-moved`, 80×34; collab's
/// `.sb/videos-plan.md`): on `MAIN_MOVED_RS`, off camera, docs with a saved change of its own far from `m.insert`
/// (line 31), fix's own way of that line committed, and main's own way of it saved (C2's `main ≈ fix` said and gone); on
/// camera, the floating list in main (three workspaces, `main ≈ fix`), main's commit typed with the list up, its card
/// bottom-right under the list (fix would conflict, docs clean) — the one emphasis, the punchline (`@punch`), read at
/// its two workspaces' rows (the card's 52 columns past the 46 a phone reads at 2.5 mm) —, `C-q`
/// giving it the keys, `y` moving docs only. Then fix, the one that didn't move: C2's next round says its conflict with
/// docs, now on main's way of the line (`fix ≈ docs`), and its own way back is its bring card — `G` `b` on its row,
/// rebase first —, the take ending on that card (nothing comes after it by itself). No beat waits to be read: the
/// scene goes at the keys' pace, tarae's and a hand's, but for the card, which it stands on while the camera eases in,
/// holds and eases back out (CLAUDE.md's Shorts bullet), its three phrases over it; the typing a key at a time
/// (`@type:` — the wide video's too), `y` a stamp, the move's landing the settle.
const MAIN_MOVED: &[&str] = &[
    "@1",
    ":",
    "sh git config user.name demo && git config user.email demo@example.com && git config commit.gpgsign false \
     && git worktree add -q ../fix && git worktree add -q ../docs && echo checkouts made",
    "\r",
    "@20?checkouts made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    "/",
    "println",
    "\r",
    "A",
    " // docs",
    "\x1b",
    ":",
    "w",
    "\r",
    "@1",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    "/",
    "m.insert",
    "\r",
    "A",
    " // fix",
    "\x1b",
    ":",
    "w",
    "\r",
    "@1",
    ":",
    "sh git commit -qm fix src/main.rs && echo fix committed",
    "\r",
    "@20?fix committed",
    // (back in main, its own way of the line saved — C2's `main ≈ fix` said, then gone —, the list opened again: the
    // opening shot. The edit is V1's story; this one's is the commit)
    " ",
    "W",
    "@1",
    "1",
    "@3",
    "/",
    "m.insert",
    "\r",
    "A",
    " // main",
    "\x1b",
    ":",
    "w",
    "\r",
    "@20?would conflict",
    "@30!would conflict",
    " ",
    "W",
    "@10?sessions",
    "@1",
    "@rec",
    "@beat:Three workspaces on one branch.",
    "@note:three workspaces, one branch → card:sessions",
    "@beat:Commit on main, and main moves on.",
    ":",
    // (the commit's phrase once the first one has stood, ~1.3 s; by the list, so it stands through the wait for the
    // card — 0.6 s or 3.7 s: keepclose's round starts at once, or after C2's quiet where main's HEAD move is seen first)
    "@type:sh git commit ",
    "@note:main commits → card:sessions",
    "@type:-qm main src/main.rs",
    "\r",
    "@20?main moved",
    "@beat:Everyone behind it hears at once, and where they'd clash.",
    // (the one emphasis, the punchline: the card as it comes, every workspace behind main named with what a move does
    // to it. The one wait for a look — the camera's 1.2 s move in, 2 s held, 1.2 s back out —, a phrase each ~1.5 s.
    // Read at the two workspaces' rows, 46 columns: the card's whole 52 would be under 2.5 mm; at 38 px the window
    // shows its text whole, its left line and pad cut)
    "@zoom:rows:fix — … docs —",
    "@note:every workspace hears → card:main moved",
    "@punch",
    "@1.5",
    "@note:fix would conflict → would conflict",
    "@1.5",
    "@note:docs would move clean → goes with it",
    "@1.4",
    "@beat:y moves the clean ones. Nobody's work is lost.",
    "\x11",
    "@10?esc back",
    // (the last phrase from `C-q`, by the list: after the move, `fix ≈ docs` is said 0.5 s or ~4 s on, too soon for a
    // phrase that starts there)
    "@note:y moves docs only → card:sessions",
    "@0.6",
    "@sound:stamp",
    "y",
    "@20?moved onto main",
    "@sound:settle",
    // (fix's beat — the title's every workspace, the one that stays: C2's next round says its conflict with docs, on
    // main's way of the line now (`fix ≈ docs`, top right), 0.5 s or ~4 s after the move — waited for, so the beat
    // goes in its order whichever —; then its own way back, its bring card, `b` on its row (the list's last, `G`):
    // rebase first. That card is what reads on the whole — a card coming to the asks' corner —, where fix's `↓1` is two
    // cells the move doesn't change and main's card, with fix's row, went with its `y`)
    "@beat:fix stays where it was; its own bring card rebases it first.",
    "@20?fix ≈ docs",
    // (a hand's: the toast seen before you act on it)
    "@0.6",
    "G",
    "@note:fix: rebase, then bring → card:Bring fix into main",
    "b",
    "@20?Bring fix into main",
];
/// The fourth short video, V4 "Claude's edit, in your buffer" (`--video --shorts edit-in-buffer`, 80×34; collab's
/// `.sb/videos-plan.md`): your cursor on `m.insert` (line 12), a question on the ask line; the `v3` turn reads the
/// lines in front of you and writes its edit of three changes into your buffer at the stream's pace — the view never
/// leaving it, the follow card saying each step, no chat column; the review over the buffer, `y` `n` `y` and their
/// stamps. Claude's writing reads on the whole (green rows landing in your buffer); the one emphasis is the review and
/// its three answers (the camera easing in as the review comes, the stamps inside its hold) — the Short's point,
/// what must be read; then, on the whole once the turn has ended at its own pace, `space u` takes the whole turn back — Claude's edit is yours to take
/// back, the lines as they were and its toast saying so —, the take ending there. No beat waits to be read: the scene
/// goes at the stand-in Claude's pace and a hand's, but for the ease in it lets the review be reached before `y`
/// (CLAUDE.md's Shorts bullet).
const EDIT_IN_BUFFER: &[&str] = &[
    // (your cursor on the line the question is about: the ask line carries it)
    "12G",
    "@1",
    "@rec",
    "@beat:Ask Claude about the code in front of you.",
    // (one key, as `]r`'s: `space` sent apart stands long enough for which-key, a pop over the whole)
    " l",
    // (the question in the caption as it's typed, from its first letter)
    "@type:w",
    "@note:you ask: {} → card:ask claude",
    "@type:hy does insert clone the name?",
    "\r",
    "@beat:It reads your lines, then writes its edit right there.",
    "@10?main.rs:8-16",
    "@note:Claude reads your lines → code:8-16",
    // (Claude's writing reads on the whole as it lands: green rows in your buffer)
    "@20?editing",
    "@sound:writing",
    "@note:written in your buffer → code:8-13",
    // (the one emphasis, the review and its stamps — what must be read: the camera eases in as it comes, the keys
    // waiting out the ease in (the review seen whole meanwhile), so the three answers, a hand's 0.6 s apart, land
    // inside the hold. From line 7, the row above the first change's head: the head inside the target, not its air,
    // as the view settles a row. The follow card stands from column 41, so the window narrows to it, the card wholly
    // out (`camera::framed_among`) — 41 columns, ~2.8 mm. No head's keys fit 46 (change 1's run to its `n no` at 53):
    // each head shows its change and its first pair, the answers' words whole once stamped (change 2's `▐n▌ rejected`
    // ends at column 40) — change 1's `a` of `a all 3` at the edge for the ease in, before its `y`)
    "@20?Change 1 of 3",
    "@sound:still",
    "@zoom:code:7-13",
    "@note:you answer each change → of 3",
    "@1.2",
    "@sound:stamp",
    "y",
    // (the answers a hand's 0.6 s apart: a key's own 0.35 s and this, the three inside the hold but the last's end)
    "@0.25",
    "@sound:stamp",
    "n",
    // (the Short opens on the review held: two stamps in, the third change in hand)
    "@punch",
    "@0.25",
    "@sound:stamp",
    "y",
    "@10!of 3",
    "@note:two taken, one refused → code:8-13",
    // (the hold's rest and the ease out at the turn's own pace, its last words and its end; then on the whole, a hand's
    // after it, the whole turn taken back — one key, as `space l`: which-key never pops over the whole. The turn's end
    // as drawn: follow over, its `● follow` gone from the status line — `@=done` can't reach the state socket in a
    // recording, its path past `sun_path` in a temp folder)
    "@20!● follow",
    "@beat:And the whole turn is yours to take back, in one key.",
    "@0.6",
    "@note:yours to take back → undid",
    " u",
    "@10?undid",
];
/// The fifth short video, V5 "Watch it work: follow and replay" (`--video --shorts follow-replay`, 80×43; collab's
/// `.sb/videos-plan.md`): a question on the ask line; the `v4` turn reads `spool.rs` far apart, the view following
/// while your cursor stays on line 1; `j` takes the view back as Claude's words come, `space F` follows again at its
/// next; two edits land unseen (review at the end), the view on each as it's written; once the turn's review opens,
/// `[r` steps back through its stops — the last edit, then the first. The one emphasis is that first edit's stop (line
/// 46: the change, the answer's sentence under it, the follow card's `edit` row) — the punchline, the one thing on
/// the screen that must be read; the replay's two steps are a hand's on the whole, so the view jumps twice, not
/// four times. No beat waits to be read: the scene goes at the stand-in Claude's pace and a hand's, but for the
/// emphasis's ease in, look and ease out (CLAUDE.md's Shorts bullet).
const FOLLOW_REPLAY: &[&str] = &[
    "@1",
    "@rec",
    "@beat:Ask, and the view follows Claude as it reads.",
    // (one key, as V4's: `space` sent apart stands long enough for which-key)
    " l",
    "@type:w",
    "@note:you ask: {} → card:ask claude",
    "@type:hy is the spool out of order?",
    "\r",
    "@10?spool.rs:3-7",
    "@note:Claude reads the doc → code:3-7",
    "@10?spool.rs:42-56",
    "@note:the view follows it → code:42-56",
    "@beat:Your first key takes the view back; space F follows again.",
    // (at the turn's pace: `j` as its words come, `space F` at its next — each phrase ~1.5 s)
    "@10?It pops",
    "j",
    "@note:you take it back → code:1-2",
    "@10?And at the end",
    " F",
    "@note:space F follows again → code:42-56",
    "@beat:It writes where the view is.",
    "@20?editing",
    "@sound:landing",
    "@note:it writes, you watch → code:44-48",
    "@20?Popped from",
    "@note:then drops the sort → code:76-78",
    // (the turn's end: its review opens at the first change, the answer's card under it — the replay waits for it, else
    // the review's move takes the view off the stop `[r` went to)
    "@20?Claude changed",
    "@sound:still",
    "@beat:Step back through what it did.",
    // (each pair typed as one key: sent apart, `[` stood long enough for which-key)
    "[r",
    "@0.6",
    "[r",
    // (the one emphasis, the punchline: the first change's stop, the answer's sentence under it. The one wait for a
    // look — the camera's 1.2 s move in, 2 s held, 1.2 s back out —, a phrase for the ease in and one for the rest; the
    // take ends with it. The replay's phrase is the stop's, not the first `[r`'s: 0.6 s apart, it would stand under
    // 1.2 s, over the follow card at the screen's foot)
    "@zoom:code:44-48",
    "@note:[r replays the turn → code:44-48",
    "@punch",
    "@1.2",
    "@note:its change, and why → code:44-48",
    "@2",
    "@1.2",
];
/// The first short video, V1 "Three Claudes, one file" (`--video three-claudes`, ~60 s at 120×34; collab's
/// `.sb/videos-plan.md`): each session its own Claude, by its checkout (`script_for`: `v1-main`, `v1-fix`, `v1-docs`).
/// On V2's file (`MAIN_MOVED_RS`: lines short enough for a line and its tag to read on a phone). Off camera, fix and
/// docs opened beside main and each asked: docs explains `thread.rs` slowly, fix's edit of `wind`'s key (line 30) held
/// mid-line until main's own edit is on main's disk. On camera, the floating list (three at work); main's view at
/// `wind`, fix's mark and `‹fix ●›` there; main asked, its edit of `m.insert` (line 31) written and landing
/// unseen (review at the end), so fix's lands too —
/// the same line two ways, and C2's `would conflict` once the trees are still; `:weave`, the conflict (side by side,
/// stacked at 80); a press on fix's part and `enter`: fix's workspace at that line, main's `‹main ●›` on it. The
/// Claudes go on talking after their edits (`v1_talk`): no turn ends on camera — a turn's end opens its review, its
/// answer's card and its toasts over the beats. Its Short opens on the woven conflict (`@punch`); every beat stands on
/// the whole and one is read, the woven conflict — the punchline (`camera::course`) —, its captions in step with what
/// they name (`@note:`, `captions.rs`), each written for the time the scene gives it (`captions::STAND_MIN_MS`): the
/// ask and its question as it's typed, main writing beside fix by main's tag, the warning, the woven conflict and the
/// move into fix. No beat waits to be read: the
/// scene goes at the stand-in Claudes' pace, the keys' and a hand's, but for the look at the conflict its read stands
/// on (CLAUDE.md's Shorts bullet: the camera still on the whole, the emphasis the one wait).
const THREE_CLAUDES: &[&str] = &[
    "@1",
    ":",
    "sh git worktree add -q ../fix && git worktree add -q ../docs && echo checkouts made",
    "\r",
    "@20?checkouts made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "how is a thread measured?",
    "\r",
    "@10?thread.rs",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    " ",
    "l",
    "@1",
    "trim a name before it keys the map",
    "\r",
    "@20?name.trim",
    ":",
    "workspace-switch 0",
    "\r",
    "@10?tarae-demo › src › main.rs",
    // (main's cursor on `wind`'s key, where fix is writing: its tag stands while your cursor is on its line)
    "/",
    "t.name.clone",
    "\r",
    "z",
    "z",
    "@1",
    " ",
    "W",
    "@10?sessions",
    "@1",
    "@rec",
    // (a Short: every beat on the whole, its captions in step with it — `@note:`; one read, the woven conflict)
    // (no caption on the list: it closes on the first key, too soon for a phrase to stand)
    "@beat:Three Claudes at work, each in a workspace of its own.",
    "@beat:You ask.",
    "q",
    " ",
    "l",
    // (the question in the caption as it's typed: no zoom — from its first letter, the input's faint hint never said)
    "@type:k",
    "@note:you ask: {} → card:ask claude",
    "@type:eep the first strand of a name",
    "\r",
    // (as it's sent: main's writing held mid-line — `v1_main` —, fix's beside it)
    "@beat:Main's Claude writes beside fix's, in its colour.",
    "@10?‹main ●›",
    // (one phrase for both tags: fix's comes 0.2 s after main's, too close for a phrase of its own)
    "@note:main writes beside fix → ‹main",
    "@sound:writing",
    "@20?or_insert",
    "@sound:still",
    "@20?would conflict",
    "@beat:The moment two would conflict, tarae says so.",
    "@note:they'd conflict → card:would conflict",
    "@beat:Every workspace's changes, woven into one file.",
    ":",
    "@type:weave",
    "\r",
    "@10?conflict 1 of 1",
    // (the conflict to the middle: opened at line 1 it stands at the foot, where a press scrolls it under the follow
    // card)
    "30G",
    "zz",
    "@zoom:code:29-32",
    "@note:both, woven → code:29-32",
    "@punch",
    // (the one wait for a look: the emphasis's — the camera's 1.2 s move in and 2.1 s held —, then the hand to fix's
    // part, the camera easing back out as it goes)
    "@3.3",
    "@beat:Enter takes you to whoever wrote it.",
    "@press?let _ = m.insert",
    "@release?let _ = m.insert",
    "@0.6",
    "\r",
    "@note:enter → fix's workspace → fix › src › main.rs",
    "@10?fix › src › main.rs",
];
/// The sixth short video, V6 "The manager's view" (`--video --shorts managers-view`, 80×43; collab's
/// `.sb/videos-plan.md` V6): three sessions, each its own Claude by its checkout (`v6-main`, `v6-fix`, `v6-docs`), on
/// V2's file. Off camera, main's commit (a line of `thread.rs`) leaves fix and docs a commit behind (`↓ 1`), and each
/// session is asked twice — the first turn a short answer whose result names its context window (the list says a
/// context only once a result has: `ctx 34%`, `62%`, `87%` in warning), the second the work, which never ends on camera:
/// fix's edit of `m.insert` (line 31) lands at once and main's own way of it after, so C2 says they'd conflict; docs asks
/// to run its tests outside the sandbox, its card left waiting. The columns each Claude's opened by itself at its first
/// answer hidden again (`space L`). On camera, the floating list (`▲ waits`, `≈`, `↓ 1`, `ctx`) — the one emphasis, the
/// punchline (`@punch`): every Claude's state in one place is what the Short is for —, then on the whole `j` `f`: into
/// fix's workspace following it — the moment it's followed (`@touch:`) fix writes `wind`'s key (line 30) —; docs' card
/// a peek there, `C-q` taking you to it, `y`; the list again, docs at work. The take ends before docs' next words
/// (`@cut?`). No beat waits to be read: the scene goes at the stand-in Claudes' pace and a hand's, but for the list it
/// stands on while the camera eases in, holds and eases back out (CLAUDE.md's Shorts bullet), its four phrases over it.
const MANAGERS_VIEW: &[&str] = &[
    "@1",
    ":",
    "sh git worktree add -q ../fix && git worktree add -q ../docs && echo '// main moved on' >> src/thread.rs \
     && git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -qm 'main: a note' \
     src/thread.rs && echo checkouts made",
    "\r",
    // (`:sh` is a job: `../fix` opened before it's made opens nothing, and every message of fix's went to main's Claude —
    // #342. Its end is its first line, said on the status line)
    "@20?checkouts made",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    " ",
    "l",
    "@1",
    "how is a thread keyed?",
    "\r",
    // (the first answer's last word: `@=done` asks the state socket, whose answer now and then never came — 20 s)
    "@20?spaces",
    " ",
    "L",
    "@1",
    " ",
    "l",
    "@1",
    "trim a name before it keys the map",
    "\r",
    "@3",
    // (fix's view your own again — a move of your cursor, which stands on line 1: the take's `f` follows it from main,
    // and following it there already, `f` says so)
    "G",
    "@1",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "how is a thread measured?",
    "\r",
    "@20?summed",
    " ",
    "L",
    "@1",
    " ",
    "l",
    "@1",
    "run its tests",
    "\r",
    "@20?Its reason",
    "@1",
    ":",
    "workspace-switch 0",
    "\r",
    "@3",
    " ",
    "l",
    "@1",
    "why does a repeat replace a strand?",
    "\r",
    "@20?newest",
    " ",
    "L",
    "@1",
    " ",
    "l",
    "@1",
    "keep the first strand of a name",
    "\r",
    "@30?would conflict",
    "\x1b",
    "@15!would conflict",
    " ",
    "W",
    "@10?sessions",
    "@1",
    "@rec",
    "@beat:Every Claude at work, in one list.",
    // (the one emphasis, the punchline: the list whole — what waits for you, a clash ahead, how far behind, how full.
    // The one wait for a look — the camera's 1.2 s move in, 2 s held, 1.2 s back out —, a phrase for each of what it
    // says, each its `captions::STAND_MIN_MS` at least)
    "@zoom:card:sessions",
    "@note:three Claudes, one list → card:sessions",
    "@1.3",
    "@note:docs waits on you → ▲ waits",
    "@punch",
    "@1.2",
    "@note:main, fix would clash → ≈ fix",
    "@1.2",
    "@note:behind, context 87% full → ctx 87%",
    "@0.7",
    "@beat:f follows one, in its own workspace.",
    "j",
    "@0.6",
    "f",
    "@touch:.v6-follow",
    "@10?fix › src › main.rs",
    "@note:f follows fix → fix › src › main.rs",
    "@sound:writing",
    // (docs' peek said once fix's line is half written: the two phrases ~2.5 s and ~1.5 s at the writing's pace)
    "@20?name.trim",
    "@beat:Another Claude's card waits here, never in your way.",
    "@note:docs' card waits below → docs asks",
    "@20?trim().to_string()",
    "@sound:still",
    "@0.6",
    "@beat:C-q takes you to it, and one key answers.",
    "\x11",
    "@10?Its reason",
    // (by docs' follow card, which stays through `y` and the run: the card goes at `y`, too soon for a phrase)
    "@note:C-q takes you there → run its tests",
    "@0.6",
    "@sound:stamp",
    "y",
    "@0.6",
    "@beat:And the list says it as it happens.",
    // (one key, as V4's `space l`: `space` sent apart stands long enough for which-key)
    " W",
    "@10?sessions",
    "@note:the list keeps up → card:sessions",
    // (the take ends before docs' next words, its run done: nothing this Short is about)
    "@cut?It builds",
];
/// Polish landing 4's one bring card in motion (`--record bring`): fix's own commit (the readme), main moved since (a
/// line of `thread.rs`, apart from it), the project's quick preflight two passing steps (`run_files`) — then
/// `:workspace-bring` from fix: the card, its `y`, the strip's `bringing · rebasing`, `bringing · rebased · preflight
/// 1/2`, the run's panel, and the one toast at its end.
const BRING: &[&str] = &[
    "@?Trust this",
    "@1",
    "y",
    "@1",
    ":",
    "sh git worktree add -q ../fix && echo checkout made",
    "\r",
    "@20?checkout made",
    ":",
    "sh cd ../fix && echo 'Tied by fix.' >> README.md && git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -qam 'fix: the readme' && echo fix committed",
    "\r",
    "@20?fix committed",
    ":",
    "sh echo '// main moved on' >> src/thread.rs && git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -qam 'main: a note' && echo main committed",
    "\r",
    "@20?main committed",
    ":",
    "workspace-open ../fix",
    "\r",
    // (fix's checkout is a path of its own: its `.tarae.toml` asks again)
    "@10?Trust this",
    "@1",
    "\x11",
    "y",
    "@2",
    "@beat:fix is a commit ahead, main moved on since",
    ":",
    "workspace-bring",
    "\r",
    "@10?Bring fix",
    "@beat:One card: rebase, the quick preflight, bring",
    "@3",
    "y",
    "@beat:Its progress where the session is named",
    "@30?Brought",
    "@beat:One toast at its end",
    "@4",
];
/// Three checkouts changing `src/main.rs` far apart — main's `threads.insert` line saved, docs's `println!` saved, fix's
/// `mod thread` unsaved — then `:weave` in fix: a clean merge, each change in its session's colour, its prefix in the
/// sign column (`weaveclean`, collab C2b); the cursor on fix's change, `‹fix›` at its end.
const WEAVE_CLEAN: &[&str] = &[
    "@1",
    "/",
    "threads.insert",
    "\r",
    "A",
    " // main's way",
    "\x1b",
    ":",
    "w",
    "\r",
    "@1",
    ":",
    "sh git worktree add -q ../fix && git worktree add -q ../docs && echo checkouts made",
    "\r",
    "@20?checkouts made",
    ":",
    "workspace-open ../docs",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    "/",
    "println",
    "\r",
    "A",
    " // docs' way",
    "\x1b",
    ":",
    "w",
    "\r",
    "@1",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    "/",
    "mod thread",
    "\r",
    "A",
    " // fix's way",
    "\x1b",
    ":",
    "weave",
    "\r",
    "@10?docs' way",
    "/",
    "fix's way",
    "\r",
    ";",
    "@1",
];
/// collab C2b part 1b: main takes the two lines after `threads.insert` out (saved), fix changes `mod thread` unsaved,
/// `:weave` in fix — main's removal a head where its lines went (`m ─ main · 2 lines removed ──`), the path bar's
/// `enter go there`, the buffer list's `woven · src/main.rs`; the cursor under the head (`]g` twice).
const WEAVE_GONE: &[&str] = &[
    "@1",
    "/",
    "threads.insert",
    "\r",
    "j",
    "x",
    "x",
    "d",
    ":",
    "w",
    "\r",
    "@1",
    ":",
    "sh git worktree add -q ../fix && echo checkout made",
    "\r",
    "@20?checkout made",
    ":",
    "workspace-open ../fix",
    "\r",
    "@3",
    ":",
    "e src/main.rs",
    "\r",
    "@1",
    "/",
    "mod thread",
    "\r",
    "A",
    " // fix's way",
    "\x1b",
    ":",
    "weave",
    "\r",
    "@10?lines removed",
    "g",
    "g",
    "]",
    "g",
    "]",
    "g",
    "@1",
];

const QUOTE_CHIP: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@?Change 1 of 1",
    "@1",
    "y",
    "@20?Nothing else",
    "@1",
    "\x1b",
    "@?normal · chat",
    "x",
    "x",
    "@?enter quote",
    "\r",
    "@?goes with it",
    "@1",
];

/// The finished turn, in its view mode the paragraph that wraps found and selected with `x` (a paragraph is a line).
const TRANSCRIPT_SELECT: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@?Change 1 of 1",
    "@1",
    "y",
    "@20?Nothing else",
    "@1",
    "\x1b",
    "@?normal · chat",
    "/",
    "cloned because",
    "\r",
    "x",
    "@1",
];
/// The same, a search for a phrase long enough to cross a wrap at both widths.
const TRANSCRIPT_SEARCH: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@?Change 1 of 1",
    "@1",
    "y",
    "@20?Nothing else",
    "@1",
    "\x1b",
    "@?normal · chat",
    "/",
    "moves into the map in the same call. Keying by the name",
    "\r",
    "@1",
];
/// `QUOTE_CHIP`'s chip, then a question of two lines typed under it (`A-ret` between them).
const INPUT_LINES: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@?Change 1 of 1",
    "@1",
    "y",
    "@20?Nothing else",
    "@1",
    "\x1b",
    "@?normal · chat",
    "x",
    "x",
    "@?enter quote",
    "\r",
    "@?goes with it",
    "and if a name repeats?",
    "\x1b\r",
    "keep the first strand",
    "@1",
];

/// The finished turn (`chatdone`'s), then the keys to the code: one `esc` (before the chat's view mode), two.
const CURSOR_TURN_ESC1: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@?Change 1 of 1",
    "@1",
    "y",
    "@20?Nothing else",
    "@1",
    "\x1b",
    "@1",
];
const CURSOR_TURN_ESC2: &[&str] = &[
    "@1",
    " ",
    "l",
    "@1",
    "why does insert clone the name?",
    "\r",
    "@?Change 1 of 1",
    "@1",
    "y",
    "@20?Nothing else",
    "@1",
    "\x1b",
    "@?normal · chat",
    "\x1b",
    "@1",
];

/// `diag`'s config (its scene's and the docs' `ux-error-lens.png`): Rust's server the stand-in, this xtask as
/// `scenes --lsp`.
fn stand_in_lsp(me: &Path, toml: String) -> String {
    stand_in_lsp_as(me, toml, "")
}

/// …with its mode's word after `--lsp` (`knows`: #389's scenes).
fn stand_in_lsp_as(me: &Path, toml: String, mode: &str) -> String {
    let me = toml::Value::String(me.to_string_lossy().into_owned());
    let toml = toml.replacen("lsp = false\n", "", 1);
    let mode = if mode.is_empty() { String::new() } else { format!(", \"{mode}\"") };
    toml + &format!(
        "\n[lsp.stand-in]\ncommand = {me}\nargs = [\"scenes\", \"--lsp\"{mode}]\n\n[lang.rust]\nlsp = [\"stand-in\"]\n"
    )
}

const SCENES: &[Scene] = &[
    Scene { name: "start", claude: "idle", files: &[], keys: &["@?Open a file", "@1"] },
    // (an error and a warning from the stand-in language server, `scenes --lsp`: the sign and the line end)
    Scene { name: "diag", claude: "idle", files: MAIN, keys: &["@?unused import", "@1"] },
    Scene { name: "code", claude: "idle", files: MAIN, keys: &["@1", " ", "e", "@?thread.rs", "@1"] },
    Scene { name: "picker", claude: "idle", files: MAIN, keys: &["@1", " ", "f", "@?README", "@1"] },
    // (an error, then a save: two toasts in one card)
    Scene {
        name: "toast",
        claude: "idle",
        files: MAIN,
        keys: &["@1", ":", "frobnicate", "\r", "@1", ":", "w", "\r", "@1"],
    },
    Scene { name: "git", claude: "idle", files: MAIN, keys: &["@1", " ", "g", "@?knot.rs", "@1"] },
    // (polish landing 5: a file whose lines end otherwise than LF — the status line's one word of its form)
    Scene { name: "status-crlf", claude: "idle", files: MAIN, keys: &["@?utf-8 CRLF", "@1"] },
    // (the run panels' output with the keys: a task's, two lines of it selected — its settings trusted first)
    Scene {
        name: "output-task",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            // (a card that came on its own waits for `C-q` — polish landing 1)
            "\x11",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            "\x17",
            "j",
            "@1",
            "k",
            "k",
            "k",
            "x",
            "x",
            "@1",
        ],
    },
    // (a run still printing, held still under the keys: `k` — what came since is said in its head)
    Scene {
        name: "output-live",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task watch",
            "\r",
            "@20?knot 12 tied",
            "\x17",
            "j",
            "@1",
            "k",
            "k",
            "@2",
        ],
    },
    // (a preflight's, its first step's heading at the top: `gg`)
    Scene {
        name: "output-preflight",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "preflight",
            "\r",
            "@30?test result",
            "@1",
            "\x17",
            "j",
            "@1",
            "g",
            "g",
            "@1",
        ],
    },
    // (the tests' failure, searched: `/left`, `n` — a real `cargo test` of the demo's)
    Scene {
        name: "output-tests",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "x", "f", "@90?right: 8", "@1", "\x17", "j", "@1", "/", "left", "\r", "n", "@1"],
    },
    // (panes P3: the tests group under the code while the code has the keys — where the two meet)
    Scene {
        name: "panes-tests-code",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "x", "f", "@90?right: 8", "@1"],
    },
    // (two tabs in the slot — the task's, the tests' in front — between the side panel and the chat: the joins on the
    // meeting line. The task first: a task started gives a finished test run's panel over)
    Scene {
        name: "panes-task-tabs",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            " ",
            "e",
            "@1",
            "\x1b",
            " ",
            "l",
            "@1",
            "\x1b",
            "@1",
        ],
    },
    // (panes P3 step 3: a task's places beside its output — the second selected, its line in sight)
    Scene {
        name: "panes-task-places",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "j",
            "@1",
        ],
    },
    // (…and a preflight's steps beside its output — the failed one selected, its heading at the top)
    Scene {
        name: "panes-preflight-steps",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "preflight",
            "\r",
            "@30?test result",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "j",
            "j",
            "@1",
        ],
    },
    // (panes P3 step 6: the tests group docked right of the code from its list — `C-w L` — the task in the slot; at 80
    // folded back into the slot's tabs)
    Scene {
        name: "dock-tests-right",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "\x17",
            "L",
            "@1",
        ],
    },
    // (…turned side by side — `C-w t` — and three `C-w >` wider, where both fit: at 160 over and under by its shape)
    Scene {
        name: "dock-tests-beside",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "\x17",
            "L",
            "@1",
            "\x17",
            "t",
            "@1",
            "\x17",
            ">",
            "\x17",
            ">",
            "\x17",
            ">",
            "@1",
        ],
    },
    // (…then the code split side by side, the keys there: the task in the slot below the split, the docked group faint)
    Scene {
        name: "dock-tests-split",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "\x17",
            "L",
            "@1",
            "\x1b",
            "@1",
            "\x17",
            "v",
            "@1",
        ],
    },
    // (…and docked over the code instead — `C-w K`: its line at the top, the path bar under it)
    Scene {
        name: "dock-tests-top",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "\x17",
            "K",
            "@1",
        ],
    },
    // (panes P3 step 3b: each group with its list's keys, then its detail's — which member's name and stretch of the
    // line are lit)
    Scene {
        name: "group-tests-list",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "x", "f", "@90?right: 8", "@1", "\x17", "j", "@1", "\x1b[Z", "@1"],
    },
    // (panes P3 step 7: the tests group floating over the code, from its list — `C-w e` —, centred, its keys lit)
    Scene {
        name: "float-tests",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "\x17",
            "e",
            "@1",
        ],
    },
    // (…moved by `C-w m` — left and up —, then wider and taller by `C-w > +`; the code's cursor under it hidden)
    Scene {
        name: "float-tests-moved",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "\x17",
            "e",
            "@1",
            "\x17",
            "m",
            "h",
            "h",
            "h",
            "k",
            "k",
            "@1",
            "\r",
            "\x17",
            ">",
            "\x17",
            ">",
            "\x17",
            "+",
            "@1",
        ],
    },
    Scene {
        name: "group-tests-detail",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "x", "f", "@90?right: 8", "@1", "\x17", "j", "@1", "\x1b[Z", "@1", "\t", "@1"],
    },
    Scene {
        name: "group-task-detail",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "j",
            "@1",
            "\t",
            "@1",
        ],
    },
    Scene {
        name: "group-preflight-detail",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "preflight",
            "\r",
            "@30?test result",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x1b[Z",
            "@1",
            "j",
            "j",
            "@1",
            "\t",
            "@1",
        ],
    },
    // (…and with the side panel up: at 80 too narrow for both members — stacked, the list's keys; the tests' detail's)
    Scene {
        name: "group-task-stacked",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "y",
            "@1",
            ":",
            "task check",
            "\r",
            "@20?2 warnings",
            "@1",
            " ",
            "e",
            "@1",
            "\x1b",
            "@1",
            "\x17",
            "j",
            "@1",
        ],
    },
    Scene {
        name: "group-tests-stacked",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "x",
            "f",
            "@90?right: 8",
            "@1",
            " ",
            "e",
            "@1",
            "\x1b",
            "@1",
            "\x17",
            "j",
            "@1",
            "\t",
            "@1",
        ],
    },
    // (panes polish: a preflight under way, its test step printing — the side panel up, so at 80 its group is
    // stacked, at 160 side by side; the steps' list with the keys: the running row's mark one cell, its time whole)
    Scene {
        name: "polish-preflight-list",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            // (`C-q`: a card that came on its own takes the keys once focused)
            "\x11",
            "y",
            "@1",
            ":",
            "preflight",
            "\r",
            "@30?tests::knot_5 ",
            "@1",
            " ",
            "e",
            "@1",
            "\x1b",
            "@1",
            "\x17",
            "j",
            "@4",
        ],
    },
    // (…its output in front: stacked at 80, as tall as the output)
    Scene {
        name: "polish-preflight-output",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@?Trust this",
            "@1",
            "\x11",
            "y",
            "@1",
            ":",
            "preflight",
            "\r",
            "@30?tests::knot_5 ",
            "@1",
            " ",
            "e",
            "@1",
            "\x1b",
            "@1",
            "\x17",
            "j",
            "@1",
            "\t",
            "@3",
        ],
    },
    // (…a task under way: the working sign once, by its name; the time it has run in the details)
    Scene {
        name: "polish-task-running",
        claude: "idle",
        files: MAIN,
        keys: &["@?Trust this", "@1", "\x11", "y", "@1", ":", "task watch", "\r", "@20?knot 12 tied", "@1"],
    },
    // (a floating frame over the tests group — which-key's top line beside the slot's)
    Scene {
        name: "panes-float",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "x", "f", "@90?right: 8", "@1", " ", "@1"],
    },
    // (Claude's answers first, each its own key — a read `a` this chat, a host through `also` `s` this session, a
    // read `A` always — then the list, the hand on its last row (`G`: at 24 rows the defaults fill it), the `a`. The
    // code zoomed first (`space w z`): at 80 the chat beside it would cut the cards and fold the list's stamps. The
    // host's card needs the sandbox: run outside one)
    Scene {
        name: "granted",
        claude: "grants",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "read the notes the strands follow",
            "\r",
            "@1",
            "\x1b",
            "@1",
            " ",
            "w",
            "z",
            "@20?Claude wants to read",
            "@1",
            "\x11",
            "@1",
            "a",
            "@20?Open the sandbox a little",
            "@1",
            "\x11",
            "@1",
            "s",
            "@20?Claude wants to read",
            "@1",
            "\x11",
            "@1",
            "A",
            "@3",
            ":",
            "granted",
            "\r",
            "@1",
            "G",
            "@1",
        ],
    },
    // (the demo repository's settings trusted on their card — its `.tarae.toml` and a git command of its own — then
    // the list)
    Scene {
        name: "trusted",
        claude: "idle",
        files: MAIN,
        keys: &["@?Trust this", "@1", "y", "@1", ":", "granted", "\r", "@1"],
    },
    // (the answer stops mid code block: the follow card, then the panel itself — `C-f` leaves follow)
    Scene {
        name: "chatmid",
        claude: "mid",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "why does insert clone the name?", "\r", "@?entry(first", "@1"],
    },
    // (the panel, its answer stopping in a list after a quote — a line in a script without spaces at its head)
    Scene {
        name: "chatpanel",
        claude: "midlist",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?keeps the first",
            "\x06",
            "@1",
        ],
    },
    // (the panel mid-turn, a stop in it: the answer under way after its read — `C-f` leaves follow, the panel back)
    Scene {
        name: "chatthread",
        claude: "mid",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "why does insert clone the name?", "\r", "@?entry(first", "\x06", "@1"],
    },
    // (the follow card beside the panel: mid-turn, the panel brought back — `esc`, `space l`)
    Scene {
        name: "cardpanel",
        claude: "mid",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?entry(first",
            "\x1b",
            " ",
            "l",
            "@1",
        ],
    },
    // (the follow card replaying the turn once it's done: `[r` in the code, its newest stop)
    Scene {
        name: "cardreplay",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            "[",
            "r",
            "@?stop by stop",
            "@1",
        ],
    },
    // (`space i` on a line: its card while the answer is written)
    Scene {
        name: "ask",
        claude: "ask",
        files: MAIN,
        keys: &["@1", "9", "G", "x", " ", "i", "@1", "keep the first strand", "\r", "@?claude writing", "@1"],
    },
    // (`space i` on two touching lines, one selection each, one edit of both: a change each in its review)
    Scene {
        name: "asktouching",
        claude: "asktouching",
        files: MAIN,
        keys: &[
            "@1",
            "9",
            "G",
            "x",
            "x",
            "\x1bs",
            "@1",
            " ",
            "i",
            "@1",
            "keep the first, two knots",
            "\r",
            "@?Change 1 of",
            "@1",
        ],
    },
    // (Claude's edit: its review; accepted and the turn's end; then `space v t` on that turn)
    Scene {
        name: "review",
        claude: "full",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "why does insert clone the name?", "\r", "@?Change 1 of 1", "@1"],
    },
    // (mouse S: a drag over the review selects what it shows — the line above the change, the one it takes out, the one
    // it proposes, the line under it — on the selection's ground, the code's colours kept; let go, copied)
    Scene {
        name: "review-select",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "@press+4?first = Thread",
            "@drag+6?for (name, t) in &threads",
            "@release+6?for (name, t) in &threads",
            "@1",
        ],
    },
    // (글타래 P7: Claude's edit of three changes as it's written — two whole, the third's new line mid-way, its head
    // bleeding; nothing applied, no review yet)
    Scene {
        name: "writing",
        claude: "writemid",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "why does insert clone the name?", "\r", "@20?knots\", t.k", "@1"],
    },
    // (the same with the keys back in the code: the cursor on the line being rewritten, Claude's caret at the end of
    // what it writes — two places, one cursor)
    Scene {
        name: "writingcode",
        claude: "writemid",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@20?knots\", t.k",
            "\x1b",
            "@1",
        ],
    },
    // (motion §5's sheet: Claude's edit of two changes, the first accepted — its head's stamp pressed, the frames of
    // its drying kept — the second in hand)
    Scene {
        name: "press",
        claude: "two",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 2",
            "@1",
            "@~400",
            "y",
            "@0.3",
        ],
    },
    // (motion §2's sheets: review at the end, Claude's edit of six lines lands unwatched and dries — follow off; follow
    // on; and the turn ending at once, its review opening over the lines still drying: the review's wash deeper)
    Scene { name: "wet", claude: "six", files: MAIN, keys: WET_KEYS },
    // (motion §3's sheets: the turn's word from the question's `enter` — prose streaming for 1 s; a think, silent then
    // landing; prose, a stall, prose again; the think in the follow card; prose, a read and the wait for the next answer)
    Scene { name: "shimmer-prose", claude: "shimmer-prose", files: MAIN, keys: SHIMMER_KEYS[0] },
    Scene { name: "shimmer-think", claude: "shimmer-think", files: MAIN, keys: SHIMMER_KEYS[1] },
    Scene { name: "shimmer-stall", claude: "shimmer-stall", files: MAIN, keys: SHIMMER_KEYS[2] },
    Scene { name: "shimmer-follow", claude: "shimmer-think", files: MAIN, keys: SHIMMER_KEYS[3] },
    Scene { name: "shimmer-read", claude: "shimmer-read", files: MAIN, keys: SHIMMER_KEYS[4] },
    Scene { name: "wetfollow", claude: "sixfollow", files: MAIN, keys: WET_KEYS },
    // (the working sign's sheets, #159: one turn through every state — its start, a think, prose, a stall, a read and
    // the wait for the next answer, prose — the chat open; following)
    Scene { name: "working-chat", claude: "shimmer-all", files: MAIN, keys: WORKING_KEYS[0] },
    Scene { name: "working-follow", claude: "shimmer-all", files: MAIN, keys: WORKING_KEYS[1] },
    Scene { name: "working-cursor", claude: "shimmer-all", files: MAIN, keys: WORKING_KEYS[2] },
    // (#163: the chat's prose at its automatic width; at a width you set, ten `C-w >` from the empty input — it fills it)
    Scene { name: "measure-auto", claude: "shimmer-prose", files: MAIN, keys: MEASURE_KEYS[0] },
    Scene { name: "measure-set", claude: "shimmer-prose", files: MAIN, keys: MEASURE_KEYS[1] },
    // (…the turn waiting on you, Claude's run card up: the chat open, one session — its top line's cell `▲`; following —
    // the follow card's head says `▲ waits`, its cell blank)
    // (#185: the mouse over what goes with the message — a long quote's lines; `3 things`, each)
    Scene { name: "with-quote", claude: "idle", files: MAIN, keys: WITH_KEYS[0] },
    Scene { name: "with-three", claude: "idle", files: MAIN, keys: WITH_KEYS[1] },
    Scene { name: "with-press", claude: "idle", files: MAIN, keys: WITH_KEYS[2] },
    // (#187: a cut row whole on its card — the chat's row in hand, a list's selected row, a long one's `↓ N more lines`
    // and its press)
    Scene { name: "cut-hand-chat", claude: "cuthand", files: MAIN, keys: CUT_KEYS[0] },
    Scene { name: "cut-hand-list", claude: "idle", files: LONG_NAME, keys: CUT_KEYS[1] },
    Scene { name: "cut-more", claude: "strip", files: MAIN, keys: CUT_KEYS[2] },
    Scene { name: "cut-code-chat", claude: "cutcode", files: MAIN, keys: CUT_CODE_KEYS },
    // (#239: the last answer's places at their lines — the line ends, then `]n`'s card with the paragraph)
    Scene { name: "chat-notes", claude: "chatnotes", files: MAIN, keys: CHAT_NOTES_KEYS[0] },
    Scene { name: "chat-notes-card", claude: "chatnotes", files: MAIN, keys: CHAT_NOTES_KEYS[1] },
    Scene { name: "cut-more-press", claude: "strip", files: MAIN, keys: CUT_KEYS[3] },
    Scene { name: "working-waits-chat", claude: "card", files: MAIN, keys: WORKING_KEYS[3] },
    Scene { name: "working-waits-follow", claude: "card", files: MAIN, keys: WORKING_KEYS[4] },
    // (what Claude is doing now on the status line, a turn running the tests: the chat open, follow off; following, the
    // chat minimized; following paused by a key in the code; Claude's card up while following)
    Scene { name: "now-open", claude: "now", files: MAIN, keys: NOW_KEYS[0] },
    Scene { name: "now-follow", claude: "now", files: MAIN, keys: NOW_KEYS[1] },
    Scene { name: "now-paused", claude: "now", files: MAIN, keys: NOW_KEYS[2] },
    Scene { name: "now-card", claude: "nowcard", files: MAIN, keys: NOW_KEYS[3] },
    // (…the mouse over the line: its words whole on a card over it)
    Scene { name: "now-hover", claude: "now", files: MAIN, keys: NOW_KEYS[4] },
    // (focus B1: following, a turn reading, running and waiting on its card — the chat open, then minimized: the step
    // said once, on the follow card)
    Scene { name: "b1-read-open", claude: "nowread", files: MAIN, keys: B1_KEYS[0] },
    Scene { name: "b1-read-min", claude: "nowread", files: MAIN, keys: B1_KEYS[1] },
    Scene { name: "b1-run-open", claude: "now", files: MAIN, keys: B1_KEYS[2] },
    Scene { name: "b1-run-min", claude: "now", files: MAIN, keys: B1_KEYS[3] },
    Scene { name: "b1-wait-open", claude: "nowcard", files: MAIN, keys: B1_KEYS[4] },
    Scene { name: "b1-wait-min", claude: "nowcard", files: MAIN, keys: B1_KEYS[5] },
    // (…and an edit being written, its call not made: the head says what it writes — no knot in hand says it)
    Scene { name: "b1-write-min", claude: "nowwrite", files: MAIN, keys: B1_KEYS[6] },
    // (#267: the follow card's size yours — dragged wide by its left line, dragged narrow, `C-w =` back to automatic)
    Scene { name: "f267-wide", claude: "now", files: MAIN, keys: F267_KEYS[0] },
    Scene { name: "f267-narrow", claude: "now", files: MAIN, keys: F267_KEYS[1] },
    Scene { name: "f267-auto", claude: "now", files: MAIN, keys: F267_KEYS[2] },
    // (focus A1: the answer's place — an answer with one placed and two place-less paragraphs, then one naming no place;
    // the chat open and closed, the line ends, then the answer's note's card)
    Scene { name: "a1-explain-open", claude: "focus-explain", files: MAIN, keys: A1_KEYS[0] },
    Scene { name: "a1-explain-open-card", claude: "focus-explain", files: MAIN, keys: A1_KEYS[1] },
    Scene { name: "a1-explain-closed", claude: "focus-explain", files: MAIN, keys: A1_KEYS[2] },
    Scene { name: "a1-explain-closed-card", claude: "focus-explain", files: MAIN, keys: A1_KEYS[3] },
    Scene { name: "a1-none-open", claude: "a1-none", files: MAIN, keys: A1_KEYS[0] },
    Scene { name: "a1-none-open-card", claude: "a1-none", files: MAIN, keys: A1_KEYS[4] },
    Scene { name: "a1-none-closed", claude: "a1-none", files: MAIN, keys: A1_KEYS[2] },
    Scene { name: "a1-none-closed-card", claude: "a1-none", files: MAIN, keys: A1_KEYS[5] },
    // (focus E1: the turn as a map)
    Scene { name: "e1-tree", claude: "e1-map", files: MAIN, keys: E1_TREE },
    Scene { name: "e1-outline", claude: "e1-map", files: MAIN, keys: E1_OUTLINE },
    Scene { name: "e1-closed", claude: "e1-map", files: MAIN, keys: E1_CLOSED },
    Scene { name: "e1-hand", claude: "e1-map", files: MAIN, keys: E1_HAND },
    Scene { name: "e1-folded", claude: "e1-map", files: MAIN, keys: E1_FOLDED },
    Scene { name: "e1-failed", claude: "e1-failed", files: MAIN, keys: E1_TREE },
    Scene { name: "e1-sessions", claude: "e1-map", files: MAIN, keys: E1_SESSIONS },
    Scene { name: "f0-card", claude: "f0-saw", files: MAIN, keys: F0_CARD },
    Scene { name: "f0-lines", claude: "f0-saw", files: MAIN, keys: F0_LINES },
    Scene { name: "f0-wide", claude: "f0-saw", files: MAIN, keys: F0_WIDE },
    Scene { name: "f0-edited", claude: "f0-saw", files: MAIN, keys: F0_EDITED },
    Scene { name: "f0-tested", claude: "f0-tested", files: MAIN, keys: F0_WIDE },
    Scene { name: "f0-failed", claude: "f0-failed", files: MAIN, keys: F0_WIDE },
    Scene { name: "f0-older", claude: "f0-saw", files: MAIN, keys: F0_OLDER },
    Scene { name: "f0-line", claude: "f0-saw", files: MAIN, keys: F0_LINE },
    // (focus A1c: the note card's role, on a real answer's shape)
    Scene { name: "a1c-end", claude: "a1c", files: SPOOL, keys: A1C_END },
    Scene { name: "a1c-n1", claude: "a1c", files: SPOOL, keys: A1C_N1 },
    Scene { name: "a1c-n2", claude: "a1c", files: SPOOL, keys: A1C_N2 },
    Scene { name: "a1c-n3", claude: "a1c", files: SPOOL, keys: A1C_N3 },
    Scene { name: "a1c-n4", claude: "a1c", files: SPOOL, keys: A1C_N4 },
    Scene { name: "a1c-n5", claude: "a1c", files: SPOOL, keys: A1C_N5 },
    Scene { name: "a1c-read", claude: "a1c", files: SPOOL, keys: A1C_READ },
    Scene { name: "a1c-doc", claude: "a1c", files: SPOOL, keys: A1C_DOC },
    Scene { name: "a1c-wide", claude: "a1c", files: SPOOL, keys: A1C_WIDE },
    // (#273: no follow card stays for a walk-through's stop after the turn)
    Scene { name: "a1c-f273-stop", claude: "a1c", files: SPOOL, keys: F273_STOP },
    Scene { name: "a1c-f273-ask", claude: "a1c", files: SPOOL, keys: F273_ASK },
    Scene { name: "a1c-f273-replay", claude: "a1c", files: SPOOL, keys: F273_REPLAY },
    Scene { name: "a1c-f273-replay-off", claude: "a1c", files: SPOOL, keys: F273_REPLAY_OFF },
    Scene { name: "a1c-f273-follow", claude: "a1c", files: SPOOL, keys: F273_FOLLOW },
    // (the flow plan's piece 1: the reader floats or docks, `space L` opens it at your line's paragraph)
    Scene { name: "flow1-l", claude: "a1c", files: SPOOL, keys: FLOW1_L },
    Scene { name: "flow1-float", claude: "a1c", files: SPOOL, keys: FLOW1_FLOAT },
    Scene { name: "flow1-dock", claude: "a1c", files: SPOOL, keys: FLOW1_DOCK },
    Scene { name: "flow1-float-l", claude: "a1c", files: SPOOL, keys: FLOW1_FLOAT_L },
    Scene { name: "flow1-float-read", claude: "a1c", files: SPOOL, keys: FLOW1_FLOAT_READ },
    Scene { name: "flow1-none-read", claude: "a1-none", files: MAIN, keys: FLOW1_NONE_READ },
    Scene { name: "flow1-flow-read", claude: "a1-flow", files: MAIN, keys: FLOW1_FLOW_READ },
    // (the flow plan's piece 2: the ask line, the default flipped, the column's tenants moved)
    Scene { name: "flow2-line", claude: "a1c", files: SPOOL, keys: FLOW2_LINE },
    Scene { name: "flow2-sel", claude: "a1c", files: SPOOL, keys: FLOW2_SEL },
    Scene { name: "flow2-quote", claude: "a1c", files: SPOOL, keys: FLOW2_QUOTE },
    Scene { name: "flow2-typing", claude: "a1c", files: SPOOL, keys: FLOW2_TYPING },
    Scene { name: "flow2-sent", claude: "a1c", files: SPOOL, keys: FLOW2_SENT },
    Scene { name: "flow2-answered", claude: "a1c", files: SPOOL, keys: FLOW2_ANSWERED },
    Scene { name: "flow2-grow", claude: "a1c", files: SPOOL, keys: FLOW2_GROW },
    Scene { name: "flow2-foot", claude: "a1c", files: SPOOL, keys: FLOW2_FOOT },
    Scene { name: "flow2-continue", claude: "a1c", files: SPOOL, keys: FLOW2_CONTINUE },
    // (the flow plan's piece 3: `◂ new`, `── new ──`, the reader opening by itself for a place-less answer and an error)
    Scene { name: "flow3-new", claude: "a1c", files: SPOOL, keys: FLOW3_NEW },
    Scene { name: "flow3-read", claude: "a1c", files: SPOOL, keys: FLOW3_READ },
    Scene { name: "flow3-docked", claude: "a1c", files: SPOOL, keys: FLOW2_FOOT },
    Scene { name: "flow3-pilcrow", claude: "a1c", files: SPOOL, keys: FLOW3_PILCROW },
    Scene { name: "flow3-none", claude: "flow3-none", files: MAIN, keys: FLOW3_ALONE },
    // (#389 step 4: `gd` on a name the answer's paragraph cites, `space k` on one in the lines it quotes, a word that
    // isn't code — the stand-in server answering from the project's files)
    Scene {
        name: "chatlink-gd",
        claude: "chatlink",
        files: MAIN,
        keys: chatlink_keys!("/", "Thread", "\r", "@0.5", "g", "d"),
    },
    Scene {
        name: "chatlink-hover",
        claude: "chatlink",
        files: MAIN,
        keys: chatlink_keys!("/", "fn length", "\r", "@0.5", " ", "k"),
    },
    Scene {
        name: "chatlink-miss",
        claude: "chatlink",
        files: MAIN,
        keys: chatlink_keys!("/", "printed", "\r", "@0.5", "g", "d"),
    },
    // (#389 step 5: a name the answer cites no place for, asked by name — `say` defined once, `tie` twice)
    Scene {
        name: "chatlink-named-one",
        claude: "chatlink-named",
        files: MAIN,
        keys: chatlink_keys!("/", "by say", "\r", "@0.5", "g", "d"),
    },
    Scene {
        name: "chatlink-named-list",
        claude: "chatlink-named",
        files: MAIN,
        keys: chatlink_keys!("/", "by tie", "\r", "@0.5", "g", "d"),
    },
    // (#389 step 5b: names written as code underlined once they come to a definition — `say`, `tie`; `grow` (Claude's new
    // code) left plain; `for` a keyword, never asked, said at once)
    Scene { name: "chatlink-under", claude: "chatlink-under", files: MAIN, keys: chatlink_keys!("@1") },
    Scene {
        name: "chatlink-keyword",
        claude: "chatlink-under",
        files: MAIN,
        keys: chatlink_keys!("/", "the fo", "\r", "@0.5", "g", "d"),
    },
    Scene {
        name: "chatlink-plain",
        claude: "chatlink-under",
        files: MAIN,
        keys: chatlink_keys!("/", "defines gro", "\r", "@0.5", "g", "d"),
    },
    // (the flow plan's piece 5: a message queued, steered, then heard at Claude's next tool answer; the ask line at work)
    Scene { name: "steer-queued", claude: "steer", files: MAIN, keys: STEER_QUEUED },
    Scene { name: "steer-steering", claude: "steer", files: MAIN, keys: STEER_STEERING },
    Scene { name: "steer-steered", claude: "steer", files: MAIN, keys: STEER_STEERED },
    Scene { name: "steer-ask", claude: "steer", files: MAIN, keys: STEER_ASK },
    Scene { name: "flow3-error", claude: "flow3-error", files: MAIN, keys: FLOW3_ALONE },
    Scene { name: "flow3-follow-keys", claude: "now", files: MAIN, keys: FLOW3_FOLLOW_KEYS },
    Scene { name: "flow3-follow-wider", claude: "now", files: MAIN, keys: FLOW3_FOLLOW_WIDER },
    Scene { name: "flow3-follow-head", claude: "now", files: MAIN, keys: FLOW3_FOLLOW_HEAD },
    Scene { name: "flow2-strip", claude: "idle", files: MAIN, keys: FLOW2_STRIP },
    Scene { name: "flow2-strip-l", claude: "idle", files: MAIN, keys: FLOW2_STRIP_L },
    // (#269: a folded doc comment stays folded while you select it)
    Scene { name: "f269-drag", claude: "idle", files: SPOOL, keys: F269_DRAG },
    Scene { name: "f269-into", claude: "idle", files: SPOOL, keys: F269_INTO },
    Scene { name: "f269-x", claude: "idle", files: SPOOL, keys: F269_X },
    Scene { name: "f269-xs", claude: "idle", files: SPOOL, keys: F269_XS },
    Scene { name: "f269-insert", claude: "idle", files: SPOOL, keys: F269_INSERT },
    Scene { name: "f269-back", claude: "idle", files: SPOOL, keys: F269_BACK },
    Scene { name: "f269-d", claude: "idle", files: SPOOL, keys: F269_D },
    Scene { name: "f269-dg", claude: "idle", files: SPOOL, keys: F269_DG },
    // (focus C1: `space n` at the lines it's about — the ask line, its lines quoted since #388 step 3; its answer in the
    // chat, the lines a flag of it)
    Scene { name: "c1-new", claude: "idle", files: MAIN, keys: C1_NEW },
    Scene { name: "c1-new-chat", claude: "idle", files: MAIN, keys: C1_NEW_CHAT },
    Scene { name: "c1-bottom", claude: "idle", files: MAIN, keys: C1_BOTTOM },
    Scene { name: "a1-line", claude: "focus-line", files: MAIN, keys: A1_LINE },
    Scene { name: "a1b-line-card", claude: "focus-line", files: MAIN, keys: A1B_LINE },
    Scene { name: "s3-ask", claude: "idle", files: MAIN, keys: S3_ASK },
    Scene { name: "s3-list", claude: "focus-line", files: MAIN, keys: S3_LIST },
    Scene { name: "a1b-flow-cut", claude: "a1-flow", files: MAIN, keys: A1B_CUT },
    // (focus A1b step 2: the conversation's answers kept, newest first, the newest opening itself)
    Scene { name: "a1b2-turn-open", claude: "a1b-two", files: MAIN, keys: A1B_TWO[0] },
    Scene { name: "a1b2-turn-closed", claude: "a1b-two", files: MAIN, keys: A1B_TWO[1] },
    Scene { name: "a1b2-earlier", claude: "a1b-two", files: MAIN, keys: A1B_TWO[2] },
    Scene { name: "a1b2-continued", claude: "a1b-two", files: MAIN, keys: A1B_TWO[3] },
    Scene { name: "a1b2-continued-card", claude: "a1b-two", files: MAIN, keys: A1B_TWO[4] },
    // (#388 step 2: the chat whole — an older answer where its stubs were, `]n`, `space N`'s flag rows)
    Scene { name: "whole-older", claude: "a1b-two", files: MAIN, keys: WHOLE_OLDER },
    Scene { name: "whole-older-up", claude: "a1b-two", files: MAIN, keys: WHOLE_OLDER_UP },
    Scene { name: "whole-all-placed", claude: "a1b-all", files: MAIN, keys: WHOLE_OLDER },
    Scene { name: "whole-next", claude: "a1b-two", files: MAIN, keys: WHOLE_NEXT },
    Scene { name: "whole-next-older", claude: "a1b-two", files: MAIN, keys: WHOLE_NEXT_OLDER },
    Scene { name: "whole-list", claude: "a1b-two", files: MAIN, keys: WHOLE_LIST },
    // (#388 step 1: an answer's places are flags at the code — lit, dim, hovered, pressed, beside a note)
    Scene { name: "flag-lit", claude: "a1b-two", files: MAIN, keys: FLAG_LIT },
    Scene { name: "flag-dim", claude: "a1b-two", files: MAIN, keys: FLAG_DIM },
    Scene { name: "flag-hover", claude: "a1b-two", files: MAIN, keys: FLAG_HOVER },
    Scene { name: "flag-rest", claude: "a1b-two", files: MAIN, keys: FLAG_REST },
    Scene { name: "flag-press", claude: "a1b-two", files: MAIN, keys: FLAG_PRESS },
    Scene { name: "flag-note", claude: "a1b-two", files: MAIN, keys: FLAG_NOTE },
    // (#271: a selection on an answer's line · #272: every row of a cut text its hover zone)
    Scene { name: "f271-x", claude: "f271", files: MAIN, keys: F271_X },
    Scene { name: "f272-first", claude: "f271", files: MAIN, keys: F272_FIRST },
    Scene { name: "f272-last", claude: "f271", files: MAIN, keys: F272_LAST },
    Scene { name: "f272-hand", claude: "f271", files: MAIN, keys: F272_HAND },
    Scene { name: "a1-flow-open-card", claude: "a1-flow", files: MAIN, keys: A1_FLOW[0] },
    Scene { name: "a1-flow-closed-card", claude: "a1-flow", files: MAIN, keys: A1_FLOW[1] },
    // (W: a word that does its key, lit under the mouse — the chat's `paused`, the follow card's keys, the chat's foot)
    Scene { name: "words-paused", claude: "now", files: MAIN, keys: WORDS_KEYS[0] },
    Scene { name: "words-card", claude: "now", files: MAIN, keys: WORDS_KEYS[1] },
    Scene { name: "words-foot", claude: "footshort", files: MAIN, keys: WORDS_KEYS[2] },
    Scene { name: "words-follow", claude: "footshort", files: MAIN, keys: WORDS_KEYS[3] },
    Scene { name: "words-tab", claude: "idle", files: &["src/main.rs", "src/wide.rs"], keys: WORDS_TAB },
    // (#200: a card's answers lit — focused, the mouse on `y`; waiting, on `C-q answer`)
    Scene { name: "words-answer", claude: "card", files: MAIN, keys: WORDS_ANSWER[0] },
    Scene { name: "words-answer-waiting", claude: "card", files: MAIN, keys: WORDS_ANSWER[1] },
    // (motion §4: a turn of 20 stops — reads, lookups, edits, runs, one failing — its trail rows; the follow card's strip)
    Scene { name: "strip-trail", claude: "strip", files: MAIN, keys: STRIP_KEYS[0] },
    Scene { name: "strip", claude: "strip", files: MAIN, keys: STRIP_KEYS[1] },
    Scene { name: "strip-hover", claude: "strip", files: MAIN, keys: STRIP_KEYS[2] },
    Scene { name: "strip-replay", claude: "strip", files: MAIN, keys: STRIP_KEYS[3] },
    Scene { name: "strip-inhand", claude: "strip", files: MAIN, keys: STRIP_KEYS[4] },
    Scene { name: "wetreview", claude: "sixend", files: MAIN, keys: WET_KEYS },
    // (Claude's edit of three changes: the first accepted, the second refused, the third in hand)
    Scene {
        name: "decided",
        claude: "three",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 3",
            "@1",
            "y",
            "@1",
            "n",
            "@1",
        ],
    },
    Scene {
        name: "chatdone",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
        ],
    },
    // (scrolling alike, uxaudit D7: the finished turn, `C-u` from the empty input — half the rows in sight)
    Scene {
        name: "scroll-chat",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x15",
            "@1",
        ],
    },
    // (Z2: the terminal's past by `S-pageup` while the shell has the keys — every terminal emulator's key)
    Scene {
        name: "scroll-term",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "T", "@3", "seq 1 300", "\r", "@?298", "@1", "\x1b[5;2~", "@1"],
    },
    // (D7: the wheel over `:procs` scrolls it, the selection stays — filling the screen, `C-w z`, so the wheel's
    // row is the panel's at 80×24 and 160×45 alike; a shell first, a row more)
    Scene {
        name: "scroll-procs",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "T",
            "@3",
            "\x1c",
            "@1",
            ":procs",
            "\r",
            "@2",
            "\x17",
            "z",
            "@1",
            "\x1b[<65;20;8M",
            "@1",
        ],
    },
    // (panes P3 step 4: `:procs` as a group — the table with the keys, a shell's row selected; at 80 stacked)
    Scene {
        name: "group-procs-list",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "T", "@3", "\x1c", "@1", "\x1b", "@1", ":procs", "\r", "@2", "G", "@1"],
    },
    // (…and its detail with the keys: what that row is — its command line whole, its parents up to tarae)
    Scene {
        name: "group-procs-detail",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1", " ", "T", "@3", "\x1c", "@1", "\x1b", "@1", ":procs", "\r", "@2", "G", "@1", "\t", "@1",
        ],
    },
    // (panes P3 step 5: the git tab with a changed file picked — `thread.rs`, its changes inline in the code while
    // the tab has the keys)
    Scene {
        name: "group-git-file",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "g", "@?thread.rs", "@1", "j", "@1", "k", "@2"],
    },
    // (the review's rail the same: `space v v`, the rail given the keys, `j` onto the next file — shown as the
    // preview, the rail keeping the keys)
    Scene {
        name: "group-rail-preview",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "v", "v", "@?2 files", "@2", " ", "e", "@1", "j", "@2"],
    },
    // (a drag over the code, over lines: what's selected keeps its syntax colours under the selection's ground)
    Scene {
        name: "selectcode",
        claude: "idle",
        files: MAIN,
        keys: &["@1", "@press?fn main", "@drag+20?println", "@release+20?println", "@1"],
    },
    // (several selections: `%` then `s threads` — the primary on the cursor's line, the others on lines of their own,
    // none of them reading as the cursor line)
    Scene { name: "selectmany", claude: "idle", files: MAIN, keys: &["@1", "%", "s", "threads", "\r", "@1"] },
    // (a review's hunk taken by `]g`: what's selected keeps the diff's and the syntax's colours)
    Scene {
        name: "reviewhunk",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "v", "v", "@?2 files", "@1", "\x1b", "@1", "]", "g", "@1"],
    },
    // (…and `enter` on its row: a buffer of its own — upright in the tabs, its hunks under it)
    Scene {
        name: "group-git-kept",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "g", "@?thread.rs", "@1", "j", "@1", "k", "@2", "\r", "@2"],
    },
    // (…and `esc`: the code has the keys, the index's gutter back)
    Scene {
        name: "group-git-code",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "g", "@?thread.rs", "@1", "j", "@1", "k", "@2", "\x1b", "@1"],
    },
    // (H1, Helix's picker keys: the file picker's hints, then `C-v` — the file opened beside, in a split — and `C-t`,
    // the preview put away)
    Scene {
        name: "picker-hints",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "f", "@?README", "READ", "@1"],
    },
    Scene {
        name: "picker-split",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "f", "@?README", "READ", "@1", "\x16", "@1"],
    },
    Scene {
        name: "picker-nopreview",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "f", "@?README", "READ", "@1", "\x14", "@1"],
    },
    // (the chat's view mode on the finished turn: `esc` from the input, up a few rows, a word and a line selected)
    Scene {
        name: "chatview",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            "@?normal · chat",
            "k",
            "k",
            "x",
            "w",
            "@1",
        ],
    },
    // (one cursor on screen, where the keys go: the finished turn, then the keys in the code — `esc` before view
    // mode, `esc esc` with it (`-1`/`-2`) —, in the chat's view mode, in the terminal's program from the code)
    // (view mode for a first-timer: the space menu's way in, a key it has no use for said with both ways on,
    // `y`'s copy said)
    Scene { name: "view-space", claude: "idle", files: MAIN, keys: &["@1", " ", "@?Close chat", "@1"] },
    // (an empty conversation: nothing to view — its input's hint row says `esc code`)
    Scene { name: "view-empty", claude: "idle", files: MAIN, keys: &["@1", " ", "l", "@?Ask about", "@1"] },
    Scene {
        name: "view-said",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            "@?normal · chat",
            "d",
            "@?read-only",
            "@1",
        ],
    },
    Scene {
        name: "view-copy",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            "@?normal · chat",
            "x",
            "y",
            "@?copied",
            "@1",
        ],
    },
    // (view P2, quoting: a selection on the conversation and its `enter quote` hint; `enter`'s chip over the input;
    // `space >` on the code's lines; the message sent with it, its chip a link; the terminal's rows quoted)
    Scene { name: "quote-view", claude: "full", files: MAIN, keys: QUOTE_VIEW },
    Scene { name: "quote-chip", claude: "full", files: MAIN, keys: QUOTE_CHIP },
    // (panes P2c: the transcript a document — `x` on a wrapped paragraph, a search for a phrase across a wrap, the
    // input a buffer of two lines under a quote's chip)
    Scene { name: "transcript-select", claude: "full", files: MAIN, keys: TRANSCRIPT_SELECT },
    Scene { name: "transcript-search", claude: "full", files: MAIN, keys: TRANSCRIPT_SEARCH },
    Scene { name: "input-lines", claude: "full", files: MAIN, keys: INPUT_LINES },
    Scene {
        name: "quote-code",
        claude: "idle",
        files: MAIN,
        keys: &["@1", "j", "j", "x", "x", " ", ">", "@?goes with it", "@1"],
    },
    Scene {
        name: "quote-sent",
        claude: "idle",
        files: MAIN,
        // (`C-f`: not following — the chat stays whole, the message's row in sight)
        keys: &[
            "@1",
            "j",
            "j",
            "x",
            "x",
            " ",
            ">",
            "@?goes with it",
            "what do these do?",
            "\r",
            "@?what do these",
            "\x06",
            "@?L3",
            "@1",
        ],
    },
    // (the terminal's view, rows selected — HEAD's build takes it too — then `enter`'s chip)
    Scene {
        name: "quote-termview",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1", " ", "T", "@3", "seq 1 40", "\r", "@?40", "@1", "\x1c", "@?view", "k", "k", "x", "x", "@1",
        ],
    },
    Scene {
        name: "quote-term",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "T",
            "@3",
            "seq 1 40",
            "\r",
            "@?40",
            "@1",
            "\x1c",
            "@?view",
            "k",
            "k",
            "x",
            "x",
            "@?enter quote",
            "\r",
            "@?goes with it",
            "@1",
        ],
    },
    Scene { name: "cursor-code-1", claude: "full", files: MAIN, keys: CURSOR_TURN_ESC1 },
    Scene { name: "cursor-code-2", claude: "full", files: MAIN, keys: CURSOR_TURN_ESC2 },
    Scene {
        name: "cursor-view",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            "@?normal · chat",
            "@1",
        ],
    },
    Scene {
        name: "cursor-term-1",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            "@1",
            " ",
            "T",
            "@3",
        ],
    },
    Scene {
        name: "cursor-term-2",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            "@?normal · chat",
            "\x1b",
            "@1",
            " ",
            "T",
            "@3",
        ],
    },
    Scene {
        name: "turn",
        claude: "full",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 1",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            " ",
            "v",
            "t",
            "@2",
        ],
    },
    // (the same turn of three changes, decided — accepted, refused, accepted — then `space v t` on it, its file
    // marked viewed: kept)
    Scene {
        name: "decidedturn",
        claude: "three",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "why does insert clone the name?",
            "\r",
            "@?Change 1 of 3",
            "@1",
            "y",
            "@1",
            "n",
            "@1",
            "y",
            "@20?Nothing else",
            "@1",
            "\x1b",
            " ",
            "v",
            "t",
            "@2",
            " ",
            "v",
            "m",
            "@1",
        ],
    },
    // (idle compaction, `idlecompact.rs`: a big conversation on the 1h cache — the chat's last row says when tarae
    // compacts it if it's left idle, `compacts 14:26` beside `warm until 14:32`; at 80 columns, as it folds)
    Scene {
        name: "compactnext",
        claude: "big",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "how is the map filled?", "\r", "@?warm until ", "@1"],
    },
    // (the chat's foot: a long answer — the transcript's thumb in its outer cell — with the plan's weekly window at
    // 41 % (not on the row) and at 93 % (on it, with its reset); a short one, no thumb)
    Scene { name: "chatfoot41", claude: "foot41", files: MAIN, keys: FOOT },
    Scene { name: "chatfoot93", claude: "foot93", files: MAIN, keys: FOOT },
    Scene { name: "chatfootshort", claude: "footshort", files: MAIN, keys: FOOT },
    // (a drag over the conversation: its view mode, what's between the press and the pointer selected, over rows)
    Scene {
        name: "chatdrag",
        claude: "footshort",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "how is the map filled?",
            "\r",
            " ",
            "L",
            "@30=done",
            "@1",
            "@press?one pass",
            "@drag+4?over the strands",
            "@drag+11?writes to it",
            "@release+11?writes to it",
            "@1",
        ],
    },
    // (a drag across two rows of a wrapped input: what's between the press and the pointer selected, insert mode kept)
    Scene {
        name: "inputdrag",
        claude: "footshort",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "Please keep the first strand whenever two strands share a name, and say which tests cover the entry change.",
            "@1",
            "@press?first strand",
            "@drag+4?which tests",
            "@release+4?which tests",
            "@1",
        ],
    },
    // (a run outside the sandbox: its card)
    Scene {
        name: "card",
        claude: "card",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "run the tests", "\r", "@?Its reason", "@1"],
    },
    // (panes P2b: the default layout — the side panel, the code, the chat — the keys back on the code by `C-w h`)
    Scene {
        name: "panes-default",
        claude: "idle",
        files: MAIN,
        keys: &["@1", " ", "e", "@1", " ", "l", "@1", "\x17", "h", "@1"],
    },
    // (two code panes side by side: the divider lit beside the one with the keys)
    Scene {
        name: "panes-split",
        claude: "idle",
        files: MAIN,
        keys: &["@1", "\x17", "v", "@1", "8", "j", "@1"],
    },
    // (the tests group: a failing test written and run, `C-w j` to the group, `C-w h` to its list — its title lit)
    Scene {
        name: "panes-tests",
        claude: "idle",
        files: &["src/thread.rs"],
        keys: &[
            "@1",
            "G",
            "o",
            "\r#[test]\rfn ties() {\rassert!(false, \"a knot slipped\");",
            "\x1b",
            ":w\r",
            "@1",
            " ",
            "x",
            "f",
            "@60?FAILURE",
            "@1",
            "\x17",
            "j",
            "@1",
            "\x17",
            "h",
            "@1",
        ],
    },
    // (a card is a pane that asks: `C-q` from the chat gives it the keys — it floats with them, the chat's again after)
    Scene {
        name: "panes-card",
        claude: "card",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "run the tests", "\r", "@?Its reason", "@1", "\x11", "@1"],
    },
    // (P3 step 7b: Claude's card comes while you're in the code — `j j` move the code, `y` yanks; it waits in its
    // faint frame, `▲1 waiting · C-q`)
    Scene { name: "focus-card", claude: "card", files: MAIN, keys: FOCUS_CARD },
    // (…`y` in the code: a yank — before 7b the card's yes, its run gone outside the sandbox)
    Scene { name: "focus-card-y", claude: "card", files: MAIN, keys: FOCUS_CARD_Y },
    // (…`C-q` focuses it: its frame lit, its answers its own)
    Scene { name: "focus-card-q", claude: "card", files: MAIN, keys: FOCUS_CARD_Q },
    // (#189: the mouse on a frame without the keys — its lines half lit: Claude's card, the follow card, `:messages`)
    Scene { name: "frame-hover-card", claude: "card", files: MAIN, keys: FRAME_HOVER[0] },
    Scene { name: "frame-hover-follow", claude: "now", files: MAIN, keys: FRAME_HOVER[1] },
    Scene { name: "frame-hover-messages", claude: "idle", files: MAIN, keys: FRAME_HOVER[2] },
    // (…a card your own key asked for over it — `:features-reset` — focused at once)
    Scene { name: "focus-card-yours", claude: "card", files: MAIN, keys: FOCUS_CARD_YOURS },
    // (one toast as it comes — news, never a pane with the keys: borderless on the peek's ground, a message a row)
    Scene { name: "toast-one", claude: "idle", files: MAIN, keys: &["@1", ":w\r", "@1"] },
    // (…three said together, two errors among them: one card, the newest on top)
    Scene { name: "toast-three", claude: "idle", files: MAIN, keys: TOAST_THREE },
    // (…an error alone, waiting for a key of yours)
    Scene { name: "toast-error", claude: "idle", files: MAIN, keys: &["@1", ":nope\r", "@3"] },
    // (…top right, beside main's ask waiting bottom right as a peek: the two one-row looks one family)
    Scene { name: "toast-peek", claude: "card", files: MAIN, keys: TOAST_PEEK },
    // (the polish week's landing 1, to be recorded: another session's card arriving while the session list floats)
    Scene { name: "waits-60s", claude: "cardlater", files: MAIN, keys: WAITS_60S },
    // (`:messages`, a floating list at the newest — the session list's frame)
    Scene { name: "messages", claude: "idle", files: MAIN, keys: MESSAGES },
    // (the same run asked again after a yes, the program it runs rebuilt since — git ignores it: the card's "Same as")
    Scene {
        name: "samebinary",
        claude: "samebinary",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "l",
            "@1",
            "run the probe",
            "\r",
            "@?Its reason",
            "@1",
            "\x11",
            "@1",
            "y",
            "@20?Same as card",
            "@1",
        ],
    },
    // (tarae's search of a folder outside the project: its read card, the pattern named)
    Scene {
        name: "searchcard",
        claude: "searchcard",
        files: MAIN,
        keys: &["@1", " ", "l", "@1", "find the card record", "\r", "@?goes to Claude", "@1"],
    },
    // (the board under way: an angle done, two reading, one waiting its turn — then, done, a confirmed finding's
    // card: the angles that found it, its skeptic's trace)
    Scene {
        name: "board",
        claude: "boardmid",
        files: MAIN,
        keys: &["@1", " ", "v", "C", "@?Start a board", "@1", "y", "@?2 found", "@1"],
    },
    // (the card a board asks on first: the session, what boards cost, the angles — frictions32)
    Scene {
        name: "boardcard",
        claude: "boardmid",
        files: MAIN,
        keys: &["@1", " ", "v", "C", "@?Start a board", "@1"],
    },
    // (the same on a narrow screen, the rail given the keys: the four angles folded into rows of marks —
    // `--size 74x30` is where it folds)
    Scene {
        name: "boardnarrow",
        claude: "boardmid",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "v",
            "C",
            "@?Start a board",
            "@1",
            "y",
            "@?the board reads",
            "@3",
            " ",
            "e",
            "@?skeptics",
            "@1",
        ],
    },
    Scene {
        name: "boardnote",
        claude: "boarddone",
        files: MAIN,
        keys: &["@1", " ", "v", "C", "@?Start a board", "@1", "y", "@30?the board is done", "@2"],
    },
    // (bgreview: the reviewer in the background, no review open — the status line says it: reading, then what it
    // found and the key that reads it; the board's head; then a review opened after a file changed, its rail's
    // `read … at` row)
    Scene {
        name: "bgreviewing",
        claude: "reviewing",
        files: MAIN,
        keys: &["@1", " ", "v", "c", "@?3 reads", "@3"],
    },
    Scene {
        name: "bgreviewdone",
        claude: "reviewdone",
        files: MAIN,
        keys: &["@1", " ", "v", "c", "@30?1 finding · space v r", "@3"],
    },
    Scene {
        name: "bgboard",
        claude: "boardmid",
        files: MAIN,
        keys: &["@1", " ", "v", "C", "@?Start a board", "@1", "y", "@?¶ board", "@3"],
    },
    // (two sessions, each its reviewer reading: the one in front says it on the status line, its tab without its
    // `¶` — the user's decision 2026-10-20 — the one behind on its tab)
    Scene {
        name: "bgsessions",
        claude: "reviewing",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "v",
            "c",
            "@?3 reads",
            // (another checkout beside it, opened as a session — the shot's own folders, not tarae's worktrees')
            ":",
            "sh git worktree add -q ../fix && echo checkout made",
            "\r",
            "@20?checkout made",
            ":",
            "workspace-open ../fix",
            "\r",
            "@3",
            ":",
            "e notes.txt",
            "\r",
            "i",
            "a note",
            "\x1b",
            ":",
            "w",
            "\r",
            "@1",
            " ",
            "v",
            "c",
            "@?3 reads",
            " ",
            "W",
            "@1",
            "1",
            "@1",
            " ",
            "l",
            "@?¶ reviewing",
            "@2",
        ],
    },
    // (collab C1: the main checkout's Claude writing src/main.rs while you read the same file in another checkout
    // — its lines marked there, its name tag at the first)
    Scene { name: "presence", claude: "writemid", files: MAIN, keys: PRESENCE },
    // (…the list says where each session is)
    Scene { name: "presencelist", claude: "writemid", files: MAIN, keys: PRESENCE_LIST },
    // (…the file tree marks the file another session is in)
    Scene { name: "presencefiles", claude: "writemid", files: MAIN, keys: PRESENCE_FILES },
    // (…`space F`: following the next session at work takes you into its checkout, `● follow <name>`)
    Scene { name: "presencefollow", claude: "writemid", files: MAIN, keys: PRESENCE_FOLLOW },
    // (…`:procs`: each session under its heading, main's the working sign while it writes)
    Scene { name: "presenceprocs", claude: "writemid", files: MAIN, keys: PRESENCE_PROCS },
    // (…the sessions' strip over a chat: every session's name in its colour, what it does after it)
    Scene { name: "presencestrip", claude: "writemid", files: MAIN, keys: PRESENCE_STRIP },
    // (…a session waiting on you behind — its card parked: the strip and the list say it in warning)
    Scene { name: "presencewaits", claude: "card", files: MAIN, keys: PRESENCE_WAITS },
    Scene { name: "presencewaitslist", claude: "card", files: MAIN, keys: PRESENCE_WAITS_LIST },
    // (…two Claudes writing one file in two checkouts, each its colour; follow holds the view, your cursor elsewhere)
    Scene { name: "presencetwo", claude: "writemid", files: MAIN, keys: PRESENCE_TWO },
    // (one wave on the screen, the user 2026-11-03: two Claudes at work — the follow card's head waves, the status line
    // and the strip still; follow off, the status line's; the session list over it, its rows' `working`)
    Scene { name: "sign1-card", claude: "writemid", files: MAIN, keys: PRESENCE_TWO },
    Scene { name: "sign1-nocard", claude: "writemid", files: MAIN, keys: SIGN1_NOCARD },
    Scene { name: "sign1-list", claude: "writemid", files: MAIN, keys: SIGN1_LIST },
    // (collab C2: the same line changed and saved two ways in two checkouts — the toast, the strip, the list, the tree)
    Scene { name: "overlap", claude: "writemid", files: MAIN, keys: OVERLAP },
    Scene { name: "overlapstrip", claude: "writemid", files: MAIN, keys: OVERLAP_STRIP },
    Scene { name: "overlaplist", claude: "writemid", files: MAIN, keys: OVERLAP_LIST },
    // (panes P3 step 7: the session list floating over the code with main's chat open — the manager's overview: fix
    // working, overlapping main and a commit behind it; docs waiting on its card, behind too)
    Scene { name: "sessions-float", claude: "manager", files: MAIN, keys: SESSIONS_FLOAT },
    Scene { name: "overlapfiles", claude: "writemid", files: MAIN, keys: OVERLAP_FILES },
    // (collab C2b: the woven view — a conflict side by side and stacked, a clean weave of three in their colours, and a
    // removal's head where its lines went)
    Scene { name: "weave", claude: "writemid", files: MAIN, keys: WEAVE },
    Scene { name: "weaveclean", claude: "writemid", files: MAIN, keys: WEAVE_CLEAN },
    Scene { name: "weavegone", claude: "writemid", files: MAIN, keys: WEAVE_GONE },
    // (collab C3: main moved — one card for every workspace behind it, what would conflict where)
    Scene { name: "basemoved", claude: "writemid", files: MAIN, keys: BASE_MOVED },
    // (…as the second short video, V2: from the floating list to `y`)
    Scene { name: "main-moved", claude: "idle", files: MAIN, keys: MAIN_MOVED },
    Scene { name: "edit-in-buffer", claude: "v3", files: MAIN, keys: EDIT_IN_BUFFER },
    Scene { name: "follow-replay", claude: "v4", files: SPOOL, keys: FOLLOW_REPLAY },
    Scene {
        name: "three-claudes",
        claude: "main=v1-main,fix=v1-fix,docs=v1-docs",
        files: MAIN,
        keys: THREE_CLAUDES,
    },
    Scene {
        name: "managers-view",
        claude: "main=v6-main,fix=v6-fix,docs=v6-docs",
        files: MAIN,
        keys: MANAGERS_VIEW,
    },
    // (polish landing 4: the one bring card — rebased, its quick preflight, brought — said in the strip as it goes)
    Scene { name: "bring", claude: "idle", files: MAIN, keys: BRING },
    // (panes P3 step 7d: Claude's card with the chat closed — the screen's right, above the status line)
    Scene { name: "asks-closed", claude: "card", files: MAIN, keys: ASKS_CLOSED },
    // (…and with the chat open — `space l` brings it back from following: over its foot, the transcript above it)
    Scene { name: "asks-open", claude: "card", files: MAIN, keys: ASKS_OPEN },
    // (…four asks over two sessions in one stack: two cards your keys asked for in front, fix's Claude's behind them,
    // main's a peek named in its colour)
    Scene { name: "asks-stack", claude: "card", files: MAIN, keys: ASKS_STACK },
    // (#244: the chat's transcript beside an ask over it — the card waiting with the keys in the code, then focused)
    Scene { name: "asks-beside", claude: "cardlong", files: MAIN, keys: ASKS_BESIDE },
    Scene { name: "asks-beside-focused", claude: "cardlong", files: MAIN, keys: ASKS_BESIDE_FOCUSED },
    // (orchestra P1a: Claude asks to start sessions — one card for the set, focused: one, three, a long brief cut)
    Scene { name: "sessionset-one", claude: "startone", files: MAIN, keys: SESSION_SET },
    Scene { name: "sessionset-three", claude: "startthree", files: MAIN, keys: SESSION_SET },
    Scene { name: "sessionset-long", claude: "startlong", files: MAIN, keys: SESSION_SET },
    // (…and waiting unfocused, as it comes: its frame faint, `C-q answer` its bottom line's first pair)
    Scene { name: "sessionset-waits", claude: "startthree", files: MAIN, keys: SESSION_SET_WAITS },
    // (…its `y`, and the child's chat: main's brief its first message, headed `main's Claude`, a step down from yours)
    Scene {
        name: "sessionset-child",
        claude: "main=startgo,knot-count=childcount",
        files: MAIN,
        keys: SESSION_SET_CHILD,
    },
    // (orchestra P2: the set card with what the last ones cost and that main is woken; main woken; a child's end offered
    // as its bring lands; main's row with its children's cost)
    Scene { name: "orchestra-estimate", claude: "startthreewake", files: MAIN, keys: SESSION_SET },
    Scene {
        name: "orchestra-woke",
        claude: "main=parentwake,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_WOKE,
    },
    Scene {
        name: "orchestra-end",
        claude: "main=startgo,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_END,
    },
    Scene {
        name: "orchestra-list-cost",
        claude: "main=startgo3,knot-count=childcost,thread-docs=card,spool-clones=writemid",
        files: MAIN,
        keys: ORCHESTRA_LIST,
    },
    // (orchestra P3: main's Claude proposes knot-count's bring, and asks for a board on it — the real cards, their
    // first line who proposed them and why)
    Scene {
        name: "orchestra-propose-bring",
        claude: "main=parentpropose,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_PROPOSE_BRING,
    },
    Scene {
        name: "orchestra-propose-bring-waits",
        claude: "main=parentpropose,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_PROPOSE_BRING_WAITS,
    },
    Scene {
        name: "orchestra-propose-board",
        claude: "main=parentpropose,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_PROPOSE_BOARD,
    },
    // (orchestra P1b: three children of main's after it in the list and on the chat's top line, a faint `›` each; fix,
    // a session of yours, after them)
    Scene {
        name: "orchestra-list",
        claude: "main=startgo3,knot-count=childcount,thread-docs=card,spool-clones=writemid",
        files: MAIN,
        keys: ORCHESTRA_LIST,
    },
    Scene {
        name: "orchestra-strip",
        claude: "main=startgo3,knot-count=childcount,thread-docs=card,spool-clones=writemid",
        files: MAIN,
        keys: ORCHESTRA_STRIP,
    },
    // (orchestra P1b, the plan's `orchestraparent`: the parent's chat after news and a `sessions` call — a short report's
    // head, a long one's; its fresh send; the child's new conversation)
    Scene {
        name: "orchestra-parent",
        claude: "main=parent,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_PARENT,
    },
    Scene {
        name: "orchestra-parent-long",
        claude: "main=parent,knot-count=childlong",
        files: MAIN,
        keys: ORCHESTRA_PARENT,
    },
    Scene {
        name: "orchestra-fresh-parent",
        claude: "main=parent,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_FRESH_PARENT,
    },
    Scene {
        name: "orchestra-fresh",
        claude: "main=parent,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_FRESH,
    },
    // (…the mouse on each new row's `…`)
    Scene {
        name: "orchestra-parent-hover",
        claude: "main=parent,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_PARENT_HOVER,
    },
    Scene {
        name: "orchestra-fresh-parent-hover",
        claude: "main=parent,knot-count=childcount",
        files: MAIN,
        keys: ORCHESTRA_FRESH_PARENT_HOVER,
    },
    // (…the follow card framed, one session: named `follow`, the turn's time at its line's right)
    // (overlays A1: the demo's keycast row in the asks' corner — alone, and gone while Claude's ask stands there)
    Scene { name: "overlay-keys", claude: "idle", files: MAIN, keys: OVERLAY_KEYS },
    Scene { name: "overlay-keys-ask", claude: "card", files: MAIN, keys: OVERLAY_KEYS_ASK },
    Scene { name: "follow-one", claude: "writemid", files: MAIN, keys: FOLLOW_ONE },
    // (panes P3 step 8: per-pane follow — your keys in the other pane on the same file, on another, and `C-f` moving it)
    Scene { name: "follow-pane", claude: "writemid", files: MAIN, keys: FOLLOW_PANE },
    Scene { name: "follow-pane-files", claude: "writemid", files: MAIN, keys: FOLLOW_PANE_FILES },
    Scene { name: "follow-pane-cf", claude: "writemid", files: MAIN, keys: FOLLOW_PANE_CF },
    // (#157: the follow card in a split's top pane, the lines read under its foot — `--size 160x40`)
    Scene { name: "follow-split-s", claude: "readlast", files: WIDE, keys: FOLLOW_SPLIT[0] },
    Scene { name: "follow-split-v", claude: "readlast", files: WIDE, keys: FOLLOW_SPLIT[1] },
    // (…the chat's top line with nine sessions: three waiting, two overlapping, one done — whole names, `+N`)
    Scene { name: "strip-nine", claude: "nine", files: MAIN, keys: STRIP_NINE },
    // (#237: `follow` keeps its room after the names, 2, 5 and 9 sessions — and `no sandbox` (`llm.sandbox = false`,
    // the names ending `-nosandbox`) its own, before `follow`)
    Scene { name: "f237-two", claude: "nine", files: MAIN, keys: F237_TWO },
    Scene { name: "f237-five", claude: "nine", files: MAIN, keys: F237_FIVE },
    Scene { name: "f237-nine", claude: "nine", files: MAIN, keys: STRIP_NINE },
    Scene { name: "f237-five-nosandbox", claude: "nine", files: MAIN, keys: F237_FIVE },
    Scene { name: "f237-nine-nosandbox", claude: "nine", files: MAIN, keys: STRIP_NINE },
    // (…the session list docked right, `enter` on the next row: switched, the list still there)
    Scene { name: "sessions-docked", claude: "card", files: MAIN, keys: SESSIONS_DOCKED },
    // (the hover audit, 2026-10-28: the pointer over each surface that reacts — and a few that shouldn't — at 140 wide)
    Scene { name: "hover-code", claude: "idle", files: MAIN, keys: HOVER_KEYS[0] },
    Scene { name: "hover-code-own", claude: "idle", files: MAIN, keys: HOVER_KEYS[1] },
    Scene { name: "hover-divider", claude: "idle", files: MAIN, keys: HOVER_KEYS[2] },
    Scene { name: "hover-split-pane", claude: "idle", files: MAIN, keys: HOVER_KEYS[3] },
    Scene { name: "hover-slot-line", claude: "idle", files: MAIN, keys: HOVER_KEYS[4] },
    Scene { name: "hover-side-row", claude: "idle", files: MAIN, keys: HOVER_KEYS[5] },
    Scene { name: "hover-rail", claude: "idle", files: MAIN, keys: HOVER_KEYS[6] },
    Scene { name: "hover-chat-name", claude: "card", files: MAIN, keys: HOVER_CHAT_NAME },
    Scene { name: "hover-chat-more", claude: "nine", files: MAIN, keys: HOVER_CHAT_MORE },
    Scene { name: "hover-chat-tool", claude: "cuthand", files: MAIN, keys: HOVER_KEYS[7] },
    Scene { name: "hover-chat-type", claude: "cuthand", files: MAIN, keys: HOVER_KEYS[8] },
    Scene { name: "hover-tabs", claude: "idle", files: TWO, keys: HOVER_KEYS[9] },
    // (#215: the buffer names beside the chat — the pointer on one, the keys in the code; then pressed)
    Scene {
        name: "hover-tabs-chat",
        claude: "idle",
        files: TWO,
        keys: &["@1", " ", "l", "@1", "\x17", "h", "@1", "@hover?thread.rs", "@1"],
    },
    Scene {
        name: "press-tabs-chat",
        claude: "idle",
        files: TWO,
        keys: &["@1", " ", "l", "@1", "\x17", "h", "@1", "@press?thread.rs", "@release?thread.rs", "@1"],
    },
    Scene { name: "hover-waiting", claude: "card", files: MAIN, keys: HOVER_KEYS[10] },
    Scene { name: "hover-answer", claude: "card", files: MAIN, keys: HOVER_KEYS[11] },
    Scene { name: "hover-peek", claude: "card", files: MAIN, keys: HOVER_PEEK },
    Scene { name: "hover-toast", claude: "idle", files: MAIN, keys: HOVER_KEYS[12] },
    Scene { name: "hover-follow", claude: "now", files: MAIN, keys: HOVER_KEYS[13] },
    Scene { name: "hover-messages", claude: "idle", files: MAIN, keys: HOVER_KEYS[14] },
    Scene { name: "hover-sessions", claude: "card", files: MAIN, keys: HOVER_SESSIONS },
    Scene { name: "hover-picker", claude: "idle", files: MAIN, keys: HOVER_KEYS[15] },
    Scene { name: "hover-mode", claude: "idle", files: MAIN, keys: HOVER_KEYS[16] },
    Scene { name: "hover-gutter", claude: "idle", files: MAIN, keys: HOVER_KEYS[17] },
    // (kinds C: the trust card of forty tasks, cut to fit, then read to its end by the wheel — its width and its left
    // edge kept)
    Scene { name: "kindsc-card", claude: "idle", files: MAIN, keys: KINDSC_CARD[0] },
    Scene { name: "kindsc-card-end", claude: "idle", files: MAIN, keys: KINDSC_CARD[1] },
    Scene { name: "hover-side-edge", claude: "idle", files: MAIN, keys: HOVER_MORE[0] },
    Scene { name: "hover-divider-off", claude: "idle", files: MAIN, keys: HOVER_MORE[1] },
    Scene { name: "hover-hsplit-line", claude: "idle", files: MAIN, keys: HOVER_MORE[2] },
    Scene { name: "hover-path-name", claude: "idle", files: MAIN, keys: HOVER_MORE[3] },
    Scene { name: "hover-rail-keys", claude: "idle", files: MAIN, keys: HOVER_MORE[4] },
    // (piece B round 2: a comment saved as a draft, the rail's foot `space v s send 1` — plain faint words, the pointer
    // on them lighting nothing)
    Scene {
        name: "hover-rail-send",
        claude: "idle",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "v",
            "v",
            "@?knot.rs",
            "@1",
            "\x17",
            "l",
            "@1",
            " ",
            "n",
            "@1",
            "tighten this",
            "\r",
            "@?send 1",
            "@1",
            "@hover?send 1",
            "@1",
        ],
    },
    // (the docs' `claude-reviews.png`: the reviewer read in the background — three findings on thread.rs's lines,
    // one's card opened under it (`enter` on its line: a card opens by an act only), its report beside it in a split
    // (`space v r` — read, the status line's `¶ 3 findings` goes, as decided))
    Scene {
        name: "bgreviewnote",
        claude: "reviewfull",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "v",
            "c",
            "@30?3 findings · space v r",
            ":",
            "e src/thread.rs",
            "\r",
            "@?Ties a knot",
            ":",
            "vsplit",
            "\r",
            "@1",
            " ",
            "v",
            "r",
            "@?Verdict",
            "\x17",
            "h",
            "@1",
            ":",
            "15",
            "\r",
            "@8",
            "\r",
            "@1",
        ],
    },
    // (the same without the report: the findings on their lines, one's card, the status line's `¶ 3 findings ▲ ·
    // space v r` — the docs' picture's other candidate)
    Scene {
        name: "bgreviewfindings",
        claude: "reviewfull",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "v",
            "c",
            "@30?3 findings · space v r",
            ":",
            "e src/thread.rs",
            "\r",
            "@?Ties a knot",
            ":",
            "15",
            "\r",
            "@8",
            "\r",
            "@1",
        ],
    },
    Scene {
        name: "bgreviewrail",
        claude: "reviewdone",
        files: MAIN,
        keys: &[
            "@1",
            " ",
            "v",
            "c",
            "@30?1 finding · space v r",
            "@1",
            "g",
            "g",
            "O",
            "/",
            "/",
            " ",
            "x",
            "\x1b",
            ":",
            "w",
            "\r",
            "@1",
            " ",
            "v",
            "v",
            "@?changed since",
            "@1",
        ],
    },
];

/// `scenes --compare A B`: each shot in `A` against `B`'s of its name, pixel for pixel — `same`, or the box the
/// differing pixels lie in, to look at there (a turn's time or a temp path moves from run to run).
fn compare(a: &Path, b: &Path) -> Result<()> {
    let mut names: Vec<String> = std::fs::read_dir(a)
        .map_err(|e| format!("{}: {e}", a.display()))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".png"))
        .collect();
    names.sort();
    let (mut same, mut unreadable) = (0, 0);
    for n in &names {
        let c = compared(a, b, n);
        same += usize::from(c == Compared::Same);
        unreadable += usize::from(matches!(c, Compared::Unreadable(_)));
        println!("{n}: {}", c.said());
    }
    println!("{same} of {} the same", names.len());
    match unreadable {
        0 => Ok(()),
        n => Err(format!("{n} couldn't be read as pictures")),
    }
}

/// How a shot compares with its twin of the same name.
#[derive(Debug, PartialEq)]
enum Compared {
    Same,
    /// What differs: the box the differing pixels lie in, or the two sizes.
    Differs(String),
    /// No twin.
    New,
    /// One of the two couldn't be read as a PNG: which, and why.
    Unreadable(String),
}

impl Compared {
    fn said(&self) -> String {
        match self {
            Compared::Same => "same".into(),
            Compared::Differs(how) => format!("differs {how}"),
            Compared::New => "new — none to compare it with".into(),
            Compared::Unreadable(why) => format!("can't be compared: {why}"),
        }
    }
}

/// Shot `n` in `a` against its twin in `b`, pixel for pixel.
fn compared(a: &Path, b: &Path, n: &str) -> Compared {
    compared_but(a, b, n, &|_, _, _, _| false)
}

/// `compared`, passing over the pixels `left_out(x, y, w, h)` names in a `w`×`h` shot (a docs row's cells that may
/// differ between takes — `docshots.rs`'s `varies`).
fn compared_but(
    a: &Path,
    b: &Path,
    n: &str,
    left_out: &dyn Fn(usize, usize, usize, usize) -> bool,
) -> Compared {
    let read = |p: &Path| -> Result<(u32, u32, Vec<u8>)> {
        let file = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut r = png::Decoder::new(io::BufReader::new(file))
            .read_info()
            .map_err(|e| format!("{}: {e}", p.display()))?;
        let mut buf = vec![0; r.output_buffer_size().ok_or("too big a shot")?];
        let info = r.next_frame(&mut buf).map_err(|e| format!("{}: {e}", p.display()))?;
        buf.truncate(info.buffer_size());
        Ok((info.width, info.height, buf))
    };
    if !b.join(n).exists() {
        return Compared::New;
    }
    let (x, y) = match (read(&a.join(n)), read(&b.join(n))) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => return Compared::Unreadable(e),
    };
    if (x.0, x.1) != (y.0, y.1) {
        return Compared::Differs(format!("in size: {}×{} against {}×{}", x.0, x.1, y.0, y.1));
    }
    let (w, h, px) = (x.0 as usize, x.1 as usize, x.2.len() / (x.0 * x.1).max(1) as usize);
    let mut boxed: Option<(usize, usize, usize, usize)> = None;
    for (i, (p, q)) in x.2.chunks(px.max(1)).zip(y.2.chunks(px.max(1))).enumerate() {
        let (cx, cy) = (i % w, i / w);
        if p != q && !left_out(cx, cy, w, h) {
            let (x0, y0, x1, y1) = boxed.unwrap_or((cx, cy, cx, cy));
            boxed = Some((x0.min(cx), y0.min(cy), x1.max(cx), y1.max(cy)));
        }
    }
    match boxed {
        None => Compared::Same,
        Some((x0, y0, x1, y1)) => {
            let cells = cells_differing(w, h, px.max(1), &x.2, &y.2, left_out);
            Compared::Differs(format!("in x {x0}..={x1}, y {y0}..={y1} (of {}×{}){cells}", x.0, x.1))
        }
    }
}

/// The cells two shots of one `w`×`h` size differ in, a row at a time (`row 3: 10–14, 40`): the grid as `shot` lays
/// it out — 9×21 pixels a cell, `PAD` around, at the scale that fits the picture whole; empty when none does.
fn cells_differing(
    w: usize,
    h: usize,
    px: usize,
    a: &[u8],
    b: &[u8],
    left_out: &dyn Fn(usize, usize, usize, usize) -> bool,
) -> String {
    let Some(s) = (1..=4).find(|s| {
        let (pad, cw, ch) = (shot::PAD * s, 9 * s, 21 * s);
        w > 2 * pad && h > 2 * pad && (w - 2 * pad).is_multiple_of(cw) && (h - 2 * pad).is_multiple_of(ch)
    }) else {
        return String::new();
    };
    let (pad, cw, ch) = (shot::PAD * s, 9 * s, 21 * s);
    let (cols, rows) = ((w - 2 * pad) / cw, (h - 2 * pad) / ch);
    let differs = |c: usize, r: usize| {
        (r * ch + pad..(r + 1) * ch + pad).any(|y| {
            (c * cw + pad..(c + 1) * cw + pad).any(|x| {
                let i = (y * w + x) * px;
                a[i..i + px] != b[i..i + px] && !left_out(x, y, w, h)
            })
        })
    };
    let mut said = Vec::new();
    for r in 0..rows {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for c in (0..cols).filter(|&c| differs(c, r)) {
            match runs.last_mut() {
                Some(run) if run.1 + 1 == c => run.1 = c,
                _ => runs.push((c, c)),
            }
        }
        if !runs.is_empty() {
            let runs: Vec<String> =
                runs.iter().map(|&(a, b)| if a == b { a.to_string() } else { format!("{a}–{b}") }).collect();
            said.push(format!("row {r}: {}", runs.join(", ")));
        }
    }
    format!("\n    cells (row: columns, from 0) — {}", said.join(" · "))
}

const SIZES: [(u16, u16); 2] = [(80, 24), (160, 45)];
const BACKGROUNDS: [&str; 2] = ["dark", "light"];

pub fn run(args: &[String]) -> Result<()> {
    if args.first().is_some_and(|a| a == "--claude") {
        let script = args.get(1).ok_or(USAGE)?;
        let script = script.as_str();
        let stdin = io::stdin();
        let pause = Duration::from_millis(40);
        return claude(script, stdin.lock(), &mut io::stdout().lock(), pause)
            .map_err(|e| format!("the stand-in claude: {e}"));
    }
    if args.first().is_some_and(|a| a == "--lsp") {
        let knows = args.get(1).is_some_and(|a| a == "knows");
        return lsp(&mut io::stdin().lock(), &mut io::stdout().lock(), knows)
            .map_err(|e| format!("the stand-in language server: {e}"));
    }
    if args.first().is_some_and(|a| a == "--compare") {
        let [_, a, b] = args else { return Err(USAGE.into()) };
        return compare(Path::new(a), Path::new(b));
    }
    if args.first().is_some_and(|a| a == "--docs") {
        return docshots::run(&args[1..]);
    }
    let (mut out, mut names) = (None, Vec::new());
    let mut sizes = None;
    let mut transparent = false;
    let mut recording = false;
    let mut video = false;
    let mut wire = false;
    let mut shorts = false;
    let mut tint = false;
    let mut backgrounds = BACKGROUNDS.to_vec();
    let mut bin: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => {
                SCENES.iter().for_each(|s| println!("{}", s.name));
                return Ok(());
            }
            "--out" => out = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
            // (in motion: every frame kept — `record`)
            "--record" => recording = true,
            // (…and made a video — `video.rs`)
            "--video" => (recording, video) = (true, true),
            // (…or only what a remote interface's wire would carry — `wire.rs`, the remote split's spike)
            "--wire" => (recording, wire) = (true, true),
            // (…a YouTube Short instead: upright, a camera over the screen, its sound — `video.rs`)
            "--shorts" => shorts = true,
            // (the polish plan's "cells tinted in the code" on the last frame — `tint_one`)
            "--tint" => tint = true,
            "--theme" => {
                let bg = it.next().ok_or(USAGE)?;
                backgrounds = vec![BACKGROUNDS.into_iter().find(|b| b == bg).ok_or(USAGE)?];
            }
            // (sizes instead of the two — each `--size` one more: a width where something folds)
            "--size" => {
                let s = it.next().ok_or(USAGE)?;
                let (c, r) = s.split_once('x').ok_or(USAGE)?;
                let size = (c.parse().map_err(|_| USAGE)?, r.parse().map_err(|_| USAGE)?);
                sizes.get_or_insert_with(Vec::new).push(size);
            }
            // (the transparent themes: what has no background of its own)
            "--transparent" => transparent = true,
            // (another build of tarae — main's, for the shots before a change: A/B)
            "--bin" => bin = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
            n if !n.starts_with('-') => names.push(n.to_string()),
            _ => return Err(USAGE.into()),
        }
    }
    if (recording || tint) && (names.is_empty() || transparent) || shorts && (!video || sizes.is_some()) {
        return Err(USAGE.into());
    }
    let form = if shorts { video::Form::Short } else { video::Form::Wide };
    let scenes = chosen(&names)?;
    let out = out.unwrap_or_else(|| {
        root().join(match (recording, tint) {
            (true, _) => "target/rec",
            (_, true) => "target/tint",
            _ => "target/scenes",
        })
    });
    let short = if shorts { video::short_size()? } else { RECORD_SIZE };
    let sizes_of = |_: &Scene| match &sizes {
        Some(s) => s.clone(),
        None if shorts => vec![short],
        None if recording => vec![RECORD_SIZE],
        None => SIZES.to_vec(),
    };
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let bin = match bin {
        Some(b) if b.is_absolute() && b.is_file() => b,
        Some(b) => return Err(format!("--bin {}: an absolute path to a tarae", b.display())),
        // (a recording's homes are its own: the grammars bundled, as the homepage's — else a scene with no Rust grammar
        // asks to download it, a card no one with it sees)
        None if recording => shot::build(&root(), "shot-docs", &["bundled-grammars"])?,
        None => shot::build_shot(&root())?,
    };
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let mut failed = Vec::new();
    for scene in scenes {
        for &bg in &backgrounds {
            for size in sizes_of(scene) {
                if wire {
                    // (one theme: the bytes are the same but for colours' digits)
                    let theme = if bg == "dark" { Bg::Dark } else { Bg::Light };
                    if bg != backgrounds[0] {
                        continue;
                    }
                    match take(scene, theme, size, &bin, &me, None, false) {
                        Ok(rec) => {
                            println!("{}", crate::wire::report(&rec.wire).line(scene.name, size));
                            if !rec.missed.is_empty() {
                                eprintln!("{}: {}", scene.name, rec.missed.join("; "));
                                failed.push(scene.name.to_string());
                            }
                        }
                        Err(e) => {
                            eprintln!("{}: {e}", scene.name);
                            failed.push(scene.name.to_string());
                        }
                    }
                    continue;
                }
                if recording {
                    let theme = if bg == "dark" { Bg::Dark } else { Bg::Light };
                    let at = match (video, shorts) {
                        (true, true) => out.join(format!("{}-{}-shorts", scene.name, theme.theme())),
                        (true, false) => out.join(format!("{}-{}-video", scene.name, theme.theme())),
                        _ => out.join(format!("{}-{}-{}", scene.name, theme.theme(), size.0)),
                    };
                    let made = match video {
                        true => video_of(scene, theme, size, form, &bin, &me, &at),
                        false => record(scene, theme, size, &bin, &me, &at),
                    };
                    if let Err(e) = made {
                        eprintln!("{}: {e}", at.display());
                        failed.push(at.display().to_string());
                    }
                    continue;
                }
                if tint {
                    let file = out.join(format!("{}-{bg}-{}.png", scene.name, size.0));
                    match tint_one(scene, bg, size, &bin, &me, &file) {
                        Ok(line) => println!("{line}"),
                        Err(e) => {
                            eprintln!("{}: {e}", file.display());
                            failed.push(file.display().to_string());
                        }
                    }
                    continue;
                }
                // (the transparent themes' apart: `--compare` matches shots by name)
                let clear = if transparent { "-transparent" } else { "" };
                let file = out.join(format!("{}-{bg}-{}{clear}.png", scene.name, size.0));
                if let Err(e) = one(scene, bg, size, transparent, &bin, &me, &file) {
                    eprintln!("{}: {e}", file.display());
                    failed.push(file.display().to_string());
                }
            }
        }
    }
    match failed.len() {
        0 => Ok(()),
        n if video => Err(format!("{n} videos failed: {}", failed.join(", "))),
        n if recording => Err(format!("{n} recordings failed: {}", failed.join(", "))),
        n => Err(format!("{n} shots failed: {}", failed.join(", "))),
    }
}

/// The scenes `names` names, in their order — every one when none is named.
fn chosen(names: &[String]) -> Result<Vec<&'static Scene>> {
    if names.is_empty() {
        return Ok(SCENES.iter().collect());
    }
    let find = |n: &String| {
        SCENES
            .iter()
            .find(|s| s.name == n)
            .ok_or_else(|| format!("no scene `{n}` — cargo xtask scenes --list"))
    };
    names.iter().map(find).collect()
}

/// One shot of `scene`: its demo crate, config and tarae's state in a fresh folder, removed after — on `bg`'s
/// transparent theme when `transparent`.
fn one(
    scene: &Scene,
    bg: &'static str,
    size: (u16, u16),
    transparent: bool,
    bin: &Path,
    me: &Path,
    out: &Path,
) -> Result<()> {
    let dir = Fresh::new("tarae-scene")?;
    let setup = prepare(scene, bg, transparent, bin, me, &dir.0, &dir.0.join("tarae-demo"))?;
    let keys: Vec<String> = scene.keys.iter().map(|k| k.to_string()).collect();
    shot::take(&setup, &keys, size, &dir.0.join("state"), out)
}

/// `scene` measured for the polish plan's "cells tinted in the code" (`--tint`): driven as `one` drives it, its regions
/// watched; on its last frame every cell a code pane holds (the last region holding it, as `cargo xtask focus` reads
/// them) on a ground other than the code's own — the commonest in the panes' first column, the gutter's edge no wash
/// reaches (the commonest of all is a wash where one covers most of the pane) — is tinted, counted by its colour. That
/// frame is saved at `out`. What it returns: the scene's row.
fn tint_one(
    scene: &Scene,
    bg: &'static str,
    size: (u16, u16),
    bin: &Path,
    me: &Path,
    out: &Path,
) -> Result<String> {
    // (the state — its UI socket — in the project, as `focus` keeps it: the sandbox's runs make sockets only there)
    let (own, dir) = (crate::focus::Own::new()?, Fresh::new("tarae-tint")?);
    let setup = prepare(scene, bg, false, bin, me, &dir.0, &dir.0.join("tarae-demo"))?;
    let mut keys: Vec<String> = scene.keys.iter().map(|k| k.to_string()).collect();
    if !keys.iter().any(|k| k == "@rec") {
        keys.insert(0, "@rec".into());
    }
    let (frames, told) = shot::record_watched(&setup, &keys, size, &own.0.join("state"), &[], &["regions"])?;
    let (n, screen) = frames.last().ok_or("no frame")?;
    let drawn = (told.iter().rev().filter_map(crate::focus::Drawn::of).find(|d| d.written <= *n))
        .ok_or("no regions told (an older tarae?)")?;
    // (the code pane holding a cell: its rect's left edge)
    let in_code = |x: usize, y: usize| {
        let held = drawn.regions.iter().rposition(|r| {
            let (rx, ry, w, h) = r.rect;
            (rx..rx + w).contains(&x) && (ry..ry + h).contains(&y)
        });
        held.filter(|&i| drawn.regions[i].kind == "code").map(|i| drawn.regions[i].rect.0)
    };
    let mut grounds: std::collections::BTreeMap<String, usize> = Default::default();
    let mut edge: std::collections::BTreeMap<String, usize> = Default::default();
    for y in 0..usize::from(size.1) {
        for x in 0..usize::from(size.0) {
            let (Some(left), Some(cell)) = (in_code(x, y), screen.cell(y as u16, x as u16)) else { continue };
            // (a wide character's second cell: the terminal's, no ground of tarae's)
            if cell.is_wide_continuation() {
                continue;
            }
            let colour = match cell.bgcolor() {
                vt100::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
                vt100::Color::Idx(i) => format!("idx{i}"),
                vt100::Color::Default => "default".into(),
            };
            if x == left {
                *edge.entry(colour.clone()).or_default() += 1;
            }
            *grounds.entry(colour).or_default() += 1;
        }
    }
    let fonts = shot::Fonts::load(1)?;
    let (w, h, px) = shot::render(screen, &fonts, 1);
    shot::save_png(out, w, h, &px)?;
    let own = edge.iter().max_by_key(|(c, k)| (**k, std::cmp::Reverse(*c))).map(|(c, _)| c.clone());
    let cells: usize = grounds.values().sum();
    let mut tinted: Vec<(String, usize)> =
        grounds.into_iter().filter(|(c, _)| Some(c) != own.as_ref()).collect();
    tinted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let others: Vec<String> = tinted.iter().map(|(c, k)| format!("{c} {k}")).collect();
    Ok(format!(
        "{:<16} {bg:<5} {:>3}×{:<3} code {cells:>5}  tinted {:>4}  (ground {})  {}",
        scene.name,
        size.0,
        size.1,
        tinted.iter().map(|t| t.1).sum::<usize>(),
        own.unwrap_or_default(),
        others.join(" · ")
    ))
}

/// The stand-in language server (`scenes --lsp`, `diag`'s): `initialize` answered with whole-text sync, each file
/// opened told an error on its 12th line and a warning on its 4th, every other request answered `null`; gone at `exit`
/// or at its input's end. `knows` (`scenes --lsp knows`, #389's scenes): no diagnostics, and a definition and a hover
/// answered from the project's own files — the name at the place asked, its `fn`/`struct`/… line found in them.
fn lsp(input: &mut impl BufRead, out: &mut impl Write, knows: bool) -> io::Result<()> {
    let mut root = std::path::PathBuf::new();
    let mut open: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    loop {
        let mut len = None;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                return Ok(());
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length:") {
                len = n.trim().parse::<usize>().ok();
            }
        }
        let Some(len) = len else { continue };
        let mut body = vec![0; len];
        io::Read::read_exact(input, &mut body)?;
        let msg: Value = serde_json::from_slice(&body).unwrap_or_default();
        let range = |line: u32, from: u32, to: u32| json!({"start": {"line": line, "character": from}, "end": {"line": line, "character": to}});
        match msg["method"].as_str() {
            Some("initialize") if knows => {
                let uri = msg["params"]["rootUri"].as_str().unwrap_or_default();
                root = std::path::PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri));
                let caps = json!({"textDocumentSync": 1, "hoverProvider": true, "definitionProvider": true,
                    "workspaceSymbolProvider": true, "positionEncoding": "utf-8"});
                lsp_send(out, &json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": caps}}))?
            }
            Some("initialize") => lsp_send(
                out,
                &json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {"textDocumentSync": 1}}}),
            )?,
            Some("textDocument/didOpen") if knows => {
                let d = &msg["params"]["textDocument"];
                open.insert(
                    d["uri"].as_str().unwrap_or_default().into(),
                    d["text"].as_str().unwrap_or_default().into(),
                );
            }
            Some("textDocument/didChange") if knows => {
                let p = &msg["params"];
                if let Some(t) = p["contentChanges"][0]["text"].as_str() {
                    open.insert(p["textDocument"]["uri"].as_str().unwrap_or_default().into(), t.into());
                }
            }
            Some(m @ ("textDocument/definition" | "textDocument/hover")) if knows => {
                let p = &msg["params"];
                let text = open.get(p["textDocument"]["uri"].as_str().unwrap_or_default());
                let (line, ch) = (p["position"]["line"].as_u64(), p["position"]["character"].as_u64());
                let word = text.zip(line.zip(ch)).and_then(|(t, (l, c))| lsp_word(t, l as usize, c as usize));
                let found = word.and_then(|w: String| lsp_defined(&root, &w));
                let result = match (m, found) {
                    (_, None) => Value::Null,
                    ("textDocument/definition", Some((path, l, c, _, _))) => {
                        let uri = format!("file://{}", path.display());
                        json!([{"uri": uri, "range": range(l as u32, c as u32, c as u32)}])
                    }
                    (_, Some((_, _, _, sig, docs))) => {
                        json!({"contents": {"kind": "markdown", "value": format!("```rust\n{sig}\n```\n{docs}")}})
                    }
                };
                lsp_send(out, &json!({"jsonrpc": "2.0", "id": msg["id"], "result": result}))?
            }
            // (#389 step 5: every definition of the name asked, its `impl`'s type its container)
            Some("workspace/symbol") if knows => {
                let query = msg["params"]["query"].as_str().unwrap_or_default();
                let found: Vec<Value> = lsp_defs(&root, query)
                    .into_iter()
                    .map(|(path, l, c, container)| {
                        json!({"name": query, "kind": 12, "containerName": container,
                            "location": {"uri": format!("file://{}", path.display()), "range": range(l as u32, c as u32, c as u32)}})
                    })
                    .collect();
                lsp_send(out, &json!({"jsonrpc": "2.0", "id": msg["id"], "result": found}))?
            }
            Some("textDocument/didOpen") => lsp_send(
                out,
                &json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
                    "uri": msg["params"]["textDocument"]["uri"],
                    "diagnostics": [
                        {"range": range(11, 61, 71), "severity": 1, "source": "rustc",
                         "message": "mismatched types: expected `usize`, found `u32`"},
                        {"range": range(3, 4, 18), "severity": 2, "source": "rustc",
                         "message": "unused import: `thread::Thread`"},
                    ],
                }}),
            )?,
            Some("exit") => return Ok(()),
            Some(_) if !msg["id"].is_null() => {
                lsp_send(out, &json!({"jsonrpc": "2.0", "id": msg["id"], "result": null}))?
            }
            _ => {}
        }
    }
}

/// The name at `line`, `ch` (bytes) of `text`.
fn lsp_word(text: &str, line: usize, ch: usize) -> Option<String> {
    let l = text.lines().nth(line)?;
    let w = |c: char| c.is_alphanumeric() || c == '_';
    let ch = ch.min(l.len());
    let start = l[..ch].rfind(|c: char| !w(c)).map_or(0, |i| i + 1);
    let end = l[ch..].find(|c: char| !w(c)).map_or(l.len(), |i| ch + i);
    (start < end).then(|| l[start..end].to_string())
}

/// Where `word` is defined in the project's `.rs` files: the file, the line and column of the name, its line (its
/// signature, the `{` off) and the doc comment above it.
fn lsp_defined(root: &Path, word: &str) -> Option<(std::path::PathBuf, usize, usize, String, String)> {
    let mut files = Vec::new();
    let mut dirs = vec![root.join("src")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            match p.is_dir() {
                true => dirs.push(p),
                false if p.extension().is_some_and(|x| x == "rs") => files.push(p),
                false => {}
            }
        }
    }
    files.sort();
    let kinds = ["fn ", "struct ", "enum ", "trait ", "type ", "const ", "mod "];
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let Some(c) = kinds.iter().find_map(|k| {
                let at = l.find(&format!("{k}{word}"))? + k.len();
                (!l[at + word.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')).then_some(at)
            }) else {
                continue;
            };
            let docs: Vec<&str> = lines[..i]
                .iter()
                .rev()
                .map(|l| l.trim())
                .take_while(|l| l.starts_with("///"))
                .map(|l| l.trim_start_matches('/').trim())
                .collect();
            let docs: Vec<&str> = docs.into_iter().rev().collect();
            let sig = l.trim().trim_end_matches('{').trim_end().to_string();
            return Some((path, i, c, sig, docs.join(" ")));
        }
    }
    None
}

/// Every place `word` is defined in the project's `.rs` files: the file, the line and column of the name, and the type
/// of the `impl` it's in (empty outside one).
fn lsp_defs(root: &Path, word: &str) -> Vec<(std::path::PathBuf, usize, usize, String)> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(root.join("src"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let mut container = String::new();
        for (i, l) in text.lines().enumerate() {
            if let Some(rest) = l.strip_prefix("impl ") {
                container = rest.split_whitespace().next().unwrap_or_default().to_string();
            } else if l == "}" {
                container.clear();
            }
            let at = ["fn ", "struct ", "enum ", "trait ", "type ", "const "].iter().find_map(|k| {
                let at = l.find(&format!("{k}{word}"))? + k.len();
                (!l[at + word.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')).then_some(at)
            });
            if let Some(c) = at {
                out.push((path.clone(), i, c, container.clone()));
            }
        }
    }
    out
}

/// One message out, as the protocol frames it.
fn lsp_send(out: &mut impl Write, v: &Value) -> io::Result<()> {
    let text = v.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{text}", text.len())?;
    out.flush()
}

/// `scene` in motion (`--record`): its keys as `one` sends them, every frame kept from its `@rec` (none: from the
/// start), in a folder of docshots' kind — homes and a clock of its own, nothing of yours on any frame —, into `at`:
/// `rec.json` (the homepage player's), `sheet.png` and its pages, `play.html`. A wait that timed out fails it, the take
/// written all the same.
fn record(scene: &Scene, bg: Bg, size: (u16, u16), bin: &Path, me: &Path, at: &Path) -> Result<()> {
    let rec = take(scene, bg, size, bin, me, Some(at), false)?;
    let beat_times: Vec<u64> = rec.beats.iter().map(|b| b.0).collect();
    let sampled = recording::sample(&rec.frames, &beat_times);
    let beats: Vec<(u64, &str)> = rec.beats.iter().map(|(t, c)| (*t, c.as_str())).collect();
    let (fg, ground) = recording::ground(bg);
    let value = recording::encode(&sampled, &beats, fg, ground);
    let json = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    write(&at.join("rec.json"), &json)?;
    let title = format!("{} · {} · {}×{}", scene.name, bg.theme(), size.0, size.1);
    write(&at.join("play.html"), &recording::page(&value, &title)?)?;
    let mut marks = rec.waits.clone();
    marks.extend(rec.beats.iter().filter(|b| !b.1.is_empty()).cloned());
    let shown = recording::sheet(&rec.frames, &marks, &shot::Fonts::load(1)?, &at.join("sheet.png"))?;
    let end = rec.frames.last().map_or(0, |f| f.0);
    println!(
        "saved {} ({:.1} s, {} frames, {} kept, {:.1} KB; sheet.png {shown} frames; play.html)",
        at.display(),
        end as f64 / 1000.0,
        rec.frames.len(),
        value["frames"].as_array().map_or(0, Vec::len),
        json.len() as f64 / 1024.0
    );
    match rec.missed.is_empty() {
        true => Ok(()),
        false => Err(format!("{} — the take is written all the same", rec.missed.join("; "))),
    }
}

/// The tarae log's sources a take keeps (their column, padded): an overlap round, the keeping close of a base, a card
/// answered or dropped.
const TOLD_LOG: [&str; 3] = [" overlap ", " keepclose ", " card "];

/// Each of `waits` (when it ended, what it waited for) with how long after the last of `keys` sent before it it came:
/// (what, when, ms).
fn waited(keys: &[(u64, String)], waits: &[(u64, String)]) -> Vec<(String, u64, u64)> {
    (waits.iter())
        .filter_map(|(end, what)| {
            let sent = keys.iter().rev().find(|k| k.0 <= *end)?;
            Some((what.clone(), *end, end - sent.0))
        })
        .collect()
}

/// `scene` driven as `record` and `video_of` take it, its frames from `@rec`: each one passed `none_of_yours`. Into
/// `at` (made) the take's tarae log lines of what moves a base and says it (`TOLD_LOG`) as `tarae.log`; each wait's
/// time from the key before it said (#324: a card late is seen in every take, not only one that times out). A video's
/// take (`video`) has tarae draw its fades and drying at the video's rate (`TARAE_SHOT_FRAME_MS`, `editor::tick`).
fn take(
    scene: &Scene,
    bg: Bg,
    size: (u16, u16),
    bin: &Path,
    me: &Path,
    at: Option<&Path>,
    video: bool,
) -> Result<shot::Recorded> {
    let dir = Named::new(&format!("rec-{}-{}-{}", scene.name, bg.theme(), std::process::id()))?;
    let real = std::fs::canonicalize(&dir.0).map_err(|e| format!("{}: {e}", dir.0.display()))?;
    let mut env_homes = homes(&real, &|k| std::env::var_os(k))?;
    env_homes.push(("TZ", fixed_clock(now()).into()));
    let demo = real.join("home/tarae-demo");
    let mut setup = prepare(scene, bg.word(), false, bin, me, &real, &demo)?;
    if video {
        setup.env.push(("TARAE_SHOT_FRAME_MS", Some(video::FRAME_MS)));
    }
    own_build_dir(&demo)?;
    let mut keys: Vec<String> = scene.keys.iter().map(|k| k.to_string()).collect();
    if !keys.iter().any(|k| k == "@rec") {
        keys.insert(0, "@rec".into());
    }
    let rec = shot::record(&setup, &keys, size, &real.join("state"), &env_homes)?;
    let waits = waited(&rec.keys, &rec.waits);
    if !waits.is_empty() {
        let said: Vec<String> = (waits.iter())
            .map(|(what, at, ms)| {
                format!("{what} {:.1} s ({:.1} s in)", *ms as f64 / 1000.0, *at as f64 / 1000.0)
            })
            .collect();
        println!("waits, from the key before each: {}", said.join("; "));
    }
    if let Some(at) = at {
        std::fs::create_dir_all(at).map_err(|e| format!("{}: {e}", at.display()))?;
        let log = std::fs::read_to_string(real.join("state/tarae/tarae.log")).unwrap_or_default();
        let told: Vec<&str> = log.lines().filter(|l| TOLD_LOG.iter().any(|w| l.contains(w))).collect();
        write(&at.join("tarae.log"), &(told.join("\n") + "\n"))?;
    }
    let yours = std::env::var_os("HOME");
    for (t, screen) in &rec.frames {
        none_of_yours(&recording::screen_cells(screen), yours.as_deref())
            .map_err(|e| format!("at {t} ms: {e}"))?;
    }
    Ok(rec)
}

/// `scene` as a short video into `at` (`--video`; `--shorts` a Short, its camera's moves and its sound said): taken as
/// `record` takes it, made by `video::make` — `<scene>.mp4` and `sheet.png` (a Short's `<scene>.wav` too). A wait that
/// timed out, or a Short's target never found, fails it, the video made all the same.
fn video_of(
    scene: &Scene,
    bg: Bg,
    size: (u16, u16),
    form: video::Form,
    bin: &Path,
    me: &Path,
    at: &Path,
) -> Result<()> {
    let started = std::time::Instant::now();
    let rec = take(scene, bg, size, bin, me, Some(at), true)?;
    let driven = started.elapsed();
    let title = video_title(scene.name);
    let mp4 = at.join(format!("{}.mp4", scene.name));
    let made = video::make(&rec, bg, title, &mp4, std::env::var_os("HOME").as_deref(), form)?;
    if form == video::Form::Short {
        println!("{}×{} cells; the camera:", size.0, size.1);
        made.zooms.iter().for_each(|z| println!("  {z}"));
        if let Some(s) = &made.sound {
            let brush = s.brush.map_or(String::new(), |(b, a)| {
                format!(", the brush {b:+.1} LU over the rest before, {a:+.1} after")
            });
            let aac = match s.encoded {
                Ok((i, tp)) => {
                    format!("the AAC (160 kb/s) {i:.1} LUFS, true peak {tp:.1} dBTP, PLR {:.1} dB", tp - i)
                }
                Err(ref e) => format!("the AAC not measured: {e}"),
            };
            let l = s.limited;
            let limited = format!(
                "the limiter took at most {:.1} dB from the keys (at {:.1} s){}",
                l.keys,
                l.keys_at,
                l.brush.map_or(String::new(), |(own, both)| format!(
                    ", {own:.1} dB from the brush, {both:.1} dB more where the two meet"
                )),
            );
            let tried: Vec<String> = (s.ceilings.iter())
                .map(|(c, tp)| match tp {
                    Ok(tp) => format!("{c:.1} → {tp:.1}"),
                    Err(e) => format!("{c:.1} → not measured ({e})"),
                })
                .collect();
            println!(
                "sound: {:.1} LUFS ({:+.1} dB), true peak {:.1} dBTP, PLR {:.1} dB, peak {:.1} dBFS, RMS {:.1} dBFS while \
                 sounding{brush}; {limited}; the limiter's ceiling → the AAC's true peak (dBTP): {}; {aac}",
                s.lufs,
                s.gain,
                s.true_peak,
                s.true_peak - s.lufs,
                s.peak,
                s.rms,
                tried.join(", ")
            );
        }
        if let Some((keys, shown, stamps, settles)) = made.heard {
            println!(
                "heard: {keys} keys typed, {shown} steps of Claude's writing (the typist strikes for some), {stamps} \
                 stamps, {settles} settles"
            );
        }
    }
    let kb = std::fs::metadata(&mp4).map_or(0.0, |m| m.len() as f64 / 1024.0);
    println!(
        "saved {} ({:.1} s of video at {} px a glyph, {} frames, {} of them differing from the one before; {kb:.0} KB; \
         driven {:.1} s, drawn and encoded {:.1} s; sheet.png {} frames)",
        mp4.display(),
        made.seconds,
        made.px,
        made.frames,
        made.differ,
        driven.as_secs_f64(),
        (started.elapsed() - driven).as_secs_f64(),
        made.sheet,
    );
    let missed: Vec<&String> = rec.missed.iter().chain(&made.missed).collect();
    match missed.is_empty() {
        true => Ok(()),
        false => Err(format!(
            "{} — the video is made all the same",
            missed.iter().map(|m| m.as_str()).collect::<Vec<_>>().join("; ")
        )),
    }
}

/// A video's line on its title card (≤ ~40 characters): what the scene shows, said once.
fn video_title(scene: &str) -> &'static str {
    match scene {
        "waits-60s" => "A card never covers what you opened",
        "main-moved" => "When main moves, every workspace hears",
        "edit-in-buffer" => "Claude's edit, in your buffer",
        "follow-replay" => "Watch it work: follow and replay",
        "three-claudes" => "Three Claudes. One file.",
        "managers-view" => "The manager's view",
        _ => "",
    }
}

/// A recording's cargo builds `demo` in its own `target/`: the toolchain's home is yours (`homes`), and a `build-dir`
/// your cargo config names there put your home on screen as a run of the scene's built (`~/.cargo/build/…` in its
/// output). In the folder above the demo, which cargo reads from any folder under it: every checkout beside the demo
/// (V6's docs runs its tests) builds in its own, and no git status shows it.
fn own_build_dir(demo: &Path) -> Result<()> {
    let above = demo.parent().ok_or("the demo has no folder above it")?;
    write(&above.join(".cargo/config.toml"), "[build]\nbuild-dir = \"{workspace-root}/target\"\n")
}

/// A shot's demo crate at `demo`, its config at `dir`'s `xdg` and what it seeds in `dir`'s `state`: tarae's setup for
/// `scene` on `bg` (its transparent theme when `transparent`).
fn prepare(
    scene: &Scene,
    bg: &'static str,
    transparent: bool,
    bin: &Path,
    me: &Path,
    dir: &Path,
    demo: &Path,
) -> Result<Setup> {
    demo_crate_at(demo, scene.name == "trusted", &run_files(scene.name), Some(&dates().0))?;
    let config = dir.join("xdg");
    // (the overlay scenes': the SDK's demo plugin, its permissions asked as it loads — `C-q y` in their keys)
    if scene.name.starts_with("overlay-") {
        let wasm =
            std::fs::read(root().join("plugin-sdk/demo.wasm")).map_err(|e| format!("demo.wasm: {e}"))?;
        std::fs::create_dir_all(config.join("tarae/plugins")).map_err(|e| format!("plugins: {e}"))?;
        std::fs::write(config.join("tarae/plugins/demo.wasm"), wasm)
            .map_err(|e| format!("demo.wasm: {e}"))?;
    }
    write(&config.join("tarae/config.toml"), &scene_config(scene, bg, transparent, me, dir))?;
    // (orchestra P2's set card: what the last five sessions started so cost, and three wakes — the call-order record
    // in the shot's own state, as tarae keeps it: `$state/tarae/calls/<month>.jsonl`)
    if scene.name == "orchestra-estimate" {
        let mut lines = String::new();
        for (k, cost) in [1.10, 1.40, 1.25, 1.62, 1.55].iter().enumerate() {
            lines +=
                &format!("{}\n", json!({"v": 1, "what": "chat", "child": format!("c1-0-{k}"), "cost": cost}));
            lines += &format!("{}\n", json!({"v": 1, "what": "child-whole", "child": format!("c1-0-{k}")}));
        }
        for cost in [0.04, 0.05, 0.06] {
            lines += &format!("{}\n", json!({"v": 1, "what": "chat", "wake": true, "cost": cost}));
        }
        // (this month as tarae names it — local and UTC alike, should they differ today)
        for utc in [false, true] {
            let mut date = std::process::Command::new("date");
            if utc {
                date.arg("-u");
            }
            let month = date.arg("+%Y-%m").output().map_err(|e| format!("date: {e}"))?;
            let month = String::from_utf8_lossy(&month.stdout).trim().to_string();
            write(&dir.join(format!("state/tarae/calls/{month}.jsonl")), &lines)?;
        }
    }
    Ok(Setup {
        bin: bin.to_path_buf(),
        files: scene.files.iter().map(|f| f.to_string()).collect(),
        cwd: demo.to_path_buf(),
        config,
        background: Some(bg),
        // (the terminal scenes': a plain sh of the shot's own — a `$ ` prompt, no rc, no history, no user or host
        // name; never the shell of whoever takes the shots. Inside zellij or not as `cargo xtask shot` is: `ZELLIJ`
        // taken out unless `TARAE_SHOT_ZELLIJ=1` — a scene taken from inside zellij said the twins)
        env: match scene.name.starts_with("quote-term") {
            true => {
                vec![("SHELL", Some("/bin/sh")), ("PS1", Some("$ ")), ("HISTFILE", Some("")), ("ENV", None)]
            }
            false => Vec::new(),
        }
        .into_iter()
        .chain(shot::shot_env(std::env::var_os("TARAE_SHOT_ZELLIJ")))
        .chain(commits_env())
        .collect(),
    })
}

/// When a scene's commits are, whenever the shots are taken (`git`'s `@<seconds> +0000`): the demo crate's first a day
/// before this run of the xtask began, those a scene makes (a `:sh git commit` in its keys, tarae's rebase or bring) two
/// hours before — the blame says `1d ago` and `2h ago` at every retake, never the seconds a scene took.
fn dates() -> &'static (String, String) {
    static DATES: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
    DATES.get_or_init(|| {
        let at = |ago: u64| format!("@{} +0000", docshots::now().saturating_sub(ago));
        (at(86_400), at(2 * 3600))
    })
}

/// Who and when every commit made during a scene is, for tarae and what it runs: `demo`, as the demo crate's first
/// commit (`demo_crate_with`), never whoever takes the shots — a `:sh git commit` reads your own git settings.
fn commits_env() -> Vec<(&'static str, Option<&'static str>)> {
    let at = dates().1.as_str();
    vec![
        ("GIT_AUTHOR_NAME", Some("demo")),
        ("GIT_AUTHOR_EMAIL", Some("demo@example.com")),
        ("GIT_COMMITTER_NAME", Some("demo")),
        ("GIT_COMMITTER_EMAIL", Some("demo@example.com")),
        ("GIT_AUTHOR_DATE", Some(at)),
        ("GIT_COMMITTER_DATE", Some(at)),
    ]
}

/// `scene`'s config as its take writes it (`dir` the take's folder): `config_toml`'s, and what the scene adds.
fn scene_config(scene: &Scene, bg: &str, transparent: bool, me: &Path, dir: &Path) -> String {
    let mut toml = config_toml(me, scene.claude);
    // (#237's: Claude's commands without a sandbox — the chat's top line says so)
    if scene.name.ends_with("-nosandbox") {
        toml = toml.replacen("[llm]\n", "[llm]\nsandbox = false\n", 1);
    }
    if scene.name == "diag" {
        toml = stand_in_lsp(me, toml);
    }
    if scene.name.starts_with("chatlink-") {
        toml = stand_in_lsp_as(me, toml, "knows");
    }
    if transparent {
        let theme = if bg == "dark" { "meok-transparent" } else { "hanji-transparent" };
        toml = format!("theme = \"{theme}\"\n{toml}");
    }
    // (a session's checkout made by tarae — orchestra's set — in the shot's own folder, never yours)
    with_worktrees(toml, &dir.join("worktrees"))
}

/// `toml` with `[workspace] worktrees` at `at` — in its own `[workspace]` where it has one (V6's `sync`): a table named
/// twice is no TOML, and tarae with no config of yours starts the real claude.
fn with_worktrees(toml: String, at: &Path) -> String {
    let line = format!("worktrees = {}\n", toml::Value::String(at.to_string_lossy().into_owned()));
    match toml.contains("\n[workspace]\n") {
        true => toml.replacen("\n[workspace]\n", &format!("\n[workspace]\n{line}"), 1),
        false => format!("{toml}\n[workspace]\n{line}"),
    }
}

/// The shot's config: language servers off, this xtask as claude running `script` — and for the board's
/// scripts, the review settings that bring their angles (`knot.rs` high-risk, `thread.rs` the screen's).
pub(crate) fn config_toml(me: &Path, script: &str) -> String {
    let quoted = |s: &str| toml::Value::String(s.to_string()).to_string();
    let review = match script {
        "boardmid" => "\n[review]\nrisky = [\"knot.rs\"]\nscreen = [\"thread.rs\"]\nboard-at-once = 2\n",
        "boarddone" => "\n[review]\nrisky = [\"knot.rs\"]\n",
        _ => "",
    };
    // (motion §2's: review at the end, following or not)
    let llm = match script {
        "six" | "sixend" => "review-at-end = true\nfollow = false\n",
        "sixfollow" => "review-at-end = true\nfollow = true\n",
        // (V4's: the edits land unseen, so the turn runs through to its replay)
        "v4" => "review-at-end = true\n",
        // (V1's: main's edit lands unseen, so fix's — waiting on it — lands too, and the two meet)
        s if s.contains("=v1-main") => "review-at-end = true\n",
        // (V6's: fix's and main's edits land unseen and meet, the turns going on)
        s if s.contains("=v6-main") => "review-at-end = true\n",
        _ => "",
    };
    // (V6's: main's commit leaves fix and docs behind it, said by the list alone — no card for the move)
    let workspace = match script {
        s if s.contains("=v6-main") => "\n[workspace]\nsync = \"off\"\n",
        _ => "",
    };
    format!(
        "[editor]\nlsp = false\n\n[llm]\ncommand = {}\nargs = [\"scenes\", \"--claude\", {}]\n{llm}{review}{workspace}",
        quoted(&me.to_string_lossy()),
        quoted(script)
    )
}

/// A folder made new for one shot (0700; the next number when the name is taken — never one that was there),
/// by its real path; removed on drop, and only it.
pub(crate) struct Fresh(pub(crate) PathBuf);

impl Fresh {
    pub(crate) fn new(name: &str) -> Result<Fresh> {
        let base = std::env::temp_dir();
        for n in 0..1000 {
            let p = base.join(format!("{name}-{}-{n}", std::process::id()));
            match std::fs::DirBuilder::new().mode(0o700).create(&p) {
                Ok(()) => {
                    let real = std::fs::canonicalize(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    return Ok(Fresh(real));
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(format!("{}: {e}", p.display())),
            }
        }
        Err(format!("no fresh folder in {}", base.display()))
    }
}

impl Drop for Fresh {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn write(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

// ── The demo crate ───────────────────────────────────────────────────────────

const CARGO_TOML: &str =
    "[package]\nname = \"tarae-demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n";

pub(crate) const MAIN_RS: &str = r#"use std::collections::HashMap;

mod thread;
use thread::Thread;

/// 실타래 — 이어진 실 한 가닥.
fn main() {
    let mut threads: HashMap<String, Thread> = HashMap::new();
    let first = Thread { name: "타래".into(), knots: vec![1, 2, 3] };
    threads.insert(first.name.clone(), first);
    for (name, t) in &threads {
        println!("{name}: {} knots, {} long", t.knots.len(), t.length());
    }
}
"#;

const THREAD_RS: &str = r#"/// One strand of a skein: a name and the knots tied along it.
pub struct Thread {
    pub name: String,
    pub knots: Vec<u32>,
}

impl Thread {
    /// The strand's length: the sum of the gaps between its knots.
    pub fn length(&self) -> u32 {
        self.knots.iter().sum()
    }

    /// Ties a knot at the end.
    pub fn tie(&mut self, at: u32) {
        self.knots.push(at);
    }
}
"#;

/// V3's `main.rs` ("Claude's edit, in your buffer"): as long as a Short's screen, no line past 35 columns, so the
/// edit's lines read at ~2.5 mm on a phone; the `v3` turn changes lines 8, 10 and 12 (`edit_in_buffer`).
const MAIN_V3_RS: &str = r#"use std::collections::HashMap;

mod thread;
use thread::Thread;

type Map = HashMap<String, Thread>;

/// 실타래 — 이어진 실 한 가닥.
fn main() {
    let mut m: Map = Map::new();
    let t = thread("타래");
    m.insert(t.name.clone(), t);
    for (name, t) in &m {
        say(name, t);
    }
}

/// A thread of three knots.
fn thread(name: &str) -> Thread {
    let name = name.to_string();
    let knots = vec![1, 2, 3];
    Thread { name, knots }
}

/// Says what a thread is.
fn say(name: &str, t: &Thread) {
    let n = t.knots.len();
    let long = t.length();
    println!("{name}: {n} knots");
    println!("{long} long");
}

/// Ties one more knot on.
fn tie(t: &mut Thread, k: u32) {
    t.tie(k);
}

/// Every knot, on every thread.
fn knots(m: &Map) -> usize {
    let all = m.values();
    all.map(|t| t.knots.len()).sum()
}
"#;

/// V4's `spool.rs` ("follow and replay"): two screens long, so the view jumps between the turn's reads (the doc comment
/// at 3–7, `wind` at 42–56, `tidy` at 75–83), no line past 38 columns, so each reads at ~2.4 mm on a phone; the `v4`
/// turn pops from the front at line 46 and drops `tidy`'s sort (`follow_replay`).
const SPOOL_V4_RS: &str = r#"use std::collections::VecDeque;

/// A spool: strands waiting to be
/// wound, and the ones wound so far.
/// What came first is wound first:
/// the spool keeps the order the
/// strands came in.
pub struct Spool {
    pub queue: VecDeque<Strand>,
    pub wound: Vec<Strand>,
}

/// A strand: its name and knots.
#[derive(Clone, Debug)]
pub struct Strand {
    pub name: String,
    pub knots: Vec<u32>,
}

/// A new spool, nothing waiting.
pub fn spool() -> Spool {
    let queue = VecDeque::new();
    Spool { queue, wound: vec![] }
}

/// Queues a strand to be wound.
pub fn add(sp: &mut Spool, s: Strand) {
    sp.queue.push_back(s);
}

/// A strand with no knots yet.
pub fn strand(name: &str) -> Strand {
    let name = name.to_string();
    Strand { name, knots: vec![] }
}

/// Ties a knot on a strand.
pub fn tie(s: &mut Strand, at: u32) {
    s.knots.push(at);
}

/// Winds every strand waiting.
pub fn wind(sp: &mut Spool) -> usize {
    let mut wound = 0;
    let q = &mut sp.queue;
    while let Some(s) = q.pop_back() {
        if s.knots.is_empty() {
            // No knots: no strand.
            continue;
        }
        sp.wound.push(s);
        wound += 1;
    }
    tidy(sp);
    wound
}

/// How many are waiting.
pub fn waiting(sp: &Spool) -> usize {
    sp.queue.len()
}

/// The longest strand wound.
pub fn longest(sp: &Spool) -> u32 {
    let all = sp.wound.iter();
    all.map(measure).max().unwrap_or(0)
}

/// Every knot, on every strand.
pub fn knots(sp: &Spool) -> usize {
    let all = sp.wound.iter();
    all.map(|s| s.knots.len()).sum()
}

/// Puts the wound strands in order.
fn tidy(sp: &mut Spool) {
    // The last popped is the first
    // that came, so the order here
    // is the queue's reversed.
    // Sorting by length makes it
    // stable, whatever the order.
    sp.wound.sort_by_key(measure);
}

/// A strand's length: the gaps
/// between its knots.
fn measure(s: &Strand) -> u32 {
    let gaps = s.knots.windows(2);
    gaps.map(|w| w[1] - w[0]).sum()
}
"#;

/// V2's `main.rs` ("main moved"): as long as a Short's 43 rows, so its whole screen is code; from line 10 on no line
/// past 33 columns, so a zoom on a few of them (gutter and air) draws ~2.5 mm glyphs on a phone. Main's and fix's
/// change is `m.insert` (line 31), docs' the first `println` (line 38), far enough apart to merge.
const MAIN_MOVED_RS: &str = r#"use std::collections::HashMap;

mod thread;
use thread::Thread;

/// Every thread wound, by its name.
type Map = HashMap<String, Thread>;

/// 실타래 — 이어진 실 한 가닥.
fn main() {
    let mut m = Map::new();
    for name in ["타래", "실"] {
        let t = thread(name);
        wind(&mut m, t);
    }
    for t in m.values() {
        say(t);
    }
}

/// A thread of three knots.
fn thread(name: &str) -> Thread {
    let name = name.to_string();
    let knots = vec![1, 2, 3];
    Thread { name, knots }
}

/// Winds a thread in, by name.
fn wind(m: &mut Map, t: Thread) {
    let k = t.name.clone();
    m.insert(k, t);
}

/// Says what a thread is.
fn say(t: &Thread) {
    let n = t.knots.len();
    let long = t.length();
    println!("{}: {n}", t.name);
    println!("{long} long");
}

/// Ties one more knot on.
fn tie(t: &mut Thread, k: u32) {
    t.tie(k);
    say(t);
}
"#;

/// Focus A1c's file: a doc comment over a struct (line 5 one of its lines) and `wind`, lines 21–85 — past a screen.
const SPOOL_RS: &str = r#"use std::collections::VecDeque;

/// A spool: the strands waiting to be wound, and the ones wound so far.
///
/// The spool keeps its order: a strand is wound in the order it came,
/// so `wound` reads as the queue did. Each strand is tied before it is
/// measured, and one whose knot slips goes back to be tied again.
pub struct Spool {
    pub queue: VecDeque<Strand>,
    pub wound: Vec<Strand>,
}

/// One strand: its name and the knots along it.
#[derive(Clone, Debug)]
pub struct Strand {
    pub name: String,
    pub knots: Vec<u32>,
}

impl Spool {
    pub fn wind(&mut self) -> usize {
        let mut wound = 0;
        let mut slipped = 0;
        // A strand's knots are tied from the first to the last: each one
        // checked against the one before it, so a knot never sits behind
        // the knot it follows.
        let started = self.queue.len();
        if started == 0 {
            return 0;
        }
        let mut rounds = 0;
        while let Some(mut s) = self.queue.pop_back() {
            rounds += 1;
            if rounds > started * 4 {
                // Every strand had its chances: what's left stays queued.
                self.queue.push_back(s);
                break;
            }
            let mut last = 0;
            let mut tied = true;
            for k in s.knots.iter_mut() {
                if *k < last {
                    tied = false;
                    break;
                }
                last = *k;
            }
            if !tied {
                slipped += 1;
                // The knot slipped: sorted, and back to the queue to be
                // tied again on a later round.
                s.knots.sort_unstable();
                retie(&mut self.queue, s);
                continue;
            }
            let length = measure(&s);
            if length == 0 {
                // A strand with no gap between its knots is no strand:
                // it isn't wound, and it isn't queued again.
                continue;
            }
            if s.name.is_empty() {
                s.name = format!("strand {wound}");
            }
            self.wound.push(s);
            wound += 1;
            if wound % 100 == 0 {
                // A long spool: say how far it got, a hundred at a time.
                eprintln!("{wound} wound, {} waiting", self.queue.len());
            }
        }
        if slipped > 0 {
            eprintln!("{slipped} slipped and were tied again");
        }
        // What came first is wound first, or so the doc comment says.
        // The last strand popped is the first that came, and each
        // retied strand goes to the back of the queue, so the order
        // here is the queue's reversed, with the slipped ones last.
        // Sorting by length makes it stable at least: the same
        // strands give the same spool, whatever the queue's order was.
        self.wound.sort_by_key(measure);
        let total: u32 = self.wound.iter().map(measure).sum();
        eprintln!("{wound} wound, {total} long in all");
        wound
    }
}

/// The length of a strand: the gaps between its knots.
fn measure(s: &Strand) -> u32 {
    s.knots.windows(2).map(|w| w[1] - w[0]).sum()
}

/// A slipped strand, back at the queue's end.
fn retie(queue: &mut VecDeque<Strand>, s: Strand) {
    queue.push_back(s);
}
"#;

/// The crate's files as committed, then as changed since (`thread.rs` changed, `knot.rs` new) — with settings
/// that wait for trust when `trusted` (a build of its own for `:restart`).
fn demo_files(dir: &Path, changed: bool, trusted: bool) -> Result<()> {
    if !changed {
        if trusted {
            write(&dir.join(".tarae.toml"), "[editor]\nrestart-build = \"cargo build\"\n")?;
        }
        write(&dir.join("Cargo.toml"), CARGO_TOML)?;
        write(&dir.join(".gitignore"), "target/\nCargo.lock\n")?;
        write(&dir.join("README.md"), "# tarae-demo\n\nA skein of threads.\n")?;
        write(&dir.join("src/main.rs"), MAIN_RS)?;
        return write(&dir.join("src/thread.rs"), THREAD_RS);
    }
    let thread = THREAD_RS.replace("Ties a knot at the end.", "Ties a knot at the end of the strand.");
    write(&dir.join("src/thread.rs"), &thread)?;
    write(&dir.join("src/knot.rs"), "pub fn untie() {}\n")
}

/// The demo crate at `dir`: committed in a repository of its own, then changed, `extra` files committed beside its own
/// (over them where named alike). git runs with none of your settings (no hooks, no signing) and finds no repository
/// above `dir`.
pub(crate) fn demo_crate_with(dir: &Path, trusted: bool, extra: &[(&str, String)]) -> Result<()> {
    demo_crate_at(dir, trusted, extra, None)
}

/// `demo_crate_with`, its commit dated `date` (git's form) — a scene's (`dates`); none: as it's made.
fn demo_crate_at(dir: &Path, trusted: bool, extra: &[(&str, String)], date: Option<&str>) -> Result<()> {
    demo_files(dir, false, trusted)?;
    for (path, text) in extra {
        write(&dir.join(path), text)?;
    }
    let git = |args: &[&str]| -> Result<()> {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "commit.gpgsign=false"])
            .args(["-c", "core.hooksPath=/dev/null", "-c", "init.defaultBranch=main"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap_or(dir))
            .envs(date.into_iter().flat_map(|d| [("GIT_AUTHOR_DATE", d), ("GIT_COMMITTER_DATE", d)]))
            .stdout(Stdio::null())
            .status()
            .map_err(|e| format!("git: {e}"))?;
        if st.success() { Ok(()) } else { Err(format!("git {} failed in {}", args.join(" "), dir.display())) }
    };
    git(&["init", "-q"])?;
    git(&["add", "-A"])?;
    git(&["commit", "-q", "-m", "A skein of threads"])?;
    // (a command of the repository's own, held until trusted — ssh's, which only a push, pull or fetch would run:
    // the demo has no remote)
    if trusted {
        git(&["config", "core.sshCommand", "ssh -o BatchMode=yes"])?;
    }
    demo_files(dir, true, trusted)
}

/// A failing test and one that passes, below `main` (`cargo test` on the bin): the tests panel's failure.
pub(crate) const MAIN_TESTS: &str = r#"
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_sums_the_knots() {
        let t = Thread { name: "실".into(), knots: vec![1, 2, 3] };
        assert_eq!(t.length(), 6);
    }

    #[test]
    fn a_tied_knot_lengthens_it() {
        let mut t = Thread { name: "실".into(), knots: vec![1, 2] };
        t.tie(4);
        println!("knots after the tie: {:?}", t.knots);
        assert_eq!(t.length(), 8, "the knot at 4 adds its gap");
    }
}
"#;

/// A check as a compiler prints it — warnings and their places on stderr, then a passing end.
const CHECK_SH: &str = r#"echo '    Checking tarae-demo v0.1.0'
echo 'warning: method `tie` is never used' >&2
echo '  --> src/thread.rs:15:12' >&2
echo '   |' >&2
echo '15 |     pub fn tie(&mut self, at: u32) {' >&2
echo '   |            ^^^' >&2
echo '   = note: `#[warn(dead_code)]` on by default' >&2
echo 'warning: unused variable: `first`' >&2
echo '  --> src/main.rs:8:9' >&2
echo "warning: \`tarae-demo\` (bin \"tarae-demo\") generated 2 warnings" >&2
echo '    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.42s'
"#;

/// The run scenes' own files: a task (`output-task`), a preflight of three steps (`output-preflight`) — both in
/// a `.tarae.toml` trusted on its card — or a failing test (`output-tests`, its build in the demo's own folder,
/// never cargo's shared one).
pub(crate) fn run_files(scene: &str) -> Vec<(&'static str, String)> {
    match scene {
        // (kinds C: forty tasks — their trust card cut to fit)
        "kindsc-card" | "kindsc-card-end" => {
            let tasks: String = (1..=40).map(|i| format!("[task.step{i}]\ncmd = \"sh step{i}.sh\"\n")).collect();
            vec![(".tarae.toml", tasks)]
        }
        "output-task" | "panes-task-places" | "group-task-detail" | "group-task-stacked" => vec![
            (".tarae.toml", "[task.check]\ncmd = \"sh check.sh\"\n".into()),
            ("check.sh", CHECK_SH.into()),
        ],
        // (a run still printing: a line every 0.2 s for a minute)
        "output-live" | "polish-task-running" | "sign" => vec![
            (".tarae.toml", "[task.watch]\ncmd = \"sh watch.sh\"\n".into()),
            (
                "watch.sh",
                "i=0\nwhile [ $i -lt 300 ]; do echo \"knot $i tied\"; i=$((i+1)); sleep 0.2; done\n".into(),
            ),
        ],
        "output-preflight" | "panes-preflight-steps" | "group-preflight-detail" => vec![
            (
                ".tarae.toml",
                "[preflight]\nsteps = [\n  { name = \"fmt\", cmd = \"sh fmt.sh\" },\n  { name = \"clippy\", cmd = \"sh check.sh\" },\n  { name = \"test\", cmd = \"sh test.sh\" },\n]\n"
                    .into(),
            ),
            ("fmt.sh", "echo 'Diff in src/thread.rs:8:'\necho '-    /// Ties a knot.'\necho '+    /// Ties a knot at the end.'\nexit 0\n".into()),
            ("check.sh", CHECK_SH.into()),
            (
                "test.sh",
                "echo 'running 2 tests'\necho 'test tests::length_sums_the_knots ... ok'\necho 'test tests::a_tied_knot_lengthens_it ... FAILED'\necho\necho \"thread 'tests::a_tied_knot_lengthens_it' panicked at src/main.rs:30:9:\"\necho 'assertion `left == right` failed: the knot at 4 adds its gap'\necho '  left: 7'\necho ' right: 8'\necho\necho 'test result: FAILED. 1 passed; 1 failed; 0 ignored'\nexit 101\n"
                    .into(),
            ),
        ],
        // (the bring's quick preflight: two steps that pass in a second each; the full set's test never runs)
        "bring" => vec![
            (
                ".tarae.toml",
                "[preflight]\nsteps = [\n  { name = \"fmt\", cmd = \"sh fmt.sh\" },\n  { name = \"check\", cmd = \"sh check.sh\" },\n  { name = \"test\", cmd = \"sh test.sh\", full = true },\n]\n"
                    .into(),
            ),
            ("fmt.sh", "sleep 1\nexit 0\n".into()),
            (
                "check.sh",
                "echo '    Checking tarae-demo v0.1.0'\nsleep 1\necho '    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.02s'\n".into(),
            ),
            ("test.sh", "exit 1\n".into()),
        ],
        // (a preflight whose test step prints a line every 0.1 s for a minute)
        "polish-preflight-list" | "polish-preflight-output" => vec![
            (
                ".tarae.toml",
                "[preflight]\nsteps = [\n  { name = \"fmt\", cmd = \"sh fmt.sh\" },\n  { name = \"clippy\", cmd = \"sh check.sh\" },\n  { name = \"test\", cmd = \"sh test.sh\" },\n]\n"
                    .into(),
            ),
            ("fmt.sh", "exit 0\n".into()),
            ("check.sh", CHECK_SH.into()),
            (
                "test.sh",
                "echo 'running 600 tests'\ni=0\nwhile [ $i -lt 600 ]; do echo \"test tests::knot_$i ... ok\"; i=$((i+1)); sleep 0.1; done\n"
                    .into(),
            ),
        ],
        "output-tests" | "panes-tests-code" | "panes-float" | "group-tests-list" | "group-tests-detail"
        | "group-tests-stacked" | "float-tests" | "float-tests-moved" | "waits-60s" => vec![
            ("src/main.rs", format!("{MAIN_RS}{MAIN_TESTS}")),
            (".cargo/config.toml", "[build]\nbuild-dir = \"target/build\"\n".into()),
        ],
        "panes-task-tabs" | "dock-tests-right" | "dock-tests-beside" | "dock-tests-split" | "dock-tests-top" => vec![
            ("src/main.rs", format!("{MAIN_RS}{MAIN_TESTS}")),
            (".cargo/config.toml", "[build]\nbuild-dir = \"target/build\"\n".into()),
            (".tarae.toml", "[task.check]\ncmd = \"sh check.sh\"\n".into()),
            ("check.sh", CHECK_SH.into()),
        ],
        // (#157: as many lines as the followed pane's rows less one, each wide enough to reach under the card)
        "cut-hand-list" => vec![(LONG_NAME[0], "pub fn component() {}\n".into())],
        "follow-replay" => vec![("src/spool.rs", SPOOL_V4_RS.into())],
        "main-moved" | "three-claudes" | "managers-view" => vec![("src/main.rs", MAIN_MOVED_RS.into())],
        "edit-in-buffer" | "chatlink-named-one" | "chatlink-named-list" | "chatlink-under" | "chatlink-keyword"
        | "chatlink-plain" => {
            vec![("src/main.rs", MAIN_V3_RS.into())]
        }
        // (its lines ended CRLF, committed so: the gutter calm, the status line's form the one thing said)
        "status-crlf" => vec![("src/main.rs", MAIN_RS.replace('\n', "\r\n"))],
        "follow-split-s" => vec![("src/wide.rs", wide_rs(18, 112))],
        "follow-split-v" => vec![("src/wide.rs", wide_rs(36, 70))],
        // (the flow plan's piece 1 on A1c's answer too — its two on A1's answers have MAIN's files alone)
        s if s.starts_with("a1c-")
            || s.starts_with("f269-")
            || s.starts_with("flow1-") && !matches!(s, "flow1-none-read" | "flow1-flow-read")
            || s.starts_with("flow2-") && !s.starts_with("flow2-strip")
            || s.starts_with("flow3-") && !matches!(s, "flow3-none" | "flow3-error") =>
        {
            vec![("src/spool.rs", SPOOL_RS.into())]
        }
        _ => Vec::new(),
    }
}

/// `lines` lines of strands, each `width` characters, dots filling the string out (#157's scenes: wide enough to reach
/// under the follow card, narrow enough not to wrap in their pane). A line with a two-digit number is 70 characters
/// before any dots — a narrower `width` is refused.
fn wide_rs(lines: usize, width: usize) -> String {
    (1..=lines)
        .map(|i| {
            let line = format!("    strands.push(Strand::new({i:2}, \"strand {i}, tied where it crosses");
            let w = width.saturating_sub(4);
            assert!(line.len() <= w, "wide_rs: {width} is narrower than line {i}'s own {}", line.len() + 4);
            format!("{line:.<w$}\"));\n")
        })
        .collect()
}

// ── The stand-in claude ──────────────────────────────────────────────────────

/// Every script the stand-in plays: `claude`'s turns, the docs' own (`docshots::answered`), and `idle`, which
/// answers nothing (as any name it doesn't know would — hence the list: `shot`'s `TARAE_SHOT_CLAUDE` is checked
/// against it). `the_scripts_are_every_one_the_stand_in_plays` reads it against their arms.
pub(crate) const SCRIPTS: &[&str] = &[
    "idle",
    "full",
    "chatlink",
    "chatlink-named",
    "chatlink-under",
    "three",
    "v3",
    "v4",
    "v1-main",
    "v1-fix",
    "v1-docs",
    "two",
    "six",
    "sixfollow",
    "sixend",
    "strip",
    "now",
    "nowcard",
    "nowread",
    "nowwrite",
    "shimmer-prose",
    "shimmer-think",
    "shimmer-stall",
    "shimmer-read",
    "shimmer-all",
    "shimmer-sign",
    "writemid",
    "mid",
    "midlist",
    "ask",
    "asktouching",
    "card",
    "cardlong",
    "cardlater",
    "manager",
    "samebinary",
    "nine",
    "searchcard",
    "volumecard",
    "grants",
    "big",
    "foot41",
    "foot93",
    "footshort",
    "boardmid",
    "boarddone",
    "reviewing",
    "reviewdone",
    "reviewfull",
    "home-main",
    "home-fix",
    "home-docs",
    "readlast",
    "cuthand",
    "cutcode",
    "chatnotes",
    "startone",
    "startthree",
    "startlong",
    "startthreewake",
    "startgo",
    "startgo3",
    "childcount",
    "childcost",
    "parentwake",
    "childlong",
    "parent",
    "parentpropose",
    "focus-explain",
    "focus-typing",
    "focus-fix",
    "focus-asks",
    "focus-line",
    "tour",
    "a1-none",
    "flow3-none",
    "flow3-error",
    "steer",
    "a1-flow",
    "a1b-two",
    "a1b-all",
    "f271",
    "e1-map",
    "e1-failed",
    "f0-saw",
    "f0-tested",
    "f0-failed",
    "a1c",
    "models",
    "answer",
    "askdone",
    "commitdraft",
    "v6-main",
    "v6-fix",
    "v6-docs",
];

/// Whether the stand-in plays `spec`: a script it knows, or a specification by checkout whose every script it
/// knows (`script_for`).
pub(crate) fn knows(spec: &str) -> bool {
    match spec.contains('=') {
        true => {
            spec.split(',').all(|pair| pair.split_once('=').is_some_and(|(_, v)| SCRIPTS.contains(&v.trim())))
        }
        false => SCRIPTS.contains(&spec),
    }
}

/// A script specification that names each session's checkout (`main=home-main,fix=home-fix,docs=card`) picks the
/// script for `cwd`'s folder (`tarae-demo` is `main`); a plain script name stands as given.
pub(crate) fn script_for<'a>(spec: &'a str, cwd: Option<&Path>) -> &'a str {
    if !spec.contains('=') {
        return spec;
    }
    let folder = cwd.and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or_default();
    let session = if folder == "tarae-demo" { "main" } else { folder };
    spec.split(',')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(k, v)| (k.trim() == session).then_some(v.trim()))
        .unwrap_or("idle")
}

/// Reads tarae's lines from `input` and answers each user message (the line that starts `{"message"` — tarae's
/// JSON has its keys sorted) with `script`'s turn on `out`, `pause` after each piece of text: `full` explains,
/// reads and edits (its calls wait for tarae's answers), `three` the same with three changes; `mid` stops in the
/// middle of its code block and waits for tarae to go; `midlist` stops in a list after a quote, on a line in
/// Japanese; `ask` answers `space i` and stops mid-line; `card` asks to run outside the sandbox; `samebinary` asks
/// it twice, a program rebuilt between; `searchcard` searches a folder in home with tarae's `search`; `volumecard` reads
/// a file on a mounted disk with tarae's `read` (a card wherever home is: a docs shot's is a temp folder, read unasked);
/// `grants` asks
/// for what `:granted` then lists; `big` answers on a big conversation's 1h cache;
/// `boardmid` / `boarddone` are a board's members (each by what its message
/// asks — `board`); `idle` never answers.
fn claude(script: &str, input: impl BufRead, out: &mut impl Write, pause: Duration) -> io::Result<()> {
    let cwd = std::env::current_dir().ok();
    let script = script_for(script, cwd.as_deref());
    let mut lines = input.lines();
    // (the messages answered so far — a script whose turns differ)
    let mut turns = 0;
    while let Some(line) = lines.next() {
        let line = line?;
        // (the docs' own scripts first)
        if docshots::answered(script, &line, out, pause)? {
            continue;
        }
        if !line.starts_with("{\"message\"") {
            continue;
        }
        turns += 1;
        let mut turn = Turn { out: &mut *out, lines: &mut lines, pause };
        match script {
            "full" => turn.explain(true, false)?,
            "three" => turn.explain(true, true)?,
            // (V3's video: two reads the view follows, then three changes written at a stream's pace)
            "v3" => turn.edit_in_buffer()?,
            // (V5's video: three reads far apart, the view taken back and followed again, two edits under review at the end)
            "v4" => turn.follow_replay()?,
            // (V1's: three sessions' Claudes, fix's edit waiting on main's, docs talking all through)
            "v1-main" => turn.v1_main()?,
            "v1-fix" => turn.v1_fix()?,
            "v1-docs" => turn.v1_docs()?,
            // (V6's: each session's first turn short, naming its context window, the second its work)
            "v6-main" | "v6-fix" | "v6-docs" => turn.v6(&script[3..], turns)?,
            // (two changes: the press's sheet — the first answered, the second in hand)
            "two" => turn.explain_items(2)?,
            // (motion §2: an edit of six lines in one go, then a pause — or, `sixend`, the turn's end at once)
            "six" | "sixfollow" => turn.six(true)?,
            "sixend" => turn.six(false)?,
            // (motion §4: twenty stops, one run failing, held at the last)
            "strip" => turn.strip()?,
            "now" => turn.now(Held::Run)?,
            "nowcard" => turn.now(Held::Card)?,
            "nowread" => turn.now(Held::Read)?,
            "nowwrite" => turn.now(Held::Write)?,
            // (motion §3: a turn timed as the stream measured — prose from the start, a think, a stall)
            "shimmer-prose" | "shimmer-think" | "shimmer-stall" | "shimmer-read" | "shimmer-all"
            | "shimmer-sign" => turn.shimmer(&script[8..])?,
            // (its three-item edit being written: the third item's new text held mid-line)
            "writemid" => turn.explain_held(true, true, Some("nots.len());\\n\"}"))?,
            "mid" => turn.explain(false, false)?,
            "midlist" => turn.explain_listed()?,
            "ask" => turn.rewrite()?,
            "asktouching" => turn.rewrite_touching()?,
            "card" => turn.ask_outside()?,
            // (#244: prose enough to reach the ask's rows in the chat, then the same card)
            "cardlong" => turn.ask_outside_long()?,
            // (`waits-60s`: the same card ~10 s into the turn, its words coming slowly meanwhile)
            "cardlater" => turn.ask_outside_later()?,
            // (the session list's scene: a run asked for where the tests are asked, else an edit held mid-line)
            "manager" if line.contains("run the tests") => turn.ask_outside()?,
            "manager" => turn.explain_held(true, true, Some("nots.len());\\n\"}"))?,
            "samebinary" => turn.same_binary()?,
            // (nine sessions: a run asked for outside the sandbox, a run that outlasts the stuck mark, else a whole turn)
            "nine" if line.contains("run the tests") => turn.ask_outside()?,
            "nine" if line.contains("wait for the build") => turn.run_long()?,
            "nine" => turn.explain(true, false)?,
            "searchcard" => turn.search_outside()?,
            "volumecard" => turn.read_volume()?,
            "grants" => turn.grants()?,
            "big" => turn.big()?,
            "foot41" => turn.foot(41, true)?,
            "foot93" => turn.foot(93, true)?,
            "footshort" => turn.foot(41, false)?,
            "boardmid" => turn.board(&line, false)?,
            "boarddone" => turn.board(&line, true)?,
            "reviewing" => turn.reviewer(false)?,
            "reviewdone" => turn.reviewer(true)?,
            "reviewfull" => turn.reviewer_full()?,
            "home-main" => turn.home_main()?,
            "home-fix" => turn.home_fix()?,
            "home-docs" => turn.home_docs()?,
            "readlast" => turn.read_last()?,
            "cuthand" => turn.cut_hand()?,
            "cutcode" => turn.cut_code()?,
            "chatnotes" => turn.chat_notes()?,
            "startone" | "startthree" | "startlong" | "startthreewake" => {
                turn.sessions_start(&script[5..])?
            }
            "startgo" => turn.sessions_started(false, false)?,
            "startgo3" => turn.sessions_started(true, false)?,
            "childcount" => turn.child_count(&line, None)?,
            // (orchestra P2: a child whose turns say what they cost — its parent's row sums them)
            "childcost" => turn.child_count(&line, Some(0.31))?,
            "parentwake" => turn.parent_wake(&line)?,
            "childlong" => turn.child_long(&line)?,
            "parent" => turn.parent(&line)?,
            // (orchestra P3: main proposes knot-count's bring, or asks for a board on it)
            "parentpropose" => turn.parent_propose(&line)?,
            // (focus D1's turns — `cargo xtask focus`)
            "focus-explain" => turn.focus_explain(false)?,
            "focus-typing" => turn.focus_explain(true)?,
            "focus-fix" => turn.focus_fix()?,
            "focus-asks" => turn.focus_asks()?,
            "focus-line" => turn.focus_line()?,
            "e1-map" => turn.e1_map(false)?,
            "e1-failed" => turn.e1_map(true)?,
            // (focus F0's: the three verdicts on one answer, a second answer on the same lines)
            "f0-saw" => turn.f0_saw(turns, None)?,
            "f0-tested" => turn.f0_saw(turns, Some("passed in 0.4s\n$ cargo test thread\n2 passed"))?,
            "f0-failed" => {
                turn.f0_saw(turns, Some("failed in 0.4s\n$ cargo test thread\n1 failed, 1 passed"))?
            }
            // (focus A1c's: a real answer's shape)
            "a1c" => turn.a1c()?,
            "tour" => turn.tour()?,
            // (focus A1's: an answer naming no place)
            "a1-none" => turn.a1_none()?,
            // (#389's: an answer citing a place, quoting its lines, naming code in prose)
            "chatlink" => turn.chatlink()?,
            "chatlink-named" => turn.chatlink_named()?,
            "chatlink-under" => turn.chatlink_under()?,
            // (the flow plan's piece 3: an answer with no place and no stop — the reader opens by itself — and a turn
            // that fails, its error said only in the conversation)
            "flow3-none" => turn.flow3_none()?,
            "flow3-error" => turn.flow3_error()?,
            // (the flow plan's piece 5: a read, the turn held while the user queues and steers, then a plan answered at
            // once — what steers rides it)
            "steer" => turn.steer()?,
            // (…and one with a paragraph placed elsewhere: its line on the place's card)
            "a1-flow" => turn.a1_flow()?,
            // (focus A1b's: two answers naming the loop's line, the first one the line above it too)
            "a1b-two" => turn.a1b_two(turns, false)?,
            // (focus A1b step 3's: `a1b-two`, its first answer all placed)
            "a1b-all" => turn.a1b_two(turns, true)?,
            // (#271/#272's: `a1b-two`, each answer after a thinking summary that folds to two rows)
            "f271" => {
                turn.thought(
                    "Checking how the loop walks the map of threads and whether the order it prints them in is the \
                     order they were added in, since a hash map keeps no order of its own and the question is about \
                     what is printed.",
                )?;
                turn.a1b_two(turns, false)?
            }
            _ => {}
        }
    }
    Ok(())
}

/// Where the `now` turn is held: its run in flight, its card up, a read in flight, or an edit being written (its call not
/// made yet).
#[derive(Clone, Copy, PartialEq)]
enum Held {
    Run,
    Card,
    Read,
    Write,
}

/// `space i`'s edit in the scenes (`ask`, the docs' `askdone`): line 10 of the demo's `main.rs` keeping the first strand.
pub(crate) fn ask_edit() -> Value {
    json!({"edits": [{"file_path": "src/main.rs", "old_string": "    threads.insert(first.name.clone(), first);",
        "new_string": "    threads.entry(first.name.clone()).or_insert(first);"}]})
}

/// The `knot-count` session's brief in orchestra P1a's scenes.
const COUNT_BRIEF: &str = "Count the knots of each thread in src/main.rs and print the threads most-knotted first.\n\
    Keep Thread as it is; a test in src/thread.rs for the order. Done: cargo test passes.";

struct Turn<'a, W, L> {
    out: &'a mut W,
    lines: &'a mut L,
    pause: Duration,
}

impl<W: Write, L: Iterator<Item = io::Result<String>>> Turn<'_, W, L> {
    fn line(&mut self, v: Value) -> io::Result<()> {
        writeln!(self.out, "{v}")?;
        self.out.flush()
    }

    fn text(&mut self, t: &str) -> io::Result<()> {
        let delta = json!({"type": "text_delta", "text": t});
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0, "delta": delta}}))?;
        std::thread::sleep(self.pause);
        Ok(())
    }

    /// A thinking block whose summary is `s`, whole, then closed.
    fn thought(&mut self, s: &str) -> io::Result<()> {
        let ev = |e: Value| json!({"type": "stream_event", "event": e});
        let block = json!({"type": "thinking", "thinking": "", "signature": ""});
        self.line(ev(json!({"type": "content_block_start", "index": 0, "content_block": block})))?;
        let delta = json!({"type": "thinking_delta", "thinking": s});
        self.line(ev(json!({"type": "content_block_delta", "index": 0, "delta": delta})))?;
        self.line(ev(json!({"type": "content_block_stop", "index": 0})))
    }

    /// `s` a word at a time, as a model streams it.
    fn words(&mut self, s: &str) -> io::Result<()> {
        s.split(' ').try_for_each(|w| self.text(&format!("{w} ")))
    }

    /// A call of tarae's tool `name` (the turn's `n`th): its row in the chat, the request, tarae's answer waited for,
    /// then the result as claude's stream says it (the call over — a step under way is no longer it, `now::step`).
    fn call(&mut self, n: u64, name: &str, args: Value) -> io::Result<()> {
        let tool = json!({"type": "tool_use", "id": format!("t{n}"), "name": format!("mcp__tarae__{name}"), "input": args});
        self.line(json!({"type": "assistant", "message": {"id": format!("m{n}"), "content": [tool]}}))?;
        let msg = json!({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args}});
        let request = json!({"subtype": "mcp_message", "server_name": "tarae", "message": msg});
        self.line(json!({"type": "control_request", "request_id": format!("c{n}"), "request": request}))?;
        let id = format!("\"c{n}\"");
        let mut answer = None;
        for l in self.lines.by_ref() {
            let l = l?;
            if l.contains(&id) {
                answer = Some(l);
                break;
            }
        }
        // (tarae's answer's content, as claude hands it back to the model; gone with the input: nothing to say)
        let Some(answer) = answer else { return Ok(()) };
        let said: Value = serde_json::from_str(&answer).unwrap_or_default();
        let content = said
            .pointer("/response/response/mcp_response/result/content")
            .cloned()
            .unwrap_or_else(|| json!([{"type": "text", "text": ""}]));
        let result = json!({"type": "tool_result", "tool_use_id": format!("t{n}"), "content": content});
        self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))
    }

    /// orchestra P1a's scenes: Claude asks for sessions on one card — `one`, `three`, or three with a brief whose first
    /// line is long (`long`) — and waits there, its call unanswered, until tarae goes.
    fn sessions_start(&mut self, which: &str) -> io::Result<()> {
        let count = "knot-count";
        let count_brief = COUNT_BRIEF;
        let docs_brief = "Write a one-line doc comment for every public item in src/thread.rs, saying what it's for.\n\
            No code changes. Done: cargo doc warns of nothing.";
        let spool = match which {
            "long" => {
                "Find out why filling the map in src/main.rs clones each thread's name twice — once for the key and \
                once inside the Thread itself — and make it clone each name once at most, the map's order and every test kept.\n\
                Measure it before and after with 100,000 threads. Done: the clones halved, cargo test passes."
            }
            _ => {
                "Make filling the map in src/main.rs clone each thread's name once, not twice.\n\
                Measure it with 100,000 threads. Done: the clones halved, cargo test passes."
            }
        };
        let sessions = match which {
            "one" => json!({ count: count_brief }),
            _ => json!({ count: count_brief, "thread-docs": docs_brief, "spool-clones": spool }),
        };
        let words = match which {
            "one" => "The count is a piece of its own, so it can go in a session beside us.",
            _ => "These three touch different files, so each can go in a session of its own.",
        };
        self.words(words)?;
        self.text("\n")?;
        // (`threewake`: woken as each one's turn ends — the card says so, orchestra P2)
        let wake = which.ends_with("wake");
        self.call(1, "sessions_start", json!({"sessions": sessions, "base": "main", "wake": wake}))?;
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// `sessionset-child`'s main: one session asked for (`three`: `orchestra-*`'s three), then — the set made — its
    /// turn's end.
    fn sessions_started(&mut self, three: bool, wake: bool) -> io::Result<()> {
        self.words("The count is a piece of its own, so it can go in a session beside us.")?;
        self.text("\n")?;
        let sessions = match three {
            true => json!({"knot-count": COUNT_BRIEF,
                "thread-docs": "Write a one-line doc comment for every public item in src/thread.rs.",
                "spool-clones": "Make filling the map in src/main.rs clone each thread's name once, not twice."}),
            false => json!({"knot-count": COUNT_BRIEF}),
        };
        self.call(1, "sessions_start", json!({"sessions": sessions, "base": "main", "wake": wake}))?;
        self.words(if three {
            "Started all three; I'll keep to the map here."
        } else {
            "Started **knot-count**; I'll keep to the map here."
        })?;
        match wake {
            // (what it cost so far, so the wake's turn after it says its own share)
            true => self.line(json!({"type": "result", "result": "done", "session_id": "conv-main", "total_cost_usd": 0.12})),
            false => self.line(json!({"type": "result", "result": "done"})),
        }
    }

    /// orchestra P2's parent (`orchestra-woke`): main starts knot-count asking to be woken as its turn ends; woken, it
    /// reads the news it was given and says what's next — the wake's turn $0.04 of the conversation's.
    fn parent_wake(&mut self, line: &str) -> io::Result<()> {
        // (its conversation said, as claude's `init` says it: each turn's cost its share of the conversation's)
        self.line(json!({"type": "system", "subtype": "init", "session_id": "conv-main", "model": "claude-opus-5-5"}))?;
        if !line.contains("a turn you asked to be woken for ended") {
            return self.sessions_started(false, true);
        }
        self.words("knot-count is done: the threads print most-knotted first, ties by name. I'll read its change next.")?;
        self.line(
            json!({"type": "result", "result": "done", "session_id": "conv-main", "total_cost_usd": 0.16}),
        )
    }

    /// orchestra P3's parent (`orchestra-propose-*`): main starts knot-count; told to bring it in, it proposes the bring
    /// with its reason; told to have it reviewed, it asks for a board on it — each the real card, yours to answer.
    fn parent_propose(&mut self, line: &str) -> io::Result<()> {
        let (act, why, said) = if line.contains("bring it in") {
            (
                "bring",
                "its test passes, the threads print most-knotted first, and nothing else reads the order it \
                 changed — the report and the strip both sort again on their own",
                "The bring is yours — its card is up.",
            )
        } else if line.contains("board") {
            ("board", "it changes the order every report reads", "A board spends, so it's your card.")
        } else {
            return self.sessions_started(false, false);
        };
        self.words("knot-count is done and its test passes.")?;
        self.text("\n")?;
        self.call(1, "session_act", json!({"session": "knot-count", "act": act, "why": why}))?;
        self.words(said)?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// `sessionset-child`'s child: main's brief answered with a read and a plan; a message of yours after it, briefly.
    fn child_count(&mut self, line: &str, cost: Option<f64>) -> io::Result<()> {
        let done = match cost {
            Some(c) => {
                json!({"type": "result", "result": "done", "session_id": "conv-child", "total_cost_usd": c})
            }
            None => json!({"type": "result", "result": "done"}),
        };
        if cost.is_some() {
            self.line(json!({"type": "system", "subtype": "init", "session_id": "conv-child", "model": "claude-opus-5-5"}))?;
        }
        if !line.contains("from-session") {
            self.words("Right — ties keep their names' order, so the sort is stable on the name.")?;
            return self.line(done);
        }
        self.words("Reading how the threads are kept first.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:1-20"]}))?;
        self.words("Each thread keeps its knots in a list, so the count is its length. I'll sort by it, most-knotted first, \
            and add the order's test in **src/thread.rs**.")?;
        self.line(done)
    }

    /// `orchestra-parent-long`'s child: main's brief answered with a reply whose first line runs past a news item's head
    /// (100 characters) and a few lines after it.
    fn child_long(&mut self, line: &str) -> io::Result<()> {
        if !line.contains("from-session") {
            return self.child_count(line, None);
        }
        self.words(
            "Done: the threads now print most-knotted first, ties kept in their names' order, and a test in \
            src/thread.rs pins the order of three threads with the same count.",
        )?;
        self.text("\n\n")?;
        self.words(
            "The count is each thread's knot list's length, so nothing new is kept. cargo test passes.",
        )?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// orchestra P1b's parent (`orchestra-parent`, `-fresh`): main starts knot-count (`split the work`), reads its sessions
    /// once you ask how it goes, and starts it over in a new conversation once you say so.
    fn parent(&mut self, line: &str) -> io::Result<()> {
        if line.contains("how is it going") {
            self.words("Reading what knot-count reported.")?;
            self.text("\n\n")?;
            self.call(1, "sessions", json!({}))?;
            self.words("knot-count is done: threads print most-knotted first, ties by name, with a test for the order. \
                I'll look at its change next.")?;
            return self.line(json!({"type": "result", "result": "done"}));
        }
        if line.contains("start it over") {
            let message =
                "Start over, narrower: only the sort in src/main.rs, most-knotted first. No test yet.";
            self.call(
                1,
                "session_send",
                json!({"session": "knot-count", "message": message, "fresh": true}),
            )?;
            self.words("Started knot-count over in a new conversation, with the narrower brief.")?;
            return self.line(json!({"type": "result", "result": "done"}));
        }
        self.sessions_started(false, false)
    }

    /// #157's scenes: a read of `src/wide.rs`'s last five lines (as many as the file in the folder it runs in has), then
    /// held — the turn at work, its follow card up — until tarae goes.
    fn read_last(&mut self) -> io::Result<()> {
        let n = std::fs::read_to_string("src/wide.rs").map_or(18, |t| t.lines().count());
        self.words("Reading the last strands.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": [format!("src/wide.rs:{}-{n}", n - 4)]}))?;
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// A call's input as the model writes it, before the call (`input_json_delta`, tarae draws it as it comes —
    /// `writing.rs`): `json` (its keys in the order written) a few characters a piece; with `hold`, the pieces
    /// stop at the first `hold` in it — and wait there until tarae goes (a shot of it being written).
    fn input(&mut self, n: u64, name: &str, json: &str, hold: Option<&str>) -> io::Result<()> {
        let block = json!({"type": "tool_use", "id": format!("t{n}"), "name": format!("mcp__tarae__{name}"), "input": {}});
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_start", "index": 1, "content_block": block}}))?;
        let end = hold.and_then(|h| json.find(h)).unwrap_or(json.len());
        let chars: Vec<char> = json[..end].chars().collect();
        for piece in chars.chunks(12) {
            let delta = json!({"type": "input_json_delta", "partial_json": piece.iter().collect::<String>()});
            self.line(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 1, "delta": delta}}))?;
            std::thread::sleep(self.pause / 2);
        }
        if hold.is_some() {
            // (an empty piece after a pause, as the stream sends: tarae's reader sends what it holds at a line)
            std::thread::sleep(Duration::from_millis(60));
            let delta = json!({"type": "input_json_delta", "partial_json": ""});
            self.line(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 1, "delta": delta}}))?;
            return self.lines.by_ref().try_for_each(|l| l.map(drop));
        }
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_stop", "index": 1}}))
    }

    /// (`three`: the edit changes the doc comment and the print too — three changes in one review)
    fn explain(&mut self, whole: bool, three: bool) -> io::Result<()> {
        self.explain_held(whole, three, None)
    }

    /// `explain` whole, its edit of `n` changes (1 ‥ 3): the insert's, then the print's, then the doc comment's.
    fn explain_items(&mut self, n: usize) -> io::Result<()> {
        self.explain_with(true, n, None)
    }

    /// `explain`, its edit's input held at `hold` while it's written (`writemid`).
    fn explain_held(&mut self, whole: bool, three: bool, hold: Option<&str>) -> io::Result<()> {
        self.explain_with(whole, if three { 3 } else { 1 }, hold)
    }

    fn explain_with(&mut self, whole: bool, n: usize, hold: Option<&str>) -> io::Result<()> {
        self.words("Let me look at how the map is filled first.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
        self.words(
            "The name is cloned because **insert** takes the key by value, and `first` moves into the map",
        )?;
        self.words(
            "in the same call. Keying by the name alone means a second strand with the same name replaces",
        )?;
        self.words("the first:")?;
        self.text("\n\n```rust\nlet mut threads: HashMap<String, Thread> = HashMap::new();\n")?;
        if !whole {
            // (mid-answer: the shot is taken while it waits — until tarae goes)
            self.text("threads.entry(first.name.clone())")?;
            return self.lines.by_ref().try_for_each(|l| l.map(drop));
        }
        self.text("threads.entry(first.name.clone()).or_insert(first);\n```\n\n")?;
        self.words("I will switch it to the entry API so a repeat keeps the first strand.")?;
        self.text("\n")?;
        // (each item's keys in the order the model writes them: what it replaces, then what it writes)
        let item = |old: &str, new: &str| {
            let s = |t: &str| Value::from(t).to_string();
            format!(r#"{{"file_path": "src/main.rs", "old_string": {}, "new_string": {}}}"#, s(old), s(new))
        };
        let edit = item(
            "    threads.insert(first.name.clone(), first);\n",
            "    threads.entry(first.name.clone()).or_insert(first);\n",
        );
        let doc = item("/// 실타래 — 이어진 실 한 가닥.\n", "/// 실타래 — 같은 이름은 첫 가닥으로 남는다.\n");
        let print = item(
            "        println!(\"{name}: {} knots, {} long\", t.knots.len(), t.length());\n",
            "        println!(\"{name}: {} knots\", t.knots.len());\n",
        );
        let items = match n {
            3 => [doc, edit, print].join(", "),
            2 => [edit, print].join(", "),
            _ => edit,
        };
        let input = format!(r#"{{"edits": [{items}]}}"#);
        self.input(2, "edit", &input, hold)?;
        self.call(2, "edit", serde_json::from_str(&input).map_err(io::Error::other)?)?;
        self.words("Done — **main.rs** now keeps the first strand when a name repeats.")?;
        self.words("Nothing else reads the map, so no other change is needed.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// Motion §3's turns, timed as `cargo xtask stream` measured them (2026-10-25: prose about 13 characters every 50 ms;
    /// summarized thinking silent while its block is open, then landing whole): `prose` from the first moment; `think` a
    /// thinking block open and silent 1.2 s, its summary at once, then the prose; `stall` the prose 0.6 s, 2 s of
    /// nothing, the prose again; `read` a few words, a read of tarae's, its result back to Claude and a request out as
    /// claude says them, 2.5 s before the next answer's first words (2.75 s measured), the rest; `all` (the working sign's,
    /// #159) every state in one turn: 0.8 s before anything comes, the think, prose, a stall, the read and its wait, prose.
    fn shimmer(&mut self, shape: &str) -> io::Result<()> {
        let ev = |e: Value| json!({"type": "stream_event", "event": e});
        let prose = "The name is cloned because insert takes the key by value, and the first strand moves into the \
                     map in the same call, so the map keeps a key of its own while the thread keeps its name.";
        let pieces: Vec<String> =
            prose.chars().collect::<Vec<_>>().chunks(13).map(|c| c.iter().collect()).collect();
        let stream = |turn: &mut Self, pieces: &[String]| -> io::Result<()> {
            for p in pieces {
                let delta = json!({"type": "text_delta", "text": p});
                turn.line(ev(json!({"type": "content_block_delta", "index": 1, "delta": delta})))?;
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        };
        // (`sign`: the working sign's candidates (`sign.rs`) — every state of `all`, each long enough to watch)
        if shape == "sign" {
            return self.sign_turn(&pieces);
        }
        // (`all`: the turn's start, 0.8 s before anything comes)
        if shape == "all" {
            std::thread::sleep(Duration::from_millis(800));
        }
        if shape == "think" || shape == "all" {
            let block = json!({"type": "thinking", "thinking": "", "signature": ""});
            self.line(ev(json!({"type": "content_block_start", "index": 0, "content_block": block})))?;
            std::thread::sleep(Duration::from_millis(1200));
            for t in ["Checking how the map ", "keeps its keys, ", "then why the name is cloned."] {
                let delta = json!({"type": "thinking_delta", "thinking": t});
                self.line(ev(json!({"type": "content_block_delta", "index": 0, "delta": delta})))?;
            }
            self.line(ev(json!({"type": "content_block_stop", "index": 0})))?;
        }
        match shape {
            "stall" => {
                stream(self, &pieces[..12])?;
                std::thread::sleep(Duration::from_secs(2));
                stream(self, &pieces[12..])?;
            }
            // (`all`: after the think, prose 0.6 s, a stall of 1.6 s, a read and 1.5 s before the next answer, the rest)
            "all" => {
                stream(self, &pieces[..12])?;
                std::thread::sleep(Duration::from_millis(1600));
                self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
                let result = json!({"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "fn main() {"}]});
                self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))?;
                self.line(json!({"type": "system", "subtype": "status", "status": "requesting"}))?;
                std::thread::sleep(Duration::from_millis(1500));
                stream(self, &pieces[12..])?;
            }
            "read" => {
                stream(self, &pieces[..4])?;
                self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
                let result = json!({"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "fn main() {"}]});
                self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))?;
                self.line(json!({"type": "system", "subtype": "status", "status": "requesting"}))?;
                std::thread::sleep(Duration::from_millis(2500));
                stream(self, &pieces[4..])?;
            }
            _ => stream(self, &pieces)?,
        }
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// `shimmer-sign`'s turn, timed by `SIGN_TURN`: nothing, a think open and silent then its summary, prose, a stall, a
    /// read and the wait for the next answer, prose — the prose's pieces over again for as long as a phase lasts.
    fn sign_turn(&mut self, pieces: &[String]) -> io::Result<()> {
        let ev = |e: Value| json!({"type": "stream_event", "event": e});
        let ms = |n: usize| Duration::from_millis(SIGN_TURN[n].1);
        let mut chars = pieces.concat().chars().collect::<Vec<_>>().into_iter().cycle();
        // (the prose as a real stream comes: `SIGN_STREAM`'s gaps and sizes over again for as long as the phase lasts)
        let mut stream = |turn: &mut Self, d: Duration| -> io::Result<()> {
            let mut spent = 0;
            for &(gap, bytes) in SIGN_STREAM.iter().cycle() {
                if spent + gap > d.as_millis() as u64 {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(gap));
                spent += gap;
                let text: String = chars.by_ref().take(bytes).collect();
                let delta = json!({"type": "text_delta", "text": text});
                turn.line(ev(json!({"type": "content_block_delta", "index": 1, "delta": delta})))?;
            }
            Ok(())
        };
        std::thread::sleep(ms(0));
        let block = json!({"type": "thinking", "thinking": "", "signature": ""});
        self.line(ev(json!({"type": "content_block_start", "index": 0, "content_block": block})))?;
        std::thread::sleep(ms(1));
        let delta = json!({"type": "thinking_delta", "thinking": "Checking how the map keeps its keys."});
        self.line(ev(json!({"type": "content_block_delta", "index": 0, "delta": delta})))?;
        self.line(ev(json!({"type": "content_block_stop", "index": 0})))?;
        stream(self, ms(2))?;
        std::thread::sleep(ms(3));
        self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
        let result = json!({"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "fn main() {"}]});
        self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))?;
        self.line(json!({"type": "system", "subtype": "status", "status": "requesting"}))?;
        std::thread::sleep(ms(4));
        stream(self, ms(5))?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// V4's turn on `MAIN_V3_RS`: a read of `main.rs`'s `main` — the lines in front of you, so the view never leaves
    /// them —, a few words, then an edit of three changes a line apart (touching ones would be one change) — the doc
    /// comment's, the map's type dropped (the one to refuse) and the insert's — its input coming
    /// 8 characters every 50 ms (a measured stream's, `shimmer`: about 13 a 50 ms; slower, so a video reads it), and a
    /// pause once it's answered.
    fn edit_in_buffer(&mut self) -> io::Result<()> {
        self.words("A thread owns its name, so the map needs a key of its own.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:8-16"]}))?;
        std::thread::sleep(Duration::from_millis(1500));
        self.words("**insert** takes the key by value, and a repeated name replaces the first strand.")?;
        self.words("I will keep the first with the entry API, and say so in the doc comment.")?;
        self.text("\n")?;
        let item = |old: &str, new: &str| {
            let s = |t: &str| Value::from(t).to_string();
            format!(r#"{{"file_path": "src/main.rs", "old_string": {}, "new_string": {}}}"#, s(old), s(new))
        };
        let doc = item("/// 실타래 — 이어진 실 한 가닥.\n", "/// 실타래 — 첫 가닥이 남는다.\n");
        let typed = item("    let mut m: Map = Map::new();\n", "    let mut m = Map::new();\n");
        let insert = item(
            "    m.insert(t.name.clone(), t);\n",
            "    m.entry(t.name.clone())\n        .or_insert(t);\n",
        );
        self.edit_streamed(2, &format!(r#"{{"edits": [{doc}, {typed}, {insert}]}}"#))?;
        // (the review closes on its last answer: the three stamps stay in sight on the follow card's edit row)
        std::thread::sleep(Duration::from_millis(2500));
        self.words("Done — **main.rs** keeps the first strand when a name repeats.")?;
        self.words("Nothing else reads the map, so no other change is needed.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// The turn's `n`th call, an `edit` whose `input` (its JSON) streams in 8 characters every 50 ms — a measured
    /// stream's pace is about 13 a 50 ms (`shimmer`); slower, so a video reads it — then is called.
    fn edit_streamed(&mut self, n: u64, input: &str) -> io::Result<()> {
        let block =
            json!({"type": "tool_use", "id": format!("t{n}"), "name": "mcp__tarae__edit", "input": {}});
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_start", "index": 1, "content_block": block}}))?;
        for piece in input.chars().collect::<Vec<_>>().chunks(8) {
            let delta = json!({"type": "input_json_delta", "partial_json": piece.iter().collect::<String>()});
            self.line(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 1, "delta": delta}}))?;
            std::thread::sleep(Duration::from_millis(50));
        }
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_stop", "index": 1}}))?;
        self.call(n, "edit", serde_json::from_str(input).map_err(io::Error::other)?)
    }

    /// V5's turn, on `SPOOL_V4_RS` (review at the end on: its edits land unseen and the turn goes on): three reads far
    /// apart, the view following each — after the second its words come 1.5 s on and its next 1.5 s after them, the
    /// pace a key of yours takes the view back at and `space F` follows again at, the third read 2 s later — then two
    /// edits streamed in at lines 46 and 77, and an answer naming line 46 (its card at the turn's end).
    fn follow_replay(&mut self) -> io::Result<()> {
        let item = |old: &str, new: &str| {
            let s = |t: &str| Value::from(t).to_string();
            format!(
                r#"{{"edits": [{{"file_path": "src/spool.rs", "old_string": {}, "new_string": {}}}]}}"#,
                s(old),
                s(new)
            )
        };
        self.words("First what the doc comment promises.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/spool.rs:3-7"]}))?;
        std::thread::sleep(Duration::from_millis(2500));
        self.words("The queue's order. Now the loop that winds.")?;
        self.text("\n\n")?;
        self.call(2, "read", json!({"files": ["src/spool.rs:42-56"]}))?;
        std::thread::sleep(Duration::from_millis(1500));
        self.words("It pops from the back: the newest strand goes first.")?;
        std::thread::sleep(Duration::from_millis(1500));
        self.words("And at the end:")?;
        std::thread::sleep(Duration::from_millis(2000));
        self.text("\n\n")?;
        self.call(3, "read", json!({"files": ["src/spool.rs:75-83"]}))?;
        std::thread::sleep(Duration::from_millis(2500));
        self.words("A sort hides it. I will pop from the front and drop the sort.")?;
        self.text("\n\n")?;
        let pop =
            item("    while let Some(s) = q.pop_back() {\n", "    while let Some(s) = q.pop_front() {\n");
        self.edit_streamed(4, &pop)?;
        std::thread::sleep(Duration::from_millis(2000));
        let sort = item(
            "    // The last popped is the first\n    // that came, so the order here\n    // is the queue's reversed.\n    // Sorting by length makes it\n    // stable, whatever the order.\n    sp.wound.sort_by_key(measure);\n",
            "    // Popped from the front, they\n    // stay in the order they came.\n",
        );
        self.edit_streamed(5, &sort)?;
        std::thread::sleep(Duration::from_millis(2000));
        self.words("Done — **src/spool.rs:46** winds from the front now, so `wound` keeps the queue's order with no sort.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// Motion §2's turn: a word, then one edit rewriting six lines of the loop, sent whole (no input streamed: the
    /// frames kept are the landing's and its drying's) — then, with `pause`, three seconds before its last words and
    /// the result (the drying runs out before the turn's review opens), else the result at once.
    fn six(&mut self, pause: bool) -> io::Result<()> {
        self.words("I will make the loop keep the first strand, in one go.")?;
        std::thread::sleep(Duration::from_millis(500));
        let old = "    let first = Thread { name: \"타래\".into(), knots: vec![1, 2, 3] };\n    threads.insert(first.name.clone(), first);\n    for (name, t) in &threads {\n        println!(\"{name}: {} knots, {} long\", t.knots.len(), t.length());\n";
        let new = "    let first = Thread { name: \"타래\".into(), knots: vec![1, 2, 3, 5] };\n    threads.entry(first.name.clone()).or_insert(first);\n    for (name, t) in threads.iter() {\n        let long = t.length();\n        println!(\"{name}: {} knots, {long} long\", t.knots.len());\n        debug_assert!(long > 0);\n";
        self.call(
            2,
            "edit",
            json!({"edits": [{"file_path": "src/main.rs", "old_string": old, "new_string": new}]}),
        )?;
        if pause {
            std::thread::sleep(Duration::from_secs(3));
        }
        self.words("Done.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// Motion §4's turn: twenty stops — reads, lookups, edits, runs, the second-to-last run failing — each a call and its
    /// result as claude's stream says them (no request of tarae's: nothing runs, no edit waits on a review), a few words
    /// before the last; then held there until tarae goes (the follow card's strip of a turn at work).
    fn strip(&mut self) -> io::Result<()> {
        let stops: [(&str, Value, &str); 20] = [
            ("read", json!({"files": ["src/main.rs:1-20"]}), "fn main() {"),
            ("search", json!({"pattern": "Thread"}), "src/main.rs:3: struct Thread"),
            ("read", json!({"files": ["src/main.rs:20-40"]}), "}"),
            (
                "references",
                json!({"path": "src/main.rs", "symbol": "Thread", "line": 3}),
                "4 references in 1 file",
            ),
            ("definition", json!({"symbol": "length"}), "1 definition"),
            ("read", json!({"files": ["src/main.rs:7-14"]}), "let first"),
            (
                "edit",
                json!({"edits": [{"file_path": "src/main.rs", "old_string": "a", "new_string": "b"}]}),
                "edited",
            ),
            ("run", json!({"command": "cargo build -q"}), "exit 0"),
            ("read", json!({"files": ["src/main.rs:1-12"]}), "use std"),
            ("search", json!({"pattern": "knots"}), "src/main.rs:5: knots"),
            (
                "edit",
                json!({"edits": [{"file_path": "src/main.rs", "old_string": "c", "new_string": "d"}]}),
                "edited",
            ),
            ("symbols", json!({"path": "src/main.rs"}), "3 symbols"),
            ("run", json!({"command": "cargo test -q"}), "exit 101\nthread 'main' panicked"),
            ("read", json!({"files": ["src/main.rs:14-30"]}), "println"),
            (
                "edit",
                json!({"edits": [{"file_path": "src/main.rs", "old_string": "e", "new_string": "f"}]}),
                "edited",
            ),
            ("run", json!({"command": "cargo test -q"}), "exit 0"),
            ("read", json!({"files": ["src/main.rs:1-40"]}), "fn main"),
            ("search", json!({"pattern": "clone"}), "src/main.rs:9: clone"),
            (
                "edit",
                json!({"edits": [{"file_path": "src/main.rs", "old_string": "g", "new_string": "h"}]}),
                "edited",
            ),
            ("read", json!({"files": ["src/main.rs:7-14"]}), "let first"),
        ];
        self.words("I will read the loop, then tidy how the threads are kept.")?;
        self.text("\n\n")?;
        for (n, (name, args, said)) in stops.into_iter().enumerate() {
            let id = format!("s{n}");
            if n == 19 {
                self.words("One more look, then I will wait here.")?;
                self.text("\n\n")?;
            }
            let tool =
                json!({"type": "tool_use", "id": id, "name": format!("mcp__tarae__{name}"), "input": args});
            self.line(json!({"type": "assistant", "message": {"id": format!("ms{n}"), "content": [tool]}}))?;
            std::thread::sleep(Duration::from_millis(40));
            let result = json!({"type": "tool_result", "tool_use_id": id, "content": [{"type": "text", "text": said}]});
            self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))?;
            std::thread::sleep(Duration::from_millis(40));
        }
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// `explain`'s start, then a quote and a list, stopping in its last item — and waiting until tarae goes.
    fn explain_listed(&mut self) -> io::Result<()> {
        self.words("The name is cloned because **insert** takes the key by value.")?;
        self.text("\n\n> ")?;
        self.words("같은 이름은 한 가닥으로 남는다.")?;
        self.text("\n\n- ")?;
        self.words("`insert` replaces the value")?;
        self.text("\n- ")?;
        self.words("`entry` keeps the first:")?;
        // (characters the shots' fonts all have)
        for c in ["同じ", "名前は", "一本", "だ"] {
            self.text(c)?;
        }
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// `space i`'s claude (`ask.rs`): its `edit` of the line being written, its `new_string` held mid-line until
    /// tarae goes (the card's preview of it).
    fn rewrite(&mut self) -> io::Result<()> {
        self.input(1, "edit", &ask_edit().to_string(), Some("or_insert("))
    }

    /// `space i`'s claude on lines 9 and 10 of the demo's `main.rs`, a selection each: one `edit` item over both.
    fn rewrite_touching(&mut self) -> io::Result<()> {
        let old = "    let first = Thread { name: \"타래\".into(), knots: vec![1, 2, 3] };\n    threads.insert(first.name.clone(), first);";
        let new = "    let first = Thread { name: \"타래\".into(), knots: vec![1, 2] };\n    threads.entry(first.name.clone()).or_insert(first);";
        let edits = json!({"edits": [{"file_path": "src/main.rs", "old_string": old, "new_string": new}]});
        self.call(1, "edit", edits)?;
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// A turn of a big conversation (`big`): its init, a request that read 243k tokens from the 1h cache and wrote
    /// 2.1k, a short answer, its result — past idle compaction's 200k, so the row says when it compacts.
    fn big(&mut self) -> io::Result<()> {
        let id = "conv-demo";
        self.line(
            json!({"type": "system", "subtype": "init", "session_id": id, "model": "claude-opus-5-5"}),
        )?;
        self.line(json!({"type": "system", "subtype": "status", "status": "requesting"}))?;
        let (read, write, output) = (243_100, 2_140, 912);
        let usage = json!({"input_tokens": 6, "cache_read_input_tokens": read, "cache_creation_input_tokens": write,
            "cache_creation": {"ephemeral_1h_input_tokens": write}, "output_tokens": output});
        let message = json!({"id": format!("{id}-1"), "model": "claude-opus-5-5", "usage": usage});
        self.line(json!({"type": "stream_event", "event": {"type": "message_start", "message": message}}))?;
        self.words("The map is filled in one pass over the strands; nothing else writes to it.")?;
        let model = json!({"claude-opus-5-5": {"inputTokens": 6, "cacheReadInputTokens": read,
            "cacheCreationInputTokens": write, "outputTokens": output, "contextWindow": 400_000}});
        let cost = 1.8125;
        self.line(
            json!({"type": "result", "result": "ok", "session_id": id, "total_cost_usd": cost, "usage": usage,
            "modelUsage": model}),
        )
    }

    /// A turn that says the plan's weekly window at `percent` (its own `rate_limit_event`, which puts the session on
    /// the plan), then answers — `long`: a step a line for longer than the chat is tall — and its result.
    fn foot(&mut self, percent: u32, long: bool) -> io::Result<()> {
        let id = "conv-foot";
        let info = json!({"status": "allowed", "resetsAt": 4_071_000_000u64, "rateLimitType": "seven_day",
            "utilization": f64::from(percent) / 100.0});
        self.line(json!({"type": "rate_limit_event", "rate_limit_info": info}))?;
        self.line(
            json!({"type": "system", "subtype": "init", "session_id": id, "model": "claude-opus-5-5"}),
        )?;
        let usage = json!({"input_tokens": 3, "cache_read_input_tokens": 7_924, "cache_creation_input_tokens": 8_125,
            "cache_creation": {"ephemeral_1h_input_tokens": 8_125}, "output_tokens": 185});
        let message = json!({"id": format!("{id}-1"), "model": "claude-opus-5-5", "usage": usage});
        self.line(json!({"type": "stream_event", "event": {"type": "message_start", "message": message}}))?;
        self.text("The map is filled in one pass over the strands:\n\n")?;
        if long {
            for n in 1..=40 {
                self.text(&format!("{n}. strand {n} is read, its name cloned, and it goes into the map\n"))?;
            }
            self.text("\n")?;
        }
        self.text("Nothing else writes to it.\n\nThat is all.")?;
        let model = json!({"claude-opus-5-5": {"inputTokens": 3, "cacheReadInputTokens": 7_924,
            "cacheCreationInputTokens": 8_125, "outputTokens": 185, "contextWindow": 200_000}});
        self.line(
            json!({"type": "result", "result": "ok", "session_id": id, "total_cost_usd": 0.018, "usage": usage,
            "modelUsage": model}),
        )
    }

    /// Waits for tarae to go — a member still reading when the shot is taken.
    fn reads_on(&mut self) -> io::Result<()> {
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// The one pass (`space v c`, in the background — bgreview): three reads, then (`done`) a finding on
    /// knot.rs and its verdict; else it reads on.
    fn reviewer(&mut self, done: bool) -> io::Result<()> {
        for (n, f) in ["src/knot.rs", "src/main.rs", "src/thread.rs"].iter().enumerate() {
            let read = json!({"type": "tool_use", "id": format!("r{n}"), "name": "mcp__tarae__read", "input": {"files": [f]}});
            self.line(json!({"type": "assistant", "message": {"id": format!("m{n}"), "content": [read]}}))?;
        }
        if !done {
            return self.reads_on();
        }
        let text = "`untie` does nothing: a caller that unties a knot keeps it.";
        self.call(1, "note", json!({"path": "src/knot.rs", "line": 1, "text": text}))?;
        self.line(json!({"type": "result", "result": "One finding: untie keeps the knot."}))
    }

    /// The one pass as the docs show it (`claude-reviews.png`): its reads, three findings on `thread.rs`'s
    /// change, and a verdict that weighs them.
    fn reviewer_full(&mut self) -> io::Result<()> {
        for (n, f) in ["src/thread.rs", "src/main.rs", "src/knot.rs"].iter().enumerate() {
            let read = json!({"type": "tool_use", "id": format!("r{n}"), "name": "mcp__tarae__read", "input": {"files": [f]}});
            self.line(json!({"type": "assistant", "message": {"id": format!("m{n}"), "content": [read]}}))?;
        }
        let note = |line: u64, text: &str| json!({"path": "src/thread.rs", "line": line, "text": text});
        let findings = [
            (
                10,
                "`length` sums `u32` gaps: a long strand overflows — a panic in debug builds, a wrong length in release.",
            ),
            (
                14,
                "The doc says the knot goes at the strand's end, but `at` is a gap: `tie(5)` makes the strand 5 longer.",
            ),
            (
                15,
                "No test ties a knot: nothing holds the doc's claim, or catches `tie(0)` adding a knot with no length.",
            ),
        ];
        for (n, (line, text)) in findings.iter().enumerate() {
            self.call(n as u64 + 1, "note", note(*line, text))?;
        }
        let verdict = "Three findings, all in thread.rs — none blocks, the first matters most.\n\n\
            - `length` can overflow on a long strand (a panic in debug builds).\n\
            - `tie`'s doc and its argument disagree: `at` is a gap, not a place.\n\
            - No test covers `tie`.\n\n\
            knot.rs's new `untie` is empty and unused; main.rs only reads the strand.";
        // (how long its turn took, as claude says it — the report's "Done in": fixed, so retakes compare the same)
        self.line(json!({"type": "result", "result": verdict, "duration_ms": 112_400}))
    }

    /// A board's member, by what its message (`msg`) asks: an angle notes its claims on the demo's changes
    /// (`done`: every angle ends; else security reads on, three reads made, and the others after correctness
    /// read on too); a skeptic confirms `knot.rs`'s claims (the second the same failure) and, on `thread.rs`,
    /// refutes the first and confirms the second.
    fn board(&mut self, msg: &str, done: bool) -> io::Result<()> {
        let note = |path: &str, line: u64, text: &str| json!({"path": path, "line": line, "text": text});
        if msg.contains("(angle: correctness)") {
            self.call(
                1,
                "note",
                note("src/knot.rs", 1, "`untie` does nothing: a caller that unties a knot keeps it."),
            )?;
            let doc =
                "The doc says the knot goes at the strand's end, but `at` is a gap: the length grows by it.";
            self.call(2, "note", note("src/thread.rs", 13, doc))?;
            return self
                .line(json!({"type": "result", "result": "Two findings: untie, and the doc of tie."}));
        }
        if msg.contains("(angle: security)") {
            if !done {
                for (n, f) in ["src/knot.rs", "src/main.rs", "src/thread.rs"].iter().enumerate() {
                    let read = json!({"type": "tool_use", "id": format!("r{n}"), "name": "mcp__tarae__read", "input": {"files": [f]}});
                    self.line(
                        json!({"type": "assistant", "message": {"id": format!("m{n}"), "content": [read]}}),
                    )?;
                }
                return self.reads_on();
            }
            let text = "`untie` is public and empty: any caller believes the knot is gone, and nothing says it isn't.";
            self.call(1, "note", note("src/knot.rs", 1, text))?;
            return self.line(json!({"type": "result", "result": "One finding."}));
        }
        if msg.contains("(angle: tests)") && done {
            self.call(
                1,
                "note",
                note("src/thread.rs", 14, "No test ties a knot: nothing holds the doc's claim."),
            )?;
            return self.line(json!({"type": "result", "result": "One finding."}));
        }
        // (a skeptic's claims come after the diff every member is sent: its group is read from there)
        if let Some((_, claims)) = msg.split_once("(role: skeptic)") {
            let judge = |claim: u64, verdict: &str, trace: &str| json!({"claim": claim, "verdict": verdict, "trace": trace});
            if claims.contains("knot.rs") {
                let trace = "knot.rs:1 — untie's body is empty; nothing in main.rs or thread.rs takes a knot out, so a call leaves it";
                self.call(1, "judge", judge(1, "confirmed", trace))?;
                let same =
                    json!({"claim": 2, "verdict": "confirmed", "trace": "the same empty body", "same_as": 1});
                self.call(2, "judge", same)?;
            } else {
                let held = "thread.rs:15 — push puts the knot last: the doc holds";
                self.call(1, "judge", judge(1, "refuted", held))?;
                self.call(
                    2,
                    "judge",
                    judge(2, "confirmed", "thread.rs:14 — no #[test] in the crate calls tie"),
                )?;
            }
            return self.line(json!({"type": "result", "result": "Judged."}));
        }
        self.reads_on()
    }

    /// A permission prompt of claude's for its own tool `tool` (the turn's `n`th): its row in the chat, the ask
    /// (`can_use_tool`), then tarae's answer waited for — as `call` does for tarae's tools.
    fn permission(&mut self, n: u64, tool: &str, input: Value) -> io::Result<()> {
        let used = json!({"type": "tool_use", "id": format!("p{n}"), "name": tool, "input": input});
        self.line(json!({"type": "assistant", "message": {"id": format!("q{n}"), "content": [used]}}))?;
        let request = json!({"subtype": "can_use_tool", "tool_name": tool, "input": input});
        self.line(json!({"type": "control_request", "request_id": format!("r{n}"), "request": request}))?;
        let id = format!("\"r{n}\"");
        for l in self.lines.by_ref() {
            if l?.contains(&id) {
                break;
            }
        }
        Ok(())
    }

    /// What `:granted` lists, asked for one at a time: a run first (the sandbox looked at — `also` waits for that),
    /// a read outside the project, a host through `also`, another read. Places under home that aren't there
    /// (nothing is read: the answer is all), a host that resolves nowhere, `ls` for the runs (one that runs
    /// without a card).
    fn grants(&mut self) -> io::Result<()> {
        let home = std::env::var("HOME").unwrap_or_default();
        self.words("Let me look at the notes the strands follow, outside the project.")?;
        self.text("\n")?;
        self.call(1, "run", json!({"command": "ls"}))?;
        self.permission(2, "Read", json!({"file_path": format!("{home}/notes/strands.md")}))?;
        let also = json!({"command": "ls", "also": {"network": "skein.example.invalid"},
            "reason": "the knots' index is fetched from there"});
        self.call(3, "run", also)?;
        self.permission(4, "Read", json!({"file_path": format!("{home}/Sites/skein/knots.css")}))?;
        self.words("Read them: nothing in the project needs to change.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    fn ask_outside_long(&mut self) -> io::Result<()> {
        for n in 1..=10 {
            // (a paragraph at a time: ten of them word by word would outlast the scene's wait)
            self.text(&format!(
                "{n}. The skein keeps its threads in a map by name, and each thread counts its knots as it \
                 is spun; the length is the sum of the strands between them, so a thread with no knots is \
                 still as long as its one strand."
            ))?;
            self.text("\n\n")?;
        }
        self.ask_outside()
    }

    /// `ask_outside` after ~10 s of words streamed slowly: a card that arrives while you're in another session.
    fn ask_outside_later(&mut self) -> io::Result<()> {
        let reading = "Reading the tests first: the knots are counted as each thread is spun, so the summary waits \
                       until every strand is tied, and the registry has to be reached before any of it builds.";
        for w in reading.split(' ') {
            self.text(&format!("{w} "))?;
            std::thread::sleep(Duration::from_millis(290));
        }
        self.text("\n\n")?;
        // (a reason of four rows: at 80×43 the card, whole, stands on rows 29–40 and meets the tests floating on 11–30, so
        // it folds to its peek — at 80×50 three rows only touched them and folded nothing)
        self.ask_outside_for(
            "cargo needs to fetch the registry index, and the tests bind a port on this machine that the \
             sandbox keeps closed, so neither works inside it",
        )
    }

    fn ask_outside(&mut self) -> io::Result<()> {
        self.ask_outside_for("cargo needs to fetch the registry index")
    }

    fn ask_outside_for(&mut self, reason: &str) -> io::Result<()> {
        self.words(
            "The tests need the network for the registry, so I will ask to run them outside the sandbox.",
        )?;
        self.text("\n")?;
        let run = json!({"command": "cargo test --quiet", "outside_sandbox": true, "reason": reason});
        self.call(1, "run", run)?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// (`now-*`: a read, a plan, a sentence of intent, then the tests run — in flight, held there; `Card`: asked to run
    /// outside the sandbox, the card up; `Read`: a read in flight instead, held there)
    fn now(&mut self, held: Held) -> io::Result<()> {
        self.words("Let me look at how the map is filled first.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
        let steps = json!([
            {"text": "Read how the map is filled", "status": "done"},
            {"text": "Keep the first strand on a repeat", "status": "done"},
            {"text": "Run the unit tests", "status": "doing"},
            {"text": "Say what changed", "status": "todo"}
        ]);
        self.call(2, "plan", json!({"title": "Keep the first strand", "steps": steps}))?;
        self.words("A repeated name now keeps the first strand.")?;
        self.words("I will run the unit tests to check that nothing else relied on the old replace.")?;
        self.text("\n")?;
        if held == Held::Card {
            let run = json!({"command": "cargo test --quiet", "outside_sandbox": true,
                "reason": "cargo needs to fetch the registry index"});
            self.call(3, "run", run)?;
            return self.lines.by_ref().try_for_each(|l| l.map(drop));
        }
        if held == Held::Write {
            // (the two calls' results as claude's stream says them: no call left in flight, the edit's input the step)
            for id in ["t1", "t2"] {
                let result = json!({"type": "tool_result", "tool_use_id": id, "content": [{"type": "text", "text": "ok"}]});
                self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))?;
            }
            return self.input(3, "edit", &ask_edit().to_string(), Some("or_insert"));
        }
        let (name, input) = match held {
            Held::Read => ("read", json!({"files": ["src/thread.rs:1-12"]})),
            _ => ("run", json!({"command": "cargo test -q"})),
        };
        let tool =
            json!({"type": "tool_use", "id": "t3", "name": format!("mcp__tarae__{name}"), "input": input});
        self.line(json!({"type": "assistant", "message": {"id": "m3", "content": [tool]}}))?;
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// `steer`'s turn: a read, held 15 s (the scene's keys queue a message and steer it meanwhile), a plan answered at once
    /// — the steer rides its answer — a few words, and the turn held there.
    fn steer(&mut self) -> io::Result<()> {
        self.words("Let me look at how the map is filled first.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
        std::thread::sleep(Duration::from_secs(15));
        let steps = json!([
            {"text": "Read how the map is filled", "status": "done"},
            {"text": "Read src/thread.rs", "status": "doing"}
        ]);
        self.call(2, "plan", json!({"title": "Keep the first strand", "steps": steps}))?;
        self.words("Reading src/thread.rs instead, as you asked.")?;
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// #187's turn: a run of a long command — a call and its result as claude's stream says them (nothing runs, no request
    /// of tarae's) — then a few words and the turn's end.
    fn cut_hand(&mut self) -> io::Result<()> {
        let run = "cargo fmt --all && git diff --stat | tail -3 && cargo clippy --all-targets -q 2>&1 | grep -E \
                   'warning|error' | head -20 && cargo test -q --bin tarae whole";
        let stops = [("run", json!({"command": run}), "exit 0")];
        self.words("I tidied the loop and checked it.")?;
        self.text("\n\n")?;
        for (n, (name, args, said)) in stops.into_iter().enumerate() {
            let id = format!("c{n}");
            let tool =
                json!({"type": "tool_use", "id": id, "name": format!("mcp__tarae__{name}"), "input": args});
            self.line(json!({"type": "assistant", "message": {"id": format!("mc{n}"), "content": [tool]}}))?;
            let result = json!({"type": "tool_result", "tool_use_id": id, "content": [{"type": "text", "text": said}]});
            self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))?;
        }
        self.words("That is all.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// A call and its result as claude's stream says them — nothing runs, no request of tarae's (`run_tests` in a
    /// scene: no test is built).
    fn ran(&mut self, id: &str, name: &str, args: Value, said: &str) -> io::Result<()> {
        let tool =
            json!({"type": "tool_use", "id": id, "name": format!("mcp__tarae__{name}"), "input": args});
        self.line(json!({"type": "assistant", "message": {"id": format!("m{id}"), "content": [tool]}}))?;
        std::thread::sleep(Duration::from_millis(300));
        let result =
            json!({"type": "tool_result", "tool_use_id": id, "content": [{"type": "text", "text": said}]});
        self.line(json!({"type": "user", "message": {"role": "user", "content": [result]}}))
    }

    /// focus D1's `focus-explain` (and `focus-typing`, `slow`: a pause after each read, while you type): three reads, then
    /// three paragraphs — the first naming the loop's line, the other two (a summary and a caveat) naming none.
    fn focus_explain(&mut self, slow: bool) -> io::Result<()> {
        let gap = Duration::from_millis(if slow { 900 } else { 0 });
        self.words("Let me read the loop and the thread it prints.")?;
        self.text("\n\n")?;
        for (n, at) in [(1, "src/main.rs:7-14"), (2, "src/thread.rs:1-5"), (3, "src/thread.rs:7-11")] {
            self.call(n, "read", json!({"files": [at]}))?;
            std::thread::sleep(gap);
        }
        self.words(
            "The loop at src/main.rs:11 prints each thread's name, how many knots it has and its length, the sum of \
             its knots.",
        )?;
        self.text("\n\n")?;
        self.words(
            "It prints them in the map's own order, which changes from run to run: a hash map keeps no order of \
             insertion, so two runs over the same threads may list them differently.",
        )?;
        self.text("\n\n")?;
        self.words(
            "One caveat: a repeated name replaces the earlier strand before the loop ever sees it, so a skein with \
             two threads of one name prints one. That is all.",
        )?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// #389's answer: a read, then a paragraph citing `src/thread.rs:9` with names written as code, the lines there quoted,
    /// a last paragraph citing `src/main.rs:12`.
    /// #389 step 5's answer: names written as code with no place cited, one in a block of Claude's own.
    fn chatlink_named(&mut self) -> io::Result<()> {
        self.call(1, "read", json!({"files": ["src/main.rs:17-20"]}))?;
        self.words(
            "Each thread is said by `say`, and a knot goes on by `tie`. To grow every thread by one:",
        )?;
        self.text("\n\n```rust\nfn grow(m: &mut Map) {\n    for t in m.values_mut() {\n        tie(t, 4);\n    }\n}\n```\n\n")?;
        self.words("Nothing else changes.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// #389 step 5b's answer: names written as code — two the project defines, one it doesn't yet (Claude's new code),
    /// a keyword — and a block of Claude's own.
    fn chatlink_under(&mut self) -> io::Result<()> {
        self.call(1, "read", json!({"files": ["src/main.rs:17-20"]}))?;
        self.words(
            "Each thread is said by `say`, and a knot goes on by `tie`. Nothing defines `grow` yet, so add it — \
             the `for` walks every thread:",
        )?;
        self.text("\n\n```rust\nfn grow(m: &mut Map) {\n    for t in m.values_mut() {\n        tie(t, 4);\n    }\n}\n```\n\n")?;
        self.words("Nothing else changes.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    fn chatlink(&mut self) -> io::Result<()> {
        self.call(1, "read", json!({"files": ["src/thread.rs:7-11"]}))?;
        self.words("The length is summed in `Thread::length`, at src/thread.rs:9:")?;
        self.text(
            "\n\n```rust\n    pub fn length(&self) -> u32 {\n        self.knots.iter().sum()\n    }\n```\n\n",
        )?;
        self.words("It's printed at src/main.rs:12, beside how many `knots` the thread has.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// focus A1's answer that names no place: two reads, then two paragraphs naming none — its note at the last read
    /// (its first line code: a note on a folded doc comment isn't drawn yet).
    fn a1_none(&mut self) -> io::Result<()> {
        self.words("Let me read the loop and the thread it prints.")?;
        self.text("\n\n")?;
        for (n, at) in [(1, "src/main.rs:7-14"), (2, "src/thread.rs:7-11")] {
            self.call(n, "read", json!({"files": [at]}))?;
        }
        self.words(
            "Each thread is printed once, with its name, its knots and its length, the sum of its knots.",
        )?;
        self.text("\n\n")?;
        self.words(
            "Nothing else reads the map, so the order it prints in is the only thing that changes from run to run. \
             That is all.",
        )?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// The flow plan's piece 3's answer no place holds: no call, no name of a file — words only.
    fn flow3_none(&mut self) -> io::Result<()> {
        self.words(
            "A hash map finds a thread by its name in one step, where a list would walk every strand to find it.",
        )?;
        self.text("\n\n")?;
        self.words(
            "The price is the order: a hash map keeps none, so what prints comes in no order you chose.",
        )?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// The flow plan's piece 3's failed turn: a few words, then claude's error as its result.
    fn flow3_error(&mut self) -> io::Result<()> {
        self.words("Let me look at how the map is built.")?;
        self.line(json!({"type": "result", "subtype": "error_during_execution", "is_error": true,
            "result": "API Error: 529 Overloaded · the request was not completed"}))
    }

    /// focus A1's flow: focus-explain's answer with a paragraph about another place between its own and the place-less
    /// ones — on the loop's card, that paragraph is one line with its place.
    fn a1_flow(&mut self) -> io::Result<()> {
        self.words("Let me read the loop and the thread it prints.")?;
        self.text("\n\n")?;
        for (n, at) in [(1, "src/main.rs:7-14"), (2, "src/thread.rs:7-11")] {
            self.call(n, "read", json!({"files": [at]}))?;
        }
        self.words(
            "The loop at src/main.rs:11 prints each thread's name, how many knots it has and its length.",
        )?;
        self.text("\n\n")?;
        self.words("The length comes from src/thread.rs:9, which sums the knots themselves rather than the gaps between them.")?;
        self.text("\n\n")?;
        self.words(
            "It prints them in the map's own order, which changes from run to run: a hash map keeps no order of \
             insertion.",
        )?;
        self.text("\n\n")?;
        self.words(
            "One caveat: a repeated name replaces the earlier strand before the loop sees it. That is all.",
        )?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// focus A1b's two answers, by `turns` (the messages so far): the first names the loop's line and the insert above
    /// it, with a place-less paragraph; the second names the loop's line again — its card the two newest first, the
    /// insert's line only the first's (a faint `¶`).
    fn a1b_two(&mut self, turns: usize, all_placed: bool) -> io::Result<()> {
        // (a conversation of its own: kept, so a new one can continue it — `C-p`)
        let id = "conv-a1b";
        self.line(
            json!({"type": "system", "subtype": "init", "session_id": id, "model": "claude-opus-5-5"}),
        )?;
        self.words("Let me read the loop.")?;
        self.text("\n\n")?;
        self.call(turns as u64, "read", json!({"files": ["src/main.rs:7-14"]}))?;
        if turns == 1 && all_placed {
            self.words(
                "The loop at src/main.rs:11 prints each thread's name, how many knots it has and its length.",
            )?;
            self.text("\n\n")?;
            self.words("The length comes from src/thread.rs:9, which sums the knots. That is all.")?;
        } else if turns == 1 {
            self.words(
                "The loop at src/main.rs:11 prints each thread's name, how many knots it has and its length.",
            )?;
            self.text("\n\n")?;
            self.words("The first thread goes into the map at src/main.rs:10, before the loop starts.")?;
            self.text("\n\n")?;
            self.words("It prints them in the map's own order, which changes from run to run. That is all.")?;
        } else {
            self.words(
                "The loop at src/main.rs:11 walks a HashMap, so a repeated name replaces the earlier strand: one \
                 entry a name.",
            )?;
            self.text("\n\n")?;
            self.words("A Vec would keep the order, but let two threads share a name. That is all.")?;
        }
        self.line(json!({"type": "result", "result": "done", "session_id": id}))
    }

    /// focus F0's turn: a read of a range, a search through the real tool, an edit, `report` its tests' if they ran — and
    /// an answer naming a line read, the search's hit, the change and a line none of them showed. Its second answer (by
    /// `turns`, the messages so far) reads another file and names the first's lines again.
    fn f0_saw(&mut self, turns: usize, report: Option<&str>) -> io::Result<()> {
        if turns > 1 {
            self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
            self.words("The loop at src/main.rs:12 only prints it.")?;
            self.text("\n\n")?;
            self.words("So the fix at src/thread.rs:10 is the one change. Only that.")?;
            return self.line(json!({"type": "result", "result": "done"}));
        }
        self.call(1, "read", json!({"files": ["src/thread.rs:1-6"]}))?;
        self.call(2, "search", json!({"pattern": "pub fn length", "path": "src"}))?;
        let edit = json!({"edits": [{"file_path": "src/thread.rs", "old_string": "        self.knots.iter().sum()\n",
            "new_string": "        self.knots.windows(2).map(|w| w[1] - w[0]).sum()\n"}]});
        self.input(3, "edit", &edit.to_string(), None)?;
        self.call(3, "edit", edit)?;
        if let Some(report) = report {
            self.ran("f4", "run_tests", json!({"path": "src/thread.rs"}), report)?;
        }
        self.words("The knots live in src/thread.rs:4, positions along the strand.")?;
        self.text("\n\n")?;
        self.words("The length starts at src/thread.rs:9. It summed the positions: src/thread.rs:10 sums the gaps now.")?;
        self.text("\n\n")?;
        self.words("Nothing past src/thread.rs:15 reads the length, so the callers stay. That is all.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// focus A1c's turn, a real answer's shape: a read, then three paragraphs, a walk-through's stop over `wind` (65
    /// lines, 21–85), a place on a doc comment's line, a list, `**bold**`, and a paragraph ending `:` with its fenced code.
    fn a1c(&mut self) -> io::Result<()> {
        self.words("Let me read the spool.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/spool.rs"]}))?;
        self.words("Here is how a spool winds, and where its order goes.")?;
        self.text("\n\n")?;
        self.text("<show at=\"src/spool.rs:21-85\">`wind` is the whole loop: it pops each strand, ties it, measures it and keeps it.</show> ")?;
        self.words(
            "It runs until the queue is empty or every strand had four rounds, then sorts what it wound.",
        )?;
        self.text("\n\n")?;
        self.words("The doc comment at src/spool.rs:5 says **the spool keeps its order**, but nothing in `wind` keeps it.")?;
        self.text("\n\n")?;
        self.words("Three things change the order:")?;
        self.text("\n\n")?;
        self.words("- `pop_back` at src/spool.rs:32 takes the newest strand first, so the last that came is wound first.")?;
        self.text("\n")?;
        self.words("- **`retie`** sends a slipped strand to the back, behind every strand still waiting.")?;
        self.text("\n")?;
        self.words("- The `sort_by_key` at src/spool.rs:81 sorts by length in the end, which drops whatever order was left.")?;
        self.text("\n\n")?;
        self.words(
            "Of the three, the pop is the one to change first: the retie and the sort only move what the pop has \
             already reversed, and once strands come off the front the slipped ones land last, as the doc comment \
             says they should. The sort can go after that, or stay as a tie-break for strands of one length.",
        )?;
        self.text("\n\n")?;
        self.words("A strand that slips goes back at src/spool.rs:53; to keep its place it goes to the front instead:")?;
        self.text("\n\n```rust\ns.knots.sort_unstable();\nself.queue.push_front(s);\ncontinue;\n```\n\n")?;
        self.words("**In short**: nothing keeps the order the doc comment promises. That is all.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// focus D1's `focus-fix`: the tests fail, two reads, an edit (its review, answered by the scene), the tests pass,
    /// two paragraphs — the runs as the stream says them (no test is built in a scene).
    fn focus_fix(&mut self) -> io::Result<()> {
        self.words("Running the tests first to see what fails.")?;
        self.text("\n\n")?;
        let tests = json!({"path": "src/thread.rs"});
        self.ran("f0", "run_tests", tests.clone(), "1 failed: length_sums_the_gaps — left: 6, right: 2")?;
        self.call(1, "read", json!({"files": ["src/thread.rs:1-12"]}))?;
        self.call(2, "read", json!({"files": ["src/main.rs:7-10"]}))?;
        self.words("The length sums the knots themselves, not the gaps between them, which is what a strand's length is.")?;
        self.text("\n")?;
        let edit = json!({"edits": [{"file_path": "src/thread.rs", "old_string": "        self.knots.iter().sum()\n",
            "new_string": "        self.knots.windows(2).map(|w| w[1] - w[0]).sum()\n"}]});
        self.input(3, "edit", &edit.to_string(), None)?;
        self.call(3, "edit", edit)?;
        self.ran("f4", "run_tests", tests, "2 passed")?;
        self.words("Fixed: src/thread.rs:10 now sums the gaps between the knots, so knots at 1, 2 and 3 make a strand 2 long.")?;
        self.text("\n\n")?;
        self.words("Nothing else calls the length, so no other change is needed. That is all.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// focus E1's turn: a batched read, a search, a whole file, the same file again, an edit (its review, answered by the
    /// scene) and the tests of the file it changed, passing or not — the run as the stream says it.
    fn e1_map(&mut self, failed: bool) -> io::Result<()> {
        self.words("Reading how a strand's length is worked out.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/thread.rs:1-12", "src/main.rs:7-10"]}))?;
        let hits = "2 matching lines in 2 files:\nsrc/knot.rs\n  1: knots\nsrc/thread.rs\n  4: pub knots: Vec<u32>,\n";
        self.ran("m2", "search", json!({"pattern": "knots", "path": "src"}), hits)?;
        self.call(3, "read", json!({"files": ["src/knot.rs"]}))?;
        self.call(4, "read", json!({"files": ["src/main.rs:1-6"]}))?;
        self.words("The length sums the knots themselves, not the gaps between them.")?;
        self.text("\n")?;
        let edit = json!({"edits": [{"file_path": "src/thread.rs", "old_string": "        self.knots.iter().sum()\n",
            "new_string": "        self.knots.windows(2).map(|w| w[1] - w[0]).sum()\n"}]});
        self.input(5, "edit", &edit.to_string(), None)?;
        self.call(5, "edit", edit)?;
        let said = match failed {
            true => "failed in 0.4s\n$ cargo test thread\n1 failed, 1 passed",
            false => "passed in 0.4s\n$ cargo test thread\n2 passed",
        };
        self.ran("m6", "run_tests", json!({"path": "src/thread.rs"}), said)?;
        self.words("Fixed: src/thread.rs:10 now sums the gaps between the knots. That is all.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// focus D1's `focus-asks`: a run asked for outside the sandbox (its card, answered by the scene), then the answer.
    fn focus_asks(&mut self) -> io::Result<()> {
        self.words("The tests need the registry, so I will ask to run them outside the sandbox.")?;
        self.text("\n")?;
        let run = json!({"command": "cargo test --quiet", "outside_sandbox": true,
            "reason": "cargo needs to fetch the registry index"});
        self.call(1, "run", run)?;
        self.words("They pass outside the sandbox; nothing in the crate needed the change. That is all.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// focus D1's `focus-line`, the docs' `notes.png`: `space n`'s question at a line — a read, then the answer in the chat,
    /// naming no place (the lines asked about its flag — #388 step 3).
    fn focus_line(&mut self) -> io::Result<()> {
        self.words("Reading how the map is filled.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:7-12"]}))?;
        self.words(
            "It's a map, not a Vec: threads are keyed by name, so a repeated name replaces the strand before it. A Vec \
             would keep both, in order, and the loop would print each.",
        )?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// The docs' `follow-ask.png`: an answer that walks through the demo's `main.rs` in three stops — a `<show at=…>` line
    /// before each piece, in the order of the flow, as Claude is asked to write one — a sentence each.
    fn tour(&mut self) -> io::Result<()> {
        self.call(1, "read", json!({"files": ["src/main.rs"]}))?;
        for (at, what, about) in [
            (
                "src/main.rs:1-4",
                "the map and the thread type come in",
                "A HashMap keys the threads by name; Thread is the crate's own, from thread.rs.",
            ),
            (
                "src/main.rs:8-10",
                "the first thread goes into the map",
                "A thread of three knots is made and put in under its name, cloned because the map keeps the key.",
            ),
            (
                "src/main.rs:11-13",
                "each thread is printed with its knots and its length",
                "The loop walks the map in its own order and prints each name, how many knots it has and how long it is.",
            ),
        ] {
            self.text(&format!("<show at=\"{at}\">{what}</show>\n"))?;
            self.words(about)?;
            self.text("\n\n")?;
        }
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// #239's scenes: an answer whose paragraph names two places of the demo's `main.rs`, and a list item one of
    /// `thread.rs` — each a note at its line once the turn ends.
    fn chat_notes(&mut self) -> io::Result<()> {
        self.words(
            "The map is filled in `src/main.rs:8-10`, keyed by each thread's name. The loop at src/main.rs:11 \
             prints them in the map's own order, so the output's order changes from run to run.",
        )?;
        self.text("\n\n")?;
        self.words("- `src/thread.rs:9` is where `length` sums the knots.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// #229: an answer with a Rust block whose one long line the chat cuts with `…`.
    fn cut_code(&mut self) -> io::Result<()> {
        self.words("Counted by owner, in one pass:")?;
        self.text(
            "\n\n```rust\nfn count(threads: Vec<Thread>) -> HashMap<String, Vec<Thread>> {\n    let counts: \
             HashMap<String, Vec<Thread>> = threads.into_iter().fold(HashMap::new(), |mut acc, t| { \
             acc.entry(t.owner.clone()).or_default().push(t); acc });\n    counts\n}\n```\n\n",
        )?;
        self.words("That's all.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// A run in the sandbox that outlasts the stuck mark (`STUCK_AFTER`): its call unanswered past it.
    fn run_long(&mut self) -> io::Result<()> {
        self.words("The build takes a while; I will wait for it.")?;
        self.text("\n")?;
        self.call(1, "run", json!({"command": "sleep 120"}))?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// A program that isn't text (a NUL in it), under the demo's ignored `target/`: run outside the sandbox,
    /// rebuilt, run again — the second card is the first's "Same as".
    fn same_binary(&mut self) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let probe = Path::new("target/probe");
        let build = |n: &str| -> io::Result<()> {
            std::fs::create_dir_all("target")?;
            // (a NUL: what tarae takes for a program's bytes — after the lines sh runs)
            let mut bytes = format!("#!/bin/sh\necho probe {n}\nexit 0\n# build ").into_bytes();
            bytes.extend(b"\x00\xff");
            std::fs::write(probe, bytes)?;
            std::fs::set_permissions(probe, std::fs::Permissions::from_mode(0o755))
        };
        build("one")?;
        self.words("The probe needs a terminal of its own, so I will run it outside the sandbox.")?;
        self.text("\n")?;
        let run = json!({"command": "./target/probe", "outside_sandbox": true,
            "reason": "the probe opens a terminal of its own"});
        self.call(1, "run", run.clone())?;
        build("two, with a drain loop")?;
        self.words("Rebuilt with a drain loop — once more.")?;
        self.text("\n")?;
        self.call(2, "run", run)?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// tarae's `search` of a folder in home, outside the project: its read card, a pattern too long for it.
    fn search_outside(&mut self) -> io::Result<()> {
        let home = std::env::var("HOME").unwrap_or_default();
        self.words("The card record is outside the project; I will search it for the terminal grants.")?;
        self.text("\n")?;
        let pattern = r#""kind":"(grant|run|read)".*"(terminals|pty|tty)".*"answer":"(y|a|s|A)".*"session":"frictions3[0-9]""#;
        let search = json!({"pattern": pattern, "path": format!("{home}/Library/Logs")});
        self.call(1, "search", search)?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// tarae's `read` of a file on another disk, `/Volumes/…`: its read card (`volumecard`).
    fn read_volume(&mut self) -> io::Result<()> {
        self.words("The strands' notes are on the backup disk; I will read them there.")?;
        self.text("\n")?;
        self.call(1, "read", json!({"files": ["/Volumes/backup/skein/strands.md"]}))?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// Streams `json` as `input_json_delta` on block `idx` up to `hold`, flushing and running `on_hold` there so a
    /// scene beat sees the edit mid-line before the rest of `json` finishes.
    fn input_pause(
        &mut self,
        idx: u64,
        n: u64,
        name: &str,
        json: &str,
        hold: &str,
        on_hold: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let block = json!({"type": "tool_use", "id": format!("t{n}"), "name": format!("mcp__tarae__{name}"), "input": {}});
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_start", "index": idx, "content_block": block}}))?;
        let mid = json.find(hold).unwrap_or(json.len());
        for piece in json[..mid].chars().collect::<Vec<_>>().chunks(10) {
            let delta = json!({"type": "input_json_delta", "partial_json": piece.iter().collect::<String>()});
            self.line(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": idx, "delta": delta}}))?;
            std::thread::sleep(self.pause / 2);
        }
        let empty = json!({"type": "input_json_delta", "partial_json": ""});
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": idx, "delta": empty}}))?;
        if !self.pause.is_zero() {
            on_hold()?;
        }
        for piece in json[mid..].chars().collect::<Vec<_>>().chunks(12) {
            let delta = json!({"type": "input_json_delta", "partial_json": piece.iter().collect::<String>()});
            self.line(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": idx, "delta": delta}}))?;
            std::thread::sleep(self.pause / 2);
        }
        self.line(json!({"type": "stream_event", "event": {"type": "content_block_stop", "index": idx}}))
    }

    /// Homepage hero (`main` checkout): Beat 1 reads and explains; Beat 2 streams the `entry`/`or_insert` edit on
    /// line 10 while `fix` writes line 12; Beat 3 writes line 10 to disk (triggering `main ≈ fix`); Beat 6 opens
    /// the inline review (`Change 1 of 1`) once `:weave` closes and `:review-at-end off` reloads the config.
    fn home_main(&mut self) -> io::Result<()> {
        self.words("Let me look at how the map is filled first.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:7-14"]}))?;
        self.words(
            "The name is cloned because **insert** takes the key by value, and `first` moves into the map",
        )?;
        self.words("in the same call. Switching to `entry` keeps the first strand when a name repeats.")?;
        self.text("\n")?;
        let item = |old: &str, new: &str| {
            let s = |t: &str| Value::from(t).to_string();
            format!(r#"{{"file_path": "src/main.rs", "old_string": {}, "new_string": {}}}"#, s(old), s(new))
        };
        let edit1 = format!(
            r#"{{"edits": [{}]}}"#,
            item(
                "    threads.insert(first.name.clone(), first);\n",
                "    threads.entry(first.name.clone()).or_insert(first);\n",
            )
        );
        self.input_pause(1, 2, "edit", &edit1, "_insert(first);", || {
            std::thread::sleep(Duration::from_millis(2600));
            Ok(())
        })?;
        self.call(2, "edit", serde_json::from_str(&edit1).map_err(io::Error::other)?)?;
        if !self.pause.is_zero() {
            let cfg = std::env::var_os("XDG_CONFIG_HOME")
                .map(|p| PathBuf::from(p).join("tarae/config.toml"))
                .unwrap_or_else(|| PathBuf::from("../../xdg/tarae/config.toml"));
            wait_file_contains(&cfg, "review-at-end = false", Duration::from_secs(35))?;
        }
        self.words("I will also trim the redundant length from the summary line.")?;
        self.text("\n")?;
        let edit2 = format!(
            r#"{{"edits": [{}]}}"#,
            item(
                "        println!(\"{name}: {} knots, {} long\", t.knots.len(), t.length());\n",
                "        println!(\"{name}: {} knots\", t.knots.len());\n",
            )
        );
        self.call(3, "edit", serde_json::from_str(&edit2).map_err(io::Error::other)?)?;
        self.words("Done — **main.rs** keeps the first strand when a name repeats.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// Homepage hero (`fix` checkout): holds its `input_json_delta` on `src/main.rs` line 12 (`‹fix ●›` in
    /// blue) while `main` writes line 10, then writes its own change on lines 10 and 12 once `main`'s edit lands,
    /// so `overlap.rs` reports `main ≈ fix · src/main.rs L10 — would conflict`.
    fn home_fix(&mut self) -> io::Result<()> {
        self.words("Simplifying the summary line in `fix`.")?;
        self.text("\n")?;
        let item = |old: &str, new: &str| {
            let s = |t: &str| Value::from(t).to_string();
            format!(r#"{{"file_path": "src/main.rs", "old_string": {}, "new_string": {}}}"#, s(old), s(new))
        };
        let print = item(
            "        println!(\"{name}: {} knots, {} long\", t.knots.len(), t.length());\n",
            "        println!(\"{name}: {} knots\", t.knots.len());\n",
        );
        let conflict = item(
            "    threads.insert(first.name.clone(), first);\n",
            "    let _ = threads.insert(first.name.clone(), first);\n",
        );
        let input = format!(r#"{{"edits": [{print}, {conflict}]}}"#);
        self.input_pause(1, 1, "edit", &input, "nots.len());\\n\"}", || {
            wait_file_contains(Path::new("../tarae-demo/src/main.rs"), "or_insert", Duration::from_secs(25))
        })?;
        self.call(1, "edit", serde_json::from_str(&input).map_err(io::Error::other)?)?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// Homepage hero (`docs` checkout): runs a sandboxed command first, then asks on a sandbox grant card (`In the
    /// sandbox`) to reach `index.crates.io` for `cargo test --quiet`.
    fn home_docs(&mut self) -> io::Result<()> {
        self.words("Checking the doc examples in the sandbox.")?;
        self.text("\n")?;
        self.call(1, "run", json!({"command": "true"}))?;
        let also = json!({"command": "cargo test --quiet", "also": {"network": "index.crates.io"},
            "reason": "cargo needs the registry index"});
        self.call(2, "run", also)?;
        self.words("All doc tests pass in the sandbox.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }
}

impl<W: Write, L: Iterator<Item = io::Result<String>>> Turn<'_, W, L> {
    /// V1's main: a read, then its edit of `wind`'s `m.insert` (line 31) written at the stream's pace — no hold: its Short
    /// reads only the woven conflict —, landing unseen (review at the end).
    fn v1_main(&mut self) -> io::Result<()> {
        self.words("Keying by the name alone lets a repeat replace the first strand.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:28-32"]}))?;
        self.words("Switching to `entry` keeps the first one.")?;
        self.text("\n")?;
        let input = format!(
            r#"{{"edits": [{}]}}"#,
            v1_item("    m.insert(k, t);\n", "    m.entry(k).or_insert(t);\n")
        );
        self.input_pause(1, 2, "edit", &input, ".or_insert", || Ok(()))?;
        self.call(2, "edit", serde_json::from_str(&input).map_err(io::Error::other)?)?;
        self.v1_talk(&[
            "Now a repeat keeps the first strand.",
            "`main` winds the map as before,",
            "so nothing else in **main.rs** changes.",
            "`entry` hands back the strand that stays,",
            "which a later knot could be tied on.",
            "Nothing else reads the map.",
        ])
    }

    /// The rest of a V1 turn: `said` a sentence at a time, slowly, then the turn's end — its turn at work through the take,
    /// so no turn's end (its review, its answer's card, its toasts) comes on camera.
    fn v1_talk(&mut self, said: &[&str]) -> io::Result<()> {
        for s in said {
            if !self.pause.is_zero() {
                std::thread::sleep(Duration::from_millis(9000));
            }
            self.words(s)?;
        }
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// V1's fix: its edit of `wind`'s key (line 30) and of its `m.insert` its own way, held mid-line on the key until
    /// main's edit is on main's disk (`../tarae-demo`), then written and landing: the same line changed two ways. Its
    /// lines short (`.into()`, not `.to_string()`), so its part of the woven conflict reads whole in the Short's read,
    /// its gutter's `f` kept.
    fn v1_fix(&mut self) -> io::Result<()> {
        self.words("Trimming a name before it keys the map.")?;
        self.text("\n")?;
        let key = v1_item("    let k = t.name.clone();\n", V1_FIX_KEY);
        let insert = v1_item("    m.insert(k, t);\n", "    let _ = m.insert(k, t);\n");
        let input = format!(r#"{{"edits": [{key}, {insert}]}}"#);
        self.input_pause(1, 1, "edit", &input, ".into()", || {
            wait_file_contains(Path::new("../tarae-demo/src/main.rs"), "or_insert", Duration::from_secs(90))
        })?;
        self.call(1, "edit", serde_json::from_str(&input).map_err(io::Error::other)?)?;
        self.v1_talk(&[
            "A name with a space at its end keys the same strand now.",
            "The insert's result is dropped on purpose:",
            "a repeat replaces the strand before it.",
            "Nothing else keys the map.",
            "The tests don't wind a name with spaces.",
            "That's the whole change.",
        ])
    }

    /// V1's docs: a read of `thread.rs`, then an answer said a sentence at a time, slowly — at work all through the take.
    fn v1_docs(&mut self) -> io::Result<()> {
        self.words("Let me read how a thread is measured.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/thread.rs:1-13"]}))?;
        self.v1_talk(&[
            "A thread is a name and the knots tied along it.",
            "Its length is the sum of the gaps between its knots,",
            "so a knot tied at the end lengthens it by that gap.",
            "Nothing keeps the knots in order:",
            "a knot tied before the last one still adds its whole gap.",
            "The doc comment on `length` could say so.",
            "The tests measure a thread of three knots,",
            "and one with a knot tied after two.",
        ])
    }
}

/// V6's contexts, each session's two requests' (of a 200k window): main's, fix's, docs' — docs' second past the
/// list's 80 % (its `ctx` in warning).
const V6_CONTEXT: [(&str, u64, u64); 3] =
    [("main", 31_000, 68_000), ("fix", 96_000, 124_000), ("docs", 151_000, 174_000)];
/// The window V6's first results name (`modelUsage`'s `contextWindow`).
const V6_WINDOW: u64 = 200_000;
/// Where fix's writing waits for the moment you follow it: made beside the checkouts by the scene's `@touch:`.
const V6_FOLLOWED: &str = "../.v6-follow";
/// Docs' first words after its run, and when (a turn's pace after the run's answer): the Short's take ends before them.
const V6_DOCS_SAYS: &str = "It builds; `tie` is never used, in either file.";
const V6_DOCS_SAYS_MS: u64 = 2000;

impl<W: Write, L: Iterator<Item = io::Result<String>>> Turn<'_, W, L> {
    /// A V6 turn of session `who` (`main`, `fix`, `docs`): its request as a conversation's (`context` tokens carried),
    /// then the first a short answer and its result naming the window — the list says a context only once a result has —,
    /// the second its work.
    fn v6(&mut self, who: &str, turns: usize) -> io::Result<()> {
        let (_, first, second) = V6_CONTEXT.into_iter().find(|c| c.0 == who).unwrap_or(V6_CONTEXT[0]);
        let id = format!("conv-v6-{who}");
        let usage = |context: u64| {
            json!({"input_tokens": 4, "cache_read_input_tokens": context - 1_204, "cache_creation_input_tokens": 1_200,
                "cache_creation": {"ephemeral_1h_input_tokens": 1_200}, "output_tokens": 240})
        };
        let context = if turns == 1 { first } else { second };
        self.line(
            json!({"type": "system", "subtype": "init", "session_id": id, "model": "claude-opus-5-5"}),
        )?;
        let message =
            json!({"id": format!("{id}-{turns}"), "model": "claude-opus-5-5", "usage": usage(context)});
        self.line(json!({"type": "stream_event", "event": {"type": "message_start", "message": message}}))?;
        if turns > 1 {
            return match who {
                "main" => self.v6_main(),
                "fix" => self.v6_fix(),
                _ => self.v6_docs(),
            };
        }
        self.words(match who {
            // (each ends on a word the scene waits for: in no file of the demo's)
            "main" => "A repeated name replaces the strand before it: insert keeps the newest.",
            "fix" => "By its name, cloned as it is: tabs and spaces.",
            _ => "By the gaps between its knots, summed.",
        })?;
        let model = json!({"claude-opus-5-5": {"inputTokens": 4, "cacheReadInputTokens": context - 1_204,
            "cacheCreationInputTokens": 1_200, "outputTokens": 240, "contextWindow": V6_WINDOW}});
        self.line(json!({"type": "result", "result": "ok", "session_id": id, "total_cost_usd": 0.04,
            "usage": usage(context), "modelUsage": model}))
    }

    /// V6's main: a read, then its edit of `wind`'s `m.insert` (line 31) landing unseen — fix's own way of that line is
    /// there already, so C2 says they'd conflict —, then talk all through the take.
    fn v6_main(&mut self) -> io::Result<()> {
        self.words("Switching to `entry` keeps the first strand.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:28-32"]}))?;
        let input = format!(
            r#"{{"edits": [{}]}}"#,
            v1_item("    m.insert(k, t);\n", "    m.entry(k).or_insert(t);\n")
        );
        self.call(2, "edit", serde_json::from_str(&input).map_err(io::Error::other)?)?;
        self.v1_talk(&[
            "Now a repeat keeps the first strand.",
            "`main` winds the map as before,",
            "so nothing else in **main.rs** changes.",
            "`entry` hands back the strand that stays,",
            "which a later knot could be tied on.",
            "Nothing else reads the map.",
            "The tests wind two names, each once.",
            "A test with a repeat would show it.",
        ])
    }

    /// V6's fix: its edit of `m.insert` (line 31) landing at once, then `wind`'s key (line 30) — what it replaces
    /// streamed, then held until you follow it (`V6_FOLLOWED`), then its new line written slowly enough to read —, then
    /// talk.
    fn v6_fix(&mut self) -> io::Result<()> {
        self.words("The insert's result is dropped on purpose.")?;
        self.text("\n")?;
        let insert = format!(
            r#"{{"edits": [{}]}}"#,
            v1_item("    m.insert(k, t);\n", "    let _ = m.insert(k, t);\n")
        );
        self.call(1, "edit", serde_json::from_str(&insert).map_err(io::Error::other)?)?;
        self.words("Now the key: a name trimmed before it keys the map.")?;
        self.text("\n")?;
        let key = format!(
            r#"{{"edits": [{}]}}"#,
            v1_item("    let k = t.name.clone();\n", "    let k = t.name.trim().to_string();\n")
        );
        self.v6_written(2, &key, "\"new_string\"")?;
        self.call(2, "edit", serde_json::from_str(&key).map_err(io::Error::other)?)?;
        self.v1_talk(&[
            "A name with a space at its end keys the same strand now.",
            "Nothing else keys the map.",
            "The tests don't wind a name with spaces.",
            "That's the whole change.",
        ])
    }

    /// The turn's `n`th call, an `edit` whose `input` streams up to `hold` at once, waits there until you follow it
    /// (`V6_FOLLOWED`, 120 s at most), then streams the rest a character every 80 ms — a line written as a video reads it.
    fn v6_written(&mut self, n: u64, input: &str, hold: &str) -> io::Result<()> {
        let block =
            json!({"type": "tool_use", "id": format!("t{n}"), "name": "mcp__tarae__edit", "input": {}});
        let ev = |e: Value| json!({"type": "stream_event", "event": e});
        self.line(ev(json!({"type": "content_block_start", "index": 1, "content_block": block})))?;
        let delta = |s: &str| {
            ev(json!({"type": "content_block_delta", "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": s}}))
        };
        let mid = input.find(hold).unwrap_or(input.len());
        self.line(delta(&input[..mid]))?;
        if !self.pause.is_zero() {
            wait_file(Path::new(V6_FOLLOWED), Duration::from_secs(120))?;
        }
        for c in input[mid..].chars() {
            self.line(delta(&c.to_string()))?;
            if !self.pause.is_zero() {
                std::thread::sleep(Duration::from_millis(80));
            }
        }
        self.line(ev(json!({"type": "content_block_stop", "index": 1})))
    }

    /// V6's docs: a read of `thread.rs`, then a run of its tests asked outside the sandbox — its card waits for you —,
    /// then its first words on the run at a turn's pace (2 s: the Short ends before them), then talk all through the take.
    fn v6_docs(&mut self) -> io::Result<()> {
        self.words("Let me read what the tests measure.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/thread.rs:1-13"]}))?;
        self.words("The tests need the registry, so I will ask to run them outside the sandbox.")?;
        self.text("\n")?;
        let run = json!({"command": "cargo test --quiet", "outside_sandbox": true,
            "reason": "cargo needs to fetch the registry index"});
        self.call(2, "run", run)?;
        if !self.pause.is_zero() {
            std::thread::sleep(Duration::from_millis(V6_DOCS_SAYS_MS));
        }
        self.words(V6_DOCS_SAYS)?;
        self.v1_talk(&[
            "No test measures a thread yet.",
            "The doc comment on `length` could say what one would check.",
            "Nothing else needs a change.",
        ])
    }
}

/// Waits until `path` is there, `timeout` at most.
fn wait_file(path: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while !path.exists() {
        if std::time::Instant::now() >= deadline {
            let said = format!("timed out waiting for {}", path.display());
            return Err(io::Error::new(io::ErrorKind::TimedOut, said));
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    Ok(())
}

/// V1's fix: `wind`'s key, trimmed.
const V1_FIX_KEY: &str = "    let k = t.name.trim().into();\n";

/// One item of an `edit` call on `src/main.rs`, its keys in the order the model writes them: what it replaces, then
/// what it writes.
fn v1_item(old: &str, new: &str) -> String {
    let s = |t: &str| Value::from(t).to_string();
    format!(r#"{{"file_path": "src/main.rs", "old_string": {}, "new_string": {}}}"#, s(old), s(new))
}

fn wait_file_contains(path: &Path, needle: &str, timeout: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if std::fs::read_to_string(path).is_ok_and(|s| s.contains(needle)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("timed out waiting for {needle:?} in {}", path.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #342: every scene's `:sh` is waited on by its end, never a fixed time (`sh_waits_by_time`).
    #[test]
    fn every_scenes_sh_is_waited_on_by_its_end() {
        for s in SCENES {
            let late = sh_waits_by_time(s.keys);
            assert!(late.is_empty(), "{}: {late:?}", s.name);
        }
        assert_eq!(sh_waits_by_time(&[":", "sh true", "\r", "@3"]).len(), 1, "a fixed wait is found");
        assert!(sh_waits_by_time(&[":", "sh echo ok", "\r", "@20?ok"]).is_empty());
    }

    /// Each wait is timed from the last key sent at or before its end — a key after it never counts, and a wait with no
    /// key before it is left out.
    #[test]
    fn a_wait_is_timed_from_the_key_before_it() {
        let keys = |k: &[(u64, &str)]| k.iter().map(|(t, s)| (*t, s.to_string())).collect::<Vec<_>>();
        let sent = keys(&[(100, "c"), (1_000, "\r"), (21_000, "y")]);
        let waits = keys(&[(50, "early"), (20_000, "main moved"), (21_000, "moved")]);
        assert_eq!(
            waited(&sent, &waits),
            vec![("main moved".to_string(), 20_000, 19_000), ("moved".to_string(), 21_000, 0)]
        );
    }

    /// What the stand-in wrote, a JSON value a line.
    fn said(script: &str, input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        claude(script, input.as_bytes(), &mut out, Duration::ZERO).unwrap();
        String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    const ASKED: &str = "{\"request\":{\"subtype\":\"initialize\"},\"type\":\"control_request\"}\n\
                         {\"message\":{\"content\":\"why?\",\"role\":\"user\"},\"type\":\"user\"}\n\
                         {\"response\":{\"request_id\":\"c1\"},\"type\":\"control_response\"}\n\
                         {\"response\":{\"request_id\":\"c2\"},\"type\":\"control_response\"}\n";

    /// `full`: text streamed, a `read` and an `edit` of tarae's tools — each waited on — and the turn's end; the
    /// edit's old text is the demo's own line, so tarae's review finds it.
    #[test]
    fn the_stand_in_streams_a_turn_and_its_calls() {
        let lines = said("full", ASKED);
        let calls: Vec<&str> =
            lines.iter().filter_map(|l| l["request"]["message"]["params"]["name"].as_str()).collect();
        assert_eq!(calls, ["read", "edit"]);
        let rows: Vec<&str> =
            lines.iter().filter_map(|l| l["message"]["content"][0]["name"].as_str()).collect();
        assert_eq!(rows, ["mcp__tarae__read", "mcp__tarae__edit"], "each call's row in the chat");
        assert_eq!(lines.last().unwrap()["type"], "result");
        let text: String = lines.iter().filter_map(|l| l["event"]["delta"]["text"].as_str()).collect();
        assert!(text.contains("or_insert(first);") && text.contains("Nothing else reads the map"), "{text}");
        let edit =
            lines.iter().find_map(|l| l["request"]["message"]["params"]["arguments"]["edits"][0].as_object());
        let old = edit.unwrap()["old_string"].as_str().unwrap();
        assert!(MAIN_RS.contains(old), "{old:?}");
    }

    /// `mid` ends in its code block with no result (the shot is taken then); `idle` says nothing; a turn waits
    /// for a user message — tarae's other lines aren't one.
    #[test]
    fn mid_stops_in_its_code_block_and_idle_says_nothing() {
        let mid = said("mid", ASKED);
        assert!(mid.iter().all(|l| l["type"] != "result"));
        let text: String = mid.iter().filter_map(|l| l["event"]["delta"]["text"].as_str()).collect();
        assert!(text.ends_with("threads.entry(first.name.clone())"), "{text}");
        assert!(said("idle", ASKED).is_empty());
        assert!(said("full", "{\"request\":{},\"type\":\"control_request\"}\n").is_empty());
    }

    /// The config names this xtask as claude with its script, and turns language servers off.
    #[test]
    fn a_scenes_config_runs_the_stand_in_and_no_server() {
        let t: toml::Table = config_toml(Path::new("/x/\"q\"/xtask"), "card").parse().unwrap();
        assert_eq!(t["editor"]["lsp"].as_bool(), Some(false));
        assert_eq!(t["llm"]["command"].as_str(), Some("/x/\"q\"/xtask"));
        let args: Vec<&str> =
            t["llm"]["args"].as_array().unwrap().iter().filter_map(|a| a.as_str()).collect();
        assert_eq!(args, ["scenes", "--claude", "card"]);
    }

    /// Every scene has a name of its own, found by it; an unknown one is said.
    #[test]
    fn scenes_are_named_once_and_found_by_name() {
        let mut names: Vec<&str> = SCENES.iter().map(|s| s.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), SCENES.len());
        let got = chosen(&["card".into(), "start".into()]).unwrap();
        assert_eq!(got.iter().map(|s| s.name).collect::<Vec<_>>(), ["card", "start"]);
        assert_eq!(chosen(&[]).unwrap().len(), SCENES.len());
        assert!(chosen(&["nope".into()]).unwrap_err().contains("no scene `nope`"));
    }

    /// The demo's files as committed and as changed; a fresh folder is a new one each time, gone on drop.
    #[test]
    fn the_demo_is_written_in_a_fresh_folder_removed_after() {
        let (a, b) = (Fresh::new("tarae-scenes-test").unwrap(), Fresh::new("tarae-scenes-test").unwrap());
        assert_ne!(a.0, b.0);
        demo_files(&a.0, false, false).unwrap();
        demo_files(&a.0, true, false).unwrap();
        let thread = std::fs::read_to_string(a.0.join("src/thread.rs")).unwrap();
        assert!(thread.contains("at the end of the strand.") && a.0.join("src/knot.rs").is_file());
        let path = a.0.clone();
        drop(a);
        assert!(!path.exists());
    }

    /// `SCRIPTS` is every script the stand-in plays, and only those: the names its matches' arms take, in
    /// `claude` and in `docshots::answered` (read from their code), and `idle`.
    #[test]
    fn the_scripts_are_every_one_the_stand_in_plays() {
        let arms = |code: &str, head: &str| -> Vec<String> {
            let body = &code[code.find(head).unwrap()..];
            let body = &body[body.find("match script {\n").unwrap() + 15..];
            let indent = body.len() - body.trim_start().len();
            // (to the match's own `}`)
            let end = format!("{}}}", " ".repeat(indent - 4));
            let mut names = Vec::new();
            for line in body.lines().take_while(|l| *l != end) {
                // (an arm's head at the match's indentation, up to its guard or `=>` — or a head going on)
                let lead = line.len() - line.trim_start().len();
                let head = line.trim_start();
                if lead != indent || !(head.starts_with('"') || head.starts_with("| \"")) {
                    continue;
                }
                let head = head.split("=>").next().unwrap().split(" if ").next().unwrap();
                names.extend(head.split('"').skip(1).step_by(2).map(str::to_string));
            }
            names
        };
        let mut played = arms(include_str!("scenes.rs"), "\nfn claude(");
        played.extend(arms(include_str!("scenes/docshots.rs"), "\npub(super) fn answered("));
        played.push("idle".into());
        played.sort();
        played.dedup();
        let mut listed: Vec<String> = SCRIPTS.iter().map(|s| s.to_string()).collect();
        listed.sort();
        assert_eq!(listed, played);
        assert!(knows("full") && knows("main=home-main, fix=card"));
        assert!(!knows("fulll") && !knows("main=home-main,fix=nope"));
    }

    /// V1's three Claudes (`three-claudes`): each checkout its own script, edits landing unseen (so fix's, waiting on
    /// main's, lands too); main's and fix's edit inputs streamed as the model writes them — what an item replaces
    /// before what it writes, whole once streamed —, each item found in the file the scene runs on, each line it
    /// writes short enough to read whole in the Short's read, the same line changed two ways; every turn ends.
    #[test]
    fn v1s_three_claudes_each_play_their_own_turn_on_one_line() {
        let spec = SCENES.iter().find(|s| s.name == "three-claudes").unwrap().claude;
        assert_eq!(script_for(spec, Some(Path::new("/h/tarae-demo"))), "v1-main");
        assert_eq!(script_for(spec, Some(Path::new("/h/fix"))), "v1-fix");
        assert_eq!(script_for(spec, Some(Path::new("/h/docs"))), "v1-docs");
        let t: toml::Table = config_toml(Path::new("/x"), spec).parse().unwrap();
        assert_eq!(t["llm"]["review-at-end"].as_bool(), Some(true));
        let file = &run_files("three-claudes")[0];
        assert_eq!((file.0, file.1.as_str()), ("src/main.rs", MAIN_MOVED_RS));
        let played = |who: &str| -> Vec<Value> {
            let (mut out, mut lines) = (Vec::new(), std::iter::empty::<io::Result<String>>());
            let mut turn = Turn { out: &mut out, lines: &mut lines, pause: Duration::ZERO };
            match who {
                "main" => turn.v1_main(),
                "fix" => turn.v1_fix(),
                _ => turn.v1_docs(),
            }
            .unwrap();
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
        };
        let mut replaced = Vec::new();
        for who in ["main", "fix", "docs"] {
            let said = played(who);
            assert_eq!(said.last().unwrap()["type"], "result", "{who}'s turn ends");
            let input: String = (said.iter())
                .filter_map(|v| v.pointer("/event/delta/partial_json").and_then(Value::as_str))
                .collect();
            if who == "docs" {
                assert!(input.is_empty(), "docs edits nothing");
                continue;
            }
            let edits: Value = serde_json::from_str(&input).unwrap();
            for item in edits["edits"].as_array().unwrap() {
                let old = item["old_string"].as_str().unwrap();
                assert!(MAIN_MOVED_RS.contains(old), "{who}'s {old:?} is in the file");
                // (each part of the woven conflict whole in the Short's read: 38 columns, less the woven gutter's 4 and a
                // cell of air)
                let new = item["new_string"].as_str().unwrap();
                assert!(new.lines().all(|l| l.chars().count() <= 33), "{who}'s {new:?} reads whole");
                replaced.push(old.to_string());
            }
            assert!(input.find("old_string") < input.find("new_string"), "{who}: {input}");
        }
        assert_eq!(replaced.iter().filter(|o| *o == "    m.insert(k, t);\n").count(), 2, "{replaced:?}");
    }

    /// Each Short waits for a look only at its emphasis (CLAUDE.md's Shorts bullet: unless something is stressed, it runs
    /// on): on camera (past `@rec`) every timed wait is a hand's (0.6 s at most) but those in the beat of its one read,
    /// after its `@zoom` — V1's the ease in and its look (the hand's move in the ease out), V2's and V3's the ease in,
    /// its look and the ease out (`camera::DRIFT_MS`, `READ_MS`: its next keys on the whole), split for their phrases;
    /// V4's the ease in alone (Claude writing all through it; its look a hand's three answers, its ease out the turn's
    /// own pace up to the cut); V5's the ease in, its look and the ease out, the take ending with it; V6's the same, the
    /// take opening with it.
    #[test]
    fn each_short_waits_for_a_look_only_at_its_emphasis() {
        let looks = |keys: &[&str]| {
            let (mut read, mut looks) = (false, Vec::new());
            for k in keys.iter().skip_while(|k| **k != "@rec") {
                read = (read && !k.starts_with("@beat:")) || k.starts_with("@zoom");
                match k.strip_prefix('@').and_then(|s| s.parse::<f32>().ok()) {
                    Some(s) if s > 0.6 => looks.push((read, s)),
                    _ => {}
                }
            }
            looks
        };
        let (drift, read) = (crate::camera::DRIFT_MS, crate::camera::READ_MS);
        assert_eq!(looks(THREE_CLAUDES), [(true, 3.3)]);
        assert_eq!(
            ((3.3f64 * 1000.0).round() as u64).checked_sub(drift + read),
            Some(100),
            "V1's: in and held, a beat after"
        );
        assert_eq!(looks(WAITS_60S), [(true, 2.0), (true, 2.4)]);
        assert_eq!(looks(MAIN_MOVED), [(true, 1.5), (true, 1.5), (true, 1.4)]);
        assert_eq!(looks(EDIT_IN_BUFFER), [(true, drift as f32 / 1000.0)]);
        assert_eq!(looks(FOLLOW_REPLAY), [(true, 1.2), (true, 2.0), (true, 1.2)]);
        assert_eq!(looks(MANAGERS_VIEW), [(true, 1.3), (true, 1.2), (true, 1.2), (true, 0.7)]);
        let emphasis = 2 * drift + read;
        let all = [("V2", WAITS_60S), ("V3", MAIN_MOVED), ("V5", FOLLOW_REPLAY), ("V6", MANAGERS_VIEW)];
        for (v, keys) in all {
            let look: f32 = looks(keys).iter().map(|l| l.1).sum();
            assert_eq!((look * 1000.0).round() as u64, emphasis, "{v}'s look: in, held, out");
        }
    }

    /// Each Short that runs on to its end (no `@cut?`) has its last beat landed by its last step: a key after its last
    /// caption's mark (the one bringing its target) and the take ends on the wait for that target; only waits after it
    /// (V5's look held), its target there as it was marked. The take runs on `shot::TAIL_S` from the last step —
    /// `captions::TAIL_MIN_MS` before the end card (a cut one's tail is its take's: `video::make` says it short).
    #[test]
    fn each_short_that_runs_on_ends_on_its_last_beat_landing() {
        let all = [THREE_CLAUDES, WAITS_60S, MAIN_MOVED, EDIT_IN_BUFFER, FOLLOW_REPLAY, MANAGERS_VIEW];
        let mut brought = 0;
        for keys in all.into_iter().filter(|keys| !keys.iter().any(|k| k.starts_with("@cut?"))) {
            let mark = keys.iter().rposition(|k| k.starts_with("@note:")).unwrap();
            if keys[mark + 1..].iter().all(|k| k.starts_with('@')) {
                continue;
            }
            brought += 1;
            let note = &keys[mark]["@note:".len()..];
            let target = crate::captions::parse(note).unwrap().1;
            let waited = (keys.last().and_then(|k| k.strip_prefix('@')))
                .and_then(|k| k.split_once('?'))
                .filter(|(s, _)| s.parse::<f64>().is_ok());
            assert_eq!(waited.map(|w| w.1), Some(target.strip_prefix("card:").unwrap_or(target)), "{note}");
        }
        assert_eq!(brought, 2, "V3's and V4's");
    }

    /// V3 (`main-moved`): main's edit is made off camera (V1's story), its commit typed on camera before its card comes;
    /// the one read is that card at its two workspaces' rows (the card wider than the 46 columns that read at 2.5 mm
    /// at 80×34, its rows not), the Short opening on it; after `y`, fix's beat on the whole: C2's `fix ≈ docs` waited
    /// for, then `G` `b` its bring card, its phrase on it, the take ending there.
    #[test]
    fn v3_commits_on_camera_reads_its_card_and_ends_on_fixs_way_back() {
        let at = |keys: &[&str], k: &str| keys.iter().position(|x| *x == k).unwrap();
        let rec = at(MAIN_MOVED, "@rec");
        let (off, on) = MAIN_MOVED.split_at(rec);
        assert!(off.contains(&" // main") && !on.contains(&"A"), "main's edit off camera");
        assert!(at(on, "@type:-qm main src/main.rs") < at(on, "@20?main moved"));
        let zooms: Vec<&str> = on.iter().copied().filter(|k| k.starts_with("@zoom")).collect();
        assert_eq!(zooms, ["@zoom:rows:fix — … docs —"]);
        // (the rows as keepclose's card says them; the longest is the window's width)
        let rows = [
            "fix — 1 behind, would conflict in src/main.rs",
            "docs — 1 behind, its changed file goes with it",
        ];
        let widest = rows.iter().map(|r| r.chars().count()).max().unwrap();
        assert!(widest <= 46, "{widest} columns");
        assert!(at(on, zooms[0]) < at(on, "@punch") && at(on, "@punch") < at(on, "\x11"));
        // (fix's beat: after docs moved, its conflict said, then its bring card — the take's last)
        let beat =
            ["@20?moved onto main", "@20?fix ≈ docs", "G", "b", "@20?Bring fix into main"].map(|k| at(on, k));
        assert!(at(on, "y") < beat[0] && beat.windows(2).all(|w| w[0] < w[1]), "{beat:?}");
        assert_eq!(on.last(), Some(&"@20?Bring fix into main"));
        assert!(!on.iter().any(|k| k.starts_with("@cut?")), "nothing comes after the card by itself");
        let note = on.iter().position(|k| k.ends_with("→ card:Bring fix into main")).expect("its phrase");
        assert!(beat[2] < note && note < beat[3], "its phrase from the card's first frame");
    }

    /// V4 (`edit-in-buffer`): `space l` one key (no which-key over the whole); the turn reads only the lines in front of
    /// you (the view never leaves your buffer) and edits them — each item found in the scene's file —, then ends; the one
    /// read is the review, from its arrival through its three answers, the Short opening on two answers in; once the
    /// turn is over, `space u` (one key) on the whole, its phrase on its toast, the take ending there.
    #[test]
    fn v4_writes_in_your_buffer_reads_its_lines_through_the_review_and_takes_the_turn_back() {
        let at = |keys: &[&str], k: &str| keys.iter().position(|x| *x == k).unwrap();
        let on: Vec<&str> = EDIT_IN_BUFFER.iter().copied().skip_while(|k| *k != "@rec").collect();
        assert!(on.contains(&" l") && on.contains(&" u") && !on.contains(&" "), "space l, space u one key");
        let zooms: Vec<&str> = on.iter().copied().filter(|k| k.starts_with("@zoom")).collect();
        assert_eq!(zooms, ["@zoom:code:7-13"], "from the row above the first head");
        let (zoom, review) = (at(&on, "@zoom:code:7-13"), at(&on, "@20?Change 1 of 3"));
        let answers: Vec<usize> = (0..on.len()).filter(|&i| ["y", "n"].contains(&on[i])).collect();
        assert_eq!(answers.iter().map(|&i| on[i]).collect::<Vec<_>>(), ["y", "n", "y"]);
        // (the writing on the whole; the read the review's, its ease in waited out before the first answer)
        assert!(review < zoom && zoom < answers[0], "the review read, its answers in the hold");
        assert_eq!(on[answers[0] - 2], "@1.2", "the ease in waited out");
        assert!(review < answers[0] && answers[1] < at(&on, "@punch") && at(&on, "@punch") < answers[2]);
        // (the undo once the turn is over — `space u` refuses mid-turn —, on the whole: the one read was the edit's)
        let (done, undo) = (at(&on, "@20!● follow"), at(&on, " u"));
        assert!(answers[2] < done && done < undo, "{done} {undo}");
        let note = at(&on, "@note:yours to take back → undid");
        assert!(done < note && note < undo, "its phrase from the toast's first frame");
        assert_eq!(on.last(), Some(&"@10?undid"));
        assert!(!on.iter().any(|k| k.starts_with("@cut?")), "nothing comes after the undo by itself");
        let (mut out, mut lines) = (Vec::new(), std::iter::empty::<io::Result<String>>());
        let mut turn = Turn { out: &mut out, lines: &mut lines, pause: Duration::ZERO };
        turn.edit_in_buffer().unwrap();
        let said: Vec<Value> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(said.last().unwrap()["type"], "result", "the turn ends");
        let calls: Vec<&Value> = said.iter().filter_map(|v| v.pointer("/request/message/params")).collect();
        for c in &calls {
            let files = c["arguments"]["files"].as_array().into_iter().flatten();
            let edited = c["arguments"]["edits"].as_array().into_iter().flatten().map(|e| &e["file_path"]);
            for f in files.chain(edited) {
                assert!(
                    f.as_str().unwrap().starts_with("src/main.rs"),
                    "{f}: only the buffer in front of you"
                );
            }
        }
        let edit = calls.iter().find(|c| c["name"] == "edit").unwrap();
        for item in edit["arguments"]["edits"].as_array().unwrap() {
            assert!(MAIN_V3_RS.contains(item["old_string"].as_str().unwrap()), "{item}");
        }
    }

    /// V5 (`follow-replay`): `space l`, `[r` each one key (no which-key over the whole); `j` and `space F` come at the
    /// turn's own pace (its words after the second read, then its next), the replay once the turn's review has opened;
    /// the one read is the first change's stop, reached by `[r` twice (the last change, then it), the Short opening on
    /// it. The turn reads and edits only the scene's file, each edit's text found there, its words between the second
    /// and third reads, and ends; the read's lines hold the first change.
    #[test]
    fn v5_takes_the_view_back_at_the_turns_pace_and_replays_to_its_first_change() {
        let on: Vec<&str> = FOLLOW_REPLAY.iter().copied().skip_while(|k| *k != "@rec").collect();
        let at = |k: &str| on.iter().position(|x| *x == k).unwrap();
        assert!(on.contains(&" l") && on.contains(&" F") && !on.contains(&" "), "space chords one key");
        assert!(!on.contains(&"[") && !on.contains(&"]"), "[r one key");
        assert_eq!(on[at("j") - 1], "@10?It pops", "j as Claude's words come");
        assert_eq!(on[at(" F") - 1], "@10?And at the end", "space F at its next");
        let replay: Vec<usize> = (0..on.len()).filter(|&i| on[i] == "[r").collect();
        assert_eq!(replay.len(), 2, "the last change, then the first");
        assert!(at("@20?Claude changed") < replay[0], "the replay once the review has opened");
        let zooms: Vec<&str> = on.iter().copied().filter(|k| k.starts_with("@zoom")).collect();
        assert_eq!(zooms, ["@zoom:code:44-48"]);
        assert!(replay[1] < at("@zoom:code:44-48") && at("@zoom:code:44-48") < at("@punch"));
        assert!(SPOOL_V4_RS.lines().nth(45).unwrap().contains("q.pop_back()"), "line 46 the first change's");
        let (mut out, mut lines) = (Vec::new(), std::iter::empty::<io::Result<String>>());
        let mut turn = Turn { out: &mut out, lines: &mut lines, pause: Duration::ZERO };
        turn.follow_replay().unwrap();
        let said: Vec<Value> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(said.last().unwrap()["type"], "result", "the turn ends");
        let calls: Vec<&Value> = said.iter().filter_map(|v| v.pointer("/request/message/params")).collect();
        let reads: Vec<&str> = (calls.iter().filter(|c| c["name"] == "read"))
            .map(|c| c["arguments"]["files"][0].as_str().unwrap())
            .collect();
        assert_eq!(reads, ["src/spool.rs:3-7", "src/spool.rs:42-56", "src/spool.rs:75-83"]);
        let edits: Vec<&Value> = calls.iter().filter(|c| c["name"] == "edit").copied().collect();
        assert_eq!(edits.len(), 2);
        for item in edits.iter().flat_map(|e| e["arguments"]["edits"].as_array().unwrap()) {
            assert_eq!(item["file_path"], "src/spool.rs");
            assert!(SPOOL_V4_RS.contains(item["old_string"].as_str().unwrap()), "{item}");
        }
        // (in the order said: the second read's request, its words, the third's)
        let place = |what: &dyn Fn(&Value) -> bool| said.iter().position(what).unwrap();
        let text = |t: &'static str| {
            move |v: &Value| {
                v.pointer("/event/delta/text").and_then(Value::as_str).is_some_and(|s| s.contains(t))
            }
        };
        let read = |r: &'static str| {
            move |v: &Value| v.pointer("/request/message/params/arguments/files/0").is_some_and(|f| f == r)
        };
        let (second, pops) = (place(&read("src/spool.rs:42-56")), place(&text("pops")));
        let (end, third) = (place(&text("end:")), place(&read("src/spool.rs:75-83")));
        assert!(second < pops && pops < end && end < third, "{second} {pops} {end} {third}");
    }

    /// V6 (`managers-view`): the one read is the list as the take opens, the Short opening on it held; then on the whole
    /// `j` `f` follow fix, `C-q` once fix's line is written, `y`, the list again (`space W` one key: no which-key over the
    /// whole); the take ends before docs' first words after its run, which come at a turn's pace once the run answers.
    #[test]
    fn v6_reads_its_list_once_answers_docs_from_fix_and_ends_before_docs_says_more() {
        let on: Vec<&str> = MANAGERS_VIEW.iter().copied().skip_while(|k| *k != "@rec").collect();
        let at = |k: &str| on.iter().position(|x| *x == k).unwrap();
        assert!(on.contains(&" W") && !on.contains(&" "), "space W one key");
        let zooms: Vec<&str> = on.iter().copied().filter(|k| k.starts_with("@zoom")).collect();
        assert_eq!(zooms, ["@zoom:card:sessions"]);
        assert!(at("@zoom:card:sessions") < at("@punch") && at("@punch") < at("j"), "opens on the list");
        let keys = ["j", "f", "\x11", "y", " W"].map(at);
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
        assert!(at("@20?trim().to_string()") < at("\x11"), "C-q once fix's line is written");
        assert_eq!(on.last(), Some(&format!("@cut?{}", &V6_DOCS_SAYS[..9]).as_str()));
        let (mut out, mut lines) = (Vec::new(), std::iter::empty::<io::Result<String>>());
        let mut turn = Turn { out: &mut out, lines: &mut lines, pause: Duration::ZERO };
        turn.v6_docs().unwrap();
        let said: Vec<Value> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let run = said.iter().position(|v| v.pointer("/request/message/params/name") == Some(&json!("run")));
        let after: String = (said[run.expect("docs runs") + 1..].iter())
            .filter_map(|v| v.pointer("/event/delta/text").and_then(Value::as_str))
            .collect();
        assert!(after.trim_start().starts_with(V6_DOCS_SAYS), "its first words after the run: {after}");
        assert!((1000..=3000).contains(&V6_DOCS_SAYS_MS), "a turn's pace");
    }

    /// V2 (`waits-60s`): what you opened is your own tests, floated — the scene's file has them and its keys run them
    /// (`space x f`) and float them (`C-w e`) on camera before main's card comes; one session, no list.
    #[test]
    fn v2_floats_your_tests_before_its_card_comes() {
        let scene = SCENES.iter().find(|s| s.name == "waits-60s").unwrap();
        assert_eq!(scene.claude, "cardlater");
        assert!(
            run_files("waits-60s").iter().any(|(f, text)| *f == "src/main.rs" && text.contains(MAIN_TESTS))
        );
        let on: Vec<&str> = WAITS_60S.iter().copied().skip_while(|k| *k != "@rec").collect();
        let at = |seq: &[&str]| on.windows(seq.len()).position(|w| w == seq);
        let (run, float) = (at(&[" ", "x", "f"]).unwrap(), at(&["\x17", "e"]).unwrap());
        let card = at(&["@30?Claude wants to run"]).unwrap();
        assert!(run < float && float < card, "{run} {float} {card}");
        assert!(!on.contains(&"W"), "no session list");
        // (the peek read at its words whole — its air, `▎ ` and the card's title —, inside the 46 columns that read at
        // 2.5 mm at 80×34, not its own 48)
        let zooms: Vec<&str> = on.iter().copied().filter(|k| k.starts_with("@zoom")).collect();
        let words = 3 + "Claude wants to run — outside the sandbox".chars().count();
        assert!(words <= 46, "{words} columns");
        assert_eq!(zooms, [format!("@zoom:card:Claude wants to run|{words}")]);
        // (the take ends as the run's slot first shows: its top line's `─` and the command the card asked for)
        let (mut out, mut lines) = (Vec::new(), std::iter::empty::<io::Result<String>>());
        let mut turn = Turn { out: &mut out, lines: &mut lines, pause: Duration::ZERO };
        turn.ask_outside_for("a reason").unwrap();
        let asked = (String::from_utf8(out).unwrap().lines())
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .find_map(|v| v.pointer("/request/message/params/arguments/command")?.as_str().map(String::from))
            .expect("a run asked");
        assert_eq!(on.last(), Some(&format!("@cut?─ {asked}").as_str()));
    }

    /// V6's three Claudes (`managers-view`): each checkout its own script; edits landing unseen and no card for main's
    /// move; each session's first turn ends naming its window, its request carrying the context the list shows (docs'
    /// second past 80 %); the second turns' edits found in the scene's file, fix's and main's the same line two ways,
    /// fix's key streamed what it replaces first; docs asks to run outside the sandbox; every turn ends.
    #[test]
    fn v6s_three_claudes_each_say_their_context_and_meet_on_one_line() {
        let spec = SCENES.iter().find(|s| s.name == "managers-view").unwrap().claude;
        for who in ["main", "fix", "docs"] {
            let at = if who == "main" { "/h/tarae-demo".to_string() } else { format!("/h/{who}") };
            assert_eq!(script_for(spec, Some(Path::new(&at))), format!("v6-{who}"));
        }
        // (as the shot writes it: the worktrees' folder in the same table)
        let t: toml::Table =
            with_worktrees(config_toml(Path::new("/x"), spec), Path::new("/w")).parse().unwrap();
        assert_eq!(t["llm"]["review-at-end"].as_bool(), Some(true));
        assert_eq!(t["workspace"]["sync"].as_str(), Some("off"));
        assert_eq!(t["workspace"]["worktrees"].as_str(), Some("/w"));
        let plain: toml::Table =
            with_worktrees(config_toml(Path::new("/x"), "idle"), Path::new("/w")).parse().unwrap();
        assert_eq!(plain["workspace"]["worktrees"].as_str(), Some("/w"));
        let file = &run_files("managers-view")[0];
        assert_eq!((file.0, file.1.as_str()), ("src/main.rs", MAIN_MOVED_RS));
        let played = |who: &str, turns: usize| -> Vec<Value> {
            let (mut out, mut lines) = (Vec::new(), std::iter::empty::<io::Result<String>>());
            let mut turn = Turn { out: &mut out, lines: &mut lines, pause: Duration::ZERO };
            turn.v6(who, turns).unwrap();
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
        };
        let carried = |said: &[Value]| -> u64 {
            let u = said.iter().find_map(|v| v.pointer("/event/message/usage")).unwrap();
            ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]
                .iter()
                .map(|k| u[k].as_u64().unwrap())
                .sum()
        };
        let mut replaced = Vec::new();
        for (who, first, second) in V6_CONTEXT {
            let said = played(who, 1);
            let end = said.last().unwrap();
            assert_eq!(end["type"], "result", "{who}'s first turn ends");
            assert_eq!(end.pointer("/modelUsage/claude-opus-5-5/contextWindow"), Some(&json!(V6_WINDOW)));
            assert_eq!(carried(&said), first, "{who}");
            let said = played(who, 2);
            assert_eq!(said.last().unwrap()["type"], "result", "{who}'s second turn ends");
            assert_eq!(carried(&said), second, "{who}");
            assert_eq!(second * 100 / V6_WINDOW >= 80, who == "docs", "{who}: only docs' in warning");
            let calls: Vec<&Value> =
                said.iter().filter_map(|v| v.pointer("/request/message/params")).collect();
            if who == "docs" {
                let run = calls.iter().find(|c| c["name"] == "run").unwrap();
                assert_eq!(run["arguments"]["outside_sandbox"], true);
                continue;
            }
            for c in calls.iter().filter(|c| c["name"] == "edit") {
                for item in c["arguments"]["edits"].as_array().unwrap() {
                    let old = item["old_string"].as_str().unwrap();
                    assert!(MAIN_MOVED_RS.contains(old), "{who}'s {old:?} is in the file");
                    replaced.push(old.to_string());
                }
            }
            if who == "fix" {
                let input: String = (said.iter())
                    .filter_map(|v| v.pointer("/event/delta/partial_json").and_then(Value::as_str))
                    .collect();
                assert!(input.find("old_string") < input.find("new_string"), "{input}");
                assert!(input.contains("trim().to_string()"), "{input}");
            }
        }
        assert_eq!(replaced.iter().filter(|o| *o == "    m.insert(k, t);\n").count(), 2, "{replaced:?}");
    }

    /// Every scene's config, as its take writes it, gets the stand-in (docs shots': docshots' own test) (`shot::stand_in_only`): TOML tarae
    /// reads, its own `llm.command`, never the real claude. V6's first take wrote `[workspace]` twice, and its tarae
    /// started on its defaults — the real claude, which reached for api.anthropic.com.
    #[test]
    fn every_scenes_config_gets_the_stand_in() {
        let (me, dir) = (Path::new("/bin/xtask"), Path::new("/take"));
        let file = Path::new("config.toml");
        for scene in SCENES {
            for (bg, transparent) in [("dark", false), ("light", true)] {
                let text = scene_config(scene, bg, transparent, me, dir);
                crate::shot::stand_in_only(&text, file).unwrap_or_else(|e| panic!("{}: {e}", scene.name));
            }
        }
    }

    /// A per-session script specification picks each session's script by its checkout folder (`tarae-demo` →
    /// `main`), falling back to `idle` for unlisted folders, and leaves plain script names untouched.
    #[test]
    fn script_for_picks_each_sessions_script_by_checkout_folder() {
        let spec = "main=home-main,fix=home-fix,docs=card";
        assert_eq!(script_for(spec, Some(Path::new("/tmp/home/tarae-demo"))), "home-main");
        assert_eq!(script_for(spec, Some(Path::new("/tmp/home/fix"))), "home-fix");
        assert_eq!(script_for(spec, Some(Path::new("/tmp/home/docs"))), "card");
        assert_eq!(script_for(spec, Some(Path::new("/tmp/home/other"))), "idle");
        assert_eq!(script_for("full", Some(Path::new("/tmp/home/fix"))), "full");
    }
}
