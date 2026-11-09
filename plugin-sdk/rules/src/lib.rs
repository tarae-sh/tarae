//! The rules a tarae plugin is held to, and the words tarae says them in — one source for tarae and for the SDK's
//! mock host (`tarae_plugin::test`), so a plugin's test fails where tarae would refuse, in tarae's words.
//!
//! Strings and paths in, decisions and words out: no disk and no environment (tarae's code-root lint holds it) —
//! what a path resolves to, and where home is, the caller says. What stays in tarae: the approvals and their store,
//! the cards themselves, and the hosts never reached (`never_reached`: the egress proxy's list).
//!
//! SPDX-License-Identifier: MIT OR Apache-2.0 — like the rest of the SDK, so a plugin can be under any license.

pub mod secretnames;

use std::path::{Component, Path, PathBuf};

/// The longest message or description shown from a plugin (in characters).
pub const MAX_TEXT: usize = 2000;

/// A character that reorders text around it (so what's shown isn't what's there).
pub fn bidi(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
}

/// Plugin text fit for the screen: control and reordering characters (terminal escapes among them)
/// replaced, and capped.
pub fn shown(s: &str) -> String {
    let mut out: String = s
        .chars()
        .take(MAX_TEXT)
        .map(|c| if (c.is_control() && c != '\n') || bidi(c) { '\u{FFFD}' } else { c })
        .collect();
    if s.chars().nth(MAX_TEXT).is_some() {
        out.push('…');
    }
    out
}

