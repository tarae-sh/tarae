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

// --- P1b: the stage between the legs — requests decided, answers followed, keep-alive and its limits ---

/// The bytes `/download` answers with.
const DOWNLOAD: usize = 100 << 10;

/// A kept origin's answer to `request` (its head and body): `/download` `DOWNLOAD` seeded bytes, else `ANSWER` — each
/// with its length.
fn answer_to(request: &[u8]) -> Vec<u8> {
    let body = match request.starts_with(b"GET /download ") {
        true => seeded(0xd0d0, DOWNLOAD),
        false => ANSWER.as_bytes().to_vec(),
    };
    [format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes(), &body].concat()
}

/// A request's body read from `t` after its head (in `request`), added to it: its Content-Length's count, or a
/// chunked body to its end as a strict reader ends it (`check::chunks`) — false where the connection ended first.
fn read_body(t: &mut impl Read, request: &mut Vec<u8>) -> bool {
    let head = String::from_utf8_lossy(request).to_ascii_lowercase();
    let length =
        head.split("\r\n").find_map(|l| l.strip_prefix("content-length:")?.trim().parse::<usize>().ok());
    let chunked = head.contains("\r\ntransfer-encoding: chunked\r\n");
    let (start, mut b) = (request.len(), [0u8; 1]);
    let more = |request: &Vec<u8>| match length {
        Some(n) => request.len() - start < n,
        None => chunked && super::super::parse::check::chunks(&request[start..]).is_none(),
    };
    while more(request) {
        match t.read(&mut b) {
            Ok(1) => request.push(b[0]),
            _ => return false,
        }
    }
    true
}

/// An origin on loopback (`origin`'s leaf) that keeps each connection for as many requests as come: each request
/// whole — its head and its body, as they came — sent on `got` (what came of one cut short too), then an empty one
/// when its connection ends; answered `answer_to` it — `/slow` only once `go` says.
struct Kept {
    at: SocketAddr,
    public: Authority,
    accepts: Arc<std::sync::atomic::AtomicUsize>,
    got: Receiver<Vec<u8>>,
    go: Sender<()>,
}

fn kept() -> Kept {
    let public = authority("tarae test public CA");
    let config = issued(&public, &[NAME]);
    let l = TcpListener::bind((LO, 0)).unwrap();
    let at = l.local_addr().unwrap();
    let (tx, got) = mpsc::channel();
    let (go, gate) = mpsc::channel::<()>();
    let gate = Arc::new(std::sync::Mutex::new(gate));
    let accepts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = accepts.clone();
    crate::event::thread("tarae-p1b-test", move || {
        for s in l.incoming() {
            let (Ok(s), config, tx, gate) = (s, config.clone(), tx.clone(), gate.clone()) else { break };
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            crate::event::thread("tarae-p1b-test", move || {
                let _ = s.set_read_timeout(Some(WAIT));
                let mut t = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), s);
                loop {
                    let mut request = read_head(&mut t);
                    let whole = request.ends_with(b"\r\n\r\n") && read_body(&mut t, &mut request);
                    if !request.is_empty() {
                        let _ = tx.send(request.clone());
                    }
                    if !whole {
                        let _ = tx.send(Vec::new());
                        return;
                    }
                    if request.starts_with(b"GET /slow ") {
                        let _ = gate.lock().unwrap().recv_timeout(WAIT);
                    }
                    if t.write_all(&answer_to(&request)).and_then(|()| t.flush()).is_err() {
                        let _ = tx.send(Vec::new());
                        return;
                    }
                }
            });
        }
    });
    Kept { at, public, accepts, got, go }
}

type Tls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

/// A proxy whose run's rules terminate `NAME` (reached at `o`) with session `session`'s CA, under `limits` — and the
/// CA's certificate.
fn terminating_kept(o: &Kept, session: u64, limits: Limits) -> (Fixture, Egress, Vec<u8>) {
    terminating_to(o.at, &o.public, session, limits)
}

