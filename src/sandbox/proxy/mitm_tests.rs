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
const ORIGIN: &str = "origin-419.example.invalid";
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
        let over = Arc::new(AtomicBool::new(false));
        let public = authority("tarae test public CA");
        let origin = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let origin_at = origin.local_addr().unwrap();
        let served = issued(&public, &[ORIGIN]);
        accepting(origin, over.clone(), move |s| {
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
        let root = Arc::new(authority("tarae test company CA"));
        let (asked, company) = (Arc::new(Mutex::new(Vec::new())), root.clone());
        let into = asked.clone();
        let public = Arc::new(public.der);
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let at = l.local_addr().unwrap();
        accepting(l, over.clone(), move |mut s| {
            let connect = String::from_utf8_lossy(&head(&mut s)).into_owned();
            let Some(target) = connect.strip_prefix("CONNECT ").and_then(|r| r.split(' ').next()) else {
                return;
            };
            into.lock().unwrap().push(target.to_string());
            let name = target.rsplit_once(':').map_or(target, |(n, _)| n).to_string();
            let _ = s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
            // Inside: the company's own leaf for the name asked — the interception
            let mine = issued(&company, &[&name]);
            let mut inside = rustls::StreamOwned::new(rustls::ServerConnection::new(mine).unwrap(), s);
            let request = head(&mut inside);
            if request.is_empty() {
                return;
            }
            // …and the request taken on to the origin, verified as the company verifies it
            let up = TcpStream::connect(origin_at).unwrap();
            let _ = up.set_read_timeout(Some(crate::test_results::PATIENCE));
            let server = rustls::pki_types::ServerName::try_from(name).unwrap();
            let c = rustls::ClientConnection::new(trusting(&[&public]), server).unwrap();
            let mut out = rustls::StreamOwned::new(c, up);
            let mut answer = Vec::new();
            if out.write_all(&request).is_ok() {
                let _ = out.read_to_end(&mut answer);
            }
            let _ = inside.write_all(&answer);
            inside.conn.send_close_notify();
            let _ = inside.flush();
        });
        Company { at, root, asked, over }
    }
}

/// What a fetch of the run's was given: tarae's own environment's variables besides the upstream (a CA variable's
/// file), and the settings.
#[derive(Default)]
pub(crate) struct Given {
    pub vars: Vec<(&'static str, PathBuf)>,
    pub llm: crate::llm::LlmConfig,
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
    // (as tarae's environment would name its upstream and its CA files — a test's own `Here` keeps no proxy
    // variable of the process's)
    here.env.insert("HTTPS_PROXY".into(), format!("http://{}", company.at).into());
    // (the CA variables the test gives alone — a machine's own, the rust image's distro bundle, never: what the
    // bundle holds is the test's to say)
    here.env.retain(|k, _| !crate::sandboxpack::get().proxy.trust.contains(k));
    for (k, v) in &given.vars {
        here.env.insert(k.to_string(), v.into());
    }
    here.system_roots = Some(system_roots());
    // (any host: what's checked is the run's trust, not which hosts are asked)
    let llm =
        crate::llm::LlmConfig { sandbox_network: true, sandbox_hosts: crate::llm::Hosts::Any, ..given.llm };
    let w = crate::tools::Widened { llm, ..Default::default() };
    let p = crate::sandbox::policy_from(&root, w, &here).unwrap();
    if p.proxy_url().is_none() {
        skip(test, &format!("no way to the proxy here: {:?}", p.reach));
        return None;
    }
    let script = format!("curl -sS --max-time 15 https://{ORIGIN}/x; echo \" exit=$?\"");
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
