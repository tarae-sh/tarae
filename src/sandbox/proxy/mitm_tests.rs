//! #419, a company's TLS-intercepting proxy reproduced on loopback (`.sb/credential-masking-plan.md` §5, the user's
//! rule: reproduced every time). The company case is a run's TLS through tarae's proxy, relayed unread, then through
//! the company's proxy — named as tarae's environment names its upstream (`llm.sandbox-upstream`) — which answers
//! the tunnel with a leaf of its own CA's for the name asked and fetches from the origin itself (the origin's leaf a
//! "public" CA's). A sandboxed run's real `curl` fetches through it all (the real path: `sandbox::policy_from`,
//! `command`, `spawn`, `after_run`). Nothing leaves this machine: the origin's name is the company proxy's to look
//! up, and it has only its own.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::packs_tests::{finish, on_path, skip};
use crate::sandbox::cabundle::pem;
use crate::web::fetch::tests::{Authority, authority, issued, trusting};

/// The host the run asks for.
pub(crate) const ORIGIN: &str = "origin-419.example.invalid";
/// What the origin answers — what the run printed once its fetch went through.
pub(crate) const ANSWER: &str = "origin-419-ok";

/// A head read to its blank line (or the stream's end).
fn head(s: &mut impl Read) -> Vec<u8> {
    let (mut got, mut b) = (Vec::new(), [0u8; 1]);
    while !got.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) => got.push(b[0]),
            _ => break,
        }
    }
    got
}

/// Every connection `l` takes until `over`, each served by `serve` on a thread of its own.
fn accepting(l: TcpListener, over: Arc<AtomicBool>, serve: impl Fn(TcpStream) + Send + Sync + 'static) {
    l.set_nonblocking(true).unwrap();
    let serve = Arc::new(serve);
    crate::event::thread("tarae-419-test", move || {
        while !over.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    let _ = s.set_read_timeout(Some(crate::test_results::PATIENCE));
                    let serve = serve.clone();
                    crate::event::thread("tarae-419-test", move || serve(s));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    });
}

/// The stand-ins: the origin (its leaf the public CA's) and the company's proxy in front of it, its own CA's
/// leaves for every name it's asked — stopped as it drops.
pub(crate) struct Company {
    /// The company proxy's address, as tarae's environment names it.
    pub at: SocketAddr,
    /// The company's CA: the root a run must trust for its fetch to go through.
    pub root: Arc<Authority>,
    /// The tunnels it was asked for (`host:port`).
    pub asked: Arc<Mutex<Vec<String>>>,
    over: Arc<AtomicBool>,
}

impl Drop for Company {
    fn drop(&mut self) {
        self.over.store(true, Ordering::SeqCst);
    }
}

impl Company {
    pub(crate) fn start() -> Company {
        Company::started(false)
    }

    /// The same behind an https:// proxy: TLS to the proxy itself first, under a leaf its own CA issued for
    /// `localhost`, the tunnel asked and intercepted inside it (TLS inside TLS).
    pub(crate) fn start_https() -> Company {
        Company::started(true)
    }

    fn started(https: bool) -> Company {
        let over = Arc::new(AtomicBool::new(false));
        let public = authority("tarae test public CA");
        let origin = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let origin_at = origin.local_addr().unwrap();
        serve_origin(origin, over.clone(), issued(&public, &[ORIGIN]));
        let root = Arc::new(authority("tarae test company CA"));
        let (asked, company) = (Arc::new(Mutex::new(Vec::new())), root.clone());
        let into = asked.clone();
        let public = Arc::new(public.der);
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let at = l.local_addr().unwrap();
        let outer = issued(&root, &["localhost"]);
        accepting(l, over.clone(), move |s| {
            if https {
                let t = rustls::StreamOwned::new(rustls::ServerConnection::new(outer.clone()).unwrap(), s);
                intercept(t, &company, &into, origin_at, &public);
            } else {
                intercept(s, &company, &into, origin_at, &public);
            }
        });
        Company { at, root, asked, over }
    }
}

