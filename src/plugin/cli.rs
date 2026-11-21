//! `tarae plugin new|dev|test|vendor|list|example` — starting a plugin, building it while it's worked on, testing it,
//! seeing what's installed, and the examples.
//!
//! A plugin out of this repository builds against the SDK and the rules as *this* tarae has them: `new` vendors them
//! into its `tarae/` (from the sources embedded at build time, `VENDOR`), and `test` fails while that copy differs,
//! or while cargo takes the SDK from anywhere else (`held`) — a test can't be made green by editing the rules it's
//! held to. `vendor` writes this tarae's copy again.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, anyhow, bail};

use super::abi::{Command, Plugin, Subscription};
use tarae_wasm::Limits;

const WIT: &str = include_str!("../../plugin-sdk/wit/tarae.wit");
const CARGO_TOML: &str = include_str!("../../plugin-sdk/template/Cargo.toml");
const LIB_RS: &str = include_str!("../../plugin-sdk/template/src/lib.rs");
/// wasm32, and cargo's build files inside the crate (`target/build`): a sandboxed run can build it (PD1's H2).
const CARGO_CONFIG: &str = include_str!("../../plugin-sdk/template/.cargo/config.toml");
const TARGET: &str = "wasm32-unknown-unknown";
/// Fuel for each call `loads` makes.
const FUEL: u64 = 100_000_000;

/// The SDK and the rules as this tarae was built with them: each file's place in a plugin's folder, and its text —
/// what `new` and `vendor` write (`vendored`) and `test` holds a vendored copy to. Every file of `plugin-sdk/sdk`
/// and `plugin-sdk/rules` (`the_vendored_sdk_is_the_whole_sdk`).
const VENDOR: &[(&str, &str)] = &[
    ("tarae/sdk/Cargo.toml", include_str!("../../plugin-sdk/sdk/Cargo.toml")),
    ("tarae/sdk/src/lib.rs", include_str!("../../plugin-sdk/sdk/src/lib.rs")),
    ("tarae/sdk/src/http.rs", include_str!("../../plugin-sdk/sdk/src/http.rs")),
    ("tarae/sdk/src/now.rs", include_str!("../../plugin-sdk/sdk/src/now.rs")),
    ("tarae/sdk/src/tls.rs", include_str!("../../plugin-sdk/sdk/src/tls.rs")),
    ("tarae/sdk/src/test.rs", include_str!("../../plugin-sdk/sdk/src/test.rs")),
    ("tarae/sdk/src/test/tests.rs", include_str!("../../plugin-sdk/sdk/src/test/tests.rs")),
    ("tarae/rules/Cargo.toml", include_str!("../../plugin-sdk/rules/Cargo.toml")),
    ("tarae/rules/src/lib.rs", include_str!("../../plugin-sdk/rules/src/lib.rs")),
    ("tarae/rules/src/secretnames.rs", include_str!("../../plugin-sdk/rules/src/secretnames.rs")),
    ("tarae/rules/src/secret-names.txt", include_str!("../../plugin-sdk/rules/src/secret-names.txt")),
    ("tarae/wit/tarae.wit", WIT),
    ("tarae/LICENSE-MIT", include_str!("../../plugin-sdk/LICENSE-MIT")),
    ("tarae/LICENSE-APACHE", include_str!("../../plugin-sdk/LICENSE-APACHE")),
];
/// Where the vendored copy goes in a plugin's folder.
const VENDORED: &str = "tarae";
/// The SDK's manifest's last lines in this repository: its own `[workspace]`, cut from the vendored copy — inside
/// the plugin's workspace a second root is refused.
const SDK_OWN_WORKSPACE: &str = "\n# Not part of tarae's workspace";
/// Rust's keywords, which cargo refuses as package names.
const KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate", "do", "dyn",
    "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop",
    "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return", "self", "static",
    "struct", "super", "trait", "true", "try", "type", "typeof", "unsafe", "unsized", "use", "virtual",
    "where", "while", "yield",
];

fn paint(code: &str, s: &str) -> String {
    format!("\x1b[{code}m{s}\x1b[0m")
}

pub fn cli(args: &[String]) -> anyhow::Result<()> {
    match args.first().map(String::as_str) {
        Some("new") => {
            let usage = "usage: tarae plugin new <dir> [--from <example>]";
            let (dir, from) = match &args[1..] {
                [dir] => (dir, None),
                [dir, flag, example] if flag == "--from" => (dir, Some(example.as_str())),
                _ => bail!(usage),
            };
            new(Path::new(dir), from)?;
            println!("{} {}", paint("1;32", "✓"), paint("1", &format!("created {dir}")));
            match from {
                Some(e) => println!("  {}  from the example {e} — edit src/lib.rs", paint("2", "·")),
                None => println!("  {}  :words and :title — edit src/lib.rs", paint("2", "·")),
            }
            println!("  {}  the API: REFERENCE.md", paint("2", "·"));
            println!("\n  cd {dir} && tarae plugin dev");
            println!("  {}", paint("2", "(builds on every save; your running tarae reloads it)"));
            if !target_installed() {
                println!("\n  {} rustup target add {TARGET}", paint("33", "first:"));
            }
            Ok(())
        }
        Some("dev") => dev(Path::new(args.get(1).map_or(".", String::as_str))),
        Some("test") => {
            let dirs: Vec<&Path> = match &args[1..] {
                [] => vec![Path::new(".")],
                more => more.iter().map(Path::new).collect(),
            };
            test(&dirs, &mut std::io::stdout().lock())
        }
        Some("vendor") => {
            let dir = Path::new(args.get(1).map_or(".", String::as_str));
            vendor(dir)?;
            println!(
                "{} {}",
                paint("1;32", "✓"),
                paint("1", &format!("this tarae's SDK in {}", dir.join(VENDORED).display()))
            );
            Ok(())
        }
        Some("list") | None => list(),
        // (to stdout as it is: a file to read, or to pipe)
        Some("example") => match args.get(1) {
            Some(name) => {
                print!("{}", super::examples::find(name).map_err(|e| anyhow!(e))?.lib);
                Ok(())
            }
            None => {
                for e in super::examples::EXAMPLES {
                    println!("{:<14} {}", e.name, e.idea);
                }
                Ok(())
            }
        },
        Some(other) => bail!(
            "unknown plugin command '{other}' — new <dir> [--from <example>] · dev [dir] · test [dir…] · vendor [dir] · \
             list · example [name]"
        ),
    }
}

/// The vendored copy as a plugin holds it: `VENDOR`, the SDK's manifest without its own `[workspace]`, and a README
/// saying what the folder is.
fn vendored() -> Vec<(&'static str, String)> {
    let mut files: Vec<(&str, String)> = VENDOR
        .iter()
        .map(|&(path, text)| match path {
            "tarae/sdk/Cargo.toml" => {
                (path, format!("{}\n", text.split(SDK_OWN_WORKSPACE).next().unwrap_or(text).trim_end()))
            }
            _ => (path, text.to_string()),
        })
        .collect();
    let readme = format!(
        "# tarae's SDK, vendored\n\nThe SDK (`sdk/`), the rules tarae holds a plugin to (`rules/`) and the plugin API \
         (`wit/tarae.wit`), as tarae {} has them — MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`), so your plugin \
         can take any licence.\n\nDon't edit them: `tarae plugin test` fails while this folder differs from the tarae \
         you run, and `tarae plugin vendor` writes that tarae's copy again.\n",
        env!("CARGO_PKG_VERSION")
    );
    files.push(("tarae/README.md", readme));
    files
}

