//! The web reader (web-plan; the user's 2026-10-23: the core's, as it's Claude's tool — always there): a page's
//! bytes → its readable text as markdown, its links and anchors, its title, how it read — `read`, a pure function
//! (`reader.rs`; the converter `convert.rs`, code blocks' languages `lang.rs`, addresses `url.rs`, charsets
//! `charset.rs`), a gzip body unpacked (`gzip.rs`, the fetch's), the page's own language server's answers
//! (`facts.rs`), and what a file's bytes are when they aren't a page (`media.rs`: pictures for Claude). tarae fetches
//! the page and makes the document (its `src/web/`: the fetch, the page's buffer, Claude's `read`); this crate names
//! nothing of the editor.
//!
//! A page's bytes are anyone's (`media.rs`'s rule): parsed only by memory-safe code (html5ever and encoding_rs — no
//! parser of tarae's own; the tree they're read into `tree.rs`, the part read `pick.rs`), on a worker, never the
//! main thread, within limits — its first `PAGE_MAX` bytes, `DEPTH_MAX` levels of nesting, `TEXT_MAX` of text, each
//! said when hit — and never a panic (release aborts on one): no index guessed, every slice at a place found. `check`
//! holds that as a property, its loop in every `cargo test`, and `fuzz/parsers`' `web`, `facts` and `gzip` targets.
//! No `unsafe` anywhere in it, `check` and the tests too: the compiler holds the rule (`forbid`, which no inner
//! `allow` can lift).

#![forbid(unsafe_code)]

mod charset;
/// The readers' properties and the inputs tests build: for tests, tarae's (its dev-dependency turns `check` on) and
/// the fuzzer's (`fuzz/parsers`).
#[cfg(any(test, feature = "check"))]
pub mod check;
pub mod convert;
pub mod facts;
pub mod gzip;
mod lang;
pub mod media;
mod pick;
mod reader;
mod tree;
pub mod url;

pub use reader::{DEPTH_MAX, PAGE_MAX, Page, Prefer, Read, TEXT_MAX, read};

#[cfg(test)]
mod tests;