/// An origin on `l` under `served`: each request's head read, `ANSWER` sent, close_notify — until `over`.
fn serve_origin(l: TcpListener, over: Arc<AtomicBool>, served: Arc<rustls::ServerConfig>) {
    accepting(l, over, move |s| {
        let mut t = rustls::StreamOwned::new(rustls::ServerConnection::new(served.clone()).unwrap(), s);
        head(&mut t);
        let answer = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ANSWER}",
            ANSWER.len()
        );
        let _ = t.write_all(answer.as_bytes());
        t.conn.send_close_notify();
        let _ = t.flush();
    });
}

/// The host a direct fetch asks for (P1a's 1d).
pub(crate) const DIRECT: &str = "direct-p1a.example.invalid";

/// An origin a run reaches with no upstream proxy — most users' case: named `DIRECT` for tarae's proxy
/// (`TEST_NAMES`), on loopback taken for beyond (macOS, `TEST_BEYOND` once the run's root is known) or on this
/// machine's own address (Linux), under a leaf a "public" CA issued — the root the system's store would hold
/// (`Here.host_roots` on a terminated host's leg). Stopped and unnamed as it drops.
pub(crate) struct Direct {
    pub at: SocketAddr,
    pub public: Authority,
    over: Arc<AtomicBool>,
}

impl Drop for Direct {
    fn drop(&mut self) {
        self.over.store(true, Ordering::SeqCst);
        let at = self.at;
        super::TEST_NAMES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(n, a)| !(n == DIRECT && *a == at));
    }
}

impl Direct {
    /// None: no address of this machine's beyond loopback (Linux).
    pub(crate) fn start() -> Option<Direct> {
        #[cfg(target_os = "linux")]
        let ip = super::packs_tests::outside_at(Path::new("/"))?;
        #[cfg(not(target_os = "linux"))]
        let ip = std::net::IpAddr::V4(Ipv4Addr::LOCALHOST);
        let over = Arc::new(AtomicBool::new(false));
        let public = authority("tarae test public CA");
        let l = TcpListener::bind((ip, 0)).unwrap();
        let at = l.local_addr().unwrap();
        serve_origin(l, over.clone(), issued(&public, &[DIRECT]));
        super::TEST_NAMES.lock().unwrap().push((DIRECT.into(), at));
        Some(Direct { at, public, over })
    }
}