/// Writes this tarae's vendored copy into the plugin in `dir` (`vendored`), and takes out of its `tarae/` what isn't
/// part of it.
fn vendor(dir: &Path) -> anyhow::Result<()> {
    if !dir.join("Cargo.toml").is_file() {
        bail!("no Cargo.toml in {} — tarae plugin new <name> starts one", dir.display());
    }
    let files = vendored();
    for extra in vendored_files(dir).into_iter().filter(|p| !files.iter().any(|(f, _)| f == p)) {
        std::fs::remove_file(dir.join(&extra)).with_context(|| format!("removing {extra}"))?;
    }
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("in the plugin folder"))?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// The files under the plugin's `tarae/` (but a `target/`), as paths from its folder, sorted.
fn vendored_files(dir: &Path) -> Vec<String> {
    fn walk(at: &Path, rel: &str, out: &mut Vec<String>) {
        for e in std::fs::read_dir(at).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = format!("{rel}/{name}");
            match e.file_type() {
                Ok(t) if t.is_dir() && name != "target" => walk(&e.path(), &rel, out),
                Ok(t) if !t.is_dir() => out.push(rel),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&dir.join(VENDORED), VENDORED, &mut out);
    crate::sorting::sort(&mut out);
    out
}

/// What of the plugin's vendored copy isn't this tarae's — a file changed, missing or added; none when it holds no
/// vendored copy (`held` says whether it must).
fn vendored_stale(dir: &Path) -> Vec<String> {
    if !dir.join(VENDORED).is_dir() {
        return Vec::new();
    }
    let files = vendored();
    let mut stale: Vec<String> = (files.iter())
        .filter(|(path, text)| std::fs::read(dir.join(path)).ok().as_deref() != Some(text.as_bytes()))
        .map(|(path, _)| path.to_string())
        .collect();
    stale.extend(vendored_files(dir).into_iter().filter(|p| !files.iter().any(|(f, _)| f == p)));
    stale
}

/// The SDK's two crates: the package, the crate name a dependency gives it, its folder in `tarae/` and `plugin-sdk/`.
const SDK_CRATES: [(&str, &str, &str); 2] =
    [("tarae-plugin", "tarae_plugin", "sdk"), ("tarae-plugin-rules", "tarae_plugin_rules", "rules")];

/// What keeps the plugin in `dir` from being held to this tarae's rules, one sentence each: the SDK and the rules
/// come from its vendored `tarae/`, as this tarae has them — or, a plugin inside tarae's repository, from that
/// repository's `plugin-sdk/`, as this tarae has it — and from nowhere else, as cargo resolves them (`sdk_crates`:
/// a path elsewhere, a `[patch]`, a `[replace]`, a renamed crate). `test` asks before and after cargo runs (a build
/// script runs before the SDK compiles). `host`: the tests' target, beside wasm32.
fn held(dir: &Path, host: &str) -> Vec<String> {
    let fix = format!("`tarae plugin vendor {}` writes this tarae's", dir.display());
    let mut wrong = Vec::new();
    let repository = tarae_repository(dir);
    let vendored = std::fs::canonicalize(dir.join(VENDORED)).ok().filter(|v| v.is_dir());
    if vendored.is_some() {
        let stale = vendored_stale(dir);
        if !stale.is_empty() {
            wrong.push(format!("its vendored SDK isn't this tarae's ({}) — {fix}", stale.join(", ")));
        }
    } else if repository.is_none() {
        wrong.push(format!("no vendored SDK ({VENDORED}/) — {fix}"));
    }
    let crates = match sdk_crates(dir, host) {
        Ok(crates) => crates,
        Err(why) => {
            wrong.push(format!("where its SDK comes from isn't known (cargo metadata): {why}"));
            return wrong;
        }
    };
    let mut from_repository = false;
    for (package, folder, from) in crates {
        let at = match from {
            Ok(at) => std::fs::canonicalize(&at).unwrap_or(at),
            Err(source) => PathBuf::from(source),
        };
        if vendored.as_ref().is_some_and(|v| at == v.join(folder)) {
            continue;
        }
        if repository.as_ref().is_some_and(|r| at == r.join("plugin-sdk").join(folder)) {
            from_repository = true;
            continue;
        }
        wrong.push(format!(
            "it takes {package} from {}, not its vendored {VENDORED}/{folder} — a path elsewhere, a [patch] or a \
             [replace] steps around the rules it's held to; {fix}",
            at.display()
        ));
    }
    if let Some(r) = repository.filter(|_| from_repository) {
        let stale = repository_stale(&r);
        if !stale.is_empty() {
            wrong.push(format!(
                "tarae's plugin-sdk/ in {} isn't this tarae's ({}) — run the tarae built from it",
                r.display(),
                stale.join(", ")
            ));
        }
    }
    wrong
}

/// The tarae repository the plugin in `dir` sits in: the nearest folder above it whose manifest is tarae's and which
/// holds `plugin-sdk/`.
fn tarae_repository(dir: &Path) -> Option<PathBuf> {
    let dir = std::fs::canonicalize(dir).ok()?;
    let is_tarae = |a: &Path| {
        a.join("plugin-sdk/sdk/Cargo.toml").is_file()
            && (std::fs::read_to_string(a.join("Cargo.toml")).ok())
                .and_then(|m| package_name(&m))
                .is_some_and(|n| n == "tarae")
    };
    dir.ancestors().skip(1).find(|a| is_tarae(a)).map(Path::to_path_buf)
}

/// What of a tarae repository's `plugin-sdk/` isn't this tarae's (`VENDOR`, each file where the repository has it).
fn repository_stale(repository: &Path) -> Vec<String> {
    (VENDOR.iter())
        .map(|(path, text)| (format!("plugin-sdk/{}", path.strip_prefix("tarae/").unwrap_or(path)), text))
        .filter(|(path, text)| std::fs::read(repository.join(path)).ok().as_deref() != Some(text.as_bytes()))
        .map(|(path, _)| path)
        .collect()
}

/// The SDK's crates the plugin in `dir` builds with, as cargo resolves them (`cargo metadata`): each package named
/// as one, and each dependency giving a crate one's name (a renamed package) — with its package name, its folder in
/// `SDK_CRATES`, and where it comes from: its manifest's folder, or the source it names (a registry, git). Only what
/// `test` builds for, `host` and wasm32 (else cargo fetches every platform's crates).
fn sdk_crates(dir: &Path, host: &str) -> Result<Vec<SdkCrate>, String> {
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--filter-platform", host, "--filter-platform", TARGET])
        .current_dir(dir)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .map_err(|e| format!("running cargo metadata: {e}"))?;
    if !out.status.success() {
        let said = errors(&String::from_utf8_lossy(&out.stderr));
        return Err(said.into_iter().next().unwrap_or_else(|| format!("it failed ({})", out.status)));
    }
    let m: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    Ok(sdk_crates_in(&m))
}

/// One of the SDK's crates as cargo resolves it: its package name and folder in `SDK_CRATES`, and its manifest's
/// folder or the source it names.
type SdkCrate = (&'static str, &'static str, Result<PathBuf, String>);

/// `sdk_crates`, from `cargo metadata`'s JSON.
fn sdk_crates_in(m: &serde_json::Value) -> Vec<SdkCrate> {
    let packages = m["packages"].as_array().map_or(&[][..], Vec::as_slice);
    let nodes = m["resolve"]["nodes"].as_array().map_or(&[][..], Vec::as_slice);
    let mut ids: Vec<(usize, &str)> = Vec::new();
    for (i, &(package, krate, _)) in SDK_CRATES.iter().enumerate() {
        let named = packages.iter().filter(|p| p["name"] == package).filter_map(|p| p["id"].as_str());
        let deps = nodes.iter().flat_map(|n| n["deps"].as_array().map_or(&[][..], Vec::as_slice));
        let renamed = deps.filter(|d| d["name"] == krate).filter_map(|d| d["pkg"].as_str());
        for id in named.chain(renamed) {
            if !ids.contains(&(i, id)) {
                ids.push((i, id));
            }
        }
    }
    let from = |id: &str| {
        let p = packages.iter().find(|p| p["id"] == id);
        match p.and_then(|p| p["source"].as_str()) {
            Some(source) => Err(source.to_string()),
            None => Ok(p
                .and_then(|p| p["manifest_path"].as_str())
                .and_then(|m| Path::new(m).parent())
                .map_or_else(|| PathBuf::from(id), Path::to_path_buf)),
        }
    };
    ids.into_iter().map(|(i, id)| (SDK_CRATES[i].0, SDK_CRATES[i].2, from(id))).collect()
}

/// A plugin crate from the template (or example `from`'s code), in `dir` (its last component is the name).
fn new(dir: &Path, from: Option<&str>) -> anyhow::Result<()> {
    let name =
        dir.file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow!("no name in {}", dir.display()))?;
    let files = crate_files(name, from).map_err(|e| anyhow!(e))?;
    // (the folders above it are yours, typed in your terminal: made as they are)
    let above = dir.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(above).map_err(|e| anyhow!("{}: {e}", above.display()))?;
    write_crate(above, Path::new(name), &files).map_err(|e| anyhow!(e))
}

