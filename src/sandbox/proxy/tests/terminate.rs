//! P1a: a tunnel to a host its run's rules terminate, on loopback — the run's TLS ends at tarae's proxy under a
//! leaf of the session's CA, the host's TLS is tarae's own, verified against the host leg's roots (the test's
//! "public" CA standing for the system's: `Terminate.system`), directly or through tarae's upstream proxy (a
//! company's intercepting one, `mitm_tests::Company`). However the run's connection came — the supervisor's pair, a
//! run's own port (macOS), a session's port with its token — it's a run's, never tarae's own fetch's.

use super::super::mitm_tests::{ANSWER, Company, ORIGIN};
use super::super::terminate::{self, Terminate};
use super::super::upstream::{self as up, Setting};
use super::*;
use crate::web::fetch::tests::{Authority, authority, issued, trusting};

const NAME: &str = "origin.test";

/// The session numbers these tests use, apart from every other test's (the CAs are the process's).
const SESSION: u64 = 0x7e12_b000;

/// A head read to its blank line (or the stream's end).
fn read_head(s: &mut impl Read) -> Vec<u8> {
    let (mut got, mut b) = (Vec::new(), [0u8; 1]);
    while !got.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) => got.push(b[0]),
            _ => break,
        }
    }
    got
}

/// An origin on loopback under a leaf its "public" CA issued for `NAME` and `opaque.test`: every request's head sent on `got`, answered
/// `ANSWER` with close_notify.
struct Origin {
    at: SocketAddr,
    public: Authority,
    got: Receiver<Vec<u8>>,
}

fn origin() -> Origin {
    let public = authority("tarae test public CA");
    let config = issued(&public, &[NAME, "opaque.test"]);
    let l = TcpListener::bind((LO, 0)).unwrap();
    let at = l.local_addr().unwrap();
    let (tx, got) = mpsc::channel();
    crate::event::thread("tarae-p1a-test", move || {
        for s in l.incoming() {
            let (Ok(s), config, tx) = (s, config.clone(), tx.clone()) else { break };
            crate::event::thread("tarae-p1a-test", move || {
                let _ = s.set_read_timeout(Some(WAIT));
                let mut t = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), s);
                let head = read_head(&mut t);
                let _ = tx.send(head.clone());
                if head.is_empty() {
                    return;
                }
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ANSWER}",
                    ANSWER.len()
                );
                let _ = t.write_all(answer.as_bytes());
                t.conn.send_close_notify();
                let _ = t.flush();
            });
        }
    });
    Origin { at, public, got }
}

/// The rules naming `NAME` (reached at `o`) and terminating it with session `session`'s CA, the host's leg trusting
/// `o`'s public CA — and that CA.
fn terminating(
    session: u64,
    system: Vec<Vec<u8>>,
    ca_files: Vec<std::path::PathBuf>,
) -> (Egress, Arc<terminate::Ca>) {
    let ca = terminate::of_session(session).unwrap();
    let mut t = Terminate::new(vec![Pattern::parse(NAME).unwrap()], ca.clone(), ca_files);
    t.system = Some(system);
    (Egress { terminate: Some(t), ..rules(&[NAME, "opaque.test"]) }, ca)
}

/// What a run's TLS client (trusting `roots` alone) gets through an open tunnel `c` to `name`: the answer read whole,
/// and the leaf it saw — or why its TLS failed.
fn fetch_in(c: TcpStream, name: &str, roots: &[&[u8]]) -> Result<(String, Vec<u8>), String> {
    let server = rustls::pki_types::ServerName::try_from(name.to_string()).unwrap();
    let mut t = rustls::StreamOwned::new(rustls::ClientConnection::new(trusting(roots), server).unwrap(), c);
    let ask = format!("GET /x HTTP/1.1\r\nHost: {name}\r\nConnection: close\r\n\r\n");
    t.write_all(ask.as_bytes()).map_err(|e| e.to_string())?;
    let mut got = Vec::new();
    t.read_to_end(&mut got).map_err(|e| e.to_string())?;
    let leaf = t.conn.peer_certificates().and_then(|c| c.first()).map(|c| c.to_vec()).unwrap_or_default();
    Ok((String::from_utf8_lossy(&got).into_owned(), leaf))
}

/// An open tunnel to `name:443` on connection `c`: its `200` read.
fn opened(mut c: TcpStream, head: &str) -> TcpStream {
    assert_eq!(ask(&mut c, head), 200);
    c
}