/// `terminating_kept` for an origin at `at` under a leaf `public` issued.
fn terminating_to(
    at: SocketAddr,
    public: &Authority,
    session: u64,
    limits: Limits,
) -> (Fixture, Egress, Vec<u8>) {
    let (resolve, _) = resolver(vec![(NAME, vec![vec![at]])]);
    let (rules, ca) = terminating(session, vec![public.der.clone()], Vec::new());
    (start(resolve, limits), rules, ca.der().to_vec())
}

/// An origin on loopback (`origin`'s leaf) answering each request's head with `answer`, then ending — with
/// close_notify where `notify`, else its TCP shut with none (as a connection cut on the way cuts it).
fn ending(answer: &'static [u8], notify: bool) -> (SocketAddr, Authority) {
    let public = authority("tarae test public CA");
    let config = issued(&public, &[NAME]);
    let l = TcpListener::bind((LO, 0)).unwrap();
    let at = l.local_addr().unwrap();
    crate::event::thread("tarae-p1b-test", move || {
        for s in l.incoming() {
            let (Ok(s), config) = (s, config.clone()) else { break };
            crate::event::thread("tarae-p1b-test", move || {
                let _ = s.set_read_timeout(Some(WAIT));
                let mut t = rustls::StreamOwned::new(rustls::ServerConnection::new(config).unwrap(), s);
                if read_head(&mut t).is_empty() {
                    return;
                }
                let _ = t.write_all(answer);
                if notify {
                    t.conn.send_close_notify();
                }
                let _ = t.flush();
                let _ = t.sock.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    (at, public)
}

/// A tunnel to `NAME:443` on run `run`'s connection, the run's TLS over it (trusting `ca` alone) with its handshake
/// done — and the run's refusals.
fn tunnel_tls(f: &Fixture, session: u64, run: u64, rules: &Egress, ca: &[u8]) -> (Tls, Receiver<Relayed>) {
    let (c, d) = connection(&f.proxy, session, run, rules.clone());
    let c = opened(c, &connect_head(&format!("{NAME}:443")));
    let server = rustls::pki_types::ServerName::try_from(NAME).unwrap();
    let mut t = rustls::StreamOwned::new(rustls::ClientConnection::new(trusting(&[ca]), server).unwrap(), c);
    while t.conn.is_handshaking() {
        t.conn.complete_io(&mut t.sock).unwrap();
    }
    (t, d)
}

/// An answer read from `t`: its head (to its blank line), and its body by its Content-Length.
fn answer_of(t: &mut impl Read) -> (String, Vec<u8>) {
    let head = String::from_utf8(read_head(t)).unwrap();
    let low = head.to_ascii_lowercase();
    let n =
        low.split("\r\n").find_map(|l| l.strip_prefix("content-length:")?.trim().parse().ok()).unwrap_or(0);
    let mut body = vec![0; n];
    t.read_exact(&mut body).unwrap_or_else(|e| panic!("{head:?}: its body ({e})"));
    (head, body)
}

/// Whether `t` ends with close_notify — nothing more read before it (within the wait).
fn closed(t: &mut Tls) -> bool {
    matches!(t.read(&mut [0u8; 1]), Ok(0))
}

/// Whether `t` ends with no close_notify — its TCP's end alone, which its TLS reads as a truncation (`UnexpectedEof`),
/// within the wait.
fn cut(t: &mut Tls) -> bool {
    matches!(t.read_to_end(&mut Vec::new()), Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof)
}

/// What the origin got on one connection, to its end (the empty one), within the wait.
fn to_end(o: &Kept) -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    loop {
        match o.got.recv_timeout(WAIT).expect("the origin's connection ended") {
            r if r.is_empty() => return all,
            r => all.push(r),
        }
    }
}

/// A terminated tunnel is kept for the next request: two GETs one after the other, each answered, one connection
/// at the origin — each request reaching it as the run sent it.
#[test]
fn a_terminated_tunnel_keeps_its_connection_for_the_next_request() {
    let o = kept();
    let (f, rules, ca) = terminating_kept(&o, SESSION + 10, limits());
    let (mut t, _d) = tunnel_tls(&f, SESSION + 10, 1, &rules, &ca);
    let asks = [
        format!("GET /a HTTP/1.1\r\nHost: {NAME}\r\n\r\n"),
        format!("GET /b HTTP/1.1\r\nHost: {NAME}\r\nConnection: close\r\n\r\n"),
    ];
    for ask in &asks {
        t.write_all(ask.as_bytes()).unwrap();
        let (head, body) = answer_of(&mut t);
        assert!(head.starts_with("HTTP/1.1 200 ") && body == ANSWER.as_bytes(), "{head}");
    }
    assert!(closed(&mut t), "closed after the last, with close_notify");
    let got: Vec<String> = to_end(&o).into_iter().map(|r| String::from_utf8(r).unwrap()).collect();
    assert_eq!(got, asks);
    assert_eq!(o.accepts.load(std::sync::atomic::Ordering::SeqCst), 1, "one connection for both");
    terminate::forget(SESSION + 10);
}

/// A chunked upload reaches the origin byte for byte as the run sent it (extensions, trailers), and a Content-Length
/// download reaches the run byte for byte as the origin sent it.
#[test]
fn a_chunked_upload_and_a_length_download_pass_byte_for_byte() {
    let o = kept();
    let (f, rules, ca) = terminating_kept(&o, SESSION + 11, limits());
    let (mut t, _d) = tunnel_tls(&f, SESSION + 11, 1, &rules, &ca);
    let data = seeded(0x0b0d, 40_000);
    let mut up =
        format!("POST /up HTTP/1.1\r\nHost: {NAME}\r\nTransfer-Encoding: chunked\r\n\r\n").into_bytes();
    for (k, piece) in data.chunks(7000).enumerate() {
        up.extend_from_slice(format!("{:x}{}\r\n", piece.len(), if k == 1 { ";n=v" } else { "" }).as_bytes());
        up.extend_from_slice(piece);
        up.extend_from_slice(b"\r\n");
    }
    up.extend_from_slice(b"0\r\nX-Sum: 1\r\n\r\n");
    t.write_all(&up).unwrap();
    let (head, _) = answer_of(&mut t);
    assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
    assert!(o.got.recv_timeout(WAIT).unwrap() == up, "the upload as sent");
    let down = format!("GET /download HTTP/1.1\r\nHost: {NAME}\r\n\r\n");
    t.write_all(down.as_bytes()).unwrap();
    let want = answer_to(down.as_bytes());
    let mut came = vec![0; want.len()];
    t.read_exact(&mut came).unwrap();
    assert!(came == want, "the download as the origin sent it");
    terminate::forget(SESSION + 11);
}

/// A request whose `Host` names another host — or the tunnel's host on another port — is answered 421 inside the
/// run's TLS, said, and nothing of it reaches the origin.
#[test]
fn a_host_naming_another_host_is_421() {
    let o = kept();
    let (f, rules, ca) = terminating_kept(&o, SESSION + 12, limits());
    for (k, host) in ["other.test", "origin.test:8443"].into_iter().enumerate() {
        let (mut t, d) = tunnel_tls(&f, SESSION + 12, 1 + k as u64, &rules, &ca);
        t.write_all(format!("GET /x HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes()).unwrap();
        let (head, body) = answer_of(&mut t);
        let why = format!("origin.test:443: a request's Host names {host}");
        assert!(head.starts_with("HTTP/1.1 421 Misdirected Request\r\n"), "{head}");
        assert_eq!(String::from_utf8(body).unwrap(), format!("tarae's proxy: {why}\n"));
        assert!(closed(&mut t));
        assert_eq!(said(&d), format!("{why} — answered 421"));
        assert!(to_end(&o).is_empty(), "{host}: nothing reached the origin");
    }
    terminate::forget(SESSION + 12);
}

/// TRACE is 405, said as an echo, and nothing behind it reaches the origin: a GET, a TRACE and a GET pipelined in
/// one write — the first GET answered, then the 405, then the close; the second GET never arrives. TRACK the same,
/// its run half-closed (its close_notify) right after the write: the answer owed and the 405 still come.
#[test]
fn trace_is_405_and_nothing_behind_it_reaches_the_origin() {
    let o = kept();
    let (f, rules, ca) = terminating_kept(&o, SESSION + 13, limits());
    for (k, m) in ["TRACE", "TRACK"].into_iter().enumerate() {
        let (mut t, d) = tunnel_tls(&f, SESSION + 13, 1 + k as u64, &rules, &ca);
        let first = format!("GET /a HTTP/1.1\r\nHost: {NAME}\r\n\r\n");
        let all = format!(
            "{first}{m} / HTTP/1.1\r\nHost: {NAME}\r\n\r\nGET /behind HTTP/1.1\r\nHost: {NAME}\r\n\r\n"
        );
        t.write_all(all.as_bytes()).unwrap();
        if m == "TRACK" {
            t.conn.send_close_notify();
            t.flush().unwrap();
        }
        let (head, body) = answer_of(&mut t);
        assert!(
            head.starts_with("HTTP/1.1 200 ") && body == ANSWER.as_bytes(),
            "the GET before it first: {head}"
        );
        let (head, body) = answer_of(&mut t);
        let why = format!(
            "origin.test:443: a {m} request — refused on a host tarae terminates: it echoes the request back"
        );
        assert!(head.starts_with("HTTP/1.1 405 Method Not Allowed\r\n"), "{head}");
        assert_eq!(String::from_utf8(body).unwrap(), format!("tarae's proxy: {why}\n"));
        assert!(closed(&mut t), "{m}");
        assert_eq!(said(&d), format!("{why} — answered 405"));
        assert_eq!(to_end(&o), [first.into_bytes()], "{m}: only the GET before it reached the origin");
    }
    terminate::forget(SESSION + 13);
}

/// An `Upgrade` (WebSocket's) is 501 with words, and goes nowhere.
#[test]
fn an_upgrade_is_501_with_words() {
    let o = kept();
    let (f, rules, ca) = terminating_kept(&o, SESSION + 14, limits());
    let (mut t, d) = tunnel_tls(&f, SESSION + 14, 1, &rules, &ca);
    let ask =
        format!("GET /ws HTTP/1.1\r\nHost: {NAME}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n");
    t.write_all(ask.as_bytes()).unwrap();
    let (head, body) = answer_of(&mut t);
    assert!(head.starts_with("HTTP/1.1 501 Not Implemented\r\n"), "{head}");
    let why = "origin.test:443: an Upgrade — WebSocket and other upgrades aren't taken on a host tarae terminates (yet)";
    assert_eq!(String::from_utf8(body).unwrap(), format!("tarae's proxy: {why}\n"));
    assert_eq!(said(&d), format!("{why} — answered 501"));
    assert!(to_end(&o).is_empty(), "nothing reached the origin");
    terminate::forget(SESSION + 14);
}

/// Request smuggling's vectors inside a terminated tunnel: a head a reader could frame two ways is answered 400 and
/// nothing of it reaches the origin — a space before a colon, a folded line, a bare CR, a bare LF, a signed length, a
/// list, the same length twice, a coding other than one `chunked`, `chunked` twice, both framings, chunks in 1.0;
/// a chunked body that can't be followed — a size that isn't hex, one past 16 digits, extensions past their limit, a
/// size line ended by a bare LF — closes the tunnel with no close_notify, said, and the request hidden behind it never
/// reaches the origin.
#[test]
fn what_a_reader_could_frame_two_ways_never_reaches_the_origin() {
    let o = kept();
    let (f, rules, ca) = terminating_kept(&o, SESSION + 15, limits());
    let h = format!("Host: {NAME}\r\n");
    let hidden = format!("GET /hidden HTTP/1.1\r\n{h}\r\n");
    let heads = [
        format!("POST / HTTP/1.1\r\n{h}Content-Length : 3\r\n\r\nabc"),
        format!("POST / HTTP/1.1\r\n{h}X-A: 1\r\n Content-Length: 3\r\n\r\nabc"),
        format!("POST / HTTP/1.1\r\n{h}X-A: 1\rContent-Length: 3\r\n\r\nabc"),
        format!("POST / HTTP/1.1\r\n{h}X-A: 1\nContent-Length: 3\r\n\r\nabc"),
        format!("POST / HTTP/1.1\r\n{h}Content-Length: +3\r\n\r\nabc"),
        format!("POST / HTTP/1.1\r\n{h}Content-Length: 3, 3\r\n\r\nabc"),
        format!("POST / HTTP/1.1\r\n{h}Content-Length: 3\r\nContent-Length: 3\r\n\r\nabc"),
        format!("POST / HTTP/1.1\r\n{h}Transfer-Encoding: gzip\r\n\r\n0\r\n\r\n"),
        format!("POST / HTTP/1.1\r\n{h}Transfer-Encoding: chunked, chunked\r\n\r\n0\r\n\r\n"),
        format!("POST / HTTP/1.1\r\n{h}Content-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"),
        format!("POST / HTTP/1.0\r\n{h}Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n"),
    ];
    let mut run = 0;
    for head in &heads {
        run += 1;
        let (mut t, d) = tunnel_tls(&f, SESSION + 15, run, &rules, &ca);
        t.write_all(format!("{head}{hidden}").as_bytes()).unwrap();
        let (status, _) = answer_of(&mut t);
        assert!(status.starts_with("HTTP/1.1 400 "), "{head:?}: {status}");
        assert!(closed(&mut t), "{head:?}");
        assert!(said(&d).ends_with(" — answered 400"), "{head:?}");
        assert!(to_end(&o).is_empty(), "{head:?}: nothing reached the origin");
    }
    let chunked = format!("POST / HTTP/1.1\r\n{h}Transfer-Encoding: chunked\r\n\r\n");
    let long_ext = format!("1;{}\r\na\r\n0\r\n\r\n", "e".repeat(parse::CHUNK_LINE_MAX));
    for (body, why) in [
        ("zz\r\n", "a chunk size that isn't 1–16 hex digits"),
        ("11111111111111111\r\n", "a chunk size that isn't 1–16 hex digits"),
        (long_ext.as_str(), "a chunk-size line past its limit"),
        ("3\nabc\r\n0\r\n\r\n", "a chunk-size line not ended by CR LF"),
    ] {
        run += 1;
        let (mut t, d) = tunnel_tls(&f, SESSION + 15, run, &rules, &ca);
        t.write_all(format!("{chunked}{body}{hidden}").as_bytes()).unwrap();
        assert!(cut(&mut t), "{why}");
        assert_eq!(
            said(&d),
            format!("origin.test:443: the run's side: a request's body: {why} — the tunnel closed")
        );
        for r in to_end(&o) {
            assert!(
                chunked.as_bytes().starts_with(&r) || r == chunked.as_bytes(),
                "{why}: only the head: {r:?}"
            );
        }
    }
    terminate::forget(SESSION + 15);
}

/// The idle limits close a terminated tunnel: no whole first head within `head_time` of the handshake (said);
/// nothing asked within `keep_alive` of an answer (close_notify, not said); nothing moved part-way through a body
/// within `stalled` (said, no close_notify) — but never while a whole request waits for its answer, however long past
/// both.
#[test]
fn the_idle_limits_close_a_terminated_tunnel() {
    let o = kept();
    let short = Duration::from_millis(300);
    let first = Duration::from_secs(1);
    let limits = Limits { head_time: first, keep_alive: short, stalled: short, ..limits() };
    let (f, rules, ca) = terminating_kept(&o, SESSION + 16, limits);
    // No head
    let (mut t, d) = tunnel_tls(&f, SESSION + 16, 1, &rules, &ca);
    let since = Instant::now();
    assert!(closed(&mut t), "closed without a head");
    assert!(since.elapsed() >= first / 2, "not before its limit: {:?}", since.elapsed());
    assert_eq!(
        said(&d),
        "origin.test:443: no whole request head within 1 s of its TLS handshake — the tunnel closed"
    );
    // A whole request waiting past both limits is answered; then the keep-alive's end
    let (mut t, _d) = tunnel_tls(&f, SESSION + 16, 2, &rules, &ca);
    t.write_all(format!("GET /slow HTTP/1.1\r\nHost: {NAME}\r\n\r\n").as_bytes()).unwrap();
    std::thread::sleep(short * 4);
    o.go.send(()).unwrap();
    let (head, _) = answer_of(&mut t);
    assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
    let since = Instant::now();
    assert!(closed(&mut t), "closed at rest");
    assert!(since.elapsed() >= short / 2, "not before its limit: {:?}", since.elapsed());
    // Part-way through a body
    let (mut t, d) = tunnel_tls(&f, SESSION + 16, 3, &rules, &ca);
    t.write_all(format!("POST /p HTTP/1.1\r\nHost: {NAME}\r\nContent-Length: 10\r\n\r\nabc").as_bytes())
        .unwrap();
    assert!(cut(&mut t), "closed part-way, with no close_notify");
    let why = "origin.test:443: nothing moved for 0.3 s part-way through a request or its answer — the tunnel closed";
    assert_eq!(said(&d), why);
    terminate::forget(SESSION + 16);
}

/// A `Host` naming the tunnel's host in other case, or with DNS's trailing dot, names that host (`parse::name`:
/// lowercase, the dot off): each goes on as sent and is answered — no 421.
#[test]
fn a_host_in_other_case_or_with_its_trailing_dot_goes() {
    let o = kept();
    let (f, rules, ca) = terminating_kept(&o, SESSION + 17, limits());
    let (mut t, _d) = tunnel_tls(&f, SESSION + 17, 1, &rules, &ca);
    let asks = [
        "GET /a HTTP/1.1\r\nHost: ORIGIN.TEST\r\n\r\n",
        "GET /b HTTP/1.1\r\nHost: origin.test.\r\nConnection: close\r\n\r\n",
    ];
    for ask in asks {
        t.write_all(ask.as_bytes()).unwrap();
        let (head, body) = answer_of(&mut t);
        assert!(head.starts_with("HTTP/1.1 200 ") && body == ANSWER.as_bytes(), "{ask:?}: {head}");
    }
    assert!(closed(&mut t));
    let got: Vec<String> = to_end(&o).into_iter().map(|r| String::from_utf8(r).unwrap()).collect();
    assert_eq!(got, asks);
    terminate::forget(SESSION + 17);
}

/// An answer that runs to the host's close (neither length nor chunks).
const OPEN: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nall of this, until the close";

/// A truncation on the host's leg reaches the run as one: an answer that runs to the host's close is whole only by the
/// host's close_notify — ended with one, the run reads it whole, then tarae's close_notify; ended by the host's TCP
/// with none, the run's TLS reads an end without close_notify (`UnexpectedEof`), never a clean end; an answer shorter
/// than its Content-Length, though the host ended it with close_notify, the same — each truncation said.
#[test]
fn a_truncation_on_the_hosts_leg_reaches_the_run_as_one() {
    let short: &'static [u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nonly some of it";
    let unclean =
        "its TLS ended without close_notify in an answer that runs to its close (it may be cut short)";
    for (k, (answer, notify, why)) in [
        (OPEN, true, None),
        (OPEN, false, Some(unclean)),
        (short, true, Some("it closed part-way through an answer's body")),
    ]
    .into_iter()
    .enumerate()
    {
        let session = SESSION + 20 + k as u64;
        let (at, public) = ending(answer, notify);
        let (f, rules, ca) = terminating_to(at, &public, session, limits());
        let (mut t, d) = tunnel_tls(&f, session, 1, &rules, &ca);
        t.write_all(format!("GET /x HTTP/1.1\r\nHost: {NAME}\r\n\r\n").as_bytes()).unwrap();
        let mut got = Vec::new();
        let end = t.read_to_end(&mut got).map_err(|e| e.kind());
        let came = String::from_utf8_lossy(&got).into_owned();
        match why {
            None => assert!(end.is_ok() && got == answer, "{k}: {end:?} {came:?}"),
            Some(why) => {
                assert_eq!(end, Err(std::io::ErrorKind::UnexpectedEof), "{k}: no close_notify ({came:?})");
                assert!(answer.starts_with(&got), "{k}: {came:?}");
                assert_eq!(said(&d), format!("{NAME}:443: the host's side: {why} — the tunnel closed"));
            }
        }
        terminate::forget(session);
    }
}

/// The same truncation as the run's own curl reads it, through a session's port with its token: the answer that runs
/// to the host's close ended with its close_notify, exit 0 and the body whole; with none, curl fails (OpenSSL 3's
/// "unexpected eof while reading", exit 56; an empty reply where none of it went yet) — where curl's TLS tells the two
/// apart: LibreSSL's and OpenSSL 1's curl take an end without close_notify for the end (curl's choice, for old
/// servers), said, the clean end checked alone.
#[test]
fn a_truncation_on_the_hosts_leg_fails_the_runs_curl() {
    let test = "tests::terminate::a_truncation_on_the_hosts_leg_fails_the_runs_curl";
    let Some(curl) = super::super::packs_tests::on_path("curl") else {
        return super::super::packs_tests::skip(test, "no curl on PATH");
    };
    let version = std::process::Command::new(&curl).arg("--version").output().unwrap();
    let first = String::from_utf8_lossy(&version.stdout).lines().next().unwrap_or_default().to_string();
    // (curl's TLS: the one not in parentheses, a multi-SSL build's others are)
    let tells = first.contains(" OpenSSL/3");
    let body = &OPEN[OPEN.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..];
    let scratch = crate::scratch::Scratch::new("p1b-curl");
    for (k, notify) in [true, false].into_iter().enumerate() {
        let session = SESSION + 23 + k as u64;
        let (at, public) = ending(OPEN, notify);
        let (f, rules, ca) = terminating_to(at, &public, session, limits());
        let port = session_port(&f.proxy, session);
        let (token, _d) = token_run(&f.proxy, session, 1, rules);
        let file = scratch.join(format!("ca-{k}.pem"));
        std::fs::write(&file, crate::sandbox::cabundle::pem(&ca)).unwrap();
        let out = std::process::Command::new(&curl)
            .args(["-q", "-sS", "--max-time", "15", "--cacert"])
            .arg(&file)
            .args([
                "-x",
                &format!("http://127.0.0.1:{port}"),
                "--proxy-user",
                &format!("run:{}", token.token()),
            ])
            .arg(format!("https://{NAME}/x"))
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .output()
            .unwrap();
        let said = format!(
            "{first}\n{:?}: {}{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        match notify {
            true => assert!(out.status.success() && out.stdout == body, "with close_notify: {said}"),
            false if tells => assert!(!out.status.success(), "without close_notify: {said}"),
            false => super::super::packs_tests::skip(
                test,
                &format!(
                    "this curl's TLS takes an end without close_notify for the end (exit {:?}; {first})",
                    out.status.code()
                ),
            ),
        }
        terminate::forget(session);
    }
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