/// Why `name` can't name a plugin (a package cargo takes, and the plugin's file), if it can't.
pub(super) fn name_ok(name: &str) -> Result<(), String> {
    if !name.starts_with(|c: char| c.is_ascii_alphabetic())
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("a plugin's name starts with a letter, then letters, digits, - and _".into());
    }
    if KEYWORDS.contains(&name) {
        return Err(format!("“{name}” is a Rust keyword — cargo won't take it as a name"));
    }
    Ok(())
}

/// A new plugin crate's files, by path in it: `Cargo.toml` named `name`, the template's code (its test loading the
/// plugin by `name`) or example `from`'s, `REFERENCE.md` (the reference generated from the WIT, ending with the
/// examples' index), `.cargo/config.toml`, `.gitignore`, and this tarae's SDK vendored in `tarae/` (`vendored`, the
/// WIT with it) — what `tarae plugin new` and `:plugin-new` (`install.rs`) write alike.
pub(super) fn crate_files(name: &str, from: Option<&str>) -> Result<Vec<(&'static str, String)>, String> {
    name_ok(name)?;
    let lib = match from {
        Some(e) => super::examples::find(e)?.lib,
        None => LIB_RS,
    };
    let mut files = vec![
        ("Cargo.toml", CARGO_TOML.replacen("\"NAME\"", &format!("\"{name}\""), 1)),
        ("src/lib.rs", lib.replace("\"NAME\"", &format!("\"{name}\""))),
        ("REFERENCE.md", super::reference::for_crate(WIT)?),
        // (its build's own files stay in its folder, not in a shared build-dir a sandboxed run can't write)
        (".cargo/config.toml", CARGO_CONFIG.to_string()),
        (".gitignore", "/target\n".to_string()),
    ];
    files.extend(vendored());
    Ok(files)
}

/// `files` written into folder `rel` below `root` (plain names only), a folder made for them — through no link
/// (`beneath.rs`, as Claude's edits are written): a run that writes the project may have swapped a folder on the way
/// for a link to outside — a link is refused, never gone through. The
/// crate's folder is made here (`mkdirat`: one there already is refused), each file new in it (`O_EXCL`).
pub(super) fn write_crate(root: &Path, rel: &Path, files: &[(&str, String)]) -> Result<(), String> {
    let (Some(above), Some(name)) = (rel.parent(), rel.file_name()) else {
        return Err(format!("{}: no folder's name", rel.display()));
    };
    let shown = |e: std::io::Error| e.to_string();
    let top = crate::beneath::Dir::root(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let dir = top.folder(above, true).map_err(shown)?.make(name).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("{} already exists", root.join(rel).display()),
        _ => e.to_string(),
    })?;
    for (path, text) in files {
        let path = Path::new(path);
        let into = dir.folder(path.parent().unwrap_or(Path::new("")), true).map_err(shown)?;
        let file = path.file_name().expect("a file's name");
        into.create(file, text.as_bytes()).map_err(shown)?;
    }
    Ok(())
}

fn target_installed() -> bool {
    std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l == TARGET))
}

fn plugins_dir() -> anyhow::Result<std::path::PathBuf> {
    super::dir().ok_or_else(|| anyhow!("no config folder (set HOME or XDG_CONFIG_HOME)"))
}

/// What's installed, and whether each loads.
fn list() -> anyhow::Result<()> {
    list_in(&plugins_dir()?, &mut std::io::stdout().lock())
}

/// `list`, of the plugins in `dir`, written to `out`.
fn list_in(dir: &std::path::Path, out: &mut impl std::io::Write) -> anyhow::Result<()> {
    let found = super::host::scan(dir);
    if found.is_empty() {
        writeln!(out, "no plugins in {} — tarae plugin new <name> starts one", dir.display())?;
    }
    for (name, (path, (_, len))) in found {
        writeln!(
            out,
            "{}  {}",
            paint("1", &name),
            paint("2", &format!("{} KB · {}", len.div_ceil(1024), path.display()))
        )?;
        let loaded = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| loads(&b));
        match loaded {
            Ok((commands, subscriptions)) => {
                for c in commands {
                    writeln!(out, "  :{:<20} {}", super::shown(&c.name), paint("2", &super::shown(&c.doc)))?;
                }
                if !subscriptions.is_empty() {
                    let events: Vec<&str> = subscriptions.iter().map(|s| s.event.name()).collect();
                    writeln!(out, "  {}", paint("2", &format!("on {}", events.join(", "))))?;
                }
            }
            Err(e) => writeln!(out, "  {} {}", paint("31", "can't load:"), super::shown(&e))?,
        }
    }
    Ok(())
}

/// Whether a plugin loads as tarae loads it — `list` and `test` say the same: it decodes and starts, carries this
/// plugin API version (wit-bindgen's section), declares what the editor takes (`perm::invalid`: `keys` beside
/// `network` or `run`, a pattern's form), and answers its commands and subscriptions. Err: why not, in the editor's
/// words.
fn loads(bytes: &[u8]) -> Result<(Vec<Command>, Vec<Subscription>), String> {
    let mut p = Plugin::new(bytes, Limits::default(), FUEL)?;
    if let Some(why) = super::perm::invalid(&p.permissions(FUEL)?) {
        return Err(why);
    }
    Ok((p.commands(FUEL)?, p.subscriptions(FUEL)?))
}

/// The `.wasm` a `cargo build --message-format json…` built.
fn artifact(json: &str) -> Option<PathBuf> {
    artifacts(json).into_iter().next()
}