/// How a run's fetch goes out past tarae's proxy: through a company's proxy (tarae's upstream), or directly.
#[derive(Clone, Copy)]
pub(crate) enum Way<'a> {
    Through(&'a Company),
    Direct(&'a Direct),
}

/// The company's proxy on one connection `s`: the CONNECT read and noted in `asked`, then the tunnel intercepted —
/// its own leaf for the name asked — and the request taken on to the origin at `origin_at`, verified against `public`.
fn intercept(
    mut s: impl Read + Write,
    company: &Authority,
    asked: &Mutex<Vec<String>>,
    origin_at: SocketAddr,
    public: &[u8],
) {
    let connect = String::from_utf8_lossy(&head(&mut s)).into_owned();
    let Some(target) = connect.strip_prefix("CONNECT ").and_then(|r| r.split(' ').next()) else {
        return;
    };
    asked.lock().unwrap().push(target.to_string());
    let name = target.rsplit_once(':').map_or(target, |(n, _)| n).to_string();
    let _ = s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
    let _ = s.flush();
    // Inside: the company's own leaf for the name asked — the interception
    let mine = issued(company, &[&name]);
    let mut inside = rustls::StreamOwned::new(rustls::ServerConnection::new(mine).unwrap(), s);
    let request = head(&mut inside);
    if request.is_empty() {
        return;
    }
    // …and the request taken on to the origin, verified as the company verifies it
    let up = TcpStream::connect(origin_at).unwrap();
    let _ = up.set_read_timeout(Some(crate::test_results::PATIENCE));
    let server = rustls::pki_types::ServerName::try_from(name).unwrap();
    let c = rustls::ClientConnection::new(trusting(&[public]), server).unwrap();
    let mut out = rustls::StreamOwned::new(c, up);
    let mut answer = Vec::new();
    if out.write_all(&request).is_ok() {
        let _ = out.read_to_end(&mut answer);
    }
    let _ = inside.write_all(&answer);
    inside.conn.send_close_notify();
    let _ = inside.flush();
}

/// What a fetch of the run's was given: tarae's own environment's variables besides the upstream (a CA variable's
/// file), and the settings.
#[derive(Default)]
pub(crate) struct Given {
    pub vars: Vec<(&'static str, PathBuf)>,
    pub llm: crate::llm::LlmConfig,
    /// The hosts the run's proxy terminates (P1a, `Here.terminate`).
    pub terminate: Vec<&'static str>,
    /// curl's own flags besides `-sS` (`-v`: its certificate's issuer on stderr).
    pub flags: &'static str,
}

/// The system's roots a test's run starts from (`Here.system_roots`): a stand-in CA's, never the company's — the
/// real store is never read here.
fn system_roots() -> Vec<Vec<u8>> {
    static SYSTEM: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    vec![SYSTEM.get_or_init(|| authority("tarae test system root").der).clone()]
}

/// A sandboxed run's `curl https://ORIGIN/` with the network open, where tarae's environment names the company
/// proxy as its upstream — what it printed (`exit=N` its last word), and what its report says once a conversation
/// (`RunEnv.notes`). None: skipped (said) where no sandbox starts here (macOS inside one; Linux without the Landlock
/// tarae uses, or with neither a supervisor nor a bridge — tier unserved bridged is WSL2's: `keeper`'s
/// `bridged_under_a_supervisor` runs these there), or without curl on PATH.
pub(crate) fn fetched(test: &str, company: &Company, given: Given) -> Option<(String, String)> {
    fetched_then(test, company, given, |_, _| {})
}

/// `fetched`, `then` given the run's policy and its project once the run ended, before either goes.
pub(crate) fn fetched_then(
    test: &str,
    company: &Company,
    given: Given,
    then: impl FnOnce(&crate::sandbox::Policy, &Path),
) -> Option<(String, String)> {
    fetched_by(test, Way::Through(company), given, then)
}

/// `fetched_then` going out `way`: through the company's proxy, `https://ORIGIN/x`; directly, `https://DIRECT:port/x`
/// (the hosts `given` terminates then at that port).
pub(crate) fn fetched_by(
    test: &str,
    way: Way<'_>,
    given: Given,
    then: impl FnOnce(&crate::sandbox::Policy, &Path),
) -> Option<(String, String)> {
    if on_path("curl").is_none() {
        skip(test, "no curl on PATH");
        return None;
    }
    #[cfg(not(target_os = "linux"))]
    if crate::sandbox::skipped_inside(&format!("sandbox::proxy::{test}")) {
        return None;
    }
    #[cfg(target_os = "linux")]
    if crate::landlock::abi().is_ok_and(|n| n < 2) || crate::landlock::abi().is_err() {
        skip(test, "no Landlock tarae uses");
        return None;
    }
    // (a scratch folder under the checkout's `target/`, not `/tmp`: a run writes there — made new, removed on drop)
    let short = test.rsplit("::").next().unwrap_or(test);
    let target = std::env::current_dir().unwrap().join("target");
    std::fs::create_dir_all(&target).unwrap();
    let base = crate::scratch::Scratch::in_dir(&target, &format!("419-{short}"));
    let root = std::fs::canonicalize(&*base).unwrap();
    let mut here = crate::sandbox::Here::now();
    let (host, port) = match way {
        Way::Through(company) => {
            // (as tarae's environment would name its upstream and its CA files — a test's own `Here` keeps no
            // proxy variable of the process's)
            here.env.insert("HTTPS_PROXY".into(), format!("http://{}", company.at).into());
            (ORIGIN, None)
        }
        Way::Direct(d) => {
            here.host_roots = Some(vec![d.public.der.clone()]);
            // (its listener beyond this machine: loopback taken for it, or a policy that knows no own address)
            #[cfg(not(target_os = "linux"))]
            super::TEST_BEYOND.lock().unwrap().push((root.clone(), d.at.ip()));
            #[cfg(target_os = "linux")]
            {
                here.addresses = Some(Vec::new());
            }
            (DIRECT, Some(d.at.port()))
        }
    };
    // (the CA variables the test gives alone — a machine's own, the rust image's distro bundle, never: what the
    // bundle holds is the test's to say)
    here.env.retain(|k, _| !crate::sandboxpack::get().proxy.trust.contains(k));
    for (k, v) in &given.vars {
        here.env.insert(k.to_string(), v.into());
    }
    here.system_roots = Some(system_roots());
    let at_port = |h: &str| port.map_or(h.to_string(), |p| format!("{h}:{p}"));
    here.terminate = given.terminate.iter().map(|h| at_port(h)).collect();
    // (any host: what's checked is the run's trust, not which hosts are asked)
    let llm =
        crate::llm::LlmConfig { sandbox_network: true, sandbox_hosts: crate::llm::Hosts::Any, ..given.llm };
    let w = crate::tools::Widened { llm, ..Default::default() };
    let p = crate::sandbox::policy_from(&root, w, &here).unwrap();
    if p.proxy_url().is_none() {
        skip(test, &format!("no way to the proxy here: {:?}", p.reach));
        return None;
    }
    let flags = given.flags;
    let script = format!("curl -sS {flags} --max-time 15 https://{}/x; echo \" exit=$?\"", at_port(host));
    let mut cmd = crate::sandbox::command(Some(&p), "/bin/sh", &["-c".into(), script]);
    cmd.current_dir(&root);
    let ended = crate::sandbox::after_run(Some(&p));
    let (_, out, err) =
        finish(crate::sandbox::spawn(Some(&p), &cmd).expect("the run starts"), crate::test_results::PATIENCE);
    ended();
    #[cfg(target_os = "linux")]
    if let Some(Ok(c)) = &p.confine
        && let Some(k) = &c.keeper
    {
        k.end();
    }
    then(&p, &root);
    drop(base);
    Some((format!("{out}\n— stderr —\n{err}"), p.run_env.notes.join("\n")))
}

/// The company's root in a file of its own, outside the project — what a CA variable or `llm.sandbox-ca` names.
pub(crate) fn root_file(company: &Company, at: &Path) -> PathBuf {
    let f = at.join("company-root.pem");
    std::fs::write(&f, pem(&company.root.der)).unwrap();
    f
}

/// #419: behind a company's intercepting proxy, a run's own TLS client sees the company's certificate. With the
/// company's root in the file tarae's own `SSL_CERT_FILE` names (where a company's setup puts it), the run trusts
/// it too — its fetch goes through, tarae's proxy relaying it unread, the company's proxy asked for the tunnel.
#[test]
fn a_runs_fetch_through_a_company_mitm_trusts_your_ca_variables_root() {
    let test = "mitm_tests::a_runs_fetch_through_a_company_mitm_trusts_your_ca_variables_root";
    let company = Company::start();
    let scratch = crate::scratch::Scratch::new("mitm-419-var");
    let given = Given { vars: vec![("SSL_CERT_FILE", root_file(&company, &scratch))], ..Default::default() };
    let Some((said, report)) = fetched(test, &company, given) else { return };
    assert!(said.starts_with(&format!("{ANSWER} exit=0")), "through the company's proxy: {said}");
    assert_eq!(*company.asked.lock().unwrap(), [format!("{ORIGIN}:443")], "{said}");
    assert!(
        report.contains("TLS: your commands trust the system's roots + 1 from your CA variables — "),
        "{report}"
    );
}

/// The same with the company's root in a file `llm.sandbox-ca` names (yours only: where a root lives that no
/// variable names — one installed only in Windows' store, under WSL).
#[test]
fn a_runs_fetch_through_a_company_mitm_trusts_your_ca_setting() {
    let test = "mitm_tests::a_runs_fetch_through_a_company_mitm_trusts_your_ca_setting";
    let company = Company::start();
    let scratch = crate::scratch::Scratch::new("mitm-419-ca");
    let file = root_file(&company, &scratch).display().to_string();
    let llm = crate::llm::LlmConfig { sandbox_ca: vec![file], ..Default::default() };
    let Some((said, report)) = fetched(test, &company, Given { llm, ..Default::default() }) else { return };
    assert!(said.starts_with(&format!("{ANSWER} exit=0")), "through the company's proxy: {said}");
    assert!(
        report.contains("TLS: your commands trust the system's roots + 1 from llm.sandbox-ca — "),
        "{report}"
    );
}

/// Every file under `dirs` (links not followed, a file read up to 16 MB) holding one of `secrets` — their paths.
fn holding(dirs: &[PathBuf], secrets: &[Vec<u8>]) -> Vec<PathBuf> {
    assert!(secrets.len() >= 4 && secrets.iter().all(|s| s.len() >= 32), "the keys and their scalars");
    let (mut found, mut walk) = (Vec::new(), dirs.to_vec());
    while let Some(d) = walk.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let (path, Ok(kind)) = (e.path(), e.file_type()) else { continue };
            if kind.is_dir() {
                walk.push(path);
            } else if kind.is_file()
                && let Ok(f) = std::fs::File::open(&path)
            {
                let mut bytes = Vec::new();
                let _ = f.take(16 << 20).read_to_end(&mut bytes);
                if secrets.iter().any(|s| bytes.windows(s.len()).any(|w| w == s.as_slice())) {
                    found.push(path);
                }
            }
        }
    }
    found
}

