//! The cluster a `kubectl` run outside the sandbox reaches (#182): its kubeconfig's current context, said on the
//! outside card so a `y` knows which cluster. Read by tarae, the user's side, on a worker — never by a run, and
//! only that one name is said (never a credential, a server, a user). Told only where the line is one command whose
//! program is kubectl itself (its own `KUBECONFIG=` aside — no wrapper, `sudo`, `env` or script in front) running
//! one of kubectl's own verbs but `config`: `--context` in it; else the
//! files `--kubeconfig` names, or `KUBECONFIG`'s list (the command's own `KUBECONFIG=` first), or `~/.kube/config`
//! — the first of them that sets one. Any other line can't be told, and says why: more than one command (what runs
//! besides kubectl may change the kubeconfig or the context — no chasing them one by one), `kubectl config …`, a
//! plugin's verb (`kubectl ctx` runs `kubectl-ctx`), an option before the verb tarae doesn't know, the cluster named
//! itself (`--server`). The line is information, never what `y` decides, and it's read again at `y`
//! (`tools::kube_recheck`).
//! A file is read only where it's a plain file (never a terminal, a FIFO, a device), as text, a line at a time: a
//! top-level `current-context:` only (no YAML parser in the core) — one it can't read so is said, never guessed. A
//! kubeconfig Claude's runs can write (in the project, the temp folders) is said so: what it names is Claude's.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The program whose card says it.
const PROGRAM: &str = "kubectl";

/// A kubeconfig read at most this far (a real one is a few KB).
const READ_MAX: usize = 1 << 20;

/// kubectl's own verbs (`kubectl help`, 1.3x) — any other runs a plugin, `kubectl-<verb>` on PATH.
const VERBS: [&str; 45] = [
    "alpha",
    "annotate",
    "api-resources",
    "api-versions",
    "apply",
    "attach",
    "auth",
    "autoscale",
    "certificate",
    "cluster-info",
    "completion",
    "config",
    "cordon",
    "cp",
    "create",
    "debug",
    "delete",
    "describe",
    "diff",
    "drain",
    "edit",
    "events",
    "exec",
    "explain",
    "expose",
    "get",
    "help",
    "kustomize",
    "label",
    "logs",
    "options",
    "patch",
    "plugin",
    "port-forward",
    "proxy",
    "replace",
    "rollout",
    "run",
    "scale",
    "set",
    "taint",
    "top",
    "uncordon",
    "version",
    "wait",
];

/// kubectl's global options that take a value (`kubectl options`) — the word after one is that value, not its verb.
const VALUED: [&str; 25] = [
    "--as",
    "--as-group",
    "--as-uid",
    "--cache-dir",
    "--certificate-authority",
    "--client-certificate",
    "--client-key",
    "--cluster",
    "--context",
    "--kubeconfig",
    "--log-flush-frequency",
    "-n",
    "--namespace",
    "--password",
    "--profile",
    "--profile-output",
    "--request-timeout",
    "-s",
    "--server",
    "--tls-server-name",
    "--token",
    "--user",
    "--username",
    "-v",
    "--v",
];

/// kubectl's global options that take none.
const BARE: [&str; 6] = [
    "--insecure-skip-tls-verify",
    "--match-server-version",
    "--warnings-as-errors",
    "--disable-compression",
    "--help",
    "-h",
];

/// Options that name the cluster or its server past the context.
const PAST_CONTEXT: [&str; 3] = ["--cluster", "--server", "-s"];

/// Where a run's line is read for its card: the folder it runs in, the user's `KUBECONFIG` and home, and where
/// Claude's runs write (`Editor::workspaces_writes` — None: they aren't sandboxed, anywhere).
pub struct Place<'a> {
    pub cwd: &'a Path,
    pub env: Option<&'a OsStr>,
    pub home: Option<&'a Path>,
    pub writes: Option<&'a [PathBuf]>,
}

/// The card's line — information, never what `y` decides: faint where every kubectl's context is named from files
/// no run of Claude's writes, else in warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub line: String,
    /// Every kubectl's context named (not "can't be told").
    pub named: bool,
    /// A kubeconfig it reads is Claude's to write: what it names is Claude's.
    pub written: bool,
}