/// The CN of the issuer of `der`.
fn issued_by(der: &[u8]) -> String {
    super::super::issuer::of(der).and_then(|i| i.cn).unwrap_or_default()
}

/// A host its rules terminate: the run's TLS is tarae's, under a leaf of the session's CA for the CONNECT target, and
/// its request reaches the host as it sent it, the host's answer back whole — whichever way the run's connection came:
/// the supervisor's pair (`Connection`, Linux), a run's own port with its token (`RunStarts`, macOS), a session's
/// port with the run's token (`RunToken`, the unserved tier) — each a run's, never tarae's own fetch's. A host they
/// don't terminate stays the host's own TLS, relayed unread.
#[test]
fn a_listed_hosts_tunnel_is_terminated_each_way_a_run_comes() {
    let o = origin();
    let (resolve, _) = resolver(vec![(NAME, vec![vec![o.at]]), ("opaque.test", vec![vec![o.at]])]);
    let f = start(resolve, limits());
    let (rules, ca) = terminating(SESSION + 1, vec![o.public.der.clone()], Vec::new());
    let check = |c: TcpStream, way: &str| {
        let (answer, leaf) = fetch_in(c, NAME, &[ca.der()]).unwrap_or_else(|e| panic!("{way}: {e}"));
        assert!(answer.starts_with("HTTP/1.1 200 ") && answer.ends_with(ANSWER), "{way}: {answer}");
        assert_eq!(issued_by(&leaf), "tarae sandbox proxy CA", "{way}: tarae's leaf");
        let head = String::from_utf8(o.got.recv_timeout(WAIT).unwrap()).unwrap();
        assert_eq!(head, format!("GET /x HTTP/1.1\r\nHost: {NAME}\r\nConnection: close\r\n\r\n"), "{way}");
    };
    // The supervisor's pair (its `ours`: None)
    let (c, _) = connection(&f.proxy, SESSION + 1, 1, rules.clone());
    check(opened(c, &connect_head(&format!("{NAME}:443"))), "the supervisor's pair");
    // A run's own port (macOS): its token the password
    let l = TcpListener::bind((LO, 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let (told, _d) = mpsc::channel();
    let token = "tarae-fake-token-p1a".to_string();
    let (listener, process, r) = (l.into(), "curl".to_string(), Arc::new(rules.clone()));
    let started =
        ToProxy::RunStarts { run: 2, session: SESSION + 1, process, listener, token, rules: r, told };
    assert!(f.proxy.send(started));
    let c = TcpStream::connect((LO, port)).unwrap();
    c.set_read_timeout(Some(WAIT)).unwrap();
    let head =
        format!("CONNECT {NAME}:443 HTTP/1.1\r\nHost: {NAME}:443\r\n{}\r\n", basic("tarae-fake-token-p1a"));
    check(opened(c, &head), "a run's own port");
    // A session's port, the run known by its token
    let port = session_port(&f.proxy, SESSION + 1);
    let (t, _d) = token_run(&f.proxy, SESSION + 1, 3, rules.clone());
    let c = TcpStream::connect((LO, port)).unwrap();
    c.set_read_timeout(Some(WAIT)).unwrap();
    let head = format!("CONNECT {NAME}:443 HTTP/1.1\r\nHost: {NAME}:443\r\n{}\r\n", basic(t.token()));
    check(opened(c, &head), "a session's port");
    // A host they don't terminate: the host's own TLS, relayed (tarae's CA isn't trusted for it, the public one is)
    let (c, _) = connection(&f.proxy, SESSION + 1, 4, rules.clone());
    let c = opened(c, &connect_head("opaque.test:443"));
    let err = fetch_in(c, "opaque.test", &[ca.der()]).unwrap_err();
    assert!(err.contains("UnknownIssuer") || err.contains("certificate"), "{err}");
    terminate::forget(SESSION + 1);
}

/// The host's leg failing — its certificate not one its roots trust — is a 502 the run's own client reads inside its
/// TLS (its 200 was sent already), with why and the issuer the certificate names, framed as its own words; the
/// report says the same.
#[test]
fn the_hosts_leg_failing_is_a_502_inside_tls_naming_its_issuer() {
    let o = origin();
    let (resolve, _) = resolver(vec![(NAME, vec![vec![o.at]])]);
    let f = start(resolve, limits());
    let (rules, ca) = terminating(SESSION + 2, vec![authority("unrelated").der], Vec::new());
    let (c, d) = connection(&f.proxy, SESSION + 2, 1, rules);
    let (answer, _) = fetch_in(opened(c, &connect_head(&format!("{NAME}:443"))), NAME, &[ca.der()]).unwrap();
    let why = "origin.test:443: the certificate it showed isn't one this system trusts (the certificate's issuer \
               says: CN \"tarae test public CA\") — a company's root goes in the system's store, or in llm.sandbox-ca";
    assert!(answer.starts_with("HTTP/1.1 502 Bad Gateway\r\n"), "{answer}");
    assert!(answer.ends_with(&format!("tarae's proxy: {why}\n")), "{answer}");
    assert_eq!(said(&d), format!("{why} — answered 502"));
    terminate::forget(SESSION + 2);
}

/// Behind a company's intercepting proxy (tarae's own upstream): the host's leg goes through it, and trusts the
/// company's root where `llm.sandbox-ca` names it — the run's fetch through. Where it doesn't, the 502 and the report
/// say so as the upstream's (its op), naming the company's CA as its certificate does and the way out.
#[test]
fn a_terminated_tunnel_goes_through_taraes_upstream() {
    let company = Company::start();
    let scratch = crate::scratch::Scratch::new("p1a-company");
    let root = super::super::mitm_tests::root_file(&company, &scratch);
    let f = start(resolver(Vec::new()).0, limits());
    let ca = terminate::of_session(SESSION + 3).unwrap();
    let unrelated = authority("unrelated").der;
    let url = format!("http://{}", company.at);
    let env: HashMap<String, std::ffi::OsString> = [("HTTPS_PROXY".to_string(), url.into())].into();
    let rules_with = |files: Vec<std::path::PathBuf>| {
        let mut t = Terminate::new(vec![Pattern::parse(ORIGIN).unwrap()], ca.clone(), files);
        t.system = Some(vec![unrelated.clone()]);
        Egress { terminate: Some(t), upstream: up::of(&env, Setting::Env), ..rules(&[ORIGIN]) }
    };
    let head = connect_head(&format!("{ORIGIN}:443"));
    let (c, _) = connection(&f.proxy, SESSION + 3, 1, rules_with(vec![root]));
    let (answer, leaf) = fetch_in(opened(c, &head), ORIGIN, &[ca.der()]).unwrap();
    assert!(answer.ends_with(ANSWER), "{answer}");
    assert_eq!(issued_by(&leaf), "tarae sandbox proxy CA");
    assert_eq!(*company.asked.lock().unwrap(), [format!("{ORIGIN}:443")]);
    // The company's root nowhere: the upstream's failure, said with its issuer
    let (c, d) = connection(&f.proxy, SESSION + 3, 2, rules_with(Vec::new()));
    let (answer, _) = fetch_in(opened(c, &head), ORIGIN, &[ca.der()]).unwrap();
    let why = format!(
        "{ORIGIN}:443: the certificate it showed through tarae's HTTPS_PROXY isn't one this system trusts (the \
         certificate's issuer says: CN \"tarae test company CA\") — if that proxy re-signs TLS, add its root to \
         llm.sandbox-ca"
    );
    assert!(answer.starts_with("HTTP/1.1 502 ") && answer.contains(&why), "{answer}");
    let d = d.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (d.op.as_str(), d.denial.target.as_str()),
        (UPSTREAM_OP, format!("{why} — answered 502").as_str())
    );
    terminate::forget(SESSION + 3);
}

/// The same behind a company's https:// proxy: the host's leg is TLS inside tarae's TLS to the proxy (the bridge's
/// end as `up`), the company re-signing the origin behind it. With its root in `llm.sandbox-ca` the run's fetch goes
/// through; without it on the host's leg (the proxy's own TLS still trusted) the 502 names the company's CA.
#[test]
fn a_terminated_tunnel_goes_through_an_https_upstream() {
    let company = Company::start_https();
    let scratch = crate::scratch::Scratch::new("p1a-company-https");
    let root = super::super::mitm_tests::root_file(&company, &scratch);
    let (resolve, _) = resolver(vec![("localhost", vec![vec![company.at]])]);
    let f = start(resolve, limits());
    let ca = terminate::of_session(SESSION + 6).unwrap();
    let unrelated = authority("unrelated").der;
    let url = format!("https://localhost:{}", company.at.port());
    let env: HashMap<String, std::ffi::OsString> = [("HTTPS_PROXY".to_string(), url.into())].into();
    let upstream = up::of(&env, Setting::Env).with_ca(std::slice::from_ref(&root));
    let rules_with = |files: Vec<std::path::PathBuf>| {
        let mut t = Terminate::new(vec![Pattern::parse(ORIGIN).unwrap()], ca.clone(), files);
        t.system = Some(vec![unrelated.clone()]);
        Egress { terminate: Some(t), upstream: upstream.clone(), ..rules(&[ORIGIN]) }
    };
    let head = connect_head(&format!("{ORIGIN}:443"));
    let (c, _) = connection(&f.proxy, SESSION + 6, 1, rules_with(vec![root.clone()]));
    let (answer, leaf) = fetch_in(opened(c, &head), ORIGIN, &[ca.der()]).unwrap();
    assert!(answer.starts_with("HTTP/1.1 200 ") && answer.ends_with(ANSWER), "{answer}");
    assert_eq!(issued_by(&leaf), "tarae sandbox proxy CA");
    assert_eq!(*company.asked.lock().unwrap(), [format!("{ORIGIN}:443")]);
    // The host's leg without the company's root: its failure said as the upstream's, with its issuer
    let (c, d) = connection(&f.proxy, SESSION + 6, 2, rules_with(Vec::new()));
    let (answer, _) = fetch_in(opened(c, &head), ORIGIN, &[ca.der()]).unwrap();
    let why = format!(
        "{ORIGIN}:443: the certificate it showed through tarae's HTTPS_PROXY isn't one this system trusts (the \
         certificate's issuer says: CN \"tarae test company CA\") — if that proxy re-signs TLS, add its root to \
         llm.sandbox-ca"
    );
    assert!(answer.starts_with("HTTP/1.1 502 ") && answer.contains(&why), "{answer}");
    let d = d.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (d.op.as_str(), d.denial.target.as_str()),
        (UPSTREAM_OP, format!("{why} — answered 502").as_str())
    );
    terminate::forget(SESSION + 6);
}