/// P1a: a host the run's proxy terminates, behind the company's proxy whose root `llm.sandbox-ca` names: the run's own
/// curl goes through — its TLS tarae's, the leaf it saw issued by the session's CA (curl's `issuer:`), its trust
/// bundle carrying that CA first, the company's proxy asked for the tunnel by tarae. And the CA's keys — its PKCS#8
/// and its scalar, the leaves' too — are in no file of the run's folder, its runs' folder or the project (test 3).
#[test]
fn a_terminated_fetch_through_a_company_mitm_trusts_your_ca_setting() {
    let test = "mitm_tests::a_terminated_fetch_through_a_company_mitm_trusts_your_ca_setting";
    let company = Company::start();
    let scratch = crate::scratch::Scratch::new("mitm-p1a-ca");
    let file = root_file(&company, &scratch).display().to_string();
    let llm = crate::llm::LlmConfig { sandbox_ca: vec![file], ..Default::default() };
    let given = Given { llm, terminate: vec![ORIGIN], flags: "-v", ..Default::default() };
    let mut found = None;
    let fetched = fetched_then(test, &company, given, |p, root| {
        let t = p.egress.terminate.as_ref().expect("its rules terminate the origin");
        let mut dirs = vec![root.to_path_buf()];
        if let Some(run) = p.run.as_ref().map(|r| r.path().to_path_buf()) {
            dirs.extend(run.parent().map(Path::to_path_buf));
            dirs.push(run);
        }
        found = Some(holding(&dirs, &t.ca().secrets));
    });
    let Some((said, report)) = fetched else { return };
    assert!(said.starts_with(&format!("{ANSWER} exit=0")), "through tarae's TLS both ways: {said}");
    assert!(said.contains("issuer: CN=tarae sandbox proxy CA"), "the leaf it saw is tarae's: {said}");
    assert_eq!(*company.asked.lock().unwrap(), [format!("{ORIGIN}:443")], "{said}");
    assert!(
        report.contains(
            "TLS: your commands trust tarae's own CA (for the 1 host its proxy terminates) + the system's roots + 1 \
             from llm.sandbox-ca — "
        ),
        "{report}"
    );
    assert_eq!(found, Some(Vec::new()), "the CA's keys written somewhere");
}