/// A permission a plugin declares (`permissions()`), as the rules read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perm<'a> {
    Run(&'a str),
    Read(&'a str),
    Env(&'a str),
    Network,
    Project,
    Keys,
}

/// A permission as it's kept.
pub fn key(p: Perm) -> String {
    match p {
        Perm::Run(prog) => format!("run {prog}"),
        Perm::Read(pat) => format!("read {pat}"),
        Perm::Env(name) => format!("env {name}"),
        Perm::Network => "network".into(),
        Perm::Project => "project".into(),
        Perm::Keys => "keys".into(),
    }
}

/// A permission in words, for the card — what it amounts to, broad reading said so.
pub fn describe(p: Perm) -> String {
    match p {
        Perm::Run(prog) => format!("run the program {} (with any arguments, as you)", shown(prog)),
        Perm::Read(pat) => {
            let fixed: String =
                pat.split('/').take_while(|c| !c.contains(['*', '?'])).collect::<Vec<_>>().join("/");
            let warn = match fixed.as_str() {
                "" => " ⚠ every file on this computer",
                "~" => " ⚠ every file in your home folder",
                _ => "",
            };
            format!("read files at {}{warn}", shown(pat))
        }
        Perm::Env(name) => format!("read the environment variable {}", shown(name)),
        Perm::Network => "connect over the network (each new host is asked)".into(),
        Perm::Project => "read the files of the project you opened, and find them by name — not ones named like keys \
                          (.env, *.pem …): those it asks for"
            .into(),
        Perm::Keys => "hear the keys you press as commands, never a character typed — only while your own config's \
                       plugins.keys is on, said on the status line while it hears; it shows them in its overlay only, \
                       never edits, says, opens, keeps, reads, connects to or runs anything"
            .into(),
    }
}

/// Why what a plugin declares isn't what tarae takes, if it isn't: each permission as written (`invalid_one`), and
/// a plugin that hears keys asking for nothing else (overlays plan §7 1: a keylogger needs a way out, a keycast
/// none) — no `network`, no `run`, and no reads either: a path it picks is a lookup the filesystem makes, and on a
/// network mount even a missing file's lookup reaches a server. What needs no permission — an edit, a message, a
/// panel, its own file — is refused as it's answered (`keys_refused`), and its card says so (`describe`).
pub fn invalid(declared: &[Perm]) -> Option<String> {
    if let Some(why) = declared.iter().copied().find_map(invalid_one) {
        return Some(why);
    }
    let other = declared.iter().find(|p| **p != Perm::Keys);
    match (hears_keys(declared), other) {
        (true, Some(p)) => Some(format!(
            "it asks to hear keys and for “{}” — a plugin that hears keys asks for nothing else: it reads, \
             connects to and runs nothing (what it hears stays in tarae)",
            shown(&key(*p))
        )),
        _ => None,
    }
}

/// Whether a plugin that declared `declared` hears keys (`keys`) — and so writes nothing of its own.
pub fn hears_keys(declared: &[Perm]) -> bool {
    declared.contains(&Perm::Keys)
}

/// Why a declared permission isn't one tarae takes, if it isn't: a program is a plain name, a variable a
/// plain name, a pattern absolute (or from `~/`) with no `.` or `..` in it — so what the card shows is what
/// it covers.
pub fn invalid_one(p: Perm) -> Option<String> {
    let plain = |s: &str, extra: &str| {
        !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    let why = match p {
        Perm::Run(prog) if prog.len() > 64 || !plain(prog, "._+-") || prog.starts_with('.') => {
            "a program is a plain name (no path)"
        }
        Perm::Env(name)
            if name.len() > 128 || !plain(name, "_") || name.starts_with(|c: char| c.is_ascii_digit()) =>
        {
            "an environment variable's name"
        }
        Perm::Read(pat) if pat.len() > 200 => "a pattern up to 200 characters",
        Perm::Read(pat) if !(pat.starts_with('/') || pat.starts_with("~/")) => "a pattern starts at / or ~/",
        Perm::Read(pat) if pat.chars().any(|c| c.is_control() || bidi(c)) => "no control characters",
        Perm::Read(pat) if pat.split('/').skip(1).any(|c| matches!(c, "" | "." | "..")) => {
            "no `.`, `..` or empty parts in a pattern"
        }
        _ => return None,
    };
    Some(format!("its permission “{}” isn't valid: {why}", shown(&key(p))))
}

/// A request that needs `need`, from a plugin that didn't declare it.
pub fn not_asked(need: Perm) -> String {
    format!("it didn't ask to {} (permissions())", describe(need))
}

/// A read, an open or a buffer of its own, from a plugin with no `read` pattern and no `project`.
pub const NO_READS: &str = "it didn't ask to read files (permissions())";

/// An open of a file its grant doesn't cover (#457).
pub fn may_not_open(path: &str) -> String {
    format!("it may not open “{}”: not a file it may read (permissions())", shown(path))
}

/// Why `program` (a `spawn`'s) isn't a program's name, if it isn't — before its permission is looked at.
pub fn not_a_program(program: &str) -> Option<String> {
    (program.contains('/') || program.chars().any(char::is_control))
        .then(|| format!("“{}” isn't a program name", shown(program)))
}

/// A `read-file` of a path that's neither absolute nor from `~/`.
pub fn not_absolute(path: &str) -> String {
    format!("“{}”: an absolute path, or one from ~/", shown(path))
}

/// Why a `find`'s pattern isn't one from the project's folder, if it isn't.
pub fn find_pattern(pattern: &str) -> Option<String> {
    (pattern.starts_with('/') || pattern.split('/').any(|c| c == "..") || pattern.len() > 200)
        .then(|| format!("“{}”: a pattern from the project's folder", shown(pattern)))
}

/// Why a `write-own` of `len` bytes as `name` isn't one: a plain relative name, up to 1 MB.
pub fn own_file(name: &str, len: usize) -> Result<(), String> {
    let plain = !name.is_empty()
        && name.len() <= 200
        && Path::new(name).components().all(|c| matches!(c, Component::Normal(_)))
        && !name.chars().any(|c| c.is_control() || bidi(c));
    match (plain, len <= 1 << 20) {
        (false, _) => Err(format!("“{}” isn't a plain name in its folder", shown(name))),
        (_, false) => Err("over 1 MB".into()),
        (true, true) => Ok(()),
    }
}

/// Why host:port isn't one to connect to, if it isn't — by its form; the hosts never reached are tarae's own list.
pub fn connect_form(host: &str, port: u64) -> Option<String> {
    let ok = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':');
    (host.is_empty() || host.len() > 253 || !host.chars().all(ok) || !(1..=65535).contains(&port))
        .then(|| format!("“{}” port {port} isn't a host to connect to", shown(host)))
}

/// A connection's host:port, as approvals keep it (an IPv6 address in brackets).
pub fn target(host: &str, port: u64) -> String {
    if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

/// The card asking about a connection to a host it hasn't used before: its title and its question.
pub fn ask_connect(plugin: &str, target: &str) -> (String, String) {
    (format!("Plugin {plugin} wants to connect"), format!("to {target}\nAllow it, from now on?"))
}

/// The card asking about a file outside its read patterns: its title and its question.
pub fn ask_read(plugin: &str, path: &Path) -> (String, String) {
    (
        format!("Plugin {plugin} wants to read a file"),
        format!(
            "{}\n(outside the files it asked for) Allow it, from now on?",
            shown(&path.display().to_string())
        ),
    )
}

/// `~/x` → home/x (`home`: where the caller says it is); `..` and `.` taken out, lexically. None if it isn't
/// absolute after that.
pub fn expand(path: &str, home: Option<&Path>) -> Option<PathBuf> {
    let p = match path.strip_prefix("~/") {
        Some(rest) => home?.join(rest),
        None if path == "~" => home?.to_path_buf(),
        None => PathBuf::from(path),
    };
    if !p.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    Some(out)
}

/// `*` and `?` in one name.
pub fn name_matches(pat: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pat.chars().collect(), name.chars().collect());
    // (a star's last place, and where in the name it had got to)
    let (mut i, mut j, mut star) = (0, 0, None::<(usize, usize)>);
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some((i, j));
            i += 1;
        } else if let Some((si, sj)) = star {
            i = si + 1;
            j = sj + 1;
            star = Some((si, sj + 1));
        } else {
            return false;
        }
    }
    p[i..].iter().all(|&c| c == '*')
}