impl Said {
    /// Whether the card draws it in the warning color: it can't be told, or it's Claude's to choose.
    pub fn warns(&self) -> bool {
        !self.named || self.written
    }
}

/// Whether `cmd` runs kubectl (a wrapper's too) — its outside card reads the kubeconfig first.
pub fn runs(cmd: &str) -> bool {
    crate::cardwarn::runs_any(cmd, &[PROGRAM.to_string()], &crate::sandboxpack::get().wrappers)
}

/// One kubectl's context, named.
struct Named {
    name: String,
    /// Where it's from (`~/.kube/config`, `its --context`).
    from: String,
    /// A kubeconfig it reads that Claude's runs can write.
    written: Option<PathBuf>,
}

/// The card's line for `cmd` run at `p`, outside the sandbox: `kubectl's context: prod-eks (~/.kube/config)`, or why
/// it can't be told — None where `cmd` runs no kubectl. Reads files: on a worker.
pub fn said(cmd: &str, p: &Place) -> Option<Said> {
    let packs = crate::sandboxpack::get();
    let commands: Vec<(Vec<String>, Vec<String>)> =
        crate::cardwarn::command_words(cmd, &[PROGRAM.to_string()], &packs.wrappers)
            .into_iter()
            .filter(|(set, words)| !set.is_empty() || !words.is_empty())
            .collect();
    if !commands.iter().any(|(_, w)| w.first().is_some_and(|f| f == PROGRAM)) {
        return None;
    }
    let cant = |why: String| {
        let line = format!("kubectl's context: can't be told — {why}");
        Some(Said { line, named: false, written: false })
    };
    let [(set, words)] = commands.as_slice() else {
        return cant(format!("the line runs {} commands, not kubectl alone", commands.len()));
    };
    // (the line's program is kubectl itself, its own `KUBECONFIG=` aside: anything in front of it — a wrapper,
    // `sudo` (root's home, `KUBECONFIG` dropped), `env`, a script, another variable (`HOME=…` moves `~/.kube`) —
    // may change what it reads or who runs it; no list of harmless ones to keep)
    let assign = crate::cardwarn::assign;
    if let Some(w) = set.iter().find(|w| assign(w) != Some("KUBECONFIG")) {
        return cant(match assign(w) {
            Some(name) => format!("the line sets {name} for kubectl"),
            None => format!("kubectl runs under {w}, not as the line's own program"),
        });
    }
    // (set twice, the shell takes the last — one reading of the line is enough to say nothing settled)
    if set.len() > 1 {
        return cant("the line sets KUBECONFIG twice".into());
    }
    // (past `--`, the program kubectl runs: `kubectl exec p -- …`)
    let args = &words[1..];
    let args = &args[..args.iter().position(|w| w == "--").unwrap_or(args.len())];
    match verb(args) {
        Err(why) => return cant(why),
        Ok(Some("config")) => return cant("kubectl config reads or changes the kubeconfig itself".into()),
        Ok(Some(v)) if !VERBS.contains(&v) => {
            return cant(format!("kubectl {v} runs a plugin (kubectl-{v}), not kubectl itself"));
        }
        Ok(_) => {}
    }
    let e = one(set, args, p);
    let (named, written) = (e.is_ok(), e.as_ref().is_ok_and(|n| n.written.is_some()));
    Some(Said { line: format!("kubectl's context: {}", shown(&e)), named, written })
}

/// kubectl's verb in `args` (its words after the program, up to `--`): the first that's no option nor an option's
/// value — None: none (`kubectl --help`). Err: an option before it whose taking a value tarae doesn't know.
fn verb(args: &[String]) -> Result<Option<&str>, String> {
    let mut i = 0;
    while let Some(w) = args.get(i) {
        if !w.starts_with('-') {
            return Ok(Some(w));
        }
        let name = w.split_once('=').map_or(w.as_str(), |(n, _)| n);
        // (`--x=v`, a bare one, a valued one with its value; `-nprod`: a short one's value joined)
        i += match () {
            _ if w.contains('=') || BARE.contains(&name) => 1,
            _ if VALUED.contains(&name) => 2,
            _ if !name.starts_with("--") && name.get(..2).is_some_and(|s| VALUED.contains(&s)) => 1,
            _ => return Err(format!("{w} before its verb: whether it takes a value isn't known")),
        };
    }
    Ok(None)
}

