//! A run's trust bundle (#419; P0 of `.sb/credential-masking-plan.md`, D6′, the user 2026-11-07): behind a company's
//! TLS-intercepting proxy everything is re-signed with the company's own root, and tarae's proxy relays a run's TLS
//! unread — so a run's own TLS client must trust that root, as tarae's own fetch does (the platform verifier). Every
//! run with a proxy URL gets one PEM file in its own folder (`FILE`), every CA variable the packs name
//! (`[sandbox.proxy] trust`) pointing at it. What it holds, in this order, each certificate once:
//! 0. tarae's own CA, where the run's rules name hosts its proxy terminates (P1a, `proxy::terminate`) — at its head;
//! 1. the system's roots (`rustls-native-certs`: macOS's trust settings — the User, Admin and System domains, an
//!    MDM's company root the Admin one's; reading them never asks —, Linux's distro bundle as openssl-probe finds it;
//!    on both, where tarae's own environment sets `SSL_CERT_FILE` or `SSL_CERT_DIR`, those instead);
//! 2. the CERTIFICATE blocks of the files tarae's own CA variables name (the same names);
//! 3. those of `llm.sandbox-ca`'s files — yours only (`trust::YOURS_ONLY`): it decides whom runs trust.
//!
//! CERTIFICATE blocks only — a key beside one in a file is never read into it (srt's rule) — at most `CERTS_MAX`
//! certificates, a file read up to `FILE_MAX`. This is the one bundle: tarae's own CA goes in nowhere else.
//!
//! Never narrower than the tools' own defaults: where the system's roots couldn't be read there's no bundle and no
//! variable set — one of only your files would replace each tool's own store — and the run's report says why; nor
//! with nothing to put in it.
//!
//! In the run's own folder, made anew for each run: only that run reads it, and only it could change it — which
//! gives it nothing it couldn't set itself (its own `SSL_CERT_FILE`). A copy shared by a session's runs would have
//! to be closed to their writes on both systems, and Landlock can't close one file in a folder it opens. The
//! system's roots are kept between runs while what they're read from stays as it was (`stamps`, `SYSTEM_FRESH`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

use super::Here;

/// The bundle's name in the run's own folder.
pub const FILE: &str = "trust.pem";

/// The most certificates a bundle holds — a system's are a few hundred (macOS ~150, a distro's ~140); past it the
/// rest is left out, said.
pub const CERTS_MAX: usize = 2000;

/// The most of one file a CA variable or `llm.sandbox-ca` names that's read — a distro's whole bundle is ~220 KB.
pub const FILE_MAX: u64 = 4 << 20;

/// How long the system's roots are kept at most, though nothing `stamps` sees moved — a store it doesn't watch
/// (a user's own trust settings) is read again by then.
const SYSTEM_FRESH: Duration = Duration::from_secs(300);

/// Where a bundle's certificates come from, in its order.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    /// tarae's own CA (DER), and how many hosts the run's rules terminate with it — None: it terminates none.
    pub tarae: Option<(Vec<u8>, usize)>,
    /// The CA variables in tarae's own environment the packs' `trust` names, each with the file it names.
    pub vars: Vec<(String, PathBuf)>,
    /// `llm.sandbox-ca`'s files (`yours`).
    pub yours: Vec<Result<PathBuf, String>>,
    /// Tests: the system's roots (DER) — never the real store in a test but `the_systems_roots_are_read`'s one load.
    #[cfg(test)]
    pub system: Vec<Vec<u8>>,
    /// Tests: why the system's roots couldn't be read, standing for a store that fails (`system` then unused).
    #[cfg(test)]
    pub system_fails: Option<String>,
}

impl Sources {
    /// What `here` and `ca` (`llm.sandbox-ca`) say a run's bundle is made from.
    pub fn of(here: &Here, ca: &[String]) -> Sources {
        let mut vars: Vec<(String, PathBuf)> = Vec::new();
        for name in &crate::sandboxpack::get().proxy.trust {
            if let Some(f) = here.path(name)
                && !vars.iter().any(|(_, g)| *g == f)
            {
                vars.push((name.clone(), f));
            }
        }
        Sources {
            tarae: None,
            vars,
            yours: yours(here, ca),
            #[cfg(test)]
            system: here.system_roots.clone().unwrap_or_default(),
            #[cfg(test)]
            system_fails: here.system_fails.clone(),
        }
    }
}

/// `llm.sandbox-ca`'s entries as files: `~/…` from home, an absolute path as it is — another, Err(the entry, said).
pub fn yours(here: &Here, ca: &[String]) -> Vec<Result<PathBuf, String>> {
    let home = here.path("HOME");
    ca.iter()
        .map(|c| match (c.strip_prefix("~/"), &home) {
            (Some(rest), Some(h)) => Ok(h.join(rest)),
            _ if Path::new(c).is_absolute() => Ok(PathBuf::from(c)),
            _ => Err(format!("{c} (not an absolute path, nor one from ~/)")),
        })
        .collect()
}