/// P1a's 1d, most users' case: no upstream proxy — the run's own curl reaches a terminated host directly through
/// tarae's proxy, its TLS tarae's (curl's `issuer:` the session's CA), the host's leaf verified on tarae's leg against
/// the CA the system would hold (`Here.host_roots`), nothing in `llm.sandbox-ca`.
#[test]
fn a_terminated_fetch_goes_directly_with_no_upstream() {
    let test = "mitm_tests::a_terminated_fetch_goes_directly_with_no_upstream";
    let Some(direct) = Direct::start() else {
        return skip(test, "no address of this machine's beyond loopback");
    };
    let given = Given { terminate: vec![DIRECT], flags: "-v", ..Default::default() };
    let Some((said, report)) = fetched_by(test, Way::Direct(&direct), given, |_, _| {}) else { return };
    assert!(said.starts_with(&format!("{ANSWER} exit=0")), "through tarae's TLS both ways: {said}");
    assert!(said.contains("issuer: CN=tarae sandbox proxy CA"), "the leaf it saw is tarae's: {said}");
    assert!(
        report.contains("TLS: your commands trust tarae's own CA (for the 1 host its proxy terminates) + the system's roots — "),
        "{report}"
    );
}

/// P1a: the same with the company's root nowhere tarae reads one — the run's curl reads tarae's 502 inside its TLS,
/// naming the company's CA as its certificate does, and the way out.
#[test]
fn a_terminated_fetch_without_the_companys_root_reads_why() {
    let test = "mitm_tests::a_terminated_fetch_without_the_companys_root_reads_why";
    let company = Company::start();
    let given = Given { terminate: vec![ORIGIN], ..Default::default() };
    let Some((said, _)) = fetched(test, &company, given) else { return };
    let why = format!(
        "tarae's proxy: {ORIGIN}:443: the certificate it showed through tarae's HTTPS_PROXY isn't one this system \
         trusts (the certificate's issuer says: CN \"tarae test company CA\") — if that proxy re-signs TLS, add its \
         root to llm.sandbox-ca\n exit=0"
    );
    assert!(said.starts_with(&why), "{said}");
}

