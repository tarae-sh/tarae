//! Your work wiki (the work-wiki plan W1, `.sb/web-auth-plan.md`; the user's, 2026-11-09): a wiki you name in your
//! own config — `web.signed-in = [{ base = "https://corp.example/confluence", kind = "confluence-dc", token-env =
//! "CONFLUENCE_PAT" }]`, YOURS_ONLY and RISKY — and the variable that holds its token, read from tarae's own
//! environment (`Here`, passed in) where a fetch's way is made, on its worker. The token goes in the
//! `Authorization` header of tarae's own GET only: over https, beyond this machine, to the base's host and port, its
//! path at or below the base's — judged per request, so a redirect or a refresh elsewhere goes without it. It's never
//! a run's (`runenv::of` keeps the variable out whatever passes it — the token can write the wiki), never any
//! claude's (`llm::command` removes it), never said: its `Debug` holds no value, and a page that echoes it has it
//! scrubbed (`scrubbed`) before anyone reads it. Unset, a read under the base is said, never fetched without it (it
//! would land on a sign-in page).

use std::collections::HashMap;
use std::ffi::OsString;

use super::http::Target;

/// A wiki named in `web.signed-in`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wiki {
    /// Its base address, as the fetch writes it: `https://host[:port]/prefix`, no trailing `/`.
    pub base: String,
    pub host: String,
    pub port: u16,
    /// The base's path, no trailing `/` — "" at the host's root.
    pub prefix: String,
    /// Its rows in `runtime/web/sources.toml` (`source.rs`).
    pub kind: String,
    /// The variable of tarae's environment that holds its token.
    pub token_env: String,
    /// Why that variable held no token when the setting was applied (`with_env`; tarae's environment doesn't change
    /// after it starts) — then Claude's read is refused before any card.
    pub token_missing: Option<Missing>,
}

impl Wiki {
    /// `corp.example/confluence`, as the words say it.
    pub fn shown(&self) -> String {
        let t = Target {
            https: true,
            host: self.host.clone(),
            port: self.port,
            path: String::new(),
            loopback: false,
            upgraded: false,
        };
        format!("{}{}", t.authority(), self.prefix)
    }

    /// Whether a request to `t` carries the token: https, beyond this machine, the base's host and port, a path at
    /// or below the base's — never one with a dot segment in any spelling a server or a proxy on the way takes as one
    /// (`..`, `%2e%2e`, Tomcat's `..;/`), nor an encoded slash or backslash (`%2f`, `%5c`: a proxy that decodes it
    /// before normalizing takes the path out of the base).
    pub fn under(&self, t: &Target) -> bool {
        if !t.https || t.loopback || t.host != self.host || t.port != self.port {
            return false;
        }
        let path = t.path.split('?').next().unwrap_or_default();
        let lower = path.to_ascii_lowercase();
        let dot = |s: &str| {
            let s = decoded(s);
            let s = s.split(';').next().unwrap_or_default();
            s == "." || s == ".."
        };
        let dotted = lower.contains("%2f") || lower.contains("%5c") || path.split('/').any(dot);
        let below = match path.strip_prefix(self.prefix.as_str()) {
            Some(rest) => self.prefix.is_empty() || rest.is_empty() || rest.starts_with('/'),
            None => false,
        };
        below && !dotted
    }

    /// The words for a 401 (the token itself refused) or a 403 (the account, with it, isn't allowed this) from it.
    pub fn refused(&self, status: u16) -> String {
        let why = match status {
            401 => "expired or revoked?",
            _ => "your account isn't allowed this with it",
        };
        format!("{} refused the token in {} — {why}", self.host, self.token_env)
    }
}

/// A path segment percent-decoded (bytes, as the server may read them; what isn't UTF-8 replaced).
fn decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).and_then(|c| hex(*c)), b.get(i + 2).and_then(|c| hex(*c))) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `web.signed-in` as written → its wikis, or what's wrong: a list of tables, each a `base` (an `https://` address of
/// a host by its name, no query), a `kind` the sources know, a `token-env` (a variable's whole name, not one a run
/// gets as the floor) — and nothing else.
pub fn wikis_of(v: &toml::Value) -> Result<Vec<Wiki>, String> {
    const SHAPE: &str = "a list of tables: [{ base = \"https://corp.example/confluence\", kind = \"confluence-dc\", \
                         token-env = \"CONFLUENCE_PAT\" }]";
    let list = v.as_array().ok_or(SHAPE)?;
    let mut out = Vec::new();
    for w in list {
        let t = w.as_table().ok_or(SHAPE)?;
        if let Some(k) = t.keys().find(|k| !["base", "kind", "token-env"].contains(&k.as_str())) {
            return Err(format!("{k}: not one of base, kind, token-env"));
        }
        let field =
            |k: &str| t.get(k).and_then(|v| v.as_str()).ok_or_else(|| format!("{k}: missing — {SHAPE}"));
        let (base, kind, var) = (field("base")?, field("kind")?, field("token-env")?);
        if !base.trim().to_ascii_lowercase().starts_with("https://") {
            return Err(format!("\"{base}\": an https:// address — the token never goes over plain http"));
        }
        let at = super::http::target(base).map_err(|e| format!("\"{base}\": {e}"))?;
        if at.loopback {
            return Err(format!("\"{base}\": a host beyond this machine — the token never goes to this one"));
        }
        if at.path.contains('?') || base.contains('#') {
            return Err(format!("\"{base}\": a base address, no query or fragment"));
        }
        if !super::source::kinds().any(|k| k == kind) {
            let known: Vec<&str> = super::source::kinds().collect();
            return Err(format!("\"{kind}\": a kind the sources know ({})", known.join(", ")));
        }
        if var.ends_with('*')
            || !crate::sandbox::runenv::entry_ok(var)
            || !crate::readonly::env_core(var)
            || crate::sandbox::runenv::floor(var)
        {
            return Err(format!("\"{var}\": a variable's whole name, not one every run gets"));
        }
        if crate::llm::claudes_own(var) {
            return Err(format!(
                "\"{var}\": Claude Code's own — your wiki's token goes in a variable of its own (\"CONFLUENCE_PAT\")"
            ));
        }
        let prefix = at.path.trim_end_matches('/').to_string();
        let mut wiki = Wiki {
            base: String::new(),
            host: at.host,
            port: at.port,
            prefix,
            kind: kind.to_string(),
            token_env: var.to_string(),
            token_missing: Some(Missing::Unset),
        };
        wiki.base = format!("https://{}", wiki.shown());
        out.push(wiki);
    }
    Ok(out)
}