/// Each `.wasm` a `cargo build --message-format json…` built (a workspace's: one a member).
fn artifacts(json: &str) -> Vec<PathBuf> {
    json.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact")
        .flat_map(|m| m["filenames"].as_array().cloned().unwrap_or_default())
        .filter_map(|f| f.as_str().map(PathBuf::from))
        .filter(|f| f.extension().is_some_and(|e| e == "wasm"))
        .collect()
}

/// This machine's target (`rustc -vV`'s host): a plugin's tests run there — its `.cargo/config.toml` names wasm32.
fn host() -> anyhow::Result<String> {
    let out = std::process::Command::new("rustc").arg("-vV").output().context("running rustc")?;
    let out = String::from_utf8_lossy(&out.stdout);
    out.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no host in rustc -vV"))
}

/// What a `cargo test` said: each failure in one line (the test, its panic's first line and place — in a workspace,
/// its crate first), and the totals.
#[derive(Debug, Default, PartialEq)]
struct Tests {
    failures: Vec<String>,
    passed: usize,
    failed: usize,
    ignored: usize,
}

/// Reads libtest's text (stable libtest has no JSON), stdout and stderr as one stream: `Running … (…/deps/<crate>-<hash>)`
/// says whose tests follow.
fn read_tests(out: &str) -> Tests {
    let mut t = Tests::default();
    let lines: Vec<&str> = out.lines().collect();
    let mut krate = String::new();
    for (i, l) in lines.iter().enumerate() {
        let l = l.trim_end();
        if let Some(bin) = l.trim_start().strip_prefix("Running ").and_then(|r| r.rsplit_once('(')) {
            let stem =
                Path::new(bin.1.trim_end_matches(')')).file_name().map(|n| n.to_string_lossy().into_owned());
            krate = stem.and_then(|s| s.rsplit_once('-').map(|(c, _)| c.to_string())).unwrap_or_default();
        } else if let Some(name) = l.strip_prefix("---- ").and_then(|r| r.strip_suffix(" stdout ----")) {
            let rest = lines[i + 1..].iter().take_while(|l| !l.starts_with("---- "));
            let mut said = rest.skip_while(|l| !l.starts_with("thread '"));
            let at = said
                .next()
                .and_then(|l| l.split_once("' panicked at "))
                .map(|(_, at)| at.trim_end_matches(':'));
            let first = said.next().map_or("", |l| l.trim());
            let whose = if krate.is_empty() { String::new() } else { format!("{krate}: ") };
            let at = at.map(|a| format!(" ({a})")).unwrap_or_default();
            t.failures.push(format!("{whose}{name} — {first}{at}"));
        } else if let Some(r) = l.strip_prefix("test result: ") {
            // (`FAILED. 1 passed`, `1 failed`, `2 ignored`, …)
            for part in r.split("; ") {
                let Some((before, word)) = part.rsplit_once(' ') else { continue };
                let n: usize = before.rsplit(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0);
                match word {
                    "passed" => t.passed += n,
                    "failed" => t.failed += n,
                    "ignored" => t.ignored += n,
                    _ => {}
                }
            }
        }
    }
    t
}

/// The compiler's errors in cargo's text output, one line each (with its place).
fn errors(out: &str) -> Vec<String> {
    let lines: Vec<&str> = out.lines().collect();
    let mut found = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.starts_with("error")
            || l.starts_with("error: could not compile")
            || l.starts_with("error: test failed")
        {
            continue;
        }
        let at = lines.get(i + 1).and_then(|n| n.trim().strip_prefix("--> "));
        found.push(at.map_or_else(|| l.to_string(), |at| format!("{l} ({at})")));
    }
    found
}

/// `tarae plugin test`: in each folder (a plugin, or a workspace of them), the vendored SDK held to this tarae's, the
/// tests run once on this machine, then each `.wasm` built and loaded as tarae loads it (`loads`). One line a failure,
/// then each folder's totals; Err when anything failed.
fn test(dirs: &[&Path], out: &mut impl Write) -> anyhow::Result<()> {
    let host = host()?;
    let mut failed = 0;
    for dir in dirs {
        failed += test_in(dir, &host, out)?;
    }
    match failed {
        0 => Ok(()),
        1 => bail!("1 failure"),
        n => bail!("{n} failures"),
    }
}

/// `test`, of the plugin or workspace in `dir`, on `host`: how many failures, each said in `out` — first what keeps
/// it from being held to this tarae's rules (`held`), and that again after cargo ran.
fn test_in(dir: &Path, host: &str, out: &mut impl Write) -> anyhow::Result<usize> {
    if !dir.join("Cargo.toml").is_file() {
        bail!("no Cargo.toml in {} — tarae plugin new <name> starts one", dir.display());
    }
    let mut failures = 0;
    let wrong = held(dir, host);
    for w in &wrong {
        writeln!(out, "× {}: {w}", dir.display())?;
    }
    failures += wrong.len();
    // The tests, once for the workspace — stdout and stderr as one stream: whose tests follow is said on stderr
    let (read, write) = std::io::pipe()?;
    let mut cargo = std::process::Command::new("cargo");
    cargo.args(["test", "--workspace", "--no-fail-fast", "--target", host]).current_dir(dir);
    cargo.env("CARGO_TERM_COLOR", "never").stdout(write.try_clone()?).stderr(write);
    crate::testing::fixture_target(&mut cargo, dir);
    let mut child = cargo.spawn().context("running cargo test")?;
    drop(cargo);
    let text = std::io::read_to_string(read)?;
    let ran = child.wait()?;
    let tests = read_tests(&text);
    for f in &tests.failures {
        writeln!(out, "× {}", super::shown(f))?;
    }
    failures += tests.failures.len();
    if !ran.success() && tests.failures.is_empty() {
        let errors = errors(&text);
        for e in &errors {
            writeln!(out, "× {e}")?;
        }
        if errors.is_empty() {
            writeln!(out, "× cargo test failed ({ran})")?;
        }
        failures += errors.len().max(1);
    }
    // Then each plugin as tarae loads it
    let mut build = std::process::Command::new("cargo");
    build
        .args([
            "build",
            "--release",
            "--workspace",
            "--target",
            TARGET,
            "--message-format",
            "json-diagnostic-short",
        ])
        .current_dir(dir)
        .env("CARGO_TERM_COLOR", "never");
    crate::testing::fixture_target(&mut build, dir);
    let build = build.output().context("running cargo build")?;
    let json = String::from_utf8_lossy(&build.stdout);
    if !build.status.success() {
        // (the compiler's errors; else cargo's own — a manifest, a lock, a dependency)
        let mut said: Vec<String> = diagnostics(&json)
            .into_iter()
            .filter(|d| d.contains("error"))
            .map(|d| d.trim_end().to_string())
            .collect();
        if said.is_empty() {
            said = errors(&String::from_utf8_lossy(&build.stderr));
        }
        if said.is_empty() {
            said.push(format!("cargo build failed ({})", build.status));
        }
        for d in &said {
            writeln!(out, "× {d}")?;
        }
        failures += 1;
    }
    let mut loaded = Vec::new();
    let built = artifacts(&json);
    for wasm in &built {
        let name = wasm.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match std::fs::read(wasm).map_err(|e| e.to_string()).and_then(|b| loads(&b)) {
            Ok((commands, _)) => loaded.push(format!("{name} loads ({} commands)", commands.len())),
            Err(why) => {
                writeln!(out, "× {name} can't load: {}", super::shown(&why))?;
                failures += 1;
            }
        }
    }
    if build.status.success() && built.is_empty() {
        writeln!(out, "× {}: no .wasm built — a plugin's [lib] is crate-type = [\"cdylib\"]", dir.display())?;
        failures += 1;
    }
    // (again: a build script ran before the SDK compiled, and could change it or the manifest)
    for w in held(dir, host).into_iter().filter(|w| !wrong.contains(w)) {
        writeln!(out, "× {}: after cargo ran, {w}", dir.display())?;
        failures += 1;
    }
    let loads = if loaded.is_empty() { "none loads".to_string() } else { loaded.join(", ") };
    writeln!(
        out,
        "{} — {} passed, {} failed, {} ignored · {loads}",
        dir.display(),
        tests.passed,
        tests.failed,
        tests.ignored
    )?;
    Ok(failures)
}