/// One kubectl's context as the card says it.
fn shown(e: &Result<Named, String>) -> String {
    let n = match e {
        Ok(n) => n,
        Err(why) => return format!("can't be told — {why}"),
    };
    match &n.written {
        None => format!("{} ({})", n.name, n.from),
        Some(w) if crate::allow::tilde(w) == n.from => {
            format!("{} ({} — Claude's runs can write it)", n.name, n.from)
        }
        Some(w) => format!("{} ({} — Claude's runs can write {})", n.name, n.from, crate::allow::tilde(w)),
    }
}

/// The context of one kubectl, its words after the program up to `--` `args`, the `NAME=value` words before it
/// `set` — nothing having run before it in the line.
fn one(set: &[String], args: &[String], p: &Place) -> Result<Named, String> {
    // (given twice, which one kubectl takes isn't `flag`'s first — as `KUBECONFIG=` twice, one rule for every source)
    if let Some(o) = ["--context", "--kubeconfig"].iter().find(|o| given(args, o) > 1) {
        return Err(format!("its {o} is given twice"));
    }
    if let Some(o) = PAST_CONTEXT.iter().find(|o| flag(args, o).is_some()) {
        return Err(format!("its {o} names the cluster itself"));
    }
    let at = |f: &str| -> Result<PathBuf, String> {
        match (f.strip_prefix("~/"), p.home) {
            _ if f.contains(['$', '`']) => Err(format!("{f}: a variable's — not read")),
            (Some(rest), Some(h)) => Ok(h.join(rest)),
            _ if Path::new(f).is_absolute() => Ok(PathBuf::from(f)),
            _ => Ok(p.cwd.join(f)),
        }
    };
    let listed = |v: &str| v.split(':').filter(|f| !f.is_empty()).map(at).collect::<Result<Vec<_>, _>>();
    let line_env = set.iter().find_map(|w| w.strip_prefix("KUBECONFIG="));
    let files: Vec<PathBuf> = match (flag(args, "--kubeconfig"), line_env, p.env.and_then(OsStr::to_str)) {
        (Some(f), ..) => vec![at(&f)?],
        (None, Some(v), _) | (None, None, Some(v)) if !listed(v)?.is_empty() => listed(v)?,
        _ => p.home.map(|h| h.join(".kube/config")).into_iter().collect(),
    };
    let written = files.iter().find(|f| runs_write(f, p.writes)).cloned();
    if let Some(name) = flag(args, "--context") {
        if name.contains(['$', '`']) {
            return Err("its --context is a variable's".into());
        }
        return Ok(Named { name, from: "its --context".into(), written });
    }
    let mut why = Vec::new();
    for f in &files {
        match current(f) {
            Ok(Some(name)) => return Ok(Named { name, from: crate::allow::tilde(f), written }),
            Ok(None) => why.push(format!("{}: none set", crate::allow::tilde(f))),
            Err(e) => why.push(format!("{}: {e}", crate::allow::tilde(f))),
        }
    }
    Err(match why.is_empty() {
        true => "no kubeconfig".into(),
        false => why.join("; "),
    })
}

/// Whether a run of Claude's can write `f` — it, or a link in its place: where it leads or the folder it's in is
/// in a place runs write (`writes`; None: anywhere).
fn runs_write(f: &Path, writes: Option<&[PathBuf]>) -> bool {
    let Some(writes) = writes else { return true };
    let within = |w: &PathBuf| {
        crate::paths::inside(w, f).is_some()
            || f.parent().is_some_and(|d| crate::paths::inside(w, d).is_some())
    };
    writes.iter().any(within)
}