/// A bundle made, and what it holds.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Made {
    /// Its text: one CERTIFICATE block each.
    pub pem: String,
    /// The hosts tarae's own CA at its head is for (0: it isn't there).
    pub tarae: usize,
    /// tarae's own CA was asked for, and left out with the rest: the system's roots couldn't be read — its proxy
    /// terminates nothing for the run (`sandbox::for_its_run`).
    pub tarae_off: bool,
    /// Written in the run's own folder, its CA variables naming it (`sandbox::proxy_made`).
    pub written: bool,
    /// How many certificates each source gave that an earlier one hadn't: the system's, your CA variables',
    /// `llm.sandbox-ca`'s.
    pub system: usize,
    pub vars: usize,
    pub yours: usize,
    /// Why the system's roots couldn't be read, where they couldn't — then nothing else is taken either (`made`).
    pub system_failed: Option<String>,
    /// The files that couldn't be read, each with why (`SSL_CERT_FILE's /x.pem: no such file`).
    pub missed: Vec<String>,
    /// Certificates past `CERTS_MAX`, left out.
    pub cut: usize,
}

impl Made {
    /// What a run trusts, in its report's words (once a conversation: `RunEnv.notes`) — counts, never a
    /// certificate's names.
    pub fn said(&self) -> String {
        if let Some(why) = &self.system_failed {
            let off = match self.tarae_off {
                true => {
                    " tarae's proxy terminates TLS for none of their hosts: without the system's roots they couldn't \
                     trust its CA."
                }
                false => "",
            };
            return format!(
                "TLS: the system's roots couldn't be read ({why}) — your commands use their own.{off}"
            );
        }
        let mut parts = Vec::new();
        if self.tarae > 0 {
            let hosts = if self.tarae == 1 { "host" } else { "hosts" };
            parts.push(format!("tarae's own CA (for the {} {hosts} its proxy terminates)", self.tarae));
        }
        parts.push("the system's roots".to_string());
        if self.vars > 0 {
            parts.push(format!("{} from your CA variables", self.vars));
        }
        if self.yours > 0 {
            parts.push(format!("{} from llm.sandbox-ca", self.yours));
        }
        let mut s = format!("TLS: your commands trust {}", parts.join(" + "));
        s.push_str(" — SSL_CERT_FILE and the tools' other CA variables name that bundle.");
        if !self.missed.is_empty() {
            s.push_str(&format!(" Not read: {}.", self.missed.join("; ")));
        }
        if self.cut > 0 {
            s.push_str(&format!(" {} certificates past {CERTS_MAX} were left out.", self.cut));
        }
        s
    }
}

/// The bundle `s` makes — none where the system's roots couldn't be read: a bundle of only your CA variables' and
/// `llm.sandbox-ca`'s would replace each tool's own store, and an ordinary https to a public host would fail where
/// the tool's defaults pass (never narrower than they are). Reads files and the system's store: a worker's.
pub fn made(s: &Sources) -> Made {
    let mut m = Made::default();
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut certs: Vec<CertificateDer<'static>> = Vec::new();
    let mut add = |c: &CertificateDer<'_>, m: &mut Made| -> bool {
        if seen.contains(c.as_ref()) {
            return false;
        }
        if certs.len() >= CERTS_MAX {
            m.cut += 1;
            return false;
        }
        seen.insert(c.to_vec());
        certs.push(c.clone().into_owned());
        true
    };
    #[cfg(test)]
    let system: Roots = match &s.system_fails {
        Some(why) => Err(why.clone()),
        None => Ok(Arc::new(s.system.iter().map(|d| CertificateDer::from(d.clone())).collect())),
    };
    #[cfg(not(test))]
    let system = system();
    // (tarae's own CA first — only beside the system's roots: alone with yours it would narrow the tools' own)
    if let (Some((der, hosts)), Ok(_)) = (&s.tarae, &system) {
        add(&CertificateDer::from(der.as_slice()), &mut m);
        m.tarae = *hosts;
    }
    match system {
        Ok(roots) => {
            for c in roots.iter() {
                m.system += usize::from(add(c, &mut m));
            }
        }
        Err(why) => {
            m.system_failed = Some(why);
            m.tarae_off = s.tarae.is_some();
            return m;
        }
    }
    for (var, f) in &s.vars {
        match certificates(f) {
            Ok(cs) => cs.iter().for_each(|c| m.vars += usize::from(add(c, &mut m))),
            Err(e) => m.missed.push(format!("{var}'s {}: {e}", f.display())),
        }
    }
    for f in &s.yours {
        match f.as_ref().map(|f| (f, certificates(f))) {
            Ok((_, Ok(cs))) => cs.iter().for_each(|c| m.yours += usize::from(add(c, &mut m))),
            Ok((f, Err(e))) => m.missed.push(format!("llm.sandbox-ca's {}: {e}", f.display())),
            Err(e) => m.missed.push(format!("llm.sandbox-ca's {e}")),
        }
    }
    m.pem = certs.iter().map(|c| pem(c)).collect();
    m
}