// --- Measures, by hand: `cargo test --bin tarae measure_p1a -- --ignored --nocapture` (outside tarae's sandbox: the
// system's verifier asks trustd) ---

/// The median of `n` runs of `f`, in ms.
fn median_ms(n: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[n / 2]
}

/// P1a's measures: a small GET terminated against the same GET relayed unread (median of 20, ms, loopback, the test's
/// verifier); the footprint 32 open terminated tunnels add against 32 opaque ones, a tunnel's share.
#[test]
#[ignore = "a measure, by hand"]
fn measure_p1a_latency_and_memory() {
    let o = origin();
    let (resolve, _) = resolver(vec![(NAME, vec![vec![o.at]]), ("opaque.test", vec![vec![o.at]])]);
    let f = start(resolve, limits());
    let (rules, ca) = terminating(SESSION + 9, vec![o.public.der.clone()], Vec::new());
    let get = |host: &str, roots: &[&[u8]]| {
        let (c, _) = connection(&f.proxy, SESSION + 9, 1, rules.clone());
        let (answer, _) = fetch_in(opened(c, &connect_head(&format!("{host}:443"))), host, roots).unwrap();
        assert!(answer.ends_with(ANSWER));
        let _ = o.got.recv_timeout(WAIT);
    };
    let terminated = median_ms(20, || get(NAME, &[ca.der()]));
    let opaque = median_ms(20, || get("opaque.test", &[&o.public.der]));
    eprintln!("measure_p1a: a small GET, median of 20: terminated {terminated:.2} ms, opaque {opaque:.2} ms");
    #[cfg(target_os = "macos")]
    {
        let footprint =
            || crate::procs::read(std::process::id()).and_then(|p| p.footprint).unwrap_or(0) as f64;
        let hold =
            |host: &str, roots: &[&[u8]]| -> Vec<rustls::StreamOwned<rustls::ClientConnection, TcpStream>> {
                (0..32)
                    .map(|_| {
                        let (c, _) = connection(&f.proxy, SESSION + 9, 1, rules.clone());
                        let c = opened(c, &connect_head(&format!("{host}:443")));
                        let server = rustls::pki_types::ServerName::try_from(host.to_string()).unwrap();
                        let conn = rustls::ClientConnection::new(trusting(roots), server).unwrap();
                        let mut t = rustls::StreamOwned::new(conn, c);
                        while t.conn.is_handshaking() {
                            t.conn.complete_io(&mut t.sock).unwrap();
                        }
                        t
                    })
                    .collect()
            };
        let base = footprint();
        let held = hold("opaque.test", &[&o.public.der]);
        let opaque = (footprint() - base) / 32.0;
        drop(held);
        std::thread::sleep(Duration::from_millis(300));
        let base = footprint();
        let held = hold(NAME, &[ca.der()]);
        let terminated = (footprint() - base) / 32.0;
        drop(held);
        eprintln!(
            "measure_p1a: footprint a tunnel, 32 open: terminated {:.0} KiB, opaque {:.0} KiB",
            terminated / 1024.0,
            opaque / 1024.0
        );
    }
    terminate::forget(SESSION + 9);
}