/// `wikis` with whether each one's variable holds a token, by `token_in` as `of` judges it (`env`: tarae's environment
/// where the setting is applied — a stand-in in tests).
pub fn with_env(mut wikis: Vec<Wiki>, env: impl Fn(&str) -> Option<OsString>) -> Vec<Wiki> {
    for w in &mut wikis {
        w.token_missing = token_in(env(&w.token_env).as_deref()).err();
    }
    wikis
}

/// Why a variable holds no token tarae sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    /// Not in tarae's environment, or only whitespace.
    Unset,
    /// More than one visible-ASCII word: it could end the header and start another.
    NotAToken,
}

impl Missing {
    /// Its words for the wiki whose variable is `var` (`claudes`: Claude's, ending "ask the user").
    pub fn words(self, var: &str, claudes: bool) -> String {
        let ask = if claudes { " — ask the user" } else { "" };
        match self {
            Missing::Unset => {
                format!("{var} isn't set in tarae's environment — export it and start tarae again{ask}")
            }
            Missing::NotAToken => format!(
                "{var} holds what a token can't (a space, a line break, a character that isn't ASCII) — nothing is \
                 sent{ask}"
            ),
        }
    }
}

/// The token in a variable's value, trimmed: one visible-ASCII word — the one check `of` and `with_env` make.
pub fn token_in(value: Option<&std::ffi::OsStr>) -> Result<&[u8], Missing> {
    match value.map(|v| v.as_encoded_bytes().trim_ascii()) {
        None | Some([]) => Err(Missing::Unset),
        Some(v) if v.iter().all(|b| (0x21..0x7f).contains(b)) => Ok(v),
        Some(_) => Err(Missing::NotAToken),
    }
}

/// A wiki's token as tarae read it: its bytes wiped when it goes (`zeroize`), never printed.
pub struct Token(zeroize::Zeroizing<Vec<u8>>);

impl Token {
    pub fn as_str(&self) -> &str {
        // (visible ASCII only: `of` takes no other)
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

/// A wiki with its token — or why there's none, in the words a read under its base says.
#[derive(Debug)]
pub struct Signed {
    pub wiki: Wiki,
    pub token: Result<Token, String>,
}

/// The wikis with their tokens, from `env` (tarae's own — `Here`'s): a token is one visible-ASCII word (else it
/// could end the header and start another). `claudes`: the words a read of Claude's says, ending "ask the user".
pub fn of(wikis: &[Wiki], env: &HashMap<String, OsString>, claudes: bool) -> Vec<Signed> {
    wikis
        .iter()
        .map(|w| {
            let token = token_in(env.get(&w.token_env).map(OsString::as_os_str))
                .map(|v| Token(zeroize::Zeroizing::new(v.to_vec())))
                .map_err(|m| m.words(&w.token_env, claudes));
            Signed { wiki: w.clone(), token }
        })
        .collect()
}

/// The wiki whose base `t` is under (`Wiki::under`).
pub fn covering<'a>(signed: &'a [Signed], t: &Target) -> Option<&'a Signed> {
    signed.iter().find(|s| s.wiki.under(t))
}

/// `bytes` with every token of `signed` in it replaced (a page that echoes its request), and whether one was.
pub fn scrubbed(bytes: &mut Vec<u8>, signed: &[Signed]) -> bool {
    let mut found = false;
    for s in signed {
        let Ok(t) = &s.token else { continue };
        let t = t.0.as_slice();
        if t.is_empty() || bytes.len() < t.len() {
            continue;
        }
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i..].starts_with(t) {
                out.extend_from_slice(b"[token]");
                (i, found) = (i + t.len(), true);
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        *bytes = out;
    }
    found
}

/// The chat's one line on the wikis (W4) — in the system prompt only while `web.signed-in` names one, fixed for a
/// chat's process (the cache holds): where they are, and how their own search reads.
pub fn prompt(wikis: &[Wiki]) -> String {
    let found: Vec<String> = wikis
        .iter()
        .map(|w| match super::source::search_form(&w.kind) {
            Some(form) => format!("{} (search: read {}{form})", w.base, w.base),
            None => w.base.clone(),
        })
        .collect();
    match found.len() {
        0 => String::new(),
        1 => format!(" The user's wiki: {} — read its pages by their addresses.", found[0]),
        _ => format!(" The user's wikis: {} — read their pages by their addresses.", found.join(", ")),
    }
}

#[cfg(test)]
mod tests;