/// The value of option `name` in `words` (`--context x`, `--context=x`) — kubectl's own, before a `--` (the words
/// after it are the program kubectl runs: `kubectl exec p -- ls -s`).
fn flag(words: &[String], name: &str) -> Option<String> {
    let eq = format!("{name}=");
    let words = &words[..words.iter().position(|w| w == "--").unwrap_or(words.len())];
    words.iter().enumerate().find_map(|(i, w)| match w.strip_prefix(&eq) {
        Some(v) => Some(v.to_string()),
        None if w == name => words.get(i + 1).cloned(),
        None => None,
    })
}

/// How many times option `name` is in `words`, either spelling, before a `--` (as `flag` reads them).
fn given(words: &[String], name: &str) -> usize {
    let eq = format!("{name}=");
    let words = &words[..words.iter().position(|w| w == "--").unwrap_or(words.len())];
    words.iter().filter(|w| *w == name || w.starts_with(&eq)).count()
}

/// The current context `path` sets: Ok(None) where it sets none (or an empty one), Err where it can't be read or
/// its line can't be read as plain text. A link is followed to its real path, as kubectl does (a kubeconfig kept
/// with someone's dotfiles), and only a plain file there is opened (`beneath::Dir`: through no link, non-blocking,
/// its size from the open descriptor) — never a terminal, a FIFO or a device: one would take the user's keys or
/// hold the worker for ever.
fn current(path: &Path) -> Result<Option<String>, String> {
    use crate::beneath::{Dir, Kind};
    let real = std::fs::canonicalize(path).map_err(|e| io(&e))?;
    let (Some(dir), Some(name)) = (real.parent(), real.file_name()) else {
        return Err("not a file — not read".into());
    };
    let dir = Dir::root(dir).map_err(|e| io(&e))?;
    match dir.kind(name).map_err(|e| io(&e))? {
        Kind::File => {}
        Kind::Folder => return Err("a folder — not read".into()),
        _ => return Err("not a plain file (a terminal, a FIFO, a device) — not read".into()),
    }
    let bytes = match dir.read_at_most(name, READ_MAX) {
        Ok(Some(b)) => b,
        Ok(None) => return Err("No such file or directory".into()),
        Err(e) => {
            return Err(match e.kind() {
                std::io::ErrorKind::FileTooLarge => "larger than 1 MB — not read".into(),
                std::io::ErrorKind::PermissionDenied => "Permission denied".into(),
                // (swapped since its kind was looked at — a link, a FIFO now)
                _ => "not a plain file — not read".into(),
            });
        }
    };
    current_in(&String::from_utf8(bytes).map_err(|_| "not read: not UTF-8 text")?)
}

/// The OS's words for `e`, its code left off (`Permission denied`).
fn io(e: &std::io::Error) -> String {
    let said = e.to_string();
    match e.raw_os_error() {
        Some(code) => said.strip_suffix(&format!(" (os error {code})")).unwrap_or(&said).to_string(),
        None => said,
    }
}

/// `current_in` of a file's text.
fn current_in(text: &str) -> Result<Option<String>, String> {
    let Some(rest) = text.lines().find_map(|l| l.strip_prefix("current-context:")) else {
        // (JSON is a kubeconfig too: its key not at a line's start)
        return match text.contains("\"current-context\"") {
            true => Err("not read: its current-context isn't a line of its own".into()),
            false => Ok(None),
        };
    };
    let v = rest.trim();
    let v = match v.find(" #") {
        Some(i) if !v.starts_with(['"', '\'']) => v[..i].trim_end(),
        _ => v,
    };
    let unquoted = match v.chars().next() {
        Some(q @ ('"' | '\'')) => v.strip_prefix(q).and_then(|s| s.strip_suffix(q)),
        _ => Some(v),
    };
    match unquoted {
        Some("") => Ok(None),
        Some(name) if !name.chars().any(char::is_control) && !name.contains(['"', '\\']) => {
            Ok(Some(name.into()))
        }
        _ => Err("not read: its current-context isn't a plain name".into()),
    }
}

#[cfg(test)]
mod tests;