/// P1a's stack measure: a terminated GET whose host's leg is checked by the system's own verifier (macOS's
/// `SecTrust`; the public root as `llm.sandbox-ca`'s), its connection thread's stack `TARAE_P1A_STACK` KiB (256: the
/// proxy's) — run at smaller sizes by hand; a stack too small aborts the run.
#[test]
#[ignore = "a measure, by hand"]
fn measure_p1a_stack() {
    let kib: usize = std::env::var("TARAE_P1A_STACK").ok().and_then(|s| s.parse().ok()).unwrap_or(256);
    super::super::CONN_STACK.store(kib << 10, Ordering::Relaxed);
    let o = origin();
    let scratch = crate::scratch::Scratch::new("p1a-stack");
    let root = scratch.join("public.pem");
    std::fs::write(&root, crate::sandbox::cabundle::pem(&o.public.der)).unwrap();
    let (resolve, _) = resolver(vec![(NAME, vec![vec![o.at]])]);
    let f = start(resolve, limits());
    let ca = terminate::of_session(SESSION + 10).unwrap();
    // (`system` None: the system's verifier, `llm.sandbox-ca`'s root besides)
    let t = Terminate::new(vec![Pattern::parse(NAME).unwrap()], ca.clone(), vec![root]);
    let (c, _) = connection(&f.proxy, SESSION + 10, 1, Egress { terminate: Some(t), ..rules(&[NAME]) });
    let (answer, _) = fetch_in(opened(c, &connect_head(&format!("{NAME}:443"))), NAME, &[ca.der()]).unwrap();
    assert!(answer.ends_with(ANSWER), "{answer}");
    eprintln!("measure_p1a: a terminated GET, the system's verifier, on a {kib} KiB stack: through");
    terminate::forget(SESSION + 10);
}

