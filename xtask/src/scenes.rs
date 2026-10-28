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

use std::io::{self, BufRead, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

use crate::shot::{self, Setup};
use crate::{Result, root};

// (`scenes --docs`: the docs' pictures, each from a scene here)
pub(crate) mod docshots;

const USAGE: &str = "usage: cargo xtask scenes [--out DIR] [--size COLSxROWS] [--transparent] [--bin TARAE] [scene …] | scenes --list | scenes --compare A B";

/// One surface: the stand-in claude's script, the files tarae opens, the keys sent (as `cargo xtask shot`
/// takes them: `@1` waits a second, `@?text` until `text` is on screen, `@20?text` at most 20 s).
#[derive(Debug)]
struct Scene {
    name: &'static str,
    claude: &'static str,
    files: &'static [&'static str],
    keys: &'static [&'static str],
}

const MAIN: &[&str] = &["src/main.rs"];
const WIDE: &[&str] = &["src/wide.rs"];

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
        "@30?That is all.",
        "@1",
        "@hover?esc normal",
        "@1",
    ],
    &["@1", " ", "l", "@1", "how is the map filled?", "\r", "@30?That is all.", "@1", "@hover?follow", "@1"],
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

/// #187: what's cut is whole on a card over it, one way — the chat's view mode's cursor on a cut `run` row (its card on
/// the selected row's ground, the cursor kept); the side panel with the keys, its selected long name; a queued message
/// past a card's rows, `↓ N more lines` under the mouse; then pressed — the message whole in a read-only buffer.
const CUT_KEYS: [&[&str]; 4] = [
    &["@1", " ", "l", "@1", "tidy it", "\r", "@30?That is all.", "@1", "\x1b", "/", "cargo fmt", "\r", "@1"],
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
const FOOT: &[&str] = &["@1", " ", "l", "@1", "how is the map filled?", "\r", "@30?That is all.", "@1"];

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
            "sh git worktree add -q ../fix",
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
    "sh git worktree add -q ../docs",
    "\r",
    "@3",
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
            "sh git worktree add -q ../fix",
            "\r",
            "@3",
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
            "sh git worktree add -q ../fix",
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
            "@12?fix ≈ main",
            "@1",
            $($more),*
        ]
    };
}
// (the toast, once)
const OVERLAP: &[&str] = overlap_keys!();
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
    "sh git worktree add -q ../docs",
    "\r",
    "@3",
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
    "sh git commit -qam main",
    "\r",
    "@2",
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
/// orchestra P1b (`orchestra-list`): a session of yours (fix) opened, then main's Claude starts three — knot-count's turn
/// ends (done), thread-docs asks for a run (▲ waits), spool-clones writes (working) — and main's chat open with the
/// session list over it: the children right after main, each with a faint `›`, fix after them; the chat's top line
/// the same order.
const ORCHESTRA_LIST: &[&str] = &[
    "@1",
    ":",
    "sh git worktree add -q ../fix",
    "\r",
    "@3",
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
    "sh git worktree add -q ../fix",
    "\r",
    "@3",
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
        "@30?That is all.",
        "@1",
        "\x17",
        "h",
        "@1",
        "@hover?cargo fmt",
        "@1",
    ],
    &["@1", " ", "l", "@1", "tidy it", "\r", "@30?That is all.", "@1", "\x1b", "@1", "@hover?i type", "@1"],
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
const HOVER_SESSIONS: &[&str] = waits_keys!(" ", "W", "@2", "@hover+4?The tests need", "@1");
/// Nine sessions (`strip-nine`): main and fix overlapping, seven more checkouts — docs and parser waiting on their
/// Claude's card, keys's Claude on its run's card (a shot's sandbox is none: no run goes unasked, so none is stuck —
/// `session_tabs`' tests have that), web's turn ended behind (done) — then main's chat open, its top line naming them;
/// then `$more`.
macro_rules! strip_nine_keys {
    ($($more:expr),* $(,)?) => {
        overlap_keys!(
    ":",
    "sh for n in docs parser sbspeed keys pull web motion; do git worktree add -q ../$n; done",
    "\r",
    "@4",
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
/// …the pointer on its `+N` (the hover audit).
const HOVER_CHAT_MORE: &[&str] = strip_nine_keys!("@hover?+", "@1");
/// collab C3: two more checkouts — fix's own commit on the `threads.insert` line, docs untouched — then main's own way
/// of that line committed in the checkout tarae started in: main moved, and its one card names both workspaces behind
/// it, fix's would-be conflict in words (`basemoved`).
const BASE_MOVED: &[&str] = &[
    "@1",
    ":",
    "sh git worktree add -q ../fix && git worktree add -q ../docs",
    "\r",
    "@3",
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
    "sh git -c user.name=demo -c user.email=demo@example.com -c commit.gpgsign=false commit -qm fix src/main.rs",
    "\r",
    "@3",
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
    "sh git worktree add -q ../fix && git worktree add -q ../docs",
    "\r",
    "@3",
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
    "sh git worktree add -q ../fix",
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

const SCENES: &[Scene] = &[
    Scene { name: "start", claude: "idle", files: &[], keys: &["@?Open a file", "@1"] },
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
    // (the run panels' output with the keys: a task's, two lines of it selected — its settings trusted first)
    Scene {
        name: "output-task",
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
            "@20?That is all",
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
            "sh git worktree add -q ../fix",
            "\r",
            "@3",
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
    // (panes P3 step 7d: Claude's card with the chat closed — the screen's right, above the status line)
    Scene { name: "asks-closed", claude: "card", files: MAIN, keys: ASKS_CLOSED },
    // (…and with the chat open — `space l` brings it back from following: over its foot, the transcript above it)
    Scene { name: "asks-open", claude: "card", files: MAIN, keys: ASKS_OPEN },
    // (…four asks over two sessions in one stack: two cards your keys asked for in front, fix's Claude's behind them,
    // main's a peek named in its colour)
    Scene { name: "asks-stack", claude: "card", files: MAIN, keys: ASKS_STACK },
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
    Scene { name: "hover-side-edge", claude: "idle", files: MAIN, keys: HOVER_MORE[0] },
    Scene { name: "hover-divider-off", claude: "idle", files: MAIN, keys: HOVER_MORE[1] },
    Scene { name: "hover-hsplit-line", claude: "idle", files: MAIN, keys: HOVER_MORE[2] },
    Scene { name: "hover-path-name", claude: "idle", files: MAIN, keys: HOVER_MORE[3] },
    Scene { name: "hover-rail-keys", claude: "idle", files: MAIN, keys: HOVER_MORE[4] },
    // (the docs' `claude-reviews.png`: the reviewer read in the background — three findings on thread.rs's lines,
    // one's card open under it, its report beside it in a split (`space v r` — read, the status line's
    // `¶ 3 findings` goes, as decided))
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
    if args.first().is_some_and(|a| a == "--compare") {
        let [_, a, b] = args else { return Err(USAGE.into()) };
        return compare(Path::new(a), Path::new(b));
    }
    if args.first().is_some_and(|a| a == "--docs") {
        return docshots::run(&args[1..]);
    }
    let (mut out, mut names) = (root().join("target/scenes"), Vec::new());
    let mut sizes = SIZES.to_vec();
    let mut transparent = false;
    let mut bin: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--list" => {
                SCENES.iter().for_each(|s| println!("{}", s.name));
                return Ok(());
            }
            "--out" => out = PathBuf::from(it.next().ok_or(USAGE)?),
            // (one size instead of the two: a width where something folds)
            "--size" => {
                let s = it.next().ok_or(USAGE)?;
                let (c, r) = s.split_once('x').ok_or(USAGE)?;
                sizes = vec![(c.parse().map_err(|_| USAGE)?, r.parse().map_err(|_| USAGE)?)];
            }
            // (the transparent themes: what has no background of its own)
            "--transparent" => transparent = true,
            // (another build of tarae — main's, for the shots before a change: A/B)
            "--bin" => bin = Some(PathBuf::from(it.next().ok_or(USAGE)?)),
            n if !n.starts_with('-') => names.push(n.to_string()),
            _ => return Err(USAGE.into()),
        }
    }
    let scenes = chosen(&names)?;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let bin = match bin {
        Some(b) if b.is_absolute() && b.is_file() => b,
        Some(b) => return Err(format!("--bin {}: an absolute path to a tarae", b.display())),
        None => shot::build_shot(&root())?,
    };
    let me = std::env::current_exe().map_err(|e| format!("this xtask: {e}"))?;
    let mut failed = Vec::new();
    for scene in scenes {
        for bg in BACKGROUNDS {
            for &size in &sizes {
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
    let demo = dir.0.join("tarae-demo");
    demo_crate_with(&demo, scene.name == "trusted", &run_files(scene.name))?;
    let config = dir.0.join("xdg");
    // (the overlay scenes': the SDK's demo plugin, its permissions asked as it loads — `C-q y` in their keys)
    if scene.name.starts_with("overlay-") {
        let wasm =
            std::fs::read(root().join("plugin-sdk/demo.wasm")).map_err(|e| format!("demo.wasm: {e}"))?;
        std::fs::create_dir_all(config.join("tarae/plugins")).map_err(|e| format!("plugins: {e}"))?;
        std::fs::write(config.join("tarae/plugins/demo.wasm"), wasm)
            .map_err(|e| format!("demo.wasm: {e}"))?;
    }
    let mut toml = config_toml(me, scene.claude);
    if transparent {
        let theme = if bg == "dark" { "meok-transparent" } else { "hanji-transparent" };
        toml = format!("theme = \"{theme}\"\n{toml}");
    }
    // (a session's checkout made by tarae — orchestra's set — in the shot's own folder, never yours)
    let worktrees = toml::Value::String(dir.0.join("worktrees").to_string_lossy().into_owned());
    toml = format!("{toml}\n[workspace]\nworktrees = {worktrees}\n");
    write(&config.join("tarae/config.toml"), &toml)?;
    let setup = Setup {
        bin: bin.to_path_buf(),
        files: scene.files.iter().map(|f| f.to_string()).collect(),
        cwd: demo,
        config,
        background: Some(bg),
        // (the terminal scenes': a plain sh of the shot's own — a `$ ` prompt, no rc, no history, no user or host
        // name; never the shell of whoever takes the shots)
        env: match scene.name.starts_with("quote-term") {
            true => {
                vec![("SHELL", Some("/bin/sh")), ("PS1", Some("$ ")), ("HISTFILE", Some("")), ("ENV", None)]
            }
            false => Vec::new(),
        },
    };
    let keys: Vec<String> = scene.keys.iter().map(|k| k.to_string()).collect();
    shot::take(&setup, &keys, size, &dir.0.join("state"), out)
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
        _ => "",
    };
    format!(
        "[editor]\nlsp = false\n\n[llm]\ncommand = {}\nargs = [\"scenes\", \"--claude\", {}]\n{llm}{review}",
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

/// The demo crate at `dir`: committed in a repository of its own, then changed. git runs with none of your
/// settings (no hooks, no signing) and finds no repository above `dir`.
fn demo_crate(dir: &Path, trusted: bool) -> Result<()> {
    demo_crate_with(dir, trusted, &[])
}

/// `demo_crate` with `extra` files committed beside its own (over them where named alike).
pub(crate) fn demo_crate_with(dir: &Path, trusted: bool, extra: &[(&str, String)]) -> Result<()> {
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
        | "group-tests-stacked" | "float-tests" | "float-tests-moved" => vec![
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
        "follow-split-s" => vec![("src/wide.rs", wide_rs(18, 112))],
        "follow-split-v" => vec![("src/wide.rs", wide_rs(36, 70))],
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
    while let Some(line) = lines.next() {
        let line = line?;
        // (the docs' own scripts first)
        if docshots::answered(script, &line, out, pause)? {
            continue;
        }
        if !line.starts_with("{\"message\"") {
            continue;
        }
        let mut turn = Turn { out: &mut *out, lines: &mut lines, pause };
        match script {
            "full" => turn.explain(true, false)?,
            "three" => turn.explain(true, true)?,
            // (two changes: the press's sheet — the first answered, the second in hand)
            "two" => turn.explain_items(2)?,
            // (motion §2: an edit of six lines in one go, then a pause — or, `sixend`, the turn's end at once)
            "six" | "sixfollow" => turn.six(true)?,
            "sixend" => turn.six(false)?,
            // (motion §4: twenty stops, one run failing, held at the last)
            "strip" => turn.strip()?,
            "now" => turn.now(false)?,
            "nowcard" => turn.now(true)?,
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
            "startone" | "startthree" | "startlong" => turn.sessions_start(&script[5..])?,
            "startgo" => turn.sessions_started(false)?,
            "startgo3" => turn.sessions_started(true)?,
            "childcount" => turn.child_count(&line)?,
            "childlong" => turn.child_long(&line)?,
            "parent" => turn.parent(&line)?,
            _ => {}
        }
    }
    Ok(())
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

    /// `s` a word at a time, as a model streams it.
    fn words(&mut self, s: &str) -> io::Result<()> {
        s.split(' ').try_for_each(|w| self.text(&format!("{w} ")))
    }

    /// A call of tarae's tool `name` (the turn's `n`th): its row in the chat, the request, then tarae's answer
    /// waited for.
    fn call(&mut self, n: u64, name: &str, args: Value) -> io::Result<()> {
        let tool = json!({"type": "tool_use", "id": format!("t{n}"), "name": format!("mcp__tarae__{name}"), "input": args});
        self.line(json!({"type": "assistant", "message": {"id": format!("m{n}"), "content": [tool]}}))?;
        let msg = json!({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args}});
        let request = json!({"subtype": "mcp_message", "server_name": "tarae", "message": msg});
        self.line(json!({"type": "control_request", "request_id": format!("c{n}"), "request": request}))?;
        let id = format!("\"c{n}\"");
        for l in self.lines.by_ref() {
            if l?.contains(&id) {
                break;
            }
        }
        Ok(())
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
        self.call(1, "sessions_start", json!({"sessions": sessions, "base": "main"}))?;
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// `sessionset-child`'s main: one session asked for (`three`: `orchestra-*`'s three), then — the set made — its
    /// turn's end.
    fn sessions_started(&mut self, three: bool) -> io::Result<()> {
        self.words("The count is a piece of its own, so it can go in a session beside us.")?;
        self.text("\n")?;
        let sessions = match three {
            true => json!({"knot-count": COUNT_BRIEF,
                "thread-docs": "Write a one-line doc comment for every public item in src/thread.rs.",
                "spool-clones": "Make filling the map in src/main.rs clone each thread's name once, not twice."}),
            false => json!({"knot-count": COUNT_BRIEF}),
        };
        self.call(1, "sessions_start", json!({"sessions": sessions, "base": "main"}))?;
        self.words(if three {
            "Started all three; I'll keep to the map here."
        } else {
            "Started **knot-count**; I'll keep to the map here."
        })?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// `sessionset-child`'s child: main's brief answered with a read and a plan; a message of yours after it, briefly.
    fn child_count(&mut self, line: &str) -> io::Result<()> {
        if !line.contains("from-session") {
            self.words("Right — ties keep their names' order, so the sort is stable on the name.")?;
            return self.line(json!({"type": "result", "result": "done"}));
        }
        self.words("Reading how the threads are kept first.")?;
        self.text("\n\n")?;
        self.call(1, "read", json!({"files": ["src/main.rs:1-20"]}))?;
        self.words("Each thread keeps its knots in a list, so the count is its length. I'll sort by it, most-knotted first, \
            and add the order's test in **src/thread.rs**.")?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// `orchestra-parent-long`'s child: main's brief answered with a reply whose first line runs past a news item's head
    /// (100 characters) and a few lines after it.
    fn child_long(&mut self, line: &str) -> io::Result<()> {
        if !line.contains("from-session") {
            return self.child_count(line);
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
        self.sessions_started(false)
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

    fn ask_outside(&mut self) -> io::Result<()> {
        self.words(
            "The tests need the network for the registry, so I will ask to run them outside the sandbox.",
        )?;
        self.text("\n")?;
        let run = json!({"command": "cargo test --quiet", "outside_sandbox": true,
            "reason": "cargo needs to fetch the registry index"});
        self.call(1, "run", run)?;
        self.line(json!({"type": "result", "result": "done"}))
    }

    /// (`now-*`: a read, a plan, a sentence of intent, then the tests run — in flight, held there; `card`: asked to run
    /// outside the sandbox, the card up)
    fn now(&mut self, card: bool) -> io::Result<()> {
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
        if card {
            let run = json!({"command": "cargo test --quiet", "outside_sandbox": true,
                "reason": "cargo needs to fetch the registry index"});
            self.call(3, "run", run)?;
            return self.lines.by_ref().try_for_each(|l| l.map(drop));
        }
        let tool = json!({"type": "tool_use", "id": "t3", "name": "mcp__tarae__run", "input": {"command": "cargo test -q"}});
        self.line(json!({"type": "assistant", "message": {"id": "m3", "content": [tool]}}))?;
        self.lines.by_ref().try_for_each(|l| l.map(drop))
    }

    /// #187's turn: a run of a long command and a note of several lines — each a call and its result as claude's stream
    /// says them (nothing runs, no request of tarae's) — then a few words and the turn's end.
    fn cut_hand(&mut self) -> io::Result<()> {
        let run = "cargo fmt --all && git diff --stat | tail -3 && cargo clippy --all-targets -q 2>&1 | grep -E \
                   'warning|error' | head -20 && cargo test -q --bin tarae whole";
        let note = "The first strand is kept now; the tests cover the entry change.\nThe clone in the loop can go \
                    once names are borrowed — a later change.";
        let stops = [
            ("run", json!({"command": run}), "exit 0"),
            ("note", json!({"path": "src/main.rs", "line": 3, "text": note}), "noted"),
        ];
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

    /// Homepage hero (`fix` checkout): holds its `input_json_delta` on `src/main.rs` line 12 (`‹fix writing›` in
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
