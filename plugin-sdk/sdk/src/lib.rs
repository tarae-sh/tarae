//! The SDK for tarae plugins: the bindings of tarae's plugin API (`wit/tarae.wit`), and helpers beside them.
//!
//! The SDK owns the bindings — one `generate!`, here — so a plugin names tarae's types from this crate, implements
//! [`Guest`] and exports itself with [`export!`]:
//!
//! ```ignore
//! use tarae_plugin::{Action, Context, Guest /* … */};
//! struct Plugin;
//! impl Guest for Plugin { /* … */ }
//! tarae_plugin::export!(Plugin);
//! ```
//!
//! A plugin has no clock and no random source of its own: every call from tarae brings `now` (the time, and
//! 32 random bytes). Hand it over first thing, and the rest works — `getrandom` (so rustls, RustCrypto, …)
//! and the time TLS checks certificates against:
//!
//! ```ignore
//! fn run(name: String, cx: Context) -> Vec<Action> {
//!     tarae_plugin::now::set(cx.now.millis, &cx.now.entropy);
//!     // …
//! }
//! ```
//!
//! - [`now`]: the time, and the randomness it keeps stirred.
//! - [`http`]: HTTP/1.1 requests written out, responses read as their bytes come (chunked too), and a
//!   streamed body split into lines (a Kubernetes watch).
//! - [`tls`] (feature `tls`): a TLS client over bytes — rustls with RustCrypto, no sockets: what it wants
//!   sent goes out with tarae's `send`, what comes back (`data`) goes in.
//! - [`test`] (feature `test`, a dev-dependency's): the mock host a plugin's native tests drive it through,
//!   holding it to its declared permissions with tarae's own rules (`tarae-plugin-rules`).

pub mod bindings {
    //! What wit-bindgen makes of `wit/tarae.wit` — its types, the [`Guest`] trait and the `export!` macro, all
    //! re-exported at the crate's root.
    wit_bindgen::generate!({
        path: "../wit",
        world: "plugin",
        additional_derives: [PartialEq],
        pub_export_macro: true,
        default_bindings_module: "tarae_plugin::bindings",
    });
}

pub use bindings::tarae::plugin::types::*;
pub use bindings::{Guest, export};

pub mod http;
pub mod now;
#[cfg(feature = "test")]
pub mod test;
#[cfg(feature = "tls")]
pub mod tls;