/// A CONNECT to a host its rules terminate whose first bytes aren't TLS is refused as every tunnel's is — in the
/// same words — and nothing is terminated: the host's leg never starts.
#[test]
fn a_non_tls_connect_to_a_listed_host_is_refused_as_ever() {
    let o = origin();
    let (resolve, _) = resolver(vec![(NAME, vec![vec![o.at]])]);
    let f = start(resolve, limits());
    let (rules, _) = terminating(SESSION + 4, vec![o.public.der.clone()], Vec::new());
    let (c, d) = connection(&f.proxy, SESSION + 4, 1, rules);
    let mut c = opened(c, &connect_head(&format!("{NAME}:443")));
    c.write_all(b"GET /x HTTP/1.1\r\nHost: origin.test\r\n\r\n").unwrap();
    assert!(to_close(&mut c).is_empty(), "nothing back");
    let said = said(&d);
    assert!(said.starts_with("origin.test:443: not TLS") && said.ends_with(" — the tunnel closed"), "{said}");
    terminate::forget(SESSION + 4);
}

/// A run whose TLS client doesn't trust tarae's CA (one that doesn't read the run's trust bundle) refuses tarae's
/// leaf — said in words that point at the bundle.
#[test]
fn the_run_refusing_taraes_leaf_is_said() {
    let o = origin();
    let (resolve, _) = resolver(vec![(NAME, vec![vec![o.at]])]);
    let f = start(resolve, limits());
    let (rules, _) = terminating(SESSION + 5, vec![o.public.der.clone()], Vec::new());
    let (c, d) = connection(&f.proxy, SESSION + 5, 1, rules);
    let c = opened(c, &connect_head(&format!("{NAME}:443")));
    assert!(fetch_in(c, NAME, &[&o.public.der]).is_err(), "tarae's leaf isn't the public CA's");
    assert_eq!(
        said(&d),
        "origin.test:443: the run's TLS client refused tarae's certificate — it doesn't read the run's trust \
         bundle (SSL_CERT_FILE and the rest) — the tunnel closed"
    );
    terminate::forget(SESSION + 5);
}