/// Whether `path` is one `pattern` covers (`**`: any folders, none too).
pub fn path_matches(pattern: &str, path: &Path, home: Option<&Path>) -> bool {
    expand(pattern, home).is_some_and(|pat| path_matches_in(&pat, path))
}

/// `path_matches`, of a pattern already expanded.
pub fn path_matches_in(pat: &Path, path: &Path) -> bool {
    let names =
        |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
    fn go(p: &[String], n: &[String]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((first, rest)) if first == "**" => (0..=n.len()).any(|k| go(rest, &n[k..])),
            Some((first, rest)) => {
                n.split_first().is_some_and(|(name, more)| name_matches(first, name) && go(rest, more))
            }
        }
    }
    go(&names(pat), &names(path))
}

/// What a plugin's grant reaches, held against a path with no disk touched: its `read` patterns — each as written
/// (expanded: `expand`) and with its folders resolved (None: they don't resolve — it covers nothing resolved) —, the
/// project (a real path), the files allowed on its read card. A path's own real form comes from the caller.
#[derive(Clone, Debug, Default)]
pub struct Reach {
    pub patterns: Vec<(Option<PathBuf>, Option<PathBuf>)>,
    pub project: Option<PathBuf>,
    pub files: Vec<String>,
}

impl Reach {
    /// Whether it reaches `real` — and `lexical` as written, if there is one (a buffer's path was made real as it
    /// opened: `real` is all there is).
    pub fn reaches(&self, lexical: Option<&Path>, real: &Path) -> bool {
        let covered = |at: &Path| {
            self.patterns.iter().any(|(pat, _)| pat.as_ref().is_some_and(|p| path_matches_in(p, at)))
        };
        // (a pattern whose folders didn't resolve covers nothing resolved)
        let covered_real =
            |at: &Path| self.patterns.iter().any(|(_, r)| r.as_ref().is_some_and(|r| path_matches_in(r, at)));
        // The project's files, if it may read them — where they are, and where they lead; not ones named like
        // keys and tokens (`.env`, `*.pem` — `secretnames`): those are asked about like any file outside it
        let secret = lexical.is_some_and(secretnames::named_secret) || secretnames::named_secret(real);
        let in_project = !secret
            && self
                .project
                .as_ref()
                .is_some_and(|root| lexical.is_none_or(|l| l.starts_with(root)) && real.starts_with(root));
        let allowed_before = self.files.iter().any(|f| Path::new(f) == real);
        (lexical.is_none_or(covered) && covered_real(real)) || in_project || allowed_before
    }

    /// #457's rule (a): what it reaches, never a file named like a key, whatever its patterns say.
    ///
    /// The rule is `secretnames::named_secret` itself, on purpose stricter than tarae's runs' `sandbox::key_named`:
    /// that one's exceptions (a toolchain's installs and packages, tools' output folders) are there so a run can work
    /// inside a package or a build's output; a plugin is handed a buffer, and needs none of that. Don't unify them.
    pub fn covers(&self, lexical: Option<&Path>, real: &Path) -> bool {
        let secret = |p: &Path| secretnames::named_secret(p);
        !lexical.is_some_and(secret) && !secret(real) && self.reaches(lexical, real)
    }
}

/// An action's kind — one a variant of the WIT's `action`, so a rule on actions is written once for every copy of
/// the bindings (tarae's decoder, the SDK's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Status,
    Success,
    Warning,
    Error,
    ReplaceSelections,
    Edits,
    Select,
    Run,
    Open,
    StatusBar,
    Spawn,
    Connect,
    Send,
    Stop,
    ReadFile,
    ReadEnv,
    Timer,
    Panel,
    Prompt,
    Show,
    Append,
    Hints,
    Side,
    Pick,
    Badge,
    Find,
    WriteOwn,
    OfferInstall,
    Overlay,
}