/// P1a: a host the run's rules don't terminate stays the host's own TLS, relayed unread — the company's leaf, which
/// the bundle's company root (your CA variable's) verifies — though the bundle carries tarae's CA for another.
#[test]
fn a_host_not_terminated_stays_opaque_beside_one_that_is() {
    let test = "mitm_tests::a_host_not_terminated_stays_opaque_beside_one_that_is";
    let company = Company::start();
    let scratch = crate::scratch::Scratch::new("mitm-p1a-opaque");
    let vars = vec![("SSL_CERT_FILE", root_file(&company, &scratch))];
    let given = Given { vars, terminate: vec!["other.example.invalid"], flags: "-v", ..Default::default() };
    let Some((said, report)) = fetched(test, &company, given) else { return };
    assert!(said.starts_with(&format!("{ANSWER} exit=0")), "{said}");
    assert!(said.contains("issuer: CN=tarae test company CA"), "the company's own leaf: {said}");
    assert!(report.contains("TLS: your commands trust tarae's own CA (for the 1 host"), "{report}");
}

/// With the company's root nowhere tarae reads one, the run's fetch fails as curl fails a certificate it can't
/// trust (exit 60), its bundle the system's roots alone — tarae never trusts for it.
#[test]
fn a_runs_fetch_through_a_company_mitm_fails_without_its_root() {
    let test = "mitm_tests::a_runs_fetch_through_a_company_mitm_fails_without_its_root";
    let company = Company::start();
    let Some((said, report)) = fetched(test, &company, Given::default()) else { return };
    assert!(said.contains(" exit=60"), "curl's certificate error: {said}");
    assert!(!said.contains(ANSWER), "{said}");
    assert!(report.contains("TLS: your commands trust the system's roots — "), "{report}");
}