/// `files`' certificates, for tarae's own check of an upstream (`proxy::upstream::tls`), and what couldn't be read.
pub fn roots(files: &[PathBuf]) -> (Vec<CertificateDer<'static>>, Vec<String>) {
    let (mut out, mut missed) = (Vec::new(), Vec::new());
    for f in files {
        match certificates(f) {
            Ok(cs) => out.extend(cs),
            Err(e) => missed.push(format!("{}: {e}", f.display())),
        }
    }
    (out, missed)
}

/// The CERTIFICATE blocks of PEM file `f` — nothing else in it is taken (a key, a request); a file past `FILE_MAX`,
/// one that isn't a plain file (never opened: a FIFO would wait), or one with none is Err why.
fn certificates(f: &Path) -> Result<Vec<CertificateDer<'static>>, String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let meta = std::fs::metadata(f).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a plain file".into());
    }
    if meta.len() > FILE_MAX {
        return Err(format!("past {} MB", FILE_MAX >> 20));
    }
    // (O_NONBLOCK: one swapped for a FIFO since the look doesn't hold the open)
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(f);
    let mut bytes = Vec::new();
    file.and_then(|f| f.take(FILE_MAX + 1).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > FILE_MAX {
        return Err(format!("past {} MB", FILE_MAX >> 20));
    }
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<_, _>>()
        .map_err(|e| format!("its PEM: {e}"))?;
    match certs.is_empty() {
        true => Err("holds no certificate".into()),
        false => Ok(certs),
    }
}

/// `der` as one PEM CERTIFICATE block, its base64 in lines of 64.
pub fn pem(der: &[u8]) -> String {
    let b = super::proxy::parse::base64(der);
    let mut out = String::with_capacity(b.len() + b.len() / 64 + 60);
    out.push_str("-----BEGIN CERTIFICATE-----\n");
    for line in b.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(line).unwrap_or_default());
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

/// A file's mtime, size and inode — None: not there.
fn stamp(p: &Path) -> Option<(i64, i64, u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(p).ok()?;
    Some((m.mtime(), m.mtime_nsec(), m.len(), m.ino()))
}

/// What the system's roots are read from, as far as tarae can see it: the stores' files and folders, and the two
/// variables `rustls-native-certs` reads in tarae's environment instead.
#[cfg_attr(test, allow(dead_code))]
fn stamps() -> Stamps {
    let mut places: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        places.extend(
            [
                "/System/Library/Keychains/SystemRootCertificates.keychain",
                "/Library/Keychains/System.keychain",
                "/Library/Security/Trust Settings",
                "/Library/Security/Trust Settings/Admin.plist",
            ]
            .map(PathBuf::from),
        );
        if let Some(h) = std::env::var_os("HOME") {
            places.push(Path::new(&h).join("Library/Keychains/login.keychain-db"));
        }
    }
    #[cfg(not(target_os = "macos"))]
    places.extend(
        [
            "/etc/ssl/certs",
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/ssl/cert.pem",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
            "/etc/ssl/ca-bundle.pem",
        ]
        .map(PathBuf::from),
    );
    for var in ["SSL_CERT_FILE", "SSL_CERT_DIR"] {
        let named = std::env::var_os(var).unwrap_or_default();
        places.extend(std::env::split_paths(&named).filter(|p| !p.as_os_str().is_empty()));
    }
    places.into_iter().map(|p| (p.clone(), stamp(&p))).collect()
}

/// Each place `stamps` looks at, and its stamp (`stamp`).
type Stamps = Vec<(PathBuf, Option<(i64, i64, u64, u64)>)>;

/// The system's roots as read, or why they couldn't be.
type Roots = Result<Arc<Vec<CertificateDer<'static>>>, String>;

/// The system's roots, read last at that instant from what `stamps` said then.
#[cfg_attr(test, allow(dead_code))]
static SYSTEM: Mutex<Option<(Instant, Stamps, Roots)>> = Mutex::new(None);

/// The system's roots — read again where what they're read from moved, or past `SYSTEM_FRESH`. Blocks: a worker's
/// (one at a time: the next run waits for a read under way).
#[cfg_attr(test, allow(dead_code))]
fn system() -> Roots {
    let mut kept = SYSTEM.lock().unwrap_or_else(|e| e.into_inner());
    let now = stamps();
    if let Some((at, was, roots)) = kept.as_ref()
        && *was == now
        && crate::clock::since(*at) < SYSTEM_FRESH
    {
        return roots.clone();
    }
    let roots = system_read();
    if let Err(e) = &roots {
        crate::log::line("sandbox", "warning", &format!("a run's trust bundle: the system's roots: {e}"));
    }
    *kept = Some((crate::clock::now(), now, roots.clone()));
    roots
}

/// The system's roots, read now (`rustls-native-certs`). Its errors are one store's each (a domain macOS has no
/// trust settings in): what was read is kept, and only none read at all is Err.
fn system_read() -> Roots {
    let r = rustls_native_certs::load_native_certs();
    match (r.certs.is_empty(), r.errors.first()) {
        (true, Some(e)) => Err(e.to_string()),
        (true, None) => Err("the system has none".into()),
        (false, _) => Ok(Arc::new(r.certs)),
    }
}

#[cfg(test)]
mod tests;