/// The compiler's messages in that output, as it rendered them.
fn diagnostics(json: &str) -> Vec<String> {
    json.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-message")
        .filter_map(|m| m["message"]["rendered"].as_str().map(str::to_string))
        .collect()
}

/// The package name in a plugin's Cargo.toml.
pub(super) fn package_name(manifest: &str) -> Option<String> {
    let t: toml::Table = toml::from_str(manifest).ok()?;
    Some(t.get("package")?.get("name")?.as_str()?.to_string())
}

/// Builds the plugin in `dir` whenever its sources change, and puts it in the plugins folder — where a
/// running tarae notices and reloads it.
fn dev(dir: &Path) -> anyhow::Result<()> {
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml"))
        .map_err(|_| anyhow!("no Cargo.toml in {} — tarae plugin new <name> starts one", dir.display()))?;
    let name = package_name(&manifest).ok_or_else(|| anyhow!("no [package] name in Cargo.toml"))?;
    for w in held(dir, &host()?) {
        println!("{} {w}", paint("33", "note:"));
    }
    let out = plugins_dir()?;
    std::fs::create_dir_all(&out)?;
    // (where its dev marks go, and the folder they name — #458)
    let state = crate::config::state_dir().ok_or_else(|| anyhow!("no state folder (set HOME)"))?;
    // (tarae's own folder, made 0700 — #642: this process may be the first to make it)
    let _ = crate::config::own_folder(&state);
    let marks = state.join("plugin-dev");
    let source = std::fs::canonicalize(dir)?;
    let stamps = || -> Vec<SystemTime> {
        fn walk(p: &Path, out: &mut Vec<SystemTime>) {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                    out.push(t);
                }
            }
        }
        let mut v = Vec::new();
        walk(&dir.join("src"), &mut v);
        walk(&dir.join("wit"), &mut v);
        walk(&dir.join(VENDORED), &mut v);
        v.extend(std::fs::metadata(dir.join("Cargo.toml")).and_then(|m| m.modified()));
        v
    };
    println!("{} {name} → {}", paint("1;36", "tarae plugin dev"), paint("2", &out.display().to_string()));
    let mut last = Vec::new();
    loop {
        let now = stamps();
        if now != last {
            last = now;
            let started = std::time::Instant::now();
            match dev_round(dir, &out, &marks, &name, &source)? {
                Ok(dest) => {
                    let kb = std::fs::metadata(&dest).map_or(0, |m| m.len().div_ceil(1024));
                    let took = crate::term::run_duration(started.elapsed());
                    println!(
                        "{} built in {took} · {kb} KB  {}",
                        paint("1;32", "✓"),
                        paint("2", "— reloading in tarae")
                    );
                }
                Err((said, stderr)) => {
                    println!("{} build failed", paint("1;31", "✗"));
                    for d in said {
                        eprint!("{d}");
                    }
                    Write::write_all(&mut std::io::stderr(), &stderr)?;
                }
            }
        }
        crate::event::pause(Duration::from_millis(300));
    }
}

/// A failed build: the compiler's messages, and cargo's stderr.
type Failed = (Vec<String>, Vec<u8>);

/// One round of `dev`: the plugin in `dir` built, and put in `out` with its dev mark (`install`). Ok(Err): the build
/// failed.
fn dev_round(
    dir: &Path,
    out: &Path,
    marks: &Path,
    name: &str,
    source: &Path,
) -> anyhow::Result<Result<PathBuf, Failed>> {
    let mut build = std::process::Command::new("cargo");
    build
        .args(["build", "--release", "--target", TARGET, "--color", "always"])
        .args(["--message-format", "json-diagnostic-rendered-ansi"])
        .current_dir(dir);
    crate::testing::fixture_target(&mut build, dir);
    let build = build.output().context("running cargo")?;
    // Where cargo put it (a target dir or lib name of the plugin's own included)
    let json = String::from_utf8_lossy(&build.stdout);
    match artifact(&json).filter(|_| build.status.success()) {
        Some(wasm) => Ok(Ok(install(&wasm, out, marks, name, source)?)),
        None => Ok(Err((diagnostics(&json), build.stderr))),
    }
}

