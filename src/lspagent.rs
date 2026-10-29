//! #228: an agent a language server needs loaded into it — a jar its JVM runs with (`-javaagent:`), what makes the
//! code its compiler sees (Lombok's annotations for jdtls: `@Slf4j`'s `log` is made as the code compiles). Its pack's
//! `[[server.<name>.agents]]`: whether the project uses it (`files` under the server's root, `uses` in their text),
//! the version its build names (`version`), where its jars are (`jars`: in a home — the build tools' caches —, `{v}`
//! a version's folder) and the argument that loads one (`arg`, `{jar}` its path).
//!
//! An agent is code the server runs, so a server with one waits for the repository's code trust (`codetrust.rs`,
//! G13 — the pack can't give one to a server without `runs-project-code`), and it's found on a worker only once that
//! trust is there. A jar is taken only where it's yours (`project::mine`, by the opened file's real path) and where no
//! sandboxed run may write (`sandbox::write_places`): a jar a run of Claude's put there would run outside the
//! sandbox. None found — the project doesn't use it, or its build never downloaded one — the server starts as it
//! would, nothing said.

use std::io::Read as _;
use std::path::{Path, PathBuf};

/// A pack's agent.
#[derive(Debug)]
pub struct Agent {
    /// What it's called where it's said (`Lombok found — jdtls restarts with it`).
    pub name: String,
    /// Files under the server's root (`*` within a segment) whose text says whether the project uses it.
    pub files: Vec<String>,
    pub uses: regex::Regex,
    /// Where those files name its version: each with a `v` group, the first that matches taken.
    pub version: Vec<regex::Regex>,
    /// Its jars, from a home (`~/`, `{gradle}/`), `{v}` a whole segment — a version's folder — and maybe in the
    /// file's name.
    pub jars: Vec<String>,
    /// The oldest version taken.
    pub at_least: Option<String>,
    /// The argument that loads it — `{jar}` its path.
    pub arg: String,
    /// Left out when the server's own arguments hold one with this in it (an agent of yours).
    pub unless: Option<String>,
}

/// A build file read for its words at most (they're kilobytes).
const READ_MAX: u64 = 1 << 20;

/// What a look found (`find`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// Each agent loaded: its name, the argument that loads it.
    pub loaded: Vec<(String, String)>,
    /// The agents the project uses whose jar isn't there (yet: on a fresh clone the server's own import of the
    /// build downloads it) — their names.
    pub missing: Vec<String>,
}

impl Found {
    /// The arguments that load what was found.
    pub fn args(&self) -> Vec<String> {
        self.loaded.iter().map(|(_, a)| a.clone()).collect()
    }
}

/// What `root`'s server, its own arguments `args`, starts with besides: each agent of `agents` the project uses and
/// whose jar is found — `place` puts a jar's home where it is (`sandboxpack::Vars::place`), `writes` are where a
/// sandboxed run may write (None: runs aren't sandboxed), `me` whose jars are taken. Reads the disk: a worker's.
pub fn find(
    agents: &[Agent],
    args: &[String],
    root: &Path,
    place: &dyn Fn(&str) -> Option<PathBuf>,
    writes: Option<&[PathBuf]>,
    me: crate::project::Me,
) -> Found {
    let mut out = Found::default();
    for a in agents {
        if a.unless.as_ref().is_some_and(|u| args.iter().any(|x| x.contains(u.as_str()))) {
            continue;
        }
        let Some(named) = uses(a, root) else { continue };
        match jar(a, named.as_deref(), place, writes, me) {
            Some(jar) => out.loaded.push((a.name.clone(), a.arg.replace("{jar}", &jar.to_string_lossy()))),
            None => out.missing.push(a.name.clone()),
        }
    }
    out
}

/// Whether the project at `root` uses `a` — Some, with the version its build names where one does.
fn uses(a: &Agent, root: &Path) -> Option<Option<String>> {
    let (mut used, mut named) = (false, None);
    for f in &a.files {
        let mut paths = crate::readonly::glob_paths(&root.join(f));
        crate::sorting::sort(&mut paths);
        for text in paths.iter().filter_map(|p| text_of(p)) {
            if !a.uses.is_match(&text) {
                continue;
            }
            used = true;
            if named.is_none() {
                let v = |r: &regex::Regex| Some(r.captures(&text)?.name("v")?.as_str().trim().to_string());
                named = a.version.iter().find_map(v);
            }
        }
    }
    used.then_some(named)
}

