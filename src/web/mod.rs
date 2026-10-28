//! The web, the editor's side: the reader itself — a page's bytes → its text as markdown, its links, anchors and
//! title (`read`, a pure function), gzip, addresses, the page's own server's answers (`facts`) — is the
//! `tarae-web` crate's (crates/tarae-web), named here as tarae had it. tarae fetches the page (`fetch.rs`: its own
//! HTTP/1.1 GET through the runs' egress proxy, TLS by rustls, the address and request `http.rs`), reads it on a
//! worker, never the main thread, and makes the document.
//!
//! `:web https://… [main|article|page]` fetches a page into a buffer, `:web <path> [url] […]` reads a saved one
//! (`open.rs`); a page is a buffer as code is (`page.rs`). Claude's `read` takes an `https://` URL (`tool.rs`): its own gate, the page framed as data.

pub mod fetch;
mod http;
pub mod open;
pub mod page;
pub mod serve;
pub mod tool;

pub use tarae_web::{PAGE_MAX, Page, Prefer, Read, convert, facts, gzip, read, url};

#[cfg(test)]
mod tests;
