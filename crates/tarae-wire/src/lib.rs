//! The attach protocol (the remote plan's §8b): a host — the editor, its sessions, Claude and the runs — and the
//! interface showing it — a terminal client today, a browser later — joined by one ordered stream: the screen's
//! changes out, keys in. `msg` is what each side says (`ToHost`, `ToInterface`), `codec` their bytes (a `u32` length,
//! a kind byte, the body) and the `Decoder` that reads them, `mailbox` the host's writer's one-slot frame, the queue of
//! the rest and the credits its acks give.
//!
//! What arrives is anyone's bytes as far as the reader knows (a socket's peer is checked, but its bytes aren't): the
//! decoder never panics (release aborts on one), never allocates past a length it checked against `MSG_MAX`, and a
//! malformed message ends the connection with what was wrong (`Error`). `check` holds that as a property, its loop in
//! every `cargo test`, and `fuzz/parsers`' `wire_decode` target. No `unsafe` anywhere in it.
//!
//! On a stream nothing is lost and order holds, so the host diffs each frame against the last one it *sent* (what
//! the interface holds once it reads on), never the last acked (mosh's rule, for datagrams that go missing: here it
//! would leave a cell changed and changed back between two skipped frames wrong for good). A slow interface skips
//! frames in the mailbox, never on the wire; acks are credits (`IN_FLIGHT`).

#![forbid(unsafe_code)]

/// The decoder's property and the inputs tests build: for the crate's tests and the fuzzer's (`fuzz/parsers`).
#[cfg(any(test, feature = "check"))]
pub mod check;
pub mod codec;
pub mod mailbox;
pub mod msg;

pub use codec::{Decoder, Error, MSG_MAX, Message, encode};
pub use mailbox::{IN_FLIGHT, Mailbox, Next, Order, Slot};
pub use msg::*;

#[cfg(test)]
mod tests;