/// A plain file's text, up to `READ_MAX` — opened without waiting (a FIFO put there would hold the worker forever).
fn text_of(path: &Path) -> Option<String> {
    let o = crate::paths::open_read(path).ok()?;
    if !o.meta.is_file() || o.meta.len() > READ_MAX {
        return None;
    }
    let mut bytes = Vec::new();
    o.file.take(READ_MAX).read_to_end(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}

/// `a`'s jar: the version `named` where its jar is there, else the newest — a release's (dotted numbers) at least
/// `at-least`, from the first of `jars` that has it, yours and in no place a run writes. Its real path.
fn jar(
    a: &Agent,
    named: Option<&str>,
    place: &dyn Fn(&str) -> Option<PathBuf>,
    writes: Option<&[PathBuf]>,
    me: crate::project::Me,
) -> Option<PathBuf> {
    let floor = a.at_least.as_deref().and_then(numbers);
    let mut found: Vec<(Vec<u64>, String, PathBuf)> = Vec::new();
    for pattern in &a.jars {
        let Some(at) = place(pattern) else { continue };
        let parts: Vec<String> =
            at.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let Some(i) = parts.iter().position(|p| p == "{v}") else { continue };
        let glob = PathBuf::from(at.to_string_lossy().replace("{v}", "*"));
        let mut hits = crate::readonly::glob_paths(&glob);
        crate::sorting::sort(&mut hits);
        for hit in hits {
            let Some(v) = hit.components().nth(i).map(|c| c.as_os_str().to_string_lossy().into_owned())
            else {
                continue;
            };
            // (the version's folder and the file's name agree: `1.18.30/…/lombok-1.18.30.jar`)
            let mut with_v = parts.iter().zip(hit.components()).filter(|(p, _)| p.contains("{v}"));
            let same = with_v.all(|(p, c)| c.as_os_str() == p.replace("{v}", &v).as_str());
            let Some(n) = numbers(&v).filter(|n| same && floor.as_ref().is_none_or(|f| n >= f)) else {
                continue;
            };
            if found.iter().any(|(_, w, _)| *w == v) {
                continue;
            }
            if let Some(real) = taken(&hit, writes, me) {
                found.push((n, v, real));
            }
        }
    }
    let named = named.and_then(|v| found.iter().position(|(_, w, _)| w == v));
    let newest = found.iter().enumerate().max_by(|(_, x), (_, y)| x.0.cmp(&y.0)).map(|(i, _)| i);
    named.or(newest).map(|i| found.swap_remove(i).2)
}

/// `path`'s real path, where it's a plain file of yours (and its folder) that no sandboxed run may write.
fn taken(path: &Path, writes: Option<&[PathBuf]>, me: crate::project::Me) -> Option<PathBuf> {
    let o = crate::paths::open_read(path).ok()?;
    let mine = o.meta.is_file() && crate::project::mine(&o.real, &o.meta, me).is_ok();
    let written = writes.is_some_and(|w| w.iter().any(|p| o.real.starts_with(p)));
    (mine && !written).then_some(o.real)
}

/// `find` with this process's home and variables, the homes placed as the sandbox's packs place them (`{gradle}`:
/// `GRADLE_USER_HOME`, else `~/.gradle`) — or with `stand_in` as the home and no variables (a test's, a
/// simulation's). `writes`: `Editor::workspaces_writes`'. A worker's.
pub fn found(
    agents: &[Agent],
    args: &[String],
    root: &Path,
    writes: Option<Vec<PathBuf>>,
    stand_in: Option<&Path>,
) -> Found {
    let var = |n: &str| match stand_in {
        Some(home) => (n == "HOME").then(|| home.to_path_buf()),
        None => std::env::var_os(n).filter(|v| !v.is_empty()).map(PathBuf::from),
    };
    let Some(home) = var("HOME").filter(|h| h.is_absolute()) else { return Found::default() };
    let vars =
        crate::sandboxpack::Vars::new(crate::sandboxpack::get(), &home, &var, crate::sandbox::Os::HOST);
    find(agents, args, root, &|p| vars.place(p), writes.as_deref(), crate::project::Me::now())
}

/// `1.18.30` → [1, 18, 30]; None: not a release's dotted numbers (`edge-SNAPSHOT`, `1.18.31-SNAPSHOT`).
pub fn numbers(v: &str) -> Option<Vec<u64>> {
    v.split('.').map(|p| p.parse().ok()).collect()
}

#[cfg(test)]
mod tests;