/// The build `wasm` put in the plugins folder `out` as `<name>.wasm` — through a temporary name, so tarae never reads a
/// half-written file —, its dev mark (`perm::mark`, in `marks`: these bytes are a build of `source`) written first, so
/// the load the rename starts finds it: the yes given to a build of `source` holds for this one (#458).
fn install(
    wasm: &Path,
    out: &Path,
    marks: &Path,
    name: &str,
    source: &Path,
) -> anyhow::Result<std::path::PathBuf> {
    let dest = out.join(format!("{name}.wasm"));
    let tmp = out.join(format!(".{name}.wasm.tmp"));
    std::fs::copy(wasm, &tmp)?;
    // (the bytes the rename puts in place: the tmp file's, not the build's again — cargo may write it meanwhile)
    let marked = std::fs::read(&tmp)
        .and_then(|bytes| super::perm::mark(marks, name, &crate::sha256::hex(&bytes), source))
        .with_context(|| format!("writing its dev mark in {}", marks.display()));
    if let Err(e) = marked {
        std::fs::remove_file(&tmp).ok();
        return Err(e);
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #458: `install` writes the build's dev mark — its sha256, its folder — before the build lands: a mark it can't
    /// write lands no build (the load would ask again, as for any new code), and leaves no temporary file.
    #[test]
    fn install_writes_the_mark_before_the_plugin() {
        let s = crate::scratch::Scratch::new("plugin-install");
        let (out, marks, source) = (s.join("plugins"), s.join("state/plugin-dev"), s.join("src/demo"));
        std::fs::create_dir_all(&out).unwrap();
        let wasm = s.join("built.wasm");
        std::fs::write(&wasm, super::super::abi::DEMO).unwrap();
        let dest = install(&wasm, &out, &marks, "demo", &source).unwrap();
        assert_eq!(dest, out.join("demo.wasm"));
        let sha = crate::sha256::hex(&std::fs::read(&dest).unwrap());
        assert_eq!(super::super::perm::dev_source(&marks, "demo", &sha), Some(source.clone()));
        // The mark's folder a file: no mark, so no build either
        let blocked = s.join("blocked");
        std::fs::write(&blocked, "").unwrap();
        std::fs::remove_file(&dest).unwrap();
        let e = install(&wasm, &out, &blocked.join("plugin-dev"), "demo", &source).unwrap_err();
        assert!(format!("{e:#}").contains("writing its dev mark"), "{e:#}");
        let left: Vec<_> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name()).collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn new_writes_a_plugin_crate() {
        let scratch = crate::scratch::Scratch::new("plugin-new");
        let root = scratch.to_path_buf();
        let dir = root.join("my-plugin");
        new(&dir, None).unwrap();
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert_eq!(package_name(&manifest).as_deref(), Some("my-plugin"));
        assert!(
            std::fs::read_to_string(dir.join("tarae/wit/tarae.wit"))
                .unwrap()
                .contains(&format!("package tarae:plugin@{}.", super::super::abi::API))
        );
        let config = std::fs::read_to_string(dir.join(".cargo/config.toml")).unwrap();
        assert!(config.contains(TARGET), "{config}");
        // (cargo's build files inside the crate, where a sandboxed run writes)
        assert!(config.contains("build-dir = \"target/build\""), "{config}");
        assert!(std::fs::read_to_string(dir.join("REFERENCE.md")).unwrap().contains("### permission"));
        assert!(std::fs::read_to_string(dir.join("src/lib.rs")).unwrap().contains("load(\"my-plugin\""));
        // (the SDK vendored whole, as this tarae has it — its manifest without a workspace of its own)
        assert_eq!(vendored_stale(&dir), Vec::<String>::new());
        let sdk = std::fs::read_to_string(dir.join("tarae/sdk/Cargo.toml")).unwrap();
        assert!(sdk.contains("name = \"tarae-plugin\"") && !sdk.contains("[workspace]"), "{sdk}");
        assert!(new(&dir, None).is_err(), "not over an existing folder");
        assert!(new(&root.join("bad name"), None).is_err());
        assert!(new(&root.join("2fa"), None).is_err(), "cargo refuses a leading digit");
        assert!(new(&root.join("fn"), None).is_err(), "and keywords");
        // From an example: its code, the rest as the template's
        let from = root.join("tally");
        new(&from, Some("word-tally")).unwrap();
        let lib = std::fs::read_to_string(from.join("src/lib.rs")).unwrap();
        assert_eq!(lib, super::super::examples::find("word-tally").unwrap().lib);
        assert!(from.join("REFERENCE.md").is_file() && from.join(".cargo/config.toml").is_file());
        assert_eq!(vendored_stale(&from), Vec::<String>::new());
        let e = new(&root.join("other"), Some("nope")).unwrap_err().to_string();
        assert!(e.contains("no example nope") && !root.join("other").exists(), "{e}");
    }

    /// `test`, of `dirs`: what it said, and whether it passed.
    fn tested(dirs: &[&Path]) -> (String, bool) {
        let mut out = Vec::new();
        let r = test(dirs, &mut out);
        (String::from_utf8(out).unwrap(), r.is_ok())
    }

    /// A plugin `tarae plugin new` starts passes `tarae plugin test` as it is: its own test, through the mock host,
    /// and its wasm built and loaded as tarae loads it — the template keeps up with the WIT and the SDK. (Ignored:
    /// it fetches the SDK's crates; CI's network job runs it.)
    #[test]
    #[ignore = "network: CI's network job runs it"]
    fn a_new_plugin_passes_its_own_test() {
        let scratch = crate::scratch::Scratch::new("plugin-passes");
        let dir = scratch.join("hello");
        new(&dir, None).unwrap();
        let (said, ok) = tested(&[&dir]);
        assert!(ok, "{said}");
        assert!(said.contains("— 1 passed, 0 failed, 0 ignored · hello.wasm loads (2 commands)"), "{said}");
    }

    /// A plugin whose own tests pass but whose wasm is built for another API version fails `tarae plugin test`, in
    /// the words the load uses — and its vendored WIT, changed, is said too. (Ignored: it fetches crates.)
    #[test]
    #[ignore = "network: CI's network job runs it"]
    fn a_plugin_built_for_another_api_fails_its_test() {
        let scratch = crate::scratch::Scratch::new("plugin-other-api");
        let dir = scratch.join("old");
        new(&dir, None).unwrap();
        let wit = dir.join("tarae/wit/tarae.wit");
        let text = std::fs::read_to_string(&wit).unwrap();
        let api = format!("package tarae:plugin@{}.", super::super::abi::API);
        std::fs::write(&wit, text.replace(&api, "package tarae:plugin@0.1.")).unwrap();
        let (said, ok) = tested(&[&dir]);
        assert!(!ok, "{said}");
        assert!(said.contains("1 passed, 0 failed"), "its own tests pass: {said}");
        let why = format!(
            "× old.wasm can't load: it's built for tarae plugin API 0.1.0, this tarae speaks {}",
            super::super::abi::API
        );
        assert!(said.contains(&why), "{said}");
        assert!(said.contains("its vendored SDK isn't this tarae's (tarae/wit/tarae.wit)"), "{said}");
    }

    /// `tarae plugin dev`'s round on a new plugin (#458): it builds, writes the dev mark naming its folder before the
    /// build lands, and puts it in place — a build the plugin thread's load counts as a dev build (`dev_source`), and
    /// one that loads. (Ignored: it fetches crates.)
    #[test]
    #[ignore = "network: CI's network job runs it"]
    fn plugin_dev_builds_marks_and_installs_a_new_plugin() {
        let scratch = crate::scratch::Scratch::new("plugin-dev-round");
        let dir = scratch.join("devved");
        new(&dir, None).unwrap();
        let (out, marks) = (scratch.join("plugins"), scratch.join("state/plugin-dev"));
        std::fs::create_dir_all(&out).unwrap();
        let source = std::fs::canonicalize(&dir).unwrap();
        let dest = match dev_round(&dir, &out, &marks, "devved", &source).unwrap() {
            Ok(dest) => dest,
            Err((said, stderr)) => panic!("{said:?} {}", String::from_utf8_lossy(&stderr)),
        };
        assert_eq!(dest, out.join("devved.wasm"));
        let bytes = std::fs::read(&dest).unwrap();
        let sha = crate::sha256::hex(&bytes);
        assert_eq!(super::super::perm::dev_source(&marks, "devved", &sha), Some(source));
        assert!(loads(&bytes).is_ok());
    }

    /// The bundled plugins pass their own suites under `tarae plugin test` — tests, wasm built, loaded as tarae loads
    /// it —, and the SDK's mock host its own (`:preflight-full` runs this, and CI's network job). (Ignored: it fetches
    /// crates.)
    #[test]
    #[ignore = "network: CI's network job runs it"]
    fn the_bundled_plugins_pass_their_own_tests() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin-sdk");
        let mut sdk = std::process::Command::new("cargo");
        sdk.args(["test", "--features", "test", "--target", &host().unwrap()]).current_dir(root.join("sdk"));
        // (warnings as errors, as CI's `RUSTFLAGS` makes them: here too, so the preflight denies what CI denies — the
        // SDK's own config gives no rustflags for this one to replace)
        sdk.env("RUSTFLAGS", "-D warnings");
        crate::testing::fixture_target(&mut sdk, &root.join("sdk"));
        let sdk = sdk.output().unwrap();
        let said = String::from_utf8_lossy(&sdk.stdout);
        assert!(sdk.status.success(), "the SDK's tests: {said}{}", String::from_utf8_lossy(&sdk.stderr));
        let dirs: Vec<PathBuf> =
            ["db", "keycast", "kubernetes", "demo"].iter().map(|p| root.join(p)).collect();
        let (said, ok) = tested(&dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>());
        assert!(ok, "{said}");
    }

    /// Every file of the SDK and the rules is vendored (but the repository's own: a lock, a `.gitignore`, its
    /// `.cargo`): a file added to either reaches a new plugin.
    #[test]
    fn the_vendored_sdk_is_the_whole_sdk() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin-sdk");
        let mut there = Vec::new();
        for part in ["sdk", "rules"] {
            fn walk(at: &Path, rel: &str, out: &mut Vec<String>) {
                for e in std::fs::read_dir(at).unwrap().flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if e.file_type().unwrap().is_dir() {
                        if name != "target" && !name.starts_with('.') {
                            walk(&e.path(), &format!("{rel}/{name}"), out);
                        }
                    } else if name != "Cargo.lock" && !name.starts_with('.') {
                        out.push(format!("{rel}/{name}"));
                    }
                }
            }
            walk(&root.join(part), &format!("tarae/{part}"), &mut there);
        }
        let mut listed: Vec<String> = VENDOR
            .iter()
            .map(|(p, _)| p.to_string())
            .filter(|p| !p.starts_with("tarae/LICENSE") && !p.starts_with("tarae/wit/"))
            .collect();
        crate::sorting::sort(&mut there);
        crate::sorting::sort(&mut listed);
        assert_eq!(listed, there, "VENDOR and plugin-sdk/sdk, plugin-sdk/rules");
    }

    /// A vendored copy changed, or with a file of its own, is said as not this tarae's — each file named — and
    /// `vendor` writes this tarae's again.
    #[test]
    fn a_vendored_sdk_that_differs_is_said_and_vendor_mends_it() {
        let scratch = crate::scratch::Scratch::new("plugin-vendored");
        let dir = scratch.join("mine");
        new(&dir, None).unwrap();
        let rules = dir.join("tarae/rules/src/lib.rs");
        let text = std::fs::read_to_string(&rules).unwrap();
        std::fs::write(&rules, text.replace("prog.len() > 64", "prog.len() > 640")).unwrap();
        std::fs::write(dir.join("tarae/rules/src/extra.rs"), "").unwrap();
        std::fs::remove_file(dir.join("tarae/LICENSE-MIT")).unwrap();
        let stale = vendored_stale(&dir);
        assert_eq!(stale, ["tarae/rules/src/lib.rs", "tarae/LICENSE-MIT", "tarae/rules/src/extra.rs"]);
        vendor(&dir).unwrap();
        assert_eq!(vendored_stale(&dir), Vec::<String>::new());
        assert!(vendor(&scratch.join("nothing")).is_err(), "only into a plugin's folder");
    }

    /// A crate in `dir`: its manifest (`name`, then `more`) and an empty lib.rs.
    fn krate(dir: &Path, name: &str, more: &str) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let manifest =
            format!("[package]\nname = \"{name}\"\nversion = \"0.3.0\"\nedition = \"2024\"\n{more}");
        std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    }

    /// The cargo config of a plugin in `dir` the tests build: offline (its crates are paths), its build in its folder,
    /// and `more`.
    fn offline(dir: &Path, more: &str) {
        std::fs::create_dir_all(dir.join(".cargo")).unwrap();
        let config = format!("[build]\nbuild-dir = \"target/build\"\n[net]\noffline = true\n{more}");
        std::fs::write(dir.join(".cargo/config.toml"), config).unwrap();
    }

    /// The SDK and the rules come from the plugin's vendored `tarae/` and nowhere else, as cargo resolves them: a
    /// path elsewhere, a `[patch]` (in the manifest or in `.cargo`), a renamed crate, a `[replace]` (which needs a
    /// registry to resolve at all) and no `tarae/` each fail, `tarae plugin vendor` named. (Stand-in crates, so cargo
    /// needs no network: the copy itself isn't this tarae's, said apart.)
    #[test]
    fn the_sdk_comes_from_its_vendored_copy_and_nowhere_else() {
        let s = crate::scratch::Scratch::new("plugin-held");
        let p = s.join("p");
        let rules = "[dependencies]\ntarae-plugin-rules = { path = \"../rules\" }\n";
        krate(&p.join("tarae/sdk"), "tarae-plugin", rules);
        krate(&p.join("tarae/rules"), "tarae-plugin-rules", "");
        krate(&s.join("mine"), "tarae-plugin", "");
        krate(&s.join("fake"), "my-fake", "");
        offline(&p, "");
        let held_with = |deps: &str| {
            krate(&p, "p", &format!("{deps}\n[workspace]\nexclude = [\"tarae\"]\n"));
            std::fs::remove_file(p.join("Cargo.lock")).ok();
            held(&p, &host().unwrap())
        };
        // (the stand-in copy said, then the rest)
        let elsewhere = |w: Vec<String>| -> Vec<String> {
            assert!(w[0].starts_with("its vendored SDK isn't this tarae's (tarae/sdk/Cargo.toml, "), "{w:?}");
            w[1..].to_vec()
        };
        let fix = format!("`tarae plugin vendor {}` writes this tarae's", p.display());
        let takes = |from: &str| {
            let from = std::fs::canonicalize(s.join(from)).unwrap();
            vec![format!(
                "it takes tarae-plugin from {}, not its vendored tarae/sdk — a path elsewhere, a [patch] or a [replace] \
                 steps around the rules it's held to; {fix}",
                from.display()
            )]
        };
        let vendored = "[dependencies]\ntarae-plugin = { path = \"tarae/sdk\" }";
        assert_eq!(elsewhere(held_with(vendored)), Vec::<String>::new());
        let path = "[dependencies]\ntarae-plugin = { path = \"../mine\" }";
        assert_eq!(elsewhere(held_with(path)), takes("mine"), "a path elsewhere");
        let patch = concat!(
            "[dependencies]\ntarae-plugin = \"0.3\"\n",
            "[patch.crates-io]\ntarae-plugin = { path = \"../mine\" }"
        );
        assert_eq!(elsewhere(held_with(patch)), takes("mine"), "a [patch]");
        offline(&p, "[patch.crates-io]\ntarae-plugin = { path = \"../mine\" }\n");
        let registry = "[dependencies]\ntarae-plugin = \"0.3\"";
        assert_eq!(elsewhere(held_with(registry)), takes("mine"), "a [patch] in .cargo");
        offline(&p, "");
        let renamed = "[dependencies]\ntarae-plugin = { package = \"my-fake\", path = \"../fake\" }";
        assert_eq!(elsewhere(held_with(renamed)), takes("fake"), "a renamed crate");
        let replace = concat!(
            "[dependencies]\ntarae-plugin = \"0.3\"\n",
            "[replace]\n\"tarae-plugin:0.3.0\" = { path = \"../mine\" }"
        );
        let w = elsewhere(held_with(replace));
        assert!(
            w.len() == 1 && w[0].starts_with("where its SDK comes from isn't known (cargo metadata): "),
            "{w:?}"
        );
        // No vendored copy, out of tarae's repository
        std::fs::remove_dir_all(p.join("tarae")).unwrap();
        let w = held_with(vendored);
        assert_eq!(w[0], format!("no vendored SDK (tarae/) — {fix}"), "{w:?}");
    }

    /// A plugin inside tarae's repository takes the SDK from its `plugin-sdk/` — held to this tarae's too, so a
    /// folder dressed as tarae's repository steps around nothing; this repository's is this tarae's.
    #[test]
    fn a_plugin_in_taraes_repository_takes_its_sdk_as_this_tarae_has_it() {
        let here = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
        assert_eq!(tarae_repository(&here.join("plugin-sdk/db")), Some(here.clone()));
        assert_eq!(repository_stale(&here), Vec::<String>::new());
        let s = crate::scratch::Scratch::new("plugin-in-repository");
        let r = std::fs::canonicalize(&*s).unwrap().join("r");
        krate(&r, "tarae", "[workspace]\nexclude = [\"plugin-sdk\"]\n");
        let rules = "[dependencies]\ntarae-plugin-rules = { path = \"../rules\" }\n";
        krate(&r.join("plugin-sdk/sdk"), "tarae-plugin", rules);
        krate(&r.join("plugin-sdk/rules"), "tarae-plugin-rules", "");
        let mine = r.join("plugin-sdk/mine");
        krate(&mine, "mine", "[dependencies]\ntarae-plugin = { path = \"../sdk\" }\n[workspace]\n");
        offline(&mine, "");
        assert_eq!(tarae_repository(&mine), Some(r.clone()));
        let w = held(&mine, &host().unwrap());
        let not =
            format!("tarae's plugin-sdk/ in {} isn't this tarae's (plugin-sdk/sdk/Cargo.toml, ", r.display());
        assert!(w.len() == 1 && w[0].starts_with(&not), "{w:?}");
    }

    /// A build script runs before the SDK compiles: one that changes the vendored copy fails the test, said after
    /// cargo ran — the copy was this tarae's before. (A plugin with no dependencies, so cargo needs no network.)
    #[test]
    fn a_build_script_that_changes_the_sdk_fails_the_test() {
        let s = crate::scratch::Scratch::new("plugin-build-script");
        let p = s.join("p");
        krate(&p, "p", "[workspace]\nexclude = [\"tarae\"]\n");
        offline(&p, "");
        vendor(&p).unwrap();
        let build = "fn main() {
    let f = \"tarae/rules/src/lib.rs\";
    let text = std::fs::read_to_string(f).unwrap();
    std::fs::write(f, text + \"\\n// a build script was here\\n\").unwrap();
}
";
        std::fs::write(p.join("build.rs"), build).unwrap();
        let host = host().unwrap();
        assert_eq!(held(&p, &host), Vec::<String>::new());
        let mut out = Vec::new();
        let failures = test_in(&p, &host, &mut out).unwrap();
        let said = String::from_utf8(out).unwrap();
        let after = format!(
            "× {}: after cargo ran, its vendored SDK isn't this tarae's (tarae/rules/src/lib.rs)",
            p.display()
        );
        assert!(failures >= 1 && said.contains(&after), "{said}");
        assert!(!said.contains(&format!("× {}: its vendored", p.display())), "{said}");
    }

    /// libtest's text read as one line a failure — its crate, the test, its panic's first line and place — and the
    /// totals over every test binary; the compiler's errors one line each.
    #[test]
    fn a_tests_output_is_one_line_a_failure() {
        let out = "     Running unittests src/lib.rs (target/build/aarch64-apple-darwin/debug/deps/my_plugin-0a1b2c)

running 2 tests
test tests::a ... ok
test tests::b ... FAILED

failures:

---- tests::b stdout ----

thread 'tests::b' panicked at src/lib.rs:80:9:
assertion `left == right` failed
  left: 1
 right: 2
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

failures:
    tests::b

test result: FAILED. 1 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests my_plugin
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";
        let t = read_tests(out);
        let line = "my_plugin: tests::b — assertion `left == right` failed (src/lib.rs:80:9)";
        assert_eq!(t, Tests { failures: vec![line.into()], passed: 4, failed: 1, ignored: 2 });
        let built = "error[E0425]: cannot find value `x` in this scope\n  --> src/lib.rs:3:5\n   |\nerror: could not compile `p`";
        assert_eq!(errors(built), ["error[E0425]: cannot find value `x` in this scope (src/lib.rs:3:5)"]);
    }

    /// A module that carries no plugin API version (wit-bindgen's `component-type` section) is refused by the load
    /// `list` and `test` share, in `list`'s words, as by the editor's (`what_isnt_a_plugin_is_refused_with_a_reason`);
    /// the demo carries one, as built.
    #[test]
    fn a_plugin_with_no_api_version_doesnt_load() {
        let demo = super::super::abi::DEMO;
        assert!(loads(demo).is_ok());
        let mark = b"component-type:";
        let mut bytes = demo.to_vec();
        let at = bytes.windows(mark.len()).position(|w| w == mark).expect("the demo's version section");
        bytes[at..at + mark.len()].copy_from_slice(b"component-typo:");
        let why = loads(&bytes).err().unwrap();
        assert!(why.starts_with("it carries no plugin API version"), "{why}");
        let dir = crate::scratch::Scratch::new("plugin-list-api");
        std::fs::write(dir.join("demo.wasm"), &bytes).unwrap();
        let mut out = Vec::new();
        list_in(&dir, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains("can't load:\x1b[0m it carries no plugin API version"), "{out}");
    }

    /// `tarae plugin list` says why a plugin the editor refuses at load can't load — `keys` beside the network, a
    /// program or a read — as the editor does, and lists one that only hears keys as it lists any.
    #[test]
    fn list_says_why_a_plugin_that_hears_keys_and_asks_for_more_cant_load() {
        let dir = crate::scratch::Scratch::new("plugin-list-keys");
        let listed = |set: u8| {
            std::fs::write(dir.join("demo.wasm"), super::super::abi::demo_asking(set)).unwrap();
            let mut out = Vec::new();
            list_in(&dir, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        };
        let sets = [
            (b'n', "network"),
            (b'r', "run true"),
            (b'f', "read /tarae-demo/**"),
            (b'e', "env TARAE_DEMO"),
            (b'p', "project"),
        ];
        for (set, what) in sets {
            let out = listed(set);
            let why = format!(
                "can't load:\x1b[0m it asks to hear keys and for “{what}” — a plugin that hears keys asks for \
                 nothing else: it reads, connects to and runs nothing"
            );
            assert!(out.contains(&why) && !out.contains(":echo"), "{set}: {out}");
        }
        let out = listed(b'k');
        assert!(out.contains(":echo") && !out.contains("can't load"), "{out}");
    }

    /// The SDK (whose bindings every plugin uses), the demo (a fixture with its own) and the version tarae names in its
    /// messages agree, and match the WIT's major; the template and tarae's own plugins run no wit-bindgen of their
    /// own — the SDK's bindings, one `generate!`.
    #[test]
    fn the_sdk_pins_one_wit_bindgen() {
        let pin = format!("wit-bindgen = \"={}.", super::super::abi::WIT_BINDGEN);
        assert!(include_str!("../../plugin-sdk/sdk/Cargo.toml").contains(&pin), "sdk: {pin}");
        assert!(include_str!("../../plugin-sdk/demo/Cargo.toml").contains(&pin), "demo: {pin}");
        let sdk = "tarae_plugin::export!(";
        for (name, manifest, lib) in [
            ("template", CARGO_TOML, LIB_RS),
            (
                "db",
                include_str!("../../plugin-sdk/db/Cargo.toml"),
                include_str!("../../plugin-sdk/db/src/lib.rs"),
            ),
            (
                "keycast",
                include_str!("../../plugin-sdk/keycast/Cargo.toml"),
                include_str!("../../plugin-sdk/keycast/src/lib.rs"),
            ),
            (
                "kubernetes",
                include_str!("../../plugin-sdk/kubernetes/Cargo.toml"),
                include_str!("../../plugin-sdk/kubernetes/src/lib.rs"),
            ),
        ] {
            assert!(
                !manifest.contains("wit-bindgen") && !lib.contains("generate!"),
                "{name}: the SDK's bindings"
            );
            assert!(lib.contains(sdk), "{name}: {sdk}");
        }
        assert!(WIT.contains(&format!("package tarae:plugin@{}.", super::super::abi::API)));
    }
}