/// What a plugin that hears keys may not answer with — the kind, in words; None: it may (§7 1, (c)). Only what shows
/// on your screen and nowhere else is let through: its overlay (Claude's level never holds one), its timers and a
/// stop (of nothing: it runs no program). The rest would put what it heard where it's read again: your text (saved,
/// sent to other plugins, read by Claude), the cursor (Claude's context, the next plugin's input), a message (the
/// log, Claude's `ui`), status text, a panel, a side tab, a badge, hints, a buffer, a pick, a prompt, a path opened
/// (recent files, Claude's open files), a read (the path it picks is the filesystem's lookup, a server's on a network
/// mount — the load refuses its patterns too: `invalid`), a program, a connection, a file of its own, an install. A
/// new action is decided here (the match is whole).
pub fn keys_refused(a: Act) -> Option<&'static str> {
    Some(match a {
        Act::Overlay | Act::Timer | Act::Stop => return None,
        Act::ReadFile | Act::ReadEnv | Act::Find => "a read",
        Act::ReplaceSelections | Act::Edits => "an edit",
        Act::Select | Act::Run => "a move",
        Act::Status | Act::Success | Act::Warning | Act::Error => "a message",
        Act::StatusBar => "status text",
        Act::Open => "a file opened",
        Act::Spawn => "a program",
        Act::Connect | Act::Send => "a connection",
        Act::Panel => "a panel",
        Act::Prompt => "a prompt",
        Act::Show | Act::Append => "a buffer",
        Act::Hints => "hints",
        Act::Side => "a side tab",
        Act::Pick => "a pick",
        Act::Badge => "a badge",
        Act::WriteOwn => "a file of its own",
        Act::OfferInstall => "an install",
    })
}

/// The failure a request refused by `keys_refused` (`what`) gets back.
pub fn keys_refusal(what: &str) -> String {
    format!("{what}: a plugin that hears keys shows what it hears in its overlay only")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_cover_what_they_say() {
        let home = Path::new("/home/u");
        let m = |pat: &str, path: &str| path_matches(pat, Path::new(path), Some(home));
        assert!(m("~/.kube/**", "/home/u/.kube/config"));
        assert!(m("~/.kube/**", "/home/u/.kube/cache/discovery/x.json"));
        assert!(!m("~/.kube/**", "/home/u/.ssh/id_rsa"));
        assert!(m("~/.kube/*.yaml", "/home/u/.kube/dev.yaml"));
        assert!(!m("~/.kube/*.yaml", "/home/u/.kube/a/dev.yaml"), "* stays in one folder");
        assert!(m("/etc/k8s/admin.conf", "/etc/k8s/admin.conf"));
        assert!(name_matches("k?b*.conf", "kube-admin.conf") && !name_matches("*.conf", "x.yaml"));
        // `..` doesn't climb out
        assert_eq!(expand("~/.kube/../.ssh/id_rsa", Some(home)), Some(PathBuf::from("/home/u/.ssh/id_rsa")));
        assert!(!m("~/.kube/**", "/home/u/.ssh/id_rsa"));
        assert_eq!(expand("relative/path", Some(home)), None);
        assert_eq!(expand("~/x", None), None, "no home, no ~");
    }

    /// The project reaches its own files but those named like keys; a read pattern reaches one named so, and #457's
    /// `covers` never does.
    #[test]
    fn the_project_reaches_its_files_but_keys_and_covers_none_named_so() {
        let reach = Reach {
            patterns: vec![(Some("/etc/app/**".into()), Some("/etc/app/**".into()))],
            project: Some("/p".into()),
            files: vec![],
        };
        fn at(p: &str) -> (Option<&Path>, &Path) {
            (Some(Path::new(p)), Path::new(p))
        }
        let (l, r) = at("/p/src/main.rs");
        assert!(reach.reaches(l, r) && reach.covers(l, r));
        let (l, r) = at("/p/.env");
        assert!(!reach.reaches(l, r));
        let (l, r) = at("/etc/app/server.pem");
        assert!(reach.reaches(l, r) && !reach.covers(l, r));
        let (l, r) = at("/q/main.rs");
        assert!(!reach.reaches(l, r));
    }

    #[test]
    fn a_plugin_that_hears_keys_asks_for_nothing_else() {
        assert_eq!(invalid(&[Perm::Keys]), None);
        let why = invalid(&[Perm::Keys, Perm::Network]).unwrap();
        assert!(why.starts_with("it asks to hear keys and for “network”"), "{why}");
        assert!(invalid(&[Perm::Run("/bin/sh")]).unwrap().contains("a program is a plain name"));
        assert_eq!(keys_refused(Act::Overlay), None);
        assert_eq!(keys_refused(Act::Spawn), Some("a program"));
    }
}
